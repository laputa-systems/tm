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

use attached_pair::{Pair, Side, TIMEOUT, poll};
use parity::Mux;

const PREFIX: &[u8] = b"\x01";
const DOWN: &[u8] = b"\x1b[B";

fn sessions(mux: &Mux, cwd: &std::path::Path) {
    let cwd = cwd.to_str().expect("utf-8 working directory");
    mux.ok(&["new-session", "-d", "-s", "S", "-x", "40", "-y", "11", "-n", "a", "-c", cwd, "--", "cat"]);
    mux.ok(&["new-window", "-d", "-t", "S:", "-n", "b", "-c", cwd, "--", "sleep", "300"]);
    mux.ok(&["new-session", "-d", "-s", "T", "-x", "40", "-y", "11", "-c", cwd, "--", "sleep", "300"]);
    mux.ok(&["set-buffer", "-b", "one", "first"]);
    mux.ok(&["set-buffer", "-b", "two", "second"]);
}

fn cwd_of(side: &Side) -> &'static str {
    let cwd = side.mux.root().parent().expect("scratch root").join("work");
    Box::leak(cwd.to_str().expect("utf-8 path").to_owned().into_boxed_str())
}

fn rows(side: &Side, count: usize) -> Vec<String> {
    side.client.screen_rows().iter().take(count).map(|row| row.trim_end().to_owned()).collect()
}

fn mode(side: &Side) -> String {
    side.mux.body(&["display-message", "-p", "-t", "S:1", "#{pane_in_mode} #{pane_mode}"])
}

fn windows(side: &Side) -> String {
    side.mux
        .body(&["list-windows", "-t", "S", "-F", "#{window_index}#{?window_active,*,}"])
        .replace('\n', " ")
}

fn open(pair: &Pair, command: &[&str], wait_for: &str) {
    for side in [&pair.tm, &pair.reference] {
        side.mux.run(command);
    }
    pair.reference.client.wait_for_text(wait_for, TIMEOUT);
}

#[test]
fn command_prompt_inserts_raw_input_into_its_template_like_the_reference() {
    let mut pair = Pair::sized("mode-prompt", 12, 40, sessions);
    for value in ["two words", "it's", "a\"b", "100%", "héllo", "x;y"] {
        pair.both(|side| side.client.write(&[PREFIX, b"r"].concat()));
        pair.both(|side| side.client.wait_for_text("rename window:", TIMEOUT));
        pair.press(&[value.as_bytes(), b"\r"], &format!("rename prompt value {value:?}"));
    }
    pair.both(|side| side.client.write(&[PREFIX, b"n"].concat()));
    pair.both(|side| side.client.wait_for_text("name of new window:", TIMEOUT));
    pair.press(&[b"new window\r"], "new window prompt");
}

#[test]
fn display_panes_digit_selects_the_pane_like_the_reference() {
    let mut pair = Pair::sized("mode-panes", 12, 40, sessions);
    pair.both(|side| {
        side.mux.ok(&["split-window", "-d", "-t", "S:1", "-c", cwd_of(side), "--", "sleep", "300"]);
    });
    pair.assert_same_state("setup");
    pair.press(&[PREFIX, b"p", b"1"], "prefix p then 1");
    pair.press(&[PREFIX, b"p", b"0"], "prefix p then 0");
}

#[test]
fn choose_modes_are_client_overlays_in_tm_and_pane_modes_in_tmux() {
    let mut pair = Pair::sized("mode-choose", 12, 40, sessions);
    pair.both(|side| {
        side.client.wait_for_text("(S)", TIMEOUT);
    });

    open(&pair, &["choose-tree", "-t", "S"], "(7)");
    assert_eq!(mode(&pair.tm), "0 ");
    assert_eq!(mode(&pair.reference), "1 tree-mode");
    assert_eq!(
        rows(&pair.tm, 8),
        ["S", "  1: a", "    1.0", "  2: b", "    2.0", "T", "  1: sleep", "    1.0"]
    );
    let expected = [
        "(0) - S: 2 windows (attached)",
        "(1) ├─→ - 1: a*",
        "(2) │   └─→ 0: cat*",
        "(3) └─→ - 2: b",
        "(4)     └─→ 0: sleep*",
        "(5) - T: 1 windows",
        "(6) └─→ - 1: sleep*",
        "(7)     └─→ 0: sleep*",
    ];
    let actual = rows(&pair.reference, 8);
    for (actual, expected) in actual.iter().zip(expected) {
        assert!(
            actual.starts_with(expected),
            "reference row {actual:?} did not start with {expected:?}"
        );
    }
    pair.tm.client.write(&[DOWN, DOWN, DOWN, b"\r"].concat());
    pair.reference.client.write(b"3");
    poll("both clients to select window 2", || {
        (windows(&pair.tm) == "1 2*" && windows(&pair.reference) == "1 2*").then_some(())
    });
    pair.both(|side| {
        side.mux.ok(&["select-window", "-t", "S:1"]);
    });
    pair.tm.client.wait_for_text("(S) 1:a", TIMEOUT);

    let choose_window = pair.tm.mux.run(&["choose-window", "-t", "S"]);
    assert_eq!(choose_window.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&choose_window.stderr).starts_with("tm: unknown command: choose-window"));
    pair.reference.mux.ok(&["choose-window", "-t", "S"]);
    poll("the reference window chooser", || {
        (mode(&pair.reference).ends_with("tree-mode")).then_some(())
    });
    assert_eq!(mode(&pair.tm), "0 ");
    pair.reference.client.write(b"q");
    poll("the reference to leave tree mode", || (mode(&pair.reference) == "0 ").then_some(()));

    open(&pair, &["choose-buffer", "-t", "S"], "(1) one");
    assert_eq!(mode(&pair.tm), "0 ");
    assert_eq!(mode(&pair.reference), "1 buffer-mode");
    assert_eq!(rows(&pair.tm, 2), ["two", "one"]);
    let reference = rows(&pair.reference, 2);
    assert!(reference[0].starts_with("(0) two: ") && reference[0].ends_with(": second"), "{reference:?}");
    assert!(reference[1].starts_with("(1) one: ") && reference[1].ends_with(": first"), "{reference:?}");
    pair.both(|side| side.client.write(b"\r"));
    poll("both panes to receive the chosen buffer", || {
        let pasted = |side: &Side| side.mux.body(&["capture-pane", "-p", "-t", "S:1"]).trim_end() == "second";
        (pasted(&pair.tm) && pasted(&pair.reference) && mode(&pair.reference) == "0 ").then_some(())
    });

    open(&pair, &["choose-client", "-t", "S"], "(0) /dev/");
    assert_eq!(mode(&pair.tm), "0 ");
    assert_eq!(mode(&pair.reference), "1 client-mode");
    assert_eq!(rows(&pair.tm, 1), ["client0: S"]);
    let reference = rows(&pair.reference, 1);
    assert!(reference[0].starts_with("(0) /dev/") && reference[0].ends_with(": session S"), "{reference:?}");
    pair.both(|side| side.client.write(b"\r"));
    let tm = pair.tm.client.finish(TIMEOUT);
    let reference = pair.reference.client.finish(TIMEOUT);
    assert_eq!(tm, reference);
}

fn prompt(pair: &mut Pair, command: &str, step: &str) {
    pair.both(|side| side.client.write(&[PREFIX, b"/"].concat()));
    pair.both(|side| {
        side.client.wait_for_screen("command prompt", TIMEOUT, |screen| {
            screen.contents().lines().last().is_some_and(|row| row.starts_with(':'))
        })
    });
    pair.press(&[command.as_bytes(), b"\r"], step);
}

#[test]
fn omitted_and_relative_targets_resolve_in_the_attached_clients_context() {
    let mut pair = Pair::sized("mode-targeting", 12, 40, sessions);
    pair.press(&[PREFIX, b"c"], "prefix c creates in the attached session");
    pair.press(&[PREFIX, b"%"], "prefix % splits the attached window");
    pair.press(&[PREFIX, b"z"], "prefix z zooms the attached pane");
    prompt(&mut pair, "rename-window q", "prompt rename-window");
    prompt(&mut pair, "new-window -d -n d", "prompt new-window");
    prompt(&mut pair, "select-window -t :+", "prompt relative +");
    prompt(&mut pair, "select-window -t :-", "prompt relative -");
    prompt(&mut pair, "kill-pane", "prompt kill-pane");
}
