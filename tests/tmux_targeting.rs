#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/pty_harness.rs"]
mod pty_harness;

use std::time::{Duration, Instant};

use parity::{Mux, Scratch};
use ptytest::{PtyTest, Size};

fn create_windows(mux: &Mux, session: &str, names: &[&str]) {
    mux.ok(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-n",
        names[0],
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    for name in &names[1..] {
        mux.ok(&["new-window", "-d", "-t", session, "-n", name]);
    }
}

fn attach(mux: &Mux, session: &str, name: &str) -> PtyTest {
    let size = Size::new(40, 8).expect("constant size");
    let mut terminal = pty_harness::spawn_attached(
        mux,
        name,
        &["attach-session", "-t", session],
        size,
    );
    pty_harness::wait_for_text(&mut terminal, &format!("({session})"), Duration::from_secs(3));
    terminal
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

fn wait_value(tm: &Mux, arguments: &[&str], expected: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let value = tm.body(arguments);
        if value == expected || Instant::now() >= deadline {
            return value;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn window_state(mux: &Mux, session: &str) -> String {
    mux.body(&[
        "list-windows",
        "-t",
        session,
        "-F",
        "#{window_index}:#{window_name}",
    ])
}

#[test]
fn client_bindings_use_the_attached_session_and_window() {
    let scratch = Scratch::new("targeting-bindings");
    let tm = Mux::tm(&scratch, "tm");
    create_windows(&tm, "A", &["a1", "a2", "a3"]);
    create_windows(&tm, "B", &["b1", "b2"]);
    let mut client = attach(&tm, "B", "bindings");
    let baseline = client.terminal_baseline();

    send(&mut client, b"\x01\x1b[1;5C");
    assert_eq!(
        wait_value(
            &tm,
            &["display-message", "-p", "-t", "B", "#{window_index}"],
            "2"
        ),
        "2"
    );
    assert_eq!(window_state(&tm, "A"), "1:a1\n2:a2\n3:a3");

    send(&mut client, b"\x01\x1b[1;5D");
    assert_eq!(
        wait_value(
            &tm,
            &["display-message", "-p", "-t", "B", "#{window_index}"],
            "1"
        ),
        "1"
    );

    send(&mut client, b"\x01\x1b[C");
    assert_eq!(
        wait_value(
            &tm,
            &["display-message", "-p", "-t", "B", "#{window_index}"],
            "2"
        ),
        "2"
    );
    assert_eq!(
        wait_value(
            &tm,
            &["display-message", "-p", "-t", "B", "#{window_name}"],
            "b1"
        ),
        "b1"
    );
    assert_eq!(window_state(&tm, "B"), "1:b2\n2:b1");
    assert_eq!(window_state(&tm, "A"), "1:a1\n2:a2\n3:a3");

    send(&mut client, b"\x01\x1b[D");
    assert_eq!(
        wait_value(
            &tm,
            &["display-message", "-p", "-t", "B", "#{window_index}"],
            "1"
        ),
        "1"
    );
    assert_eq!(window_state(&tm, "B"), "1:b1\n2:b2");
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn slash_prompt_resolves_omitted_relative_and_stable_targets() {
    let scratch = Scratch::new("targeting-prompt");
    let tm = Mux::tm(&scratch, "tm");
    create_windows(&tm, "A", &["a1", "a2", "a3"]);
    create_windows(&tm, "B", &["b1", "b2"]);
    let mut client = attach(&tm, "B", "prompt");
    let baseline = client.terminal_baseline();

    send(&mut client, b"\x01/");
    send_text(&mut client, "split-window -h");
    send(&mut client, b"\r");
    assert_eq!(
        wait_value(
            &tm,
            &["display-message", "-p", "-t", "B:1", "#{window_panes}"],
            "2"
        ),
        "2"
    );
    assert_eq!(
        tm.body(&["display-message", "-p", "-t", "A:1", "#{window_panes}"]),
        "1"
    );

    send(&mut client, b"\x01/");
    send_text(&mut client, "split-window -h -t A:2");
    send(&mut client, b"\r");
    assert_eq!(
        wait_value(
            &tm,
            &["display-message", "-p", "-t", "A:2", "#{window_panes}"],
            "2"
        ),
        "2"
    );
    assert_eq!(
        tm.body(&["display-message", "-p", "-t", "B:1", "#{window_panes}"]),
        "2"
    );

    let session_id = tm.body(&["display-message", "-p", "-t", "A", "#{session_id}"]);
    send(&mut client, b"\x01/");
    send_text(&mut client, &format!("select-window -t {session_id}:3"));
    send(&mut client, b"\r");
    assert_eq!(
        wait_value(
            &tm,
            &["display-message", "-p", "-t", "A", "#{window_index}"],
            "3"
        ),
        "3"
    );
    assert_eq!(
        tm.body(&["display-message", "-p", "-t", "B", "#{window_index}"]),
        "1"
    );

    send(&mut client, b"\x01/");
    send_text(&mut client, "rename-window -t +1 renamed");
    send(&mut client, b"\r");
    assert_eq!(
        wait_value(
            &tm,
            &["display-message", "-p", "-t", "B:2", "#{window_name}"],
            "renamed"
        ),
        "renamed"
    );
    assert_eq!(window_state(&tm, "A"), "1:a1\n2:a2\n3:a3");
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn break_and_join_bindings_use_client_context() {
    let scratch = Scratch::new("targeting-break");
    let tm = Mux::tm(&scratch, "tm");
    create_windows(&tm, "A", &["a1", "a2"]);
    create_windows(&tm, "B", &["b1", "b2"]);
    tm.ok(&["split-window", "-h", "-t", "B:1", "--", "sleep", "30"]);
    let mut client = attach(&tm, "B", "break");
    let baseline = client.terminal_baseline();

    send(&mut client, b"\x01\x0e");
    assert_eq!(
        wait_value(
            &tm,
            &["display-message", "-p", "-t", "B", "#{window_index}"],
            "3"
        ),
        "3"
    );
    assert_eq!(window_state(&tm, "A"), "1:a1\n2:a2");
    assert_eq!(
        tm.body(&["display-message", "-p", "-t", "B:3", "#{window_panes}"]),
        "1"
    );

    send(&mut client, b"\x01m");
    send_text(&mut client, "2");
    send(&mut client, b"\r");
    assert_eq!(
        wait_value(
            &tm,
            &["display-message", "-p", "-t", "B:2", "#{window_panes}"],
            "2"
        ),
        "2"
    );
    assert_eq!(window_state(&tm, "B"), "1:b1\n2:b2");
    assert_eq!(window_state(&tm, "A"), "1:a1\n2:a2");
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn every_effective_binding_from_a_second_session_leaves_the_first_session_alone() {
    let scratch = Scratch::new("targeting-all-bindings");
    let tm = Mux::tm(&scratch, "tm");
    let bindings: &[(&str, &[u8])] = &[
        ("n", b"\x01n"),
        ("p", b"\x01p"),
        ("Enter", b"\x01\r"),
        ("C-s", b"\x01\x13"),
        ("slash", b"\x01/"),
        ("r", b"\x01r"),
        ("C-Left", b"\x01\x1b[1;5D"),
        ("C-Right", b"\x01\x1b[1;5C"),
        ("Left", b"\x01\x1b[D"),
        ("Right", b"\x01\x1b[C"),
        ("backslash", b"\x01\\"),
        ("minus", b"\x01-"),
        ("z", b"\x01z"),
        ("k", b"\x01k"),
        ("m", b"\x01m"),
        ("C-n", b"\x01\x0e"),
        ("percent", b"\x01%"),
        ("quote", b"\x01\""),
        ("h", b"\x01h"),
        ("j", b"\x01j"),
        ("l", b"\x01l"),
        ("x", b"\x01x"),
        ("ampersand", b"\x01&"),
        ("c", b"\x01c"),
        ("b", b"\x01b"),
        ("bracket", b"\x01["),
        ("d", b"\x01d"),
    ];
    for (index, (label, bytes)) in bindings.iter().enumerate() {
        let first = format!("A{index}");
        let second = format!("B{index}");
        create_windows(&tm, &first, &["one", "two"]);
        create_windows(&tm, &second, &["one", "two"]);
        tm.ok(&[
            "split-window",
            "-h",
            "-t",
            &format!("{second}:1"),
            "--",
            "sleep",
            "30",
        ]);
        let mut client = attach(&tm, &second, label);
        let baseline = client.terminal_baseline();
        send(&mut client, bytes);
        if matches!(*label, "n" | "slash" | "r" | "m") {
            send(&mut client, b"\x1b");
        }
        if *label == "p" {
            send(&mut client, b"\x1b");
        }
        if *label == "bracket" {
            tm.ok(&["copy-mode", "-q", "-t", &format!("{second}:1")]);
        }
        if *label == "d" {
            assert_eq!(
                client
                    .wait_for_exit(client.deadline(Duration::from_secs(3)))
                    .expect("detached by binding"),
                ptytest::ExitStatus::Code(0)
            );
            client
                .finish(client.deadline(Duration::from_secs(3)))
                .expect("reap detached binding client");
        } else {
            pty_harness::detach(&mut client, &baseline);
        }
        assert_eq!(
            window_state(&tm, &first),
            "1:one\n2:two",
            "binding {label} changed the other session"
        );
        assert_eq!(
            tm.body(&["display-message", "-p", "-t", &first, "#{window_panes}"]),
            "1"
        );
        assert!(tm.ok(&["has-session", "-t", &first]).is_empty());
    }
}

#[test]
fn two_clients_keep_binding_context_independent() {
    let scratch = Scratch::new("targeting-two-clients");
    let tm = Mux::tm(&scratch, "tm");
    create_windows(&tm, "A", &["a1", "a2", "a3"]);
    create_windows(&tm, "B", &["b1", "b2"]);
    let mut first = attach(&tm, "A", "first-client");
    let first_baseline = first.terminal_baseline();
    let mut second = attach(&tm, "B", "second-client");
    let second_baseline = second.terminal_baseline();

    send(&mut first, b"\x01\x1b[1;5C");
    assert_eq!(
        wait_value(
            &tm,
            &["display-message", "-p", "-t", "A", "#{window_index}"],
            "2"
        ),
        "2"
    );
    assert_eq!(
        tm.body(&["display-message", "-p", "-t", "B", "#{window_index}"]),
        "1"
    );

    send(&mut second, b"\x01\x1b[1;5C");
    assert_eq!(
        wait_value(
            &tm,
            &["display-message", "-p", "-t", "B", "#{window_index}"],
            "2"
        ),
        "2"
    );
    assert_eq!(
        tm.body(&["display-message", "-p", "-t", "A", "#{window_index}"]),
        "2"
    );

    send(&mut first, b"\x01\x1b[1;5D");
    assert_eq!(
        wait_value(
            &tm,
            &["display-message", "-p", "-t", "A", "#{window_index}"],
            "1"
        ),
        "1"
    );
    assert_eq!(
        tm.body(&["display-message", "-p", "-t", "B", "#{window_index}"]),
        "2"
    );
    pty_harness::detach(&mut first, &first_baseline);
    pty_harness::detach(&mut second, &second_baseline);
}

#[test]
fn prompt_errors_are_visible_to_the_invoking_client() {
    let scratch = Scratch::new("targeting-errors");
    let tm = Mux::tm(&scratch, "tm");
    create_windows(&tm, "A", &["a1", "a2"]);
    let mut client = attach(&tm, "A", "errors");
    let baseline = client.terminal_baseline();

    send(&mut client, b"\x01/");
    send_text(&mut client, "definitely-not-a-command");
    send(&mut client, b"\r");
    pty_harness::wait_for_text(&mut client, "unknown command", Duration::from_secs(3));
    assert!(tm.ok(&["has-session", "-t", "A"]).is_empty());
    pty_harness::detach(&mut client, &baseline);
}
