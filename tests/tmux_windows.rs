#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/pty_harness.rs"]
mod pty_harness;

use std::time::{Duration, Instant};

use parity::{Mux, Scratch};
use ptytest::{PtyTest, Size};

fn create_session(mux: &Mux, session: &str, name: &str) {
    mux.ok(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        name,
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
}

fn wait_client_size(mux: &Mux, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let clients = mux.ok(&[
            "list-clients",
            "-F",
            "#{client_width}x#{client_height}",
        ]);
        if clients.lines().any(|line| line == expected) || Instant::now() >= deadline {
            assert!(
                clients.lines().any(|line| line == expected),
                "client size {expected} did not arrive: {clients:?}"
            );
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn attach(mux: &Mux, session: &str, name: &str, size: Size) -> (PtyTest, ptytest::TerminalBaseline) {
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

fn send_text(terminal: &mut PtyTest, text: &str) {
    terminal
        .send_text(terminal.deadline(Duration::from_secs(3)), text)
        .expect("send client text");
}

fn wait_contains(terminal: &mut PtyTest, text: &str) {
    pty_harness::wait_for_text(terminal, text, Duration::from_secs(3));
}

fn wait_name(mux: &Mux, session: &str, index: &str, expected: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let name = mux.body(&[
            "display-message",
            "-p",
            "-t",
            &format!("{session}:{index}"),
            "#{window_name}",
        ]);
        if name == expected || Instant::now() >= deadline {
            return name;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn base_index_renumbering_and_window_lifecycle_match_reference() {
    let scratch = Scratch::new("windows-lifecycle");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    for mux in [&tm, &reference] {
        create_session(mux, "S", "one");
        mux.ok(&["new-window", "-d", "-t", "S", "-n", "two"]);
        mux.ok(&["new-window", "-d", "-t", "S", "-n", "three"]);
        mux.ok(&["rename-window", "-t", "S:2", "renamed"]);
        mux.ok(&["kill-window", "-t", "S:2"]);
    }
    parity::compare_commands(
        &tm,
        &reference,
        &[
            &["list-windows", "-t", "S", "-F", "#{window_index}:#{window_name}"],
            &["new-window", "-d", "-t", "S", "-n", "four"],
            &["list-windows", "-t", "S", "-F", "#{window_index}:#{window_name}"],
            &["select-window", "-t", "S:4"],
            &["last-window", "-t", "S"],
            &["display-message", "-p", "-t", "S", "#{window_index}"],
            &["show-options", "-g", "-v", "base-index"],
            &["show-options", "-g", "-v", "renumber-windows"],
            &["show-options", "-g", "-v", "automatic-rename"],
            &["show-options", "-g", "-v", "monitor-activity"],
        ],
    );
}

#[test]
fn prompt_values_are_inserted_verbatim() {
    let scratch = Scratch::new("windows-prompt-values");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    create_session(&tm, "S", "one");
    create_session(&reference, "S", "one");
    let size = Size::new(60, 8).expect("constant size");
    let (mut client, baseline) = attach(&tm, "S", "prompt-values", size);
    for value in [
        "two words",
        "it's",
        "a\"b",
        "back\\slash",
        "100%",
        "héllo",
        "",
    ] {
        send(&mut client, b"\x01r");
        send_text(&mut client, value);
        send(&mut client, b"\r");
        assert_eq!(wait_name(&tm, "S", "1", value), value);
        if !value.contains('\\') {
            reference.ok(&["rename-window", "-t", "S:1", value]);
            assert_eq!(
                tm.body(&["display-message", "-p", "-t", "S:1", "#{window_name}"]),
                reference.body(&["display-message", "-p", "-t", "S:1", "#{window_name}"]),
                "prompt value {value:?} did not match the reference"
            );
        }
    }
    send(&mut client, b"\x01n");
    send_text(&mut client, "new window");
    send(&mut client, b"\r");
    assert_eq!(wait_name(&tm, "S", "2", "new window"), "new window");
    reference.ok(&["new-window", "-d", "-t", "S", "-n", "new window"]);
    assert_eq!(
        tm.body(&[
            "list-windows",
            "-t",
            "S",
            "-F",
            "#{window_index}:#{window_name}"
        ]),
        reference.body(&[
            "list-windows",
            "-t",
            "S",
            "-F",
            "#{window_index}:#{window_name}"
        ])
    );
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn latest_client_resize_and_reattach_follow_window_size_latest() {
    let scratch = Scratch::new("windows-resize");
    let tm = Mux::tm(&scratch, "tm");
    let fixture = env!("CARGO_BIN_EXE_tm-pty-fixture");
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "S",
        "-x",
        "80",
        "-y",
        "24",
        "--",
        fixture,
    ]);
    let (mut first, first_baseline) =
        attach(&tm, "S", "first", Size::new(80, 24).expect("size"));
    wait_contains(&mut first, "VT_FIXTURE_READY");
    let (mut second, second_baseline) =
        attach(&tm, "S", "second", Size::new(100, 30).expect("size"));
    wait_contains(&mut second, "VT_FIXTURE_READY");

    send_text(&mut second, "measure\n");
    wait_contains(&mut second, "RESIZE_ACK:29x100");

    first
        .resize(Size::new(90, 28).expect("size"))
        .expect("resize non-latest client");
    wait_client_size(&tm, "90x28");
    send_text(&mut first, "measure\n");
    wait_contains(&mut first, "RESIZE_ACK:27x90");

    second
        .resize(Size::new(110, 32).expect("size"))
        .expect("resize now non-latest client");
    wait_client_size(&tm, "110x32");
    send_text(&mut second, "measure\n");
    wait_contains(&mut second, "RESIZE_ACK:31x110");

    pty_harness::detach(&mut second, &second_baseline);
    send_text(&mut first, "measure\n");
    wait_contains(&mut first, "RESIZE_ACK:27x90");
    pty_harness::detach(&mut first, &first_baseline);
}
