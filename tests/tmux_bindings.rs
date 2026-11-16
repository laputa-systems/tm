#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/raw_pty.rs"]
#[allow(dead_code)]
mod raw_pty;
#[path = "support/attached_pair.rs"]
#[allow(dead_code)]
mod attached_pair;

use std::path::Path;
use std::time::Duration;

use attached_pair::{Pair, Side, TIMEOUT, poll, styled_rows};
use parity::Mux;
use raw_pty::RawClient;

const PREFIX: &[u8] = b"\x01";
const CTRL_LEFT: &[u8] = b"\x1b[1;5D";
const CTRL_RIGHT: &[u8] = b"\x1b[1;5C";
const LEFT: &[u8] = b"\x1b[D";
const RIGHT: &[u8] = b"\x1b[C";
const REPEAT_TIME: Duration = Duration::from_millis(500);

fn split_in_work(side: &Side) {
    let cwd = side.mux.root().parent().expect("scratch root").join("work");
    let cwd = cwd.to_str().expect("utf-8 path");
    side.mux.ok(&["split-window", "-d", "-t", "S:1", "-c", cwd, "--", "sleep", "300"]);
}

fn windows(names: &'static [&'static str]) -> impl Fn(&Mux, &Path) {
    move |mux, cwd| {
        let cwd = cwd.to_str().expect("utf-8 working directory");
        mux.ok(&[
            "new-session", "-d", "-s", "S", "-x", "40", "-y", "11", "-n", names[0], "-c", cwd,
            "--", "sleep", "300",
        ]);
        for name in &names[1..] {
            mux.ok(&["new-window", "-d", "-t", "S:", "-n", name, "-c", cwd, "--", "sleep", "300"]);
        }
    }
}

#[test]
fn window_navigation_bindings_repeat_like_the_reference() {
    struct Case {
        step: &'static str,
        names: &'static [&'static str],
        start: &'static str,
        keys: &'static [&'static [u8]],
        expected: &'static str,
    }
    let cases = [
        Case {
            step: "prefix C-Right", names: &["a", "b", "c"], start: "S:1",
            keys: &[PREFIX, CTRL_RIGHT], expected: "1 a 0\n2 b 1\n3 c 0",
        },
        Case {
            step: "repeated C-Right", names: &["a", "b", "c"], start: "S:1",
            keys: &[PREFIX, CTRL_RIGHT, CTRL_RIGHT], expected: "1 a 0\n2 b 0\n3 c 1",
        },
        Case {
            step: "repeated C-Right wrapping", names: &["a", "b", "c"], start: "S:2",
            keys: &[PREFIX, CTRL_RIGHT, CTRL_RIGHT], expected: "1 a 1\n2 b 0\n3 c 0",
        },
        Case {
            step: "prefix C-Left", names: &["a", "b", "c"], start: "S:1",
            keys: &[PREFIX, CTRL_LEFT], expected: "1 a 0\n2 b 0\n3 c 1",
        },
        Case {
            step: "repeated C-Left", names: &["a", "b", "c"], start: "S:1",
            keys: &[PREFIX, CTRL_LEFT, CTRL_LEFT], expected: "1 a 0\n2 b 1\n3 c 0",
        },
        Case {
            step: "prefix Right swaps forward", names: &["a", "b", "c"], start: "S:2",
            keys: &[PREFIX, RIGHT], expected: "1 a 0\n2 c 0\n3 b 1",
        },
        Case {
            step: "repeated Right", names: &["a", "b", "c"], start: "S:2",
            keys: &[PREFIX, RIGHT, RIGHT], expected: "1 b 1\n2 c 0\n3 a 0",
        },
        Case {
            step: "prefix Left swaps back", names: &["b", "c", "a"], start: "S:1",
            keys: &[PREFIX, LEFT], expected: "1 a 0\n2 c 0\n3 b 1",
        },
        Case {
            step: "repeated Left", names: &["b", "c", "a"], start: "S:1",
            keys: &[PREFIX, LEFT, LEFT], expected: "1 a 0\n2 b 1\n3 c 0",
        },
    ];
    for case in cases {
        let mut pair = Pair::new(case.step, windows(case.names));
        pair.both(|side| { side.mux.ok(&["select-window", "-t", case.start]); });
        pair.press(case.keys, case.step);
        pair.both(|side| {
            assert_eq!(side.mux.body(&["list-windows", "-t", "S", "-F", "#{window_index} #{window_name} #{window_active}"]), case.expected, "{} {:?}", case.step, side.mux.implementation());
        });
    }

    let mut pair = Pair::new("bindings-expiry", windows(&["a", "b", "c"]));
    pair.press(&[PREFIX, CTRL_RIGHT], "start expiry case");
    std::thread::sleep(REPEAT_TIME + Duration::from_millis(150));
    pair.press_unchanged(
        &[CTRL_RIGHT],
        &[PREFIX, RIGHT],
        "C-Right after repeat-time reaches the pane",
    );
    pair.both(|side| {
        assert_eq!(side.mux.body(&["list-windows", "-t", "S", "-F", "#{window_index} #{window_name} #{window_active}"]), "1 a 0\n2 c 0\n3 b 1", "expired repeat {:?}", side.mux.implementation());
    });
}

#[test]
fn window_lifecycle_bindings_match_the_reference() {
    let mut pair = Pair::new("bindings-windows", windows(&["a"]));
    pair.both(|side| split_in_work(side));
    pair.assert_same_state("setup");
    pair.press(&[PREFIX, b"\x0e"], "prefix C-n breaks the pane out");
    pair.press(&[PREFIX, b"c"], "prefix c");
    pair.press(&[PREFIX, b"&"], "prefix &");
}

#[test]
fn pane_bindings_match_the_reference() {
    let mut pair = Pair::new("bindings-panes", windows(&["a", "b"]));
    for (keys, step) in [
        (&[PREFIX, b"%"][..], "prefix %"),
        (&[PREFIX, b"\""], "prefix \""),
        (&[PREFIX, b"h"], "prefix h"),
        (&[PREFIX, b"l"], "prefix l"),
        (&[PREFIX, b"j"], "prefix j"),
        (&[PREFIX, b"z"], "prefix z zooms"),
        (&[PREFIX, b"z"], "prefix z unzooms"),
        (&[PREFIX, b"x"], "prefix x"),
    ] {
        pair.press(keys, step);
    }
}

#[test]
fn directory_splits_keep_the_current_path_and_even_layouts() {
    let mut pair = Pair::new("bindings-splits", |mux, cwd| {
        windows(&["a", "b"])(mux, cwd);
        let sub = cwd.join("sub");
        let sub = sub.to_str().expect("utf-8 path");
        mux.ok(&["new-window", "-t", "S:", "-c", sub, "--", "sleep", "300"]);
    });
    for (keys, step) in [
        (&[PREFIX, b"%"][..], "prefix % keeps the session path"),
        (&[PREFIX, b"\\"], "prefix backslash"),
        (&[PREFIX, b"-"], "prefix -"),
        (&[PREFIX, b"\\"], "prefix backslash again"),
    ] {
        pair.press(keys, step);
    }
}

#[test]
fn kill_pane_with_message_matches_the_reference() {
    let mut pair = Pair::new("bindings-kill", windows(&["a"]));
    pair.both(|side| split_in_work(side));
    pair.press(&[PREFIX, b"k"], "prefix k");
    pair.screens_after("pane killed.", "prefix k");
}

#[test]
fn prompt_bindings_match_the_reference() {
    let mut pair = Pair::new("bindings-prompts", windows(&["a", "b"]));
    pair.both(|side| split_in_work(side));
    pair.both(|side| side.client.write(&[PREFIX, b"/"].concat()));
    pair.both(|side| side.client.wait_for_screen("command prompt", TIMEOUT, |screen| {
        screen.contents().lines().last().is_some_and(|row| row.starts_with(':'))
    }));
    pair.screens_after(":", "prefix / opens the command prompt");
    pair.both(|side| side.client.write(b"rename-window ty"));
    pair.screens_after(":rename-window ty", "partial command at prefix /");
    pair.press(&[b"ped\r"], "command typed at prefix /");
    pair.both(|side| side.client.write(&[PREFIX, b"m"].concat()));
    pair.screens_after("move pane to window #:", "prefix m opens the join prompt");
    pair.press(&[b"2\r"], "join target typed at prefix m");
}

#[test]
fn message_and_overlay_bindings_match_the_reference() {
    let mut pair = Pair::new("bindings-messages", windows(&["a"]));
    pair.both(|side| side.client.write(&[PREFIX, b"\r"].concat()));
    pair.screens_after("tm configuration is compiled in.", "prefix Enter");
    pair.both(|side| {
        side.client.wait_for_screen("message expiry", TIMEOUT, |screen| {
            !screen.contents().contains("compiled in")
        })
    });
    pair.screens_after("(S)", "message expiry");
}

#[test]
fn display_panes_binding_is_a_client_overlay_where_the_reference_has_a_pane_mode() {
    let mut pair = Pair::new("bindings-display-panes", windows(&["a"]));
    pair.both(|side| split_in_work(side));
    pair.assert_same_state("setup");
    let before = styled_rows(&pair.tm.client);
    pair.both(|side| side.client.write(&[PREFIX, b"p"].concat()));
    pair.assert_same_state("prefix p");
    poll("the tm overlay", || (styled_rows(&pair.tm.client) != before).then_some(()));
    pair.reference.client.wait_for_text("40x5", TIMEOUT);
    let text = |client: &RawClient| {
        client
            .screen_rows()
            .iter()
            .map(|row| row.trim_end().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        text(&pair.tm.client),
        ["0", "1", "", "", "", "", "", "", "", "", "", "(S) 1:a (Z)"]
    );
    assert_eq!(
        text(&pair.reference.client),
        [
            "                                    40x5",
            "",
            "                   0",
            "",
            "",
            "────────────────────────────────────────",
            "                                    40x5",
            "",
            "                   1",
            "",
            "",
            "(S) 1:a (Z)"
        ]
    );
}

#[test]
fn copy_mode_binding_matches_the_reference() {
    let mut pair = Pair::new("bindings-copy-mode", windows(&["a"]));
    pair.press(&[PREFIX, b"["], "prefix [");
    pair.screens_after("[0/0]", "prefix [");
}

#[test]
fn detach_binding_matches_the_reference() {
    let mut pair = Pair::new("bindings-detach", windows(&["a"]));
    pair.both(|side| side.client.write(&[PREFIX, b"d"].concat()));
    let tm = pair.tm.client.finish(TIMEOUT);
    let reference = pair.reference.client.finish(TIMEOUT);
    assert_eq!(tm, reference);
    assert_eq!(pair.tm.client.screen_text(), pair.reference.client.screen_text());
    assert!(pair.reference.client.screen_text().contains("[detached (from session S)]"));
    pair.assert_same_state("detach");
}
