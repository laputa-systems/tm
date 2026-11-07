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
    clipboard: Vec<Vec<u8>>,
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
        Some(raw_pty::osc52_sequences(stream)).filter(|payloads| !payloads.is_empty())
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
    #[cfg(target_os = "linux")]
    let cases = [
        ("tmux-256color", None, true, b"\x1b]52;;YWxwaGEK\x07".as_slice()),
        ("xterm-256color", None, false, b"\x1b]52;;YWxwaGEK\x1b\\".as_slice()),
        ("xterm-256color", Some(PAIR), true, b"\x1b]52;;YWxwaGEK\x07".as_slice()),
    ];
    #[cfg(not(target_os = "linux"))]
    let cases = [
        ("tmux-256color", None, true, b"\x1b]52;;YWxwaGEK\x07".as_slice()),
        ("xterm-256color", Some(PAIR), true, b"\x1b]52;;YWxwaGEK\x07".as_slice()),
    ];
    for (term, features, sync, clipboard) in cases {
        let scratch = Scratch::new("modes-features");
        let tm = Mux::tm(&scratch, "tm").with_term(term);
        let reference = Mux::reference(&scratch, "reference").with_term(term);
        let tm_effects = observe_features(&tm, features);
        let reference_effects = observe_features(&reference, features);
        assert_eq!(reference_effects.sync_used, sync, "{term} {features:?}");
        assert!(reference_effects.sync_balanced, "{term} {features:?}");
        assert_eq!(reference_effects.clipboard, [clipboard.to_vec()], "{term} {features:?}");
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

const UPDATE_PANE: &str = "echo READY; read line; echo UPDATE_ONE; read line; echo UPDATE_TWO; sleep 30";

#[derive(Debug, PartialEq)]
struct FrameObservation {
    pane_output_inside_bracket: bool,
    chrome: Vec<FrameBrackets>,
}

#[derive(Debug, PartialEq)]
struct FrameBrackets {
    bracket_pairs: usize,
    balanced: bool,
    fully_bracketed: bool,
    updates_inside: Vec<bool>,
}

fn frame_brackets(stream: &[u8], updates: &[&[u8]]) -> FrameBrackets {
    let mut depth = 0usize;
    let mut pairs = 0;
    let mut balanced = true;
    let mut outside = 0;
    let mut inside_at = vec![false; stream.len()];
    let mut index = 0;
    while index < stream.len() {
        if stream[index..].starts_with(SYNC_BEGIN) {
            balanced &= depth == 0;
            depth = 1;
            index += SYNC_BEGIN.len();
        } else if stream[index..].starts_with(SYNC_END) {
            balanced &= depth == 1;
            pairs += usize::from(depth == 1);
            depth = 0;
            index += SYNC_END.len();
        } else {
            inside_at[index] = depth == 1;
            outside += usize::from(depth == 0);
            index += 1;
        }
    }
    let updates_inside = updates
        .iter()
        .map(|update| {
            raw_pty::find(stream, update).is_some_and(|at| inside_at[at..at + update.len()].iter().all(|inside| *inside))
        })
        .collect();
    FrameBrackets {
        bracket_pairs: pairs,
        balanced: balanced && depth == 0,
        fully_bracketed: pairs > 0 && outside == 0,
        updates_inside,
    }
}

fn observe_frame_brackets(mux: &Mux, sync: bool) -> FrameObservation {
    mux.ok(&[
        "new-session", "-d", "-s", "S", "-x", "40", "-y", "6", "--", "sh", "-c", UPDATE_PANE,
    ]);
    let mut client = RawClient::spawn(mux, "frames", &["attach-session", "-t", "S"], 6, 40);
    client.wait_for_text("READY", TIMEOUT);
    client.wait_for_text("(S)", TIMEOUT);
    if sync {
        client.wait_for_bytes(raw_pty::Mark::default(), SYNC_END, TIMEOUT);
    }
    let actions: [(&[&str], &str); 4] = [
        (&["send-keys", "-t", "S", "go", "Enter"], "UPDATE_ONE"),
        (&["rename-window", "-t", "S", "RENAMED"], "RENAMED"),
        (&["new-window", "-d", "-t", "S", "-n", "W2", "sleep", "30"], "2:W2"),
        (&["set-option", "-g", "status-left", "LEFT "], "LEFT"),
    ];
    let mut pane_output_inside_bracket = None;
    let mut chrome = Vec::new();
    for (action, text) in actions {
        let mark = client.mark();
        mux.ok(action);
        client.wait_for_text(text, TIMEOUT);
        let stream = client.bytes_since(mark);
        let frame = frame_brackets(&stream, &[text.as_bytes()]);
        if pane_output_inside_bracket.is_none() {
            pane_output_inside_bracket = Some(frame.updates_inside[0]);
        } else {
            chrome.push(frame);
        }
    }
    assert_eq!(client.detach(), Some(0));
    FrameObservation {
        pane_output_inside_bracket: pane_output_inside_bracket.expect("pane output action ran"),
        chrome,
    }
}

#[test]
fn redraw_frames_are_bracketed_when_sync_is_present_like_the_reference() {
    for (term, bracketed) in [("tmux-256color", true), ("xterm-256color", false)] {
        let scratch = Scratch::new("modes-frames");
        let tm = Mux::tm(&scratch, "tm").with_term(term);
        let reference = Mux::reference(&scratch, "reference").with_term(term);
        let tm_frames = observe_frame_brackets(&tm, bracketed);
        let reference_frames = observe_frame_brackets(&reference, bracketed);
        assert!(!reference_frames.pane_output_inside_bracket, "{term}");
        for frame in &reference_frames.chrome {
            assert!(frame.balanced, "{term}");
            assert_eq!(frame.bracket_pairs, usize::from(bracketed), "{term}");
            assert_eq!(frame.fully_bracketed, bracketed, "{term}");
            assert_eq!(frame.updates_inside, [bracketed], "{term}");
        }
        assert_eq!(
            parity::observation_difference(
                &format!("frame sync brackets for {term}"),
                &tm_frames,
                &reference_frames
            ),
            None
        );
    }
}

fn pane_output_bracketed_after_non_chrome_requests(mux: &Mux) -> bool {
    mux.ok(&[
        "new-session", "-d", "-s", "S", "-x", "40", "-y", "6", "--", "sh", "-c", UPDATE_PANE,
    ]);
    let mut client = RawClient::spawn(mux, "burst", &["attach-session", "-t", "S"], 6, 40);
    client.wait_for_text("READY", TIMEOUT);
    client.wait_for_text("(S)", TIMEOUT);
    client.wait_for_bytes(raw_pty::Mark::default(), SYNC_END, TIMEOUT);
    let mark = client.mark();
    mux.ok(&["send-keys", "-t", "S", "go", "Enter"]);
    for _ in 0..4 {
        mux.ok(&["display-message", "-p", "-t", "S", "x"]);
    }
    client.wait_for_text("UPDATE_ONE", TIMEOUT);
    let stream = client.bytes_since(mark);
    assert_eq!(client.detach(), Some(0));
    frame_brackets(&stream, &[b"UPDATE_ONE"]).updates_inside[0]
}

#[test]
fn pane_output_coalesced_with_non_chrome_requests_is_not_bracketed_like_the_reference() {
    let scratch = Scratch::new("modes-burst");
    let tm = Mux::tm(&scratch, "tm").with_term("tmux-256color");
    let reference = Mux::reference(&scratch, "reference").with_term("tmux-256color");
    assert!(!pane_output_bracketed_after_non_chrome_requests(&reference));
    assert!(!pane_output_bracketed_after_non_chrome_requests(&tm));
}

#[test]
fn terminal_features_lists_as_an_indexed_array_like_the_reference() {
    let scratch = Scratch::new("modes-array-option");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    let listings: [&[&str]; 5] = [
        &["show-options", "-s", "terminal-features"],
        &["show-options", "-s", "-v", "terminal-features"],
        &["show-options", "-g", "terminal-features"],
        &["show-options", "-s", "-q", "terminal-features"],
        &["show-options", "-g", "-v", "terminal-features"],
    ];
    for mux in [&tm, &reference] {
        mux.ok(&["new-session", "-d", "-s", "S", "--", "sleep", "30"]);
    }
    parity::compare_commands("terminal_features_lists_as_an_indexed_array", &tm, &reference, &listings);
    for mux in [&tm, &reference] {
        mux.ok(&["set-option", "-s", "terminal-features", "xterm*:clipboard,foo:RGB"]);
    }
    parity::compare_commands("terminal_features_lists_as_an_indexed_array", &tm, &reference, &listings);
}
