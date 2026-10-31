#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/pty_harness.rs"]
#[allow(dead_code)]
mod pty_harness;

use std::time::{Duration, Instant};

use parity::{Mux, Scratch};
use ptytest::{PtyTest, Size};

fn create_fixture_session(mux: &Mux, session: &str) {
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
        env!("CARGO_BIN_EXE_tm-pty-fixture"),
    ]);
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
    let mut terminal = pty_harness::spawn_attached(
        mux,
        name,
        &["attach-session", "-t", session],
        size,
    );
    pty_harness::wait_for_text(&mut terminal, &format!("({session})"), Duration::from_secs(3));
    let baseline = terminal.terminal_baseline();
    (terminal, baseline)
}

fn send(terminal: &mut PtyTest, bytes: &[u8]) {
    terminal
        .send_bytes(terminal.deadline(Duration::from_secs(3)), bytes)
        .expect("send client bytes");
}

fn wait_screen(terminal: &mut PtyTest, text: &str) {
    pty_harness::wait_for_text(terminal, text, Duration::from_secs(3));
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

#[test]
fn pane_requested_paste_receives_wrapped_payload_once() {
    let scratch = Scratch::new("paste-wrapped");
    let tm = Mux::tm(&scratch, "tm");
    create_fixture_session(&tm, "S");
    let (mut client, baseline) = attach(&tm, "S", "wrapped");
    send(&mut client, b"app-paste\n");
    wait_screen(&mut client, "PASTE_READY");
    let payload = "line one\r\nC-a d C-a k \\x1b héllo".as_bytes();
    let mut bytes = b"\x1b[200~".to_vec();
    bytes.extend_from_slice(payload);
    bytes.extend_from_slice(b"\x1b[201~");
    send(&mut client, &bytes);
    wait_screen(&mut client, &format!("PASTE_ACK:{}", bytes.len()));
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn fragmented_markers_forward_the_exact_payload() {
    let scratch = Scratch::new("paste-fragments");
    let tm = Mux::tm(&scratch, "tm");
    create_fixture_session(&tm, "S");
    let (mut client, baseline) = attach(&tm, "S", "fragments");
    send(&mut client, b"app-paste\n");
    wait_screen(&mut client, "PASTE_READY");
    let payload = "a\r\nb\x01d\x01k\\x1b界".as_bytes();
    let mut bytes = b"\x1b[200~".to_vec();
    bytes.extend_from_slice(payload);
    bytes.extend_from_slice(b"\x1b[201~");
    for byte in &bytes {
        send(&mut client, &[*byte]);
    }
    wait_screen(&mut client, &format!("PASTE_ACK:{}", bytes.len()));
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn pane_without_a_paste_request_gets_payload_only() {
    let scratch = Scratch::new("paste-plain");
    let tm = Mux::tm(&scratch, "tm");
    let payload = b"ab\x01d";
    create_byte_session(&tm, "S", payload.len());
    let (mut client, baseline) = attach(&tm, "S", "plain");
    let mut bytes = b"\x1b[200~".to_vec();
    bytes.extend_from_slice(payload);
    bytes.extend_from_slice(b"\x1b[201~");
    send(&mut client, &bytes);
    wait_hex(&tm, "S", "61 62 01 64");
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn paste_buffer_brackets_only_for_a_requesting_pane() {
    let scratch = Scratch::new("paste-buffer");
    let tm = Mux::tm(&scratch, "tm");
    create_fixture_session(&tm, "S");
    tm.ok(&["set-buffer", "hello"]);
    let (mut client, baseline) = attach(&tm, "S", "buffer");
    send(&mut client, b"app-paste\n");
    wait_screen(&mut client, "PASTE_READY");
    tm.ok(&["paste-buffer", "-p", "-t", "S"]);
    wait_screen(&mut client, "PASTE_ACK:17");
    pty_harness::detach(&mut client, &baseline);

    let scratch = Scratch::new("paste-buffer-plain");
    let tm = Mux::tm(&scratch, "tm");
    create_byte_session(&tm, "S", 2);
    tm.ok(&["set-buffer", "ab"]);
    let (mut client, baseline) = attach(&tm, "S", "buffer-plain");
    tm.ok(&["paste-buffer", "-p", "-t", "S"]);
    wait_hex(&tm, "S", "61 62");
    pty_harness::detach(&mut client, &baseline);
}
