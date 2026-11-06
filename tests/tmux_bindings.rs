#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/raw_pty.rs"]
#[allow(dead_code)]
mod raw_pty;

use std::path::Path;
use std::time::{Duration, Instant};

use parity::{Mux, Scratch};
use raw_pty::RawClient;

const TIMEOUT: Duration = Duration::from_secs(3);
const PREFIX: &[u8] = b"\x01";
const CTRL_LEFT: &[u8] = b"\x1b[1;5D";
const CTRL_RIGHT: &[u8] = b"\x1b[1;5C";
const LEFT: &[u8] = b"\x1b[D";
const RIGHT: &[u8] = b"\x1b[C";
const REPEAT_TIME: Duration = Duration::from_millis(500);

struct Side {
    mux: Mux,
    client: RawClient,
}

struct Pair {
    tm: Side,
    reference: Side,
    _scratch: Scratch,
}

fn state(mux: &Mux) -> String {
    [
        mux.body(&[
            "list-sessions",
            "-F",
            "#{session_name} #{session_windows} #{session_attached}",
        ]),
        mux.body(&[
            "list-windows",
            "-a",
            "-F",
            "#{session_name}:#{window_index} #{window_name} #{window_active} #{window_zoomed_flag} #{window_panes}",
        ]),
        mux.body(&[
            "list-panes",
            "-a",
            "-F",
            "#{session_name}:#{window_index}.#{pane_index} #{pane_id} #{pane_active} #{pane_width}x#{pane_height} #{pane_left},#{pane_top} #{pane_current_path} #{pane_in_mode} #{pane_mode}",
        ]),
    ]
    .join("\n--\n")
}

fn poll<T>(what: &str, mut probe: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(value) = probe() {
            return value;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

impl Pair {
    fn new(label: &str, setup: impl Fn(&Mux, &Path)) -> Self {
        let scratch = Scratch::new(label);
        let cwd = scratch.root().join("work");
        std::fs::create_dir_all(cwd.join("sub")).expect("create working directory");
        let cwd = cwd.canonicalize().expect("canonical working directory");
        let sides = [Mux::tm(&scratch, "tm"), Mux::reference(&scratch, "reference")].map(|mux| {
            setup(&mux, &cwd);
            let client = RawClient::spawn(&mux, &format!("{label} {:?}", mux.implementation()), &["attach-session", "-t", "S"], 12, 40);
            client.wait_for_text("(S)", TIMEOUT);
            Side { mux, client }
        });
        let [tm, reference] = sides;
        Self { tm, reference, _scratch: scratch }
    }

    fn both(&mut self, mut action: impl FnMut(&mut Side)) {
        action(&mut self.tm);
        action(&mut self.reference);
    }

    fn assert_same_state(&self, step: &str) {
        let reference = state(&self.reference.mux);
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let tm = state(&self.tm.mux);
            if tm == reference || Instant::now() >= deadline {
                assert_eq!(tm, reference, "state after {step}");
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn press(&mut self, keys: &[&[u8]], step: &str) {
        let before = state(&self.reference.mux);
        let bytes = keys.concat();
        self.both(|side| side.client.write(&bytes));
        poll(&format!("the reference to react to {step}"), || {
            (state(&self.reference.mux) != before).then_some(())
        });
        self.assert_same_state(step);
    }

    fn press_unchanged(&mut self, keys: &[&[u8]], then: &[&[u8]], step: &str) {
        let before = state(&self.reference.mux);
        let bytes = keys.concat();
        self.both(|side| side.client.write(&bytes));
        let barrier = then.concat();
        self.both(|side| side.client.write(&barrier));
        poll(&format!("the reference to react to the barrier after {step}"), || {
            (state(&self.reference.mux) != before).then_some(())
        });
        self.assert_same_state(step);
    }

    fn screens_after(&mut self, text: &str, step: &str) -> String {
        self.both(|side| side.client.wait_for_text(text, TIMEOUT));
        let trimmed = |rows: Vec<String>| {
            rows.iter().map(|row| row.trim_end().to_owned()).collect::<Vec<_>>()
        };
        let reference = trimmed(self.reference.client.screen_rows());
        let tm = trimmed(self.tm.client.screen_rows());
        assert_eq!(tm, reference, "client screen after {step}");
        reference.join("\n")
    }
}

fn styled_rows(client: &RawClient) -> Vec<String> {
    let color = |color: vt100::Color| match color {
        vt100::Color::Default => '.'.to_string(),
        vt100::Color::Idx(index) => format!("{index:x}"),
        vt100::Color::Rgb(red, green, blue) => format!("#{red:02x}{green:02x}{blue:02x}"),
    };
    client
        .screen_cells()
        .chunks(40)
        .map(|row| {
            let text: String = row
                .iter()
                .map(|(text, ..)| if text.is_empty() { " " } else { text })
                .collect();
            let foreground: String = row.iter().map(|(_, fg, ..)| color(*fg)).collect();
            let background: String = row.iter().map(|(_, _, bg, _)| color(*bg)).collect();
            let inverse: String = row.iter().map(|cell| if cell.3 { 'i' } else { '.' }).collect();
            format!("{}|{foreground}|{background}|{inverse}", text.trim_end())
        })
        .collect()
}

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
    let mut pair = Pair::new("bindings-navigate", windows(&["a", "b", "c"]));
    pair.press(&[PREFIX, CTRL_RIGHT], "prefix C-Right");
    pair.press(&[CTRL_RIGHT], "repeated C-Right");
    pair.press(&[CTRL_RIGHT], "repeated C-Right wrapping");
    pair.press(&[PREFIX, CTRL_LEFT], "prefix C-Left");
    pair.press(&[CTRL_LEFT], "repeated C-Left");
    pair.press(&[PREFIX, RIGHT], "prefix Right swaps forward");
    pair.press(&[RIGHT], "repeated Right");
    pair.press(&[PREFIX, LEFT], "prefix Left swaps back");
    pair.press(&[LEFT], "repeated Left");
    std::thread::sleep(REPEAT_TIME + Duration::from_millis(150));
    pair.press_unchanged(
        &[CTRL_RIGHT],
        &[PREFIX, b"c"],
        "C-Right after repeat-time reaches the pane",
    );
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
