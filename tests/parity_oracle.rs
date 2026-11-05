#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/pty_harness.rs"]
#[allow(dead_code)]
mod pty_harness;

use std::process::Command;
use std::time::Duration;

use parity::{ExitStatus, Field, KnownDifference, Mux, Outcome, Scratch};
use ptytest::{ProtocolProfile, Size};

fn create_session(mux: &Mux) {
    mux.ok(&[
        "new-session", "-d", "-s", "S", "-x", "80", "-y", "8", "--", "sleep", "300",
    ]);
}

fn observed_pair() -> (Outcome, Outcome) {
    let scratch = Scratch::new("oracle-outcome");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    for mux in [&tm, &reference] {
        create_session(mux);
    }
    let command: &[&str] = &["display-message", "-p", "-t", "S", "hello"];
    (tm.outcome(command), reference.outcome(command))
}

#[test]
fn comparator_accepts_identical_observations_and_detects_each_mutated_field() {
    let (tm, reference) = observed_pair();
    assert_eq!(tm.status, ExitStatus::Code(0));
    assert_eq!(tm.stdout, b"hello\n");
    assert_eq!(parity::first_difference(&tm, &reference), None);

    let mut changed = tm.clone();
    changed.status = ExitStatus::Code(1);
    assert_eq!(
        parity::first_difference(&changed, &reference).map(|d| d.field),
        Some(Field::Status)
    );
    changed.status = ExitStatus::Signal(0);
    assert_eq!(
        parity::first_difference(&changed, &reference).map(|d| d.field),
        Some(Field::Status)
    );

    let mut changed = tm.clone();
    changed.stdout[0] ^= 1;
    assert_eq!(
        parity::first_difference(&changed, &reference).map(|d| d.field),
        Some(Field::Stdout)
    );

    let mut changed = tm.clone();
    changed.stderr.push(b'x');
    assert_eq!(
        parity::first_difference(&changed, &reference).map(|d| d.field),
        Some(Field::Stderr)
    );
}

#[test]
fn listed_difference_must_persist_and_unlisted_difference_fails() {
    let (tm, reference) = observed_pair();
    let command: &[&str] = &["display-message", "-p", "-t", "S", "hello"];
    let mut diverged = tm.clone();
    diverged.stderr = b"extra".to_vec();
    let known = [KnownDifference {
        test: "listed",
        command,
        parity_row: "row",
        field: Field::Stderr,
        tm: "extra",
        reference: "",
    }];

    assert!(parity::check_command(&known, "listed", command, &diverged, &reference).is_ok());
    assert!(parity::check_command(&known, "other", command, &diverged, &reference).is_err());
    let fixed = parity::check_command(&known, "listed", command, &tm, &reference)
        .expect_err("a fixed difference must demand removal of its entry");
    assert!(fixed.contains("remove"), "{fixed}");
    diverged.stderr = b"different".to_vec();
    assert!(parity::check_command(&known, "listed", command, &diverged, &reference).is_err());
}

fn attached_screen(mux: &Mux, name: &str) -> ptytest::ScreenSnapshot {
    let mut client = pty_harness::spawn_attached_with_profile(
        mux,
        name,
        &["attach-session", "-t", "S"],
        Size::new(80, 8).expect("size"),
        ProtocolProfile::xterm_minimal_v1(),
    );
    let baseline = client.terminal_baseline();
    let screen = client
        .wait_for_screen(
            client.deadline(Duration::from_secs(5)),
            "alternate screen entered",
            |screen| screen.modes().alternate_screen,
        )
        .expect("attached client enters the alternate screen");
    pty_harness::detach(&mut client, &baseline);
    screen
}

#[test]
fn comparator_detects_a_flipped_terminal_mode_of_an_attached_client() {
    let scratch = Scratch::new("oracle-modes");
    let tm = Mux::tm(&scratch, "tm");
    create_session(&tm);
    let first = attached_screen(&tm, "first").modes().clone();
    let second = attached_screen(&tm, "second").modes().clone();
    assert!(first.alternate_screen);
    assert_eq!(parity::observation_difference("terminal modes", &first, &second), None);

    let mut flipped = second.clone();
    flipped.alternate_screen = !flipped.alternate_screen;
    assert!(parity::observation_difference("terminal modes", &first, &flipped).is_some());
    let mut flipped = second.clone();
    flipped.bracketed_paste = !flipped.bracketed_paste;
    assert!(parity::observation_difference("terminal modes", &first, &flipped).is_some());
}

#[test]
fn runner_returns_four_mebibytes_of_stdout_and_stderr_without_blocking() {
    const SIZE: usize = 4 * 1024 * 1024;
    let mut command = Command::new("sh");
    command.args([
        "-c",
        "head -c 4194304 /dev/zero; head -c 4194304 /dev/zero >&2",
    ]);
    let output = parity::run_with_deadline(command, Duration::from_secs(10))
        .expect("large output completes");
    assert!(output.status.success());
    assert_eq!(output.stdout.len(), SIZE);
    assert_eq!(output.stderr.len(), SIZE);
}
