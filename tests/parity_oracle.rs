#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/raw_pty.rs"]
#[allow(dead_code)]
mod raw_pty;

use std::process::Command;
use std::time::Duration;

use parity::{ExitStatus, Field, GridLimit, IntentionalDifference, Mux, Outcome, Scratch};
use raw_pty::RawClient;

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
    let known = [IntentionalDifference {
        test: "listed",
        command,
        limit: GridLimit::NoResizeReflow,
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

fn attached_modes(mux: &Mux, name: &str) -> raw_pty::ObservedModes {
    let mut client = RawClient::spawn(mux, name, &["attach-session", "-t", "S"], 8, 80);
    client.wait_for_screen(
        "attached terminal modes and status line",
        Duration::from_secs(5),
        |screen| {
            screen.alternate_screen()
                && screen.bracketed_paste()
                && screen.mouse_protocol_mode() != vt100::MouseProtocolMode::None
                && screen.contents().contains("(S)")
        },
    );
    let modes = client.modes();
    assert_eq!(client.detach(), Some(0));
    modes
}

#[test]
fn comparator_detects_a_flipped_terminal_mode_of_an_attached_client() {
    let scratch = Scratch::new("oracle-modes");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    create_session(&tm);
    create_session(&reference);
    let observed = attached_modes(&tm, "tm");
    let expected = attached_modes(&reference, "reference");
    assert!(expected.alternate_screen && expected.bracketed_paste);
    assert_eq!(
        parity::observation_difference("terminal modes", &observed, &expected),
        None
    );

    let mut flipped = observed.clone();
    flipped.alternate_screen = !flipped.alternate_screen;
    assert!(parity::observation_difference("terminal modes", &flipped, &expected).is_some());
    let mut flipped = observed.clone();
    flipped.bracketed_paste = !flipped.bracketed_paste;
    assert!(parity::observation_difference("terminal modes", &flipped, &expected).is_some());
    let mut flipped = observed.clone();
    flipped.mouse_mode.push('x');
    assert!(parity::observation_difference("terminal modes", &flipped, &expected).is_some());
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
