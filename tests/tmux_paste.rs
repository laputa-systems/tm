#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/pty_harness.rs"]
#[allow(dead_code)]
mod pty_harness;

#[path = "support/raw_pty.rs"]
#[allow(dead_code)]
mod raw_pty;
#[path = "support/split_input.rs"]
#[allow(dead_code)]
mod split_input;

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
fn fragmented_end_after_payload_finishes_the_paste() {
    let scratch = Scratch::new("paste-payload-fragments");
    let tm = Mux::tm(&scratch, "tm");
    create_fixture_session(&tm, "S");
    let (mut client, baseline) = attach(&tm, "S", "payload-fragments");
    send(&mut client, b"app-paste\n");
    wait_screen(&mut client, "PASTE_READY");
    send(&mut client, b"\x1b[200~hello\x1b[20");
    std::thread::sleep(Duration::from_millis(100));
    send(&mut client, b"1~");
    wait_screen(&mut client, "PASTE_ACK:17");
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

#[test]
fn split_bracketed_paste_delivers_the_unsplit_pane_bytes() {
    let scratch = Scratch::new("paste-split");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    let input: &[u8] = b"\x1b[200~ab\x01d\r\n\x1b[x\x1b[201~";
    let mut observed = Vec::new();
    for mux in [&tm, &reference] {
        let mut sink = split_input::Sink::start(mux, "S", "sink", "\\033[?2004h");
        let mut client = split_input::attach(mux, "S");
        let result = split_input::observe(&mut sink, &mut client, input);
        split_input::assert_split_invariant("paste", mux, &result);
        assert_eq!(client.detach(), Some(0));
        observed.push(result);
    }
    assert_eq!(observed[0], observed[1], "tm pane bytes differ from the reference");
    assert_eq!(observed[0].unsplit, input);
}
