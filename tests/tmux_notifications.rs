#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/pty_harness.rs"]
#[allow(dead_code)]
mod pty_harness;

use std::time::Duration;

use parity::{Mux, Scratch};
use ptytest::Size;

fn create_session(mux: &Mux, command: &str) {
    mux.ok(&[
        "new-session",
        "-d",
        "-s",
        "S",
        "-x",
        "40",
        "-y",
        "10",
        "--",
        "sh",
        "-c",
        command,
    ]);
}

fn attach(mux: &Mux, name: &str) -> (ptytest::PtyTest, ptytest::TerminalBaseline) {
    let mut terminal = pty_harness::spawn_attached(
        mux,
        name,
        &["attach-session", "-t", "S"],
        Size::new(40, 10).expect("size"),
    );
    pty_harness::wait_for_text(&mut terminal, "(S)", Duration::from_secs(3));
    let baseline = terminal.terminal_baseline();
    (terminal, baseline)
}

fn count(terminal: &ptytest::PtyTest, needle: &[u8]) -> usize {
    terminal
        .raw_output()
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

fn trigger_bell(mux: &Mux) {
    mux.ok(&["send-keys", "-t", "S", "bell", "Enter"]);
}

#[test]
fn audible_bell_reaches_each_attached_client_once() {
    let scratch = Scratch::new("notify-bell");
    let tm = Mux::tm(&scratch, "tm");
    create_session(&tm, "read line; printf '\\007'; sleep 30");
    let (mut first, first_baseline) = attach(&tm, "bell-first");
    let (mut second, second_baseline) = attach(&tm, "bell-second");
    let first_before = count(&first, b"\x07");
    let second_before = count(&second, b"\x07");
    trigger_bell(&tm);
    first
        .wait_for_quiescence(first.deadline(Duration::from_secs(3)), Duration::from_millis(100))
        .expect("first bell frame");
    second
        .wait_for_quiescence(second.deadline(Duration::from_secs(3)), Duration::from_millis(100))
        .expect("second bell frame");
    assert_eq!(count(&first, b"\x07"), first_before + 1);
    assert_eq!(count(&second, b"\x07"), second_before + 1);
    pty_harness::detach(&mut first, &first_baseline);
    pty_harness::detach(&mut second, &second_baseline);
}

#[test]
fn visual_bell_suppresses_the_outer_bell() {
    let scratch = Scratch::new("notify-visual");
    let tm = Mux::tm(&scratch, "tm");
    create_session(&tm, "read line; printf '\\007'; sleep 30");
    tm.ok(&["set-option", "-g", "visual-bell", "on"]);
    let (mut client, baseline) = attach(&tm, "visual");
    let before = count(&client, b"\x07");
    trigger_bell(&tm);
    client
        .wait_for_quiescence(client.deadline(Duration::from_secs(3)), Duration::from_millis(100))
        .expect("visual bell frame");
    assert_eq!(count(&client, b"\x07"), before);
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn application_osc52_is_ignored_in_external_mode() {
    let scratch = Scratch::new("notify-osc52");
    let tm = Mux::tm(&scratch, "tm");
    create_session(
        &tm,
        "read line; printf '\\033]52;c;YXBw\\007'; sleep 30",
    );
    let (mut client, baseline) = attach(&tm, "osc52");
    tm.ok(&["send-keys", "-t", "S", "app", "Enter"]);
    client
        .wait_for_quiescence(client.deadline(Duration::from_secs(3)), Duration::from_millis(100))
        .expect("application OSC 52 frame");
    assert_eq!(count(&client, b"\x1b]52;c;"), 0);
    assert!(tm.ok(&["list-buffers"]).is_empty());
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn tm_copy_emits_osc52_once_per_capable_client() {
    let scratch = Scratch::new("notify-copy");
    let tm = Mux::tm(&scratch, "tm");
    create_session(&tm, "printf 'alpha\\nbeta\\n'; sleep 30");
    let (mut client, baseline) = attach(&tm, "copy");
    tm.ok(&["copy-mode", "-t", "S"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "S"]);
    tm.ok(&["send-keys", "-X", "begin-selection", "-t", "S"]);
    tm.ok(&["send-keys", "-X", "cursor-down", "-t", "S"]);
    tm.ok(&["send-keys", "-X", "copy-selection-and-cancel", "-t", "S"]);
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while count(&client, b"\x1b]52;c;") == 0 && std::time::Instant::now() < deadline {
        client
            .wait_for_quiescence(client.deadline(Duration::from_millis(500)), Duration::from_millis(50))
            .ok();
    }
    assert_eq!(count(&client, b"\x1b]52;c;"), 1);

    tm.ok(&["set-option", "-g", "set-clipboard", "off"]);
    tm.ok(&["copy-mode", "-t", "S"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "S"]);
    tm.ok(&["send-keys", "-X", "begin-selection", "-t", "S"]);
    tm.ok(&["send-keys", "-X", "cursor-down", "-t", "S"]);
    tm.ok(&["send-keys", "-X", "copy-selection-and-cancel", "-t", "S"]);
    client
        .wait_for_quiescence(client.deadline(Duration::from_secs(3)), Duration::from_millis(100))
        .expect("clipboard-off frame");
    assert_eq!(count(&client, b"\x1b]52;c;"), 1);
    pty_harness::detach(&mut client, &baseline);
}
