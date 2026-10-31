#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/pty_harness.rs"]
mod pty_harness;

use std::time::{Duration, Instant};

use parity::{Mux, Scratch};
use ptytest::{PtyTest, Size};

fn create_session(mux: &Mux, session: &str, windows: usize) {
    mux.ok(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        "one",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    for index in 1..windows {
        mux.ok(&[
            "new-window",
            "-d",
            "-t",
            session,
            "-n",
            &format!("w{index}"),
        ]);
    }
}

fn create_byte_session(mux: &Mux, session: &str, count: usize) {
    mux.ok(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-x",
        "80",
        "-y",
        "24",
        "--",
        "sh",
        "-c",
        &format!("stty -echo raw; od -An -tx1 -N{count}; sleep 30"),
    ]);
}

fn attach(mux: &Mux, session: &str, name: &str) -> (PtyTest, ptytest::TerminalBaseline) {
    let size = Size::new(80, 24).expect("constant size");
    let mut terminal =
        pty_harness::spawn_attached(mux, name, &["attach-session", "-t", session], size);
    pty_harness::wait_for_text(&mut terminal, &format!("({session})"), Duration::from_secs(3));
    let baseline = terminal.terminal_baseline();
    (terminal, baseline)
}

fn send(terminal: &mut PtyTest, bytes: &[u8]) {
    terminal
        .send_bytes(terminal.deadline(Duration::from_secs(3)), bytes)
        .expect("send client bytes");
}

fn wait_hex(mux: &Mux, session: &str, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let captured = mux.ok(&["capture-pane", "-p", "-t", session]);
        let actual = captured
            .split_whitespace()
            .filter(|token| token.len() == 2 && token.chars().all(|c| c.is_ascii_hexdigit()))
            .collect::<Vec<_>>()
            .join(" ");
        if actual == expected || Instant::now() >= deadline {
            assert_eq!(actual, expected, "pane bytes: {captured:?}");
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn active_window(mux: &Mux, session: &str) -> String {
    mux.body(&[
        "display-message",
        "-p",
        "-t",
        session,
        "#{window_index}",
    ])
}

fn wait_window(mux: &Mux, session: &str, expected: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let value = active_window(mux, session);
        if value == expected || Instant::now() >= deadline {
            return value;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn lone_escape_after_prefix_is_forwarded_once() {
    let scratch = Scratch::new("input-lone-escape");
    let tm = Mux::tm(&scratch, "tm");
    create_byte_session(&tm, "S", 1);
    let (mut client, baseline) = attach(&tm, "S", "escape");
    send(&mut client, b"\x01");
    std::thread::sleep(Duration::from_millis(20));
    send(&mut client, b"\x1b");
    wait_hex(&tm, "S", "1b");
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn unknown_key_after_prefix_drops_the_prefix() {
    let scratch = Scratch::new("input-unknown");
    let tm = Mux::tm(&scratch, "tm");
    create_byte_session(&tm, "S", 1);
    let (mut client, baseline) = attach(&tm, "S", "unknown");
    send(&mut client, b"\x01q");
    wait_hex(&tm, "S", "71");
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn literal_prefix_bindings_match_reference_bytes() {
    let scratch = Scratch::new("input-prefix-bytes");
    let tm = Mux::tm(&scratch, "tm");
    create_byte_session(&tm, "S", 2);
    let (mut client, baseline) = attach(&tm, "S", "prefix-bytes");
    send(&mut client, b"\x01\x13");
    wait_hex(&tm, "S", "01 01");
    pty_harness::detach(&mut client, &baseline);

    let scratch = Scratch::new("input-send-prefix");
    let tm = Mux::tm(&scratch, "tm");
    create_byte_session(&tm, "S", 1);
    let (mut client, baseline) = attach(&tm, "S", "send-prefix");
    send(&mut client, b"\x01b");
    wait_hex(&tm, "S", "01");
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn alt_and_incomplete_sequences_are_forwarded_once() {
    let scratch = Scratch::new("input-alt");
    let tm = Mux::tm(&scratch, "tm");
    create_byte_session(&tm, "S", 2);
    let (mut client, baseline) = attach(&tm, "S", "alt");
    send(&mut client, b"\x1bx");
    wait_hex(&tm, "S", "1b 78");
    pty_harness::detach(&mut client, &baseline);

    let scratch = Scratch::new("input-incomplete");
    let tm = Mux::tm(&scratch, "tm");
    create_byte_session(&tm, "S", 2);
    let (mut client, baseline) = attach(&tm, "S", "incomplete");
    send(&mut client, b"\x1b[");
    wait_hex(&tm, "S", "1b 5b");
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn fragmented_prefix_binding_is_atomic_at_every_byte_boundary() {
    let scratch = Scratch::new("input-fragmented");
    let tm = Mux::tm(&scratch, "tm");
    create_session(&tm, "S", 1);
    let (mut client, baseline) = attach(&tm, "S", "fragmented");
    for byte in b"\x01%" {
        send(&mut client, &[*byte]);
    }
    assert_eq!(
        wait_window(&tm, "S", "1"),
        "1",
        "fragmented binding changed the active window"
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let panes = tm.body(&["display-message", "-p", "-t", "S", "#{window_panes}"]);
        if panes == "2" || Instant::now() >= deadline {
            assert_eq!(panes, "2");
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn repeat_deadline_and_switching_follow_the_500ms_contract() {
    let scratch = Scratch::new("input-repeat");
    let tm = Mux::tm(&scratch, "tm");
    create_session(&tm, "S", 2);
    let (mut client, baseline) = attach(&tm, "S", "repeat");

    send(&mut client, b"\x01\x1b[1;5C");
    assert_eq!(wait_window(&tm, "S", "2"), "2");
    std::thread::sleep(Duration::from_millis(650));
    send(&mut client, b"\x1b[1;5C");
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        active_window(&tm, "S"),
        "2",
        "expired repeat mode still executed the binding"
    );
    assert_eq!(
        wait_window(&tm, "S", "2"),
        "2",
        "expired repeat mode changed the window"
    );

    send(&mut client, b"\x01\x1b[1;5C");
    assert_eq!(wait_window(&tm, "S", "1"), "1");
    send(&mut client, b"\x1b[1;5D");
    assert_eq!(
        wait_window(&tm, "S", "2"),
        "2",
        "repeat mode did not switch to another repeatable key"
    );
    pty_harness::detach(&mut client, &baseline);
}
