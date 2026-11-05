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

use std::time::Duration;

use parity::{Mux, Scratch};
use ptytest::{ProtocolProfile, PtyTest, Size};
use raw_pty::RawClient;

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

const MODES: [(&str, &[u8], &[u8]); 6] = [
    ("alternate screen", b"\x1b[?1049h", b"\x1b[?1049l"),
    ("bracketed paste", b"\x1b[?2004h", b"\x1b[?2004l"),
    ("mouse press", b"\x1b[?1000h", b"\x1b[?1000l"),
    ("mouse drag", b"\x1b[?1002h", b"\x1b[?1002l"),
    ("mouse sgr", b"\x1b[?1006h", b"\x1b[?1006l"),
    ("focus", b"\x1b[?1004h", b"\x1b[?1004l"),
];
const SYNC_BEGIN: &[u8] = b"\x1b[?2026h";
const SYNC_END: &[u8] = b"\x1b[?2026l";
const TIMEOUT: Duration = Duration::from_secs(5);

fn present(stream: &[u8], reset: bool) -> Vec<&'static str> {
    MODES
        .iter()
        .filter(|mode| raw_pty::count(stream, if reset { mode.2 } else { mode.1 }) > 0)
        .map(|mode| mode.0)
        .collect()
}

fn sync_brackets_balanced(stream: &[u8]) -> bool {
    let mut open = false;
    let mut index = 0;
    while index < stream.len() {
        if stream[index..].starts_with(SYNC_BEGIN) {
            if open {
                return false;
            }
            open = true;
            index += SYNC_BEGIN.len();
        } else if stream[index..].starts_with(SYNC_END) {
            open = false;
            index += SYNC_END.len();
        } else {
            index += 1;
        }
    }
    !open
}

fn session_for_raw_attach(mux: &Mux) {
    mux.ok(&[
        "new-session", "-d", "-s", "S", "-x", "80", "-y", "8", "--",
        env!("CARGO_BIN_EXE_tm-pty-fixture"),
    ]);
}

#[derive(Debug, PartialEq)]
struct ModeEffects {
    set_on_attach: Vec<&'static str>,
    sync_used: bool,
    sync_balanced: bool,
    reset_on_detach: Vec<&'static str>,
}

fn observe_modes(mux: &Mux) -> ModeEffects {
    session_for_raw_attach(mux);
    let mut client = RawClient::spawn(mux, "modes", &["attach-session", "-t", "S"], 8, 80);
    client.wait_for_text("VT_FIXTURE_READY", TIMEOUT);
    client.wait_for_bytes(raw_pty::Mark::default(), MODES[5].1, TIMEOUT);
    let attached = client.bytes_since(raw_pty::Mark::default());
    let set_on_attach = present(&attached, false);

    let detach = client.mark();
    assert_eq!(client.detach(), Some(0));
    let stream = client.bytes_since(raw_pty::Mark::default());
    let released = client.bytes_since(detach);
    ModeEffects {
        set_on_attach,
        sync_used: raw_pty::count(&stream, SYNC_BEGIN) > 0,
        sync_balanced: sync_brackets_balanced(&stream),
        reset_on_detach: present(&released, true),
    }
}

#[test]
fn attach_sets_and_detach_resets_each_terminal_mode_like_the_reference() {
    let scratch = Scratch::new("modes-differential");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    let tm_effects = observe_modes(&tm);
    let reference_effects = observe_modes(&reference);
    let all = MODES.iter().map(|mode| mode.0).collect::<Vec<_>>();
    assert_eq!(reference_effects.set_on_attach, all);
    assert_eq!(reference_effects.reset_on_detach, all);
    assert!(reference_effects.sync_used && reference_effects.sync_balanced);
    assert_eq!(
        parity::observation_difference("terminal mode effects", &tm_effects, &reference_effects),
        None
    );
}

#[derive(Debug, PartialEq)]
struct FeatureEffects {
    sync_used: bool,
    sync_balanced: bool,
    clipboard: Vec<String>,
}

fn observe_features(mux: &Mux, features: Option<&str>) -> FeatureEffects {
    mux.ok(&[
        "new-session", "-d", "-s", "S", "-x", "80", "-y", "8", "--", "sh", "-c",
        "printf 'alpha\\nbeta\\n'; sleep 30",
    ]);
    if let Some(features) = features {
        mux.ok(&["set-option", "-s", "terminal-features", features]);
    }
    let mut client = RawClient::spawn(mux, "features", &["attach-session", "-t", "S"], 8, 80);
    client.wait_for_text("beta", TIMEOUT);
    let copied = client.mark();
    mux.ok(&["copy-mode", "-t", "S"]);
    for command in ["history-top", "begin-selection", "cursor-down", "copy-selection-and-cancel"] {
        mux.ok(&["send-keys", "-t", "S", "-X", command]);
    }
    let clipboard = client.wait_for_stream(copied, "OSC 52 clipboard write", TIMEOUT, |stream| {
        Some(raw_pty::osc52_payloads(stream)).filter(|payloads| !payloads.is_empty())
    });
    assert_eq!(client.detach(), Some(0));
    let stream = client.bytes_since(raw_pty::Mark::default());
    FeatureEffects {
        sync_used: raw_pty::count(&stream, SYNC_BEGIN) > 0,
        sync_balanced: sync_brackets_balanced(&stream),
        clipboard,
    }
}

#[test]
fn terminal_features_sync_and_clipboard_select_for_the_term_like_the_reference() {
    const PAIR: &str = "xterm-256color:Sync:clipboard";
    let cases = [
        ("tmux-256color", None, true),
        ("xterm-256color", None, false),
        ("xterm-256color", Some(PAIR), true),
    ];
    for (term, features, sync) in cases {
        let scratch = Scratch::new("modes-features");
        let tm = Mux::tm(&scratch, "tm").with_term(term);
        let reference = Mux::reference(&scratch, "reference").with_term(term);
        let tm_effects = observe_features(&tm, features);
        let reference_effects = observe_features(&reference, features);
        assert_eq!(reference_effects.sync_used, sync, "{term} {features:?}");
        assert!(reference_effects.sync_balanced, "{term} {features:?}");
        assert_eq!(reference_effects.clipboard, ["YWxwaGEK"], "{term} {features:?}");
        assert_eq!(
            parity::observation_difference(
                &format!("terminal feature effects for {term} {features:?}"),
                &tm_effects,
                &reference_effects
            ),
            None
        );
    }
}
