#![cfg(unix)]

#[path = "support/attached_pair.rs"]
#[allow(dead_code)]
mod attached_pair;
#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/raw_pty.rs"]
#[allow(dead_code)]
mod raw_pty;
#[path = "support/split_input.rs"]
#[allow(dead_code)]
mod split_input;

use attached_pair::Pair;
use parity::{Mux, Scratch};

const PREFIX: &[u8] = b"\x01";

fn three_windows(mux: &Mux, _: &std::path::Path) {
    mux.ok(&["new-session", "-d", "-s", "S", "-x", "40", "-y", "7", "-n", "a", "--", "sleep", "300"]);
    for name in ["bb", "ccc"] {
        mux.ok(&["new-window", "-d", "-t", "S:", "-n", name, "--", "sleep", "300"]);
    }
    mux.ok(&["split-window", "-d", "-t", "S:1", "--", "sleep", "300"]);
}

fn set(pair: &mut Pair, rows: &[u16], step: &str, arguments: &'static [&'static str]) {
    pair.change_styled_rows(rows, step, |side| {
        side.mux.ok(arguments);
    });
}

#[test]
fn status_row_styles_follow_the_options_like_the_reference() {
    let mut pair = Pair::sized("status-rows", 8, 40, three_windows);
    let bottom = &[7];
    pair.assert_same_styled_rows(bottom, "compiled profile");
    pair.change_styled_rows(bottom, "zoom", |side| {
        side.mux.ok(&["resize-pane", "-Z", "-t", "S:1"]);
    });
    pair.change_styled_rows(bottom, "select window", |side| {
        side.mux.ok(&["select-window", "-t", "S:2"]);
    });
    set(&mut pair, bottom, "status-bg", &["set-option", "-g", "status-bg", "colour208"]);
    set(&mut pair, bottom, "status-fg", &["set-option", "-g", "status-fg", "brightred"]);
    set(&mut pair, bottom, "status-bg blue", &["set-option", "-g", "status-bg", "blue"]);
    set(&mut pair, bottom, "status-fg colour33", &["set-option", "-g", "status-fg", "colour33"]);
    set(&mut pair, bottom, "status-left", &["set-option", "-g", "status-left", "[#S|#I] "]);
    set(&mut pair, bottom, "status-left-length", &["set-option", "-g", "status-left-length", "4"]);
    set(&mut pair, bottom, "status-left-length 32", &["set-option", "-g", "status-left-length", "32"]);
    set(
        &mut pair,
        bottom,
        "status-left prefix highlight",
        &["set-option", "-g", "status-left", "#{?client_prefix,#[fg=yellow],}(#S) "],
    );
    pair.change_styled_rows(bottom, "prefix pressed", |side| side.client.write(PREFIX));
    pair.change_styled_rows(bottom, "prefix released", |side| side.client.write(b"q"));
    set(&mut pair, bottom, "status-right", &["set-option", "-g", "status-right", "R#{window_name}"]);
    set(
        &mut pair,
        bottom,
        "window-status-format",
        &["set-option", "-gw", "window-status-format", "#I-#W#{?window_zoomed_flag,~,}"],
    );
    set(
        &mut pair,
        bottom,
        "window-status-current-format",
        &["set-option", "-gw", "window-status-current-format", "#[fg=red]<#I:#W>"],
    );
    set(&mut pair, &[0, 7], "status-position top", &["set-option", "-g", "status-position", "top"]);
    set(&mut pair, &[0], "session status-left", &["set-option", "-t", "S", "status-left", "SESSION "]);
}

#[test]
fn window_list_overflow_is_truncated_by_the_reference_and_dropped_by_tm() {
    let pair = Pair::sized("status-overflow", 8, 30, |mux, _| {
        mux.ok(&["new-session", "-d", "-s", "S", "-x", "30", "-y", "7", "-n", "sh", "--", "sleep", "300"]);
        for name in ["inpane", "sleep", "idle"] {
            mux.ok(&["new-window", "-d", "-t", "S:", "-n", name, "--", "sleep", "300"]);
        }
    });
    let status = |side: &attached_pair::Side| {
        side.client.wait_for_text("3:sleep", attached_pair::TIMEOUT);
        side.client.screen_rows()[7].trim_end().to_owned()
    };
    assert_eq!(status(&pair.tm), "(S) 1:sh 2:inpane 3:sleep");
    assert_eq!(status(&pair.reference), "(S) 1:sh 2:inpane 3:sleep 4:i>");
}

fn pane_bytes(label: &str, modes: &str, inputs: &[&[u8]]) -> [Vec<Vec<u8>>; 2] {
    let scratch = Scratch::new(label);
    [Mux::tm(&scratch, "tm"), Mux::reference(&scratch, "reference")].map(|mux| {
        let mut sink = split_input::Sink::start(&mux, "S", "sink", modes);
        let mut client = split_input::attach(&mux, "S");
        sink.deliver(&mut client, &[], "attach");
        let observed = inputs
            .iter()
            .map(|input| sink.deliver(&mut client, &[input], "modified key"))
            .collect();
        assert_eq!(client.detach(), Some(0));
        observed
    })
}

#[test]
fn csi_u_format_reaches_extended_panes_and_legacy_panes_stay_legacy() {
    let inputs: &[&[u8]] = &[b"\x1b[98;5u", b"\x1b[13;2u", b"\x1b[27;5;105~", b"\x1b[1;5C"];
    let [tm, reference] = pane_bytes("csi-u-extended", "\\033[>4;2m", inputs);
    assert_eq!(tm, reference, "extended pane");
    assert_eq!(
        reference,
        [&b"\x1b[98;5u"[..], b"\x1b[13;2u", b"\x1b[105;5u", b"\x1b[1;5C"]
    );
    let [tm, reference] = pane_bytes("csi-u-legacy", "", inputs);
    assert_eq!(tm, reference, "legacy pane");
    assert_eq!(
        reference,
        [&b"\x02"[..], b"\r", b"\t", b"\x1b[1;5C"]
    );
}

const COPY_FORMAT: &str = "#{pane_in_mode} #{copy_cursor_x} #{copy_cursor_y} #{selection_present} #{scroll_position}";

fn copy_state(mux: &Mux) -> String {
    let state = mux.body(&["display-message", "-p", "-t", "S", COPY_FORMAT]);
    if state.starts_with('0') { "0".to_owned() } else { state }
}

#[test]
fn emacs_mode_keys_drive_copy_mode_like_the_reference() {
    let mut pair = Pair::sized("emacs-keys", 8, 40, |mux, _| {
        mux.ok(&[
            "new-session", "-d", "-s", "S", "-x", "40", "-y", "7", "--", "sh", "-c",
            "printf 'alpha one\\nwide two\\nomega three\\nfinal four\\n'; sleep 300",
        ]);
        mux.capture_until("S", "final four", attached_pair::TIMEOUT);
    });
    pair.both(|side| side.client.wait_for_text("final four", attached_pair::TIMEOUT));
    let keys: &[(&[u8], &str)] = &[
        (b"\x01[", "prefix ["),
        (b"\x1b<", "M-< history top"),
        (b"\x0e", "C-n"),
        (b"\x06", "C-f"),
        (b"\x06", "C-f again"),
        (b"\x02", "C-b"),
        (b"\x1bf", "M-f"),
        (b"\x1bb", "M-b"),
        (b"\x05", "C-e"),
        (b"\x02", "C-b after C-e"),
        (b"\x10", "C-p"),
        (b"\x00\x05", "C-Space then C-e selects to the line end"),
        (b"\x0e", "C-n extends over lines"),
        (b"\x1b>", "M-> history bottom"),
        (b"\x1bw", "M-w copies and leaves copy mode"),
    ];
    for (bytes, step) in keys {
        let before = copy_state(&pair.reference.mux);
        pair.both(|side| side.client.write(bytes));
        attached_pair::poll(&format!("the reference to react to {step}"), || {
            (copy_state(&pair.reference.mux) != before).then_some(())
        });
        let deadline = std::time::Instant::now() + attached_pair::TIMEOUT;
        loop {
            let tm = copy_state(&pair.tm.mux);
            let reference = copy_state(&pair.reference.mux);
            if tm == reference || std::time::Instant::now() >= deadline {
                assert_eq!(tm, reference, "copy state after {step}");
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    assert_eq!(pair.tm.mux.body(&["show-buffer"]), pair.reference.mux.body(&["show-buffer"]));
    assert_eq!(pair.reference.mux.body(&["show-buffer"]), "ne\nwide two\nomega three\nfinal four");
}

#[test]
fn incremental_search_prompt_differs_from_the_reference() {
    let pair = Pair::sized("copy-search", 8, 40, |mux, _| {
        mux.ok(&[
            "new-session", "-d", "-s", "S", "-x", "40", "-y", "7", "--", "sh", "-c",
            "printf 'alpha one\\nwide two\\nomega three\\n'; sleep 300",
        ]);
        mux.capture_until("S", "omega three", attached_pair::TIMEOUT);
    });
    let mut pair = pair;
    pair.both(|side| {
        side.client.write(b"\x01[");
        side.client.wait_for_text("[0/0]", attached_pair::TIMEOUT);
        side.client.write(b"\x13");
        side.client.wait_for_text("(search", attached_pair::TIMEOUT);
        side.client.write(b"o");
    });
    let prompt = |side: &attached_pair::Side| {
        side.client.wait_for_text(") o", attached_pair::TIMEOUT);
        let rows = side.client.screen_rows();
        [0, 6, 7].map(|row| rows[row].trim_end().to_owned())
    };
    assert_eq!(prompt(&pair.tm), ["alpha one                          [0/0]", "", "(search) o"]);
    assert_eq!(
        prompt(&pair.reference),
        ["alpha one              [0/0] (3 results)", "(search down) o", "(S) 1:sh"]
    );
}
