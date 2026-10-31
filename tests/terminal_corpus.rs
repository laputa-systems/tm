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

fn fixture_session(mux: &Mux) {
    mux.ok(&[
        "new-session",
        "-d",
        "-s",
        "S",
        "-x",
        "80",
        "-y",
        "24",
        "--",
        "sh",
        "-c",
        env!("CARGO_BIN_EXE_tm-pty-fixture"),
    ]);
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn pane_terminal_and_set_titles_match_the_reference() {
    let scratch = Scratch::new("corpus-terminal");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    for mux in [&tm, &reference] {
        mux.ok(&[
            "new-session",
            "-d",
            "-s",
            "S",
            "-x",
            "60",
            "-y",
            "8",
            "--",
            "sh",
            "-c",
            "printf 'TERM=%s\\n' \"$TERM\"; printf 'READY\\n'; printf '\\033]2;TITLE_MARKER\\007'; sleep 30",
        ]);
        mux.capture_until("S", "READY", Duration::from_secs(5));
    }
    let tm_term = tm.capture_until("S", "TERM=", Duration::from_secs(5));
    let reference_term = reference.capture_until("S", "TERM=", Duration::from_secs(5));
    assert!(tm_term.contains("TERM=tmux-256color"), "{tm_term:?}");
    assert_eq!(
        tm_term.trim_end_matches('\n'),
        reference_term.trim_end_matches('\n')
    );

    let tm_title = pty_harness::spawn_attached_with_profile(
        &tm,
        "title-tm",
        &["attach-session", "-t", "S"],
        Size::new(60, 8).expect("size"),
        ptytest::ProtocolProfile::xterm_minimal_v1(),
    );
    let mut tm_title = tm_title;
    pty_harness::wait_for_text(&mut tm_title, "READY", Duration::from_secs(3));
    assert!(!tm_title
        .raw_output()
        .windows(12)
        .any(|window| window == b"TITLE_MARKER"));
}

#[test]
fn device_attribute_and_decrqss_replies_match_the_reference() {
    let scratch = Scratch::new("corpus-queries");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    for mux in [&tm, &reference] {
        fixture_session(mux);
    }
    let cases = [
        ("da1", "DA1_ACK", hex(b"\x1b[?1;2c")),
        ("da2", "DA2_ACK", hex(b"\x1b[>84;0;0c")),
        ("decrqss", "DECRQSS_ACK", hex(b"\x1bP0$r\x1b\\")),
    ];
    for (command, label, expected) in cases {
        for mux in [&tm, &reference] {
            mux.ok(&["send-keys", "-t", "S", command, "Enter"]);
        }
        let tm_ack = tm.capture_until("S", label, Duration::from_secs(3));
        let reference_ack = reference.capture_until("S", label, Duration::from_secs(3));
        assert!(
            tm_ack.contains(&format!("{label}:{expected}")),
            "{label}: {tm_ack:?}"
        );
        assert!(
            reference_ack.contains(&format!("{label}:{expected}")),
            "{label}: {reference_ack:?}"
        );
    }
}

#[test]
fn vt100_corpus_rendering_matches_the_reference() {
    let scratch = Scratch::new("corpus-render");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    for mux in [&tm, &reference] {
        fixture_session(mux);
        mux.ok(&["send-keys", "-t", "S", "corpus", "Enter"]);
        mux.capture_until("S", "CORPUS_DONE", Duration::from_secs(5));
    }
    let tm_plain = tm.ok(&["capture-pane", "-p", "-t", "S"]);
    let reference_plain = reference.ok(&["capture-pane", "-p", "-t", "S"]);
    assert_eq!(
        tm_plain.trim_end_matches('\n'),
        reference_plain.trim_end_matches('\n')
    );
    let tm_escaped = tm.ok(&["capture-pane", "-p", "-e", "-t", "S"]);
    let reference_escaped = reference.ok(&["capture-pane", "-p", "-e", "-t", "S"]);
    assert_eq!(
        rendered_state(&tm_escaped),
        rendered_state(&reference_escaped)
    );
}

fn rendered_state(capture: &str) -> String {
    let mut parser = vt100::Parser::new(24, 80, 0);
    parser.process(capture.trim_end_matches('\n').replace('\n', "\r\n").as_bytes());
    let screen = parser.screen();
    let mut cells = Vec::new();
    for row in 0..24 {
        for col in 0..80 {
            let cell = screen.cell(row, col).expect("corpus cell");
            cells.push(format!(
                "{:?}/{:?}/{:?}/{}/{}/{}/{}/{}",
                cell.contents(),
                cell.fgcolor(),
                cell.bgcolor(),
                cell.bold(),
                cell.dim(),
                cell.italic(),
                cell.underline(),
                cell.inverse()
            ));
        }
    }
    format!(
        "{:?}",
        (screen.contents(), screen.cursor_position(), cells)
    )
}
