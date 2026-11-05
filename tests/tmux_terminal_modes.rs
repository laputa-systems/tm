#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/pty_harness.rs"]
#[allow(dead_code)]
mod pty_harness;

use std::time::Duration;

use parity::{Mux, Scratch};
use ptytest::{ProtocolProfile, PtyTest, Size};

fn create_fixture_session(mux: &Mux) {
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
        env!("CARGO_BIN_EXE_tm-pty-fixture"),
    ]);
}

fn attach(mux: &Mux, name: &str, profile: ProtocolProfile) -> (PtyTest, ptytest::TerminalBaseline) {
    let size = Size::new(80, 24).expect("constant size");
    let mut terminal = pty_harness::spawn_attached_with_profile(
        mux,
        name,
        &["attach-session", "-t", "S"],
        size,
        profile,
    );
    pty_harness::wait_for_text(&mut terminal, "VT_FIXTURE_READY", Duration::from_secs(3));
    let baseline = terminal.terminal_baseline();
    (terminal, baseline)
}

fn count(terminal: &PtyTest, needle: &[u8]) -> usize {
    terminal
        .raw_output()
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

fn send(terminal: &mut PtyTest, bytes: &[u8]) {
    terminal
        .send_bytes(terminal.deadline(Duration::from_secs(3)), bytes)
        .expect("send client bytes");
}

#[test]
fn attach_owns_modes_once_and_restores_them_on_detach() {
    let scratch = Scratch::new("modes-own");
    let tm = Mux::tm(&scratch, "tm");
    create_fixture_session(&tm);
    let (mut client, baseline) = attach(&tm, "modes", ProtocolProfile::xterm_minimal_v1());
    assert_eq!(count(&client, b"\x1b[?1049h"), 1);
    assert_eq!(count(&client, b"\x1b[?2004h"), 1);
    assert_eq!(count(&client, b"\x1b[?1004h"), 1);
    assert_eq!(count(&client, b"\x1b[?1000h"), 1);
    assert!(count(&client, b"\x1b[?2026h") >= 1);
    assert_eq!(count(&client, b"\x1b[>1u"), 0);

    send(&mut client, b"input\n");
    pty_harness::wait_for_text(&mut client, "INPUT_ACK", Duration::from_secs(3));
    assert_eq!(count(&client, b"\x1b[?1049h"), 1);
    assert_eq!(count(&client, b"\x1b[?2004h"), 1);
    assert_eq!(count(&client, b"\x1b[?1004h"), 1);
    assert_eq!(count(&client, b"\x1b[?1000h"), 1);

    pty_harness::detach(&mut client, &baseline);
    assert_eq!(count(&client, b"\x1b[?1049l"), 1);
    assert_eq!(count(&client, b"\x1b[?2004l"), 1);
    assert_eq!(count(&client, b"\x1b[?1004l"), 1);
    assert_eq!(count(&client, b"\x1b[?1000l"), 1);
}

#[test]
fn repeated_attach_and_detach_reapplies_each_mode_once() {
    let scratch = Scratch::new("modes-repeat");
    let tm = Mux::tm(&scratch, "tm");
    create_fixture_session(&tm);
    let mut enters = 0;
    let mut leaves = 0;
    for index in 0..2 {
        let (mut client, baseline) = attach(
            &tm,
            &format!("repeat-{index}"),
            ProtocolProfile::xterm_minimal_v1(),
        );
        enters += count(&client, b"\x1b[?1049h");
        pty_harness::detach(&mut client, &baseline);
        leaves += count(&client, b"\x1b[?1049l");
    }
    assert_eq!(enters, 2);
    assert_eq!(leaves, 2);
}

#[test]
fn xterm_256color_has_no_sync_but_keeps_common_modes() {
    let scratch = Scratch::new("modes-xterm");
    let tm = Mux::tm(&scratch, "tm").with_term("xterm-256color");
    create_fixture_session(&tm);
    let (mut client, baseline) = attach(&tm, "xterm", ProtocolProfile::xterm_minimal_v1());
    assert_eq!(count(&client, b"\x1b[?1049h"), 1);
    assert_eq!(count(&client, b"\x1b[?2004h"), 1);
    assert_eq!(count(&client, b"\x1b[?1004h"), 1);
    assert_eq!(count(&client, b"\x1b[?1000h"), 1);
    assert_eq!(count(&client, b"\x1b[?2026h"), 0);
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn unknown_term_gets_the_conservative_screen_baseline() {
    let scratch = Scratch::new("modes-unknown");
    let tm = Mux::tm(&scratch, "tm").with_term("tm-parity-unknown");
    create_fixture_session(&tm);
    let (mut client, baseline) = attach(&tm, "unknown", ProtocolProfile::xterm_minimal_v1());
    assert_eq!(count(&client, b"\x1b[?1049h"), 1);
    assert_eq!(count(&client, b"\x1b[?2004h"), 0);
    assert_eq!(count(&client, b"\x1b[?1004h"), 0);
    assert_eq!(count(&client, b"\x1b[?1000h"), 0);
    assert_eq!(count(&client, b"\x1b[?2026h"), 0);
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn runtime_extkeys_feature_uses_the_reference_pair() {
    let scratch = Scratch::new("modes-extkeys");
    let tm = Mux::tm(&scratch, "tm").with_term("xterm-256color");
    create_fixture_session(&tm);
    tm.ok(&[
        "set-option",
        "-s",
        "terminal-features",
        "xterm-256color:extkeys",
    ]);
    let (mut client, baseline) = attach(&tm, "extkeys", ProtocolProfile::disabled());
    assert_eq!(count(&client, b"\x1b[>4;2m"), 1);
    assert_eq!(count(&client, b"\x1b[>1u"), 0);
    pty_harness::detach(&mut client, &baseline);
    assert_eq!(count(&client, b"\x1b[>4m"), 1);
}
