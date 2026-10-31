#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/pty_harness.rs"]
mod pty_harness;

use std::time::Duration;

use parity::{Mux, Scratch};
use ptytest::{PtyTest, Size, TerminalBaseline};

fn create_session(mux: &Mux, name: &str, windows: &[&str]) {
    mux.ok(&[
        "new-session",
        "-d",
        "-s",
        name,
        "-n",
        windows[0],
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    for window in &windows[1..] {
        mux.ok(&["new-window", "-d", "-t", name, "-n", window]);
    }
}

fn state(mux: &Mux, session: &str) -> String {
    format!(
        "{}\n{}",
        mux.body(&[
            "list-windows",
            "-t",
            session,
            "-F",
            "#{window_index}:#{window_name}:#{window_panes}",
        ]),
        mux.body(&[
            "list-panes",
            "-t",
            session,
            "-F",
            "#{pane_id}:#{pane_index}"
        ])
    )
}

fn attach(mux: &Mux, session: &str, name: &str) -> (PtyTest, TerminalBaseline) {
    let size = Size::new(40, 8).expect("constant size");
    let mut terminal =
        pty_harness::spawn_attached(mux, name, &["attach-session", "-t", session], size);
    pty_harness::wait_for_text(
        &mut terminal,
        &format!("({session})"),
        Duration::from_secs(3),
    );
    let baseline = terminal.terminal_baseline();
    (terminal, baseline)
}

fn send_prompt(terminal: &mut PtyTest, command: &str) {
    terminal
        .send_bytes(terminal.deadline(Duration::from_secs(3)), b"\x01/")
        .expect("open prompt");
    terminal
        .send_text(terminal.deadline(Duration::from_secs(3)), command)
        .expect("type prompt command");
    terminal
        .send_bytes(terminal.deadline(Duration::from_secs(3)), b"\r")
        .expect("submit prompt");
}

#[test]
fn sole_pane_break_keeps_the_session_and_uses_the_next_index() {
    let scratch = Scratch::new("atomicity-sole-break");
    let tm = Mux::tm(&scratch, "tm");
    create_session(&tm, "S", &["one"]);
    assert!(tm.ok(&["break-pane", "-t", ":"]).is_empty());
    assert!(tm.ok(&["has-session", "-t", "S"]).is_empty());
    assert_eq!(
        tm.body(&[
            "list-windows",
            "-t",
            "S",
            "-F",
            "#{window_index}:#{window_panes}"
        ]),
        "2:1"
    );
    assert_eq!(
        tm.body(&["display-message", "-p", "-t", "S", "#{window_index}"]),
        "2"
    );
}

#[test]
fn break_to_an_occupied_index_errors_without_mutation() {
    let scratch = Scratch::new("atomicity-occupied-break");
    let tm = Mux::tm(&scratch, "tm");
    create_session(&tm, "S", &["one", "two"]);
    tm.ok(&["split-window", "-h", "-t", "S:1"]);
    let before = state(&tm, "S");
    let output = tm.run(&["break-pane", "-t", "S:2"]);
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim(),
        "tm: index in use: 2"
    );
    assert_eq!(state(&tm, "S"), before);
    assert!(tm.ok(&["break-pane", "-t", "S:3"]).is_empty());
    assert_eq!(
        tm.body(&[
            "list-windows",
            "-t",
            "S",
            "-F",
            "#{window_index}:#{window_panes}"
        ]),
        "1:1\n2:1\n3:1"
    );
}

#[test]
fn invalid_join_and_swap_targets_error_without_mutation() {
    let scratch = Scratch::new("atomicity-invalid-targets");
    let tm = Mux::tm(&scratch, "tm");
    create_session(&tm, "S", &["one", "two"]);
    tm.ok(&["split-window", "-h", "-t", "S:1"]);
    let before = state(&tm, "S");
    let output = tm.run(&["join-pane", "-t", "S:9"]);
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim(),
        "tm: can't find window: 9"
    );
    assert_eq!(state(&tm, "S"), before);

    let output = tm.run(&["join-pane", "-s", "S:1.1", "-t", "S:1"]);
    assert!(!output.status.success());
    assert_eq!(state(&tm, "S"), before);

    let output = tm.run(&["swap-pane", "-s", "S:1.0", "-t", "S:9.0"]);
    assert!(!output.status.success());
    assert_eq!(state(&tm, "S"), before);
}

#[test]
fn insufficient_split_space_leaves_the_window_unchanged() {
    let scratch = Scratch::new("atomicity-small-pane");
    let tm = Mux::tm(&scratch, "tm");
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "S",
        "-x",
        "2",
        "-y",
        "4",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    let before = state(&tm, "S");
    let output = tm.run(&["split-window", "-h", "-t", "S"]);
    assert!(!output.status.success());
    assert_eq!(state(&tm, "S"), before);
}

#[test]
fn chain_parse_error_aborts_before_any_effect() {
    let scratch = Scratch::new("atomicity-chain-parse");
    let tm = Mux::tm(&scratch, "tm");
    create_session(&tm, "S", &["one"]);
    let (mut client, baseline) = attach(&tm, "S", "chain-parse");
    send_prompt(
        &mut client,
        "split-window -h \\; definitely-not-a-command",
    );
    pty_harness::wait_for_text(&mut client, "unknown command", Duration::from_secs(3));
    assert_eq!(
        tm.body(&["display-message", "-p", "-t", "S", "#{window_panes}"]),
        "1"
    );
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn chain_runtime_error_keeps_earlier_effects_and_stops_later_commands() {
    let scratch = Scratch::new("atomicity-chain-runtime");
    let tm = Mux::tm(&scratch, "tm");
    create_session(&tm, "S", &["one"]);
    let (mut client, baseline) = attach(&tm, "S", "chain-runtime");
    send_prompt(
        &mut client,
        "split-window -h \\; join-pane -t S:9 \\; kill-pane",
    );
    pty_harness::wait_for_text(&mut client, "can't find window: 9", Duration::from_secs(3));
    assert_eq!(
        tm.body(&["display-message", "-p", "-t", "S", "#{window_panes}"]),
        "2"
    );
    pty_harness::detach(&mut client, &baseline);
}
