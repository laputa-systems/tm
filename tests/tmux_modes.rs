#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/pty_harness.rs"]
#[allow(dead_code)]
mod pty_harness;

use std::time::Duration;

use parity::{Mux, Scratch};
use ptytest::{PtyTest, Size};

fn session(mux: &Mux, command: &str) {
    mux.ok(&[
        "new-session",
        "-d",
        "-s",
        "S",
        "-x",
        "240",
        "-y",
        "30",
        "--",
        env!("CARGO_BIN_EXE_tm-pty-fixture"),
    ]);
    if !command.is_empty() {
        mux.ok(&["send-keys", "-t", "S", command, "Enter"]);
    }
}

fn attach(mux: &Mux, name: &str) -> (PtyTest, ptytest::TerminalBaseline) {
    let mut terminal = pty_harness::spawn_attached(
        mux,
        name,
        &["attach-session", "-t", "S"],
        Size::new(240, 30).expect("size"),
    );
    pty_harness::wait_for_text(&mut terminal, "(S)", Duration::from_secs(3));
    let baseline = terminal.terminal_baseline();
    (terminal, baseline)
}

fn wait(mux: &Mux, text: &str) {
    mux.capture_until("S", text, Duration::from_secs(3));
}

fn send(terminal: &mut PtyTest, bytes: &[u8]) {
    terminal
        .send_bytes(terminal.deadline(Duration::from_secs(3)), bytes)
        .expect("send client bytes");
}

fn count(terminal: &PtyTest, needle: &[u8]) -> usize {
    terminal
        .raw_output()
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

fn spaced(value: &str) -> String {
    value
        .as_bytes()
        .chunks(2)
        .map(|pair| std::str::from_utf8(pair).expect("hex pair"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn hex_tokens(tokens: &[&str]) -> String {
    tokens
        .iter()
        .map(|token| spaced(token))
        .collect::<Vec<_>>()
        .join(" ")
}

fn ack(terminal: &mut PtyTest, label: &str, expected: &str) {
    pty_harness::wait_for_text(
        terminal,
        &format!("{label}:{expected}"),
        Duration::from_secs(3),
    );
}

#[test]
fn extended_pane_receives_all_modified_printable_keys() {
    let scratch = Scratch::new("modes-extended-modifiers");
    let tm = Mux::tm(&scratch, "tm");
    session(&tm, "extkeys");
    wait(&tm, "EXTKEYS_READY");
    let (mut client, baseline) = attach(&tm, "extended-modifiers");
    let input = concat!(
        "\x1b[97;2u",
        "\x1b[97;3u",
        "\x1b[107;5u",
        "\x1b[97;4u",
        "\x1b[97;6u",
        "\x1b[97;7u",
        "\x1b[97;8u",
        "\x1b[233;3u",
    );
    send(&mut client, input.as_bytes());
    send(&mut client, b"\0");
    let expected = hex_tokens(&[
        "61",
        "1b5b39373b3375",
        "1b5b3130373b3575",
        "1b5b39373b3475",
        "1b5b39373b3675",
        "1b5b39373b3775",
        "1b5b39373b3875",
        "1b5b3233333b3375",
    ]);
    ack(&mut client, "EXTKEYS_ACK", &expected);
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn extended_pane_receives_specials_and_special_modifiers() {
    let scratch = Scratch::new("modes-extended-specials");
    let tm = Mux::tm(&scratch, "tm");
    session(&tm, "extkeys");
    wait(&tm, "EXTKEYS_READY");
    let (mut client, baseline) = attach(&tm, "extended-specials");
    let input = concat!(
        "\r",
        "\x1b[13;2u",
        "\t",
        "\x1b[9;2u",
        "\x7f",
        "\x1b[127;2u",
        "\x1b[27;3;97~",
        "\x1b[1;5D",
        "a",
    );
    send(&mut client, input.as_bytes());
    send(&mut client, b"\0");
    let expected = hex_tokens(&[
        "0d",
        "1b5b31333b3275",
        "09",
        "1b5b393b3275",
        "7f",
        "1b5b3132373b3275",
        "1b5b39373b3375",
        "1b5b313b3544",
        "61",
    ]);
    ack(&mut client, "EXTKEYS_ACK", &expected);
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn legacy_pane_receives_the_legacy_encoding() {
    let scratch = Scratch::new("modes-legacy");
    let tm = Mux::tm(&scratch, "tm");
    session(&tm, "legacy-keys");
    wait(&tm, "LEGACY_READY");
    let (mut client, baseline) = attach(&tm, "legacy");
    let input = concat!(
        "\x1b[97;2u",
        "\x1b[97;3u",
        "\x1b[107;5u",
        "\x1b[13;2u",
        "\x1b[233;3u",
        "\x1b[1;5D",
        "\x1b[27;3;97~",
        "\x1b[127;2u",
        "\r",
        "a",
    );
    send(&mut client, input.as_bytes());
    send(&mut client, b"\0");
    let expected = hex_tokens(&[
        "61",
        "1b61",
        "0b",
        "0d",
        "1bc3a9",
        "1b5b313b3544",
        "1b61",
        "7f",
        "0d",
        "61",
    ]);
    ack(&mut client, "LEGACY_ACK", &expected);
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn application_cursor_and_keypad_follow_the_active_pane() {
    let scratch = Scratch::new("modes-cursor");
    let tm = Mux::tm(&scratch, "tm");
    session(&tm, "app-cursor");
    wait(&tm, "APP_CURSOR_READY");
    let (mut client, baseline) = attach(&tm, "cursor");
    client
        .wait_for_quiescence(client.deadline(Duration::from_secs(3)), Duration::from_millis(50))
        .expect("cursor render");
    assert_eq!(count(&client, b"\x1b[?1h"), 1);
    assert_eq!(count(&client, b"\x1b="), 1);

    send(&mut client, b"\x1b[D\x1bOA\0");
    ack(&mut client, "CURSOR_ACK", &hex_tokens(&["1b4f44", "1b4f41"]));
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn switching_to_a_normal_pane_disables_application_modes() {
    let scratch = Scratch::new("modes-cursor-switch");
    let tm = Mux::tm(&scratch, "tm");
    session(&tm, "app-cursor");
    wait(&tm, "APP_CURSOR_READY");
    tm.ok(&[
        "split-window",
        "-d",
        "-v",
        "-t",
        "S",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    let (mut client, baseline) = attach(&tm, "cursor-switch");
    client
        .wait_for_quiescence(client.deadline(Duration::from_secs(3)), Duration::from_millis(50))
        .expect("cursor initial render");
    assert_eq!(count(&client, b"\x1b[?1h"), 1);
    assert_eq!(count(&client, b"\x1b="), 1);
    tm.ok(&["select-pane", "-t", "S:0.1"]);
    client
        .wait_for_quiescence(client.deadline(Duration::from_secs(3)), Duration::from_millis(50))
        .expect("normal pane render");
    assert_eq!(count(&client, b"\x1b[?1l"), 1);
    assert_eq!(count(&client, b"\x1b>"), 1);
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn focus_events_reach_only_a_requesting_pane() {
    let scratch = Scratch::new("modes-focus");
    let tm = Mux::tm(&scratch, "tm");
    session(&tm, "focus");
    wait(&tm, "FOCUS_READY");
    let (mut client, baseline) = attach(&tm, "focus-client");
    send(&mut client, b"\x1b[O\0");
    ack(&mut client, "FOCUS_ACK", &hex_tokens(&["1b5b49", "1b5b4f"]));
    pty_harness::detach(&mut client, &baseline);

    let scratch = Scratch::new("modes-focus-plain");
    let tm = Mux::tm(&scratch, "tm");
    session(&tm, "legacy-keys");
    wait(&tm, "LEGACY_READY");
    let (mut client, baseline) = attach(&tm, "focus-plain");
    send(&mut client, b"\x1b[O\0");
    ack(&mut client, "LEGACY_ACK", "");
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn mouse_routes_only_to_the_pane_that_requested_it() {
    let scratch = Scratch::new("modes-mouse");
    let tm = Mux::tm(&scratch, "tm");
    session(&tm, "app-mouse");
    wait(&tm, "MOUSE_READY");
    tm.ok(&[
        "split-window",
        "-d",
        "-h",
        "-t",
        "S",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    let (mut client, baseline) = attach(&tm, "mouse");
    send(&mut client, b"\x1b[<64;5;2M");
    ack(&mut client, "MOUSE_ACK", "10");
    send(&mut client, b"\x1b[<64;130;2M");
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let mode = tm.body(&["display-message", "-p", "-t", "S:0.1", "#{pane_in_mode}"]);
        if mode == "1" || std::time::Instant::now() >= deadline {
            assert_eq!(mode, "1", "wheel over a non-mouse pane did not enter copy mode");
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn fragmented_focus_and_extended_input_stay_atomic() {
    let scratch = Scratch::new("modes-fragments");
    let tm = Mux::tm(&scratch, "tm");
    session(&tm, "focus");
    wait(&tm, "FOCUS_READY");
    let (mut client, baseline) = attach(&tm, "focus-fragments");
    for byte in b"\x1b[O" {
        send(&mut client, &[*byte]);
    }
    send(&mut client, b"\0");
    ack(&mut client, "FOCUS_ACK", &hex_tokens(&["1b5b49", "1b5b4f"]));
    pty_harness::detach(&mut client, &baseline);

    let scratch = Scratch::new("modes-extended-fragments");
    let tm = Mux::tm(&scratch, "tm");
    session(&tm, "extkeys");
    wait(&tm, "EXTKEYS_READY");
    let (mut client, baseline) = attach(&tm, "extended-fragments");
    let bytes = b"\x1b[97;3u\x1b[1;5D";
    for chunk in bytes.chunks(2) {
        send(&mut client, chunk);
    }
    send(&mut client, b"\0");
    ack(&mut client, "EXTKEYS_ACK", &hex_tokens(&["1b5b39373b3375", "1b5b313b3544"]));
    pty_harness::detach(&mut client, &baseline);
}
