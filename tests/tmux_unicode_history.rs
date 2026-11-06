#![cfg(unix)]

#[path = "support/history.rs"]
#[allow(dead_code)]
mod history;
#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/raw_pty.rs"]
#[allow(dead_code)]
mod raw_pty;

use std::time::{Duration, Instant};

use history::{Pair, reference_pair};
use raw_pty::RawClient;

const TIMEOUT: Duration = Duration::from_secs(10);
const HISTORY_LIMIT: &str = "20";

const MARGIN_LINES: &str = "printf 'abcdefghi\\344\\270\\255Z\\nabcdefgh\\344\\270\\255Y\\nabcdefghi\\360\\237\\230\\200Z\\nabcdefgh\\360\\237\\230\\200Y\\n0123456789\\ne\\314\\201\\344\\270\\255\\n'; printf 'M01\\n'";

const OBSERVATIONS: [&[&str]; 11] = [
    &["capture-pane", "-p", "-t", "S"],
    &["capture-pane", "-p", "-J", "-t", "S"],
    &["capture-pane", "-p", "-e", "-t", "S"],
    &["capture-pane", "-p", "-e", "-J", "-t", "S"],
    &["capture-pane", "-p", "-N", "-t", "S"],
    &["capture-pane", "-p", "-S", "-", "-t", "S"],
    &["capture-pane", "-p", "-S", "-", "-E", "-", "-t", "S"],
    &["capture-pane", "-p", "-e", "-J", "-S", "-", "-t", "S"],
    &["capture-pane", "-p", "-J", "-S", "-3", "-E", "2", "-t", "S"],
    &["display-message", "-p", "-t", "S", "#{cursor_x},#{cursor_y}"],
    &["display-message", "-p", "-t", "S", "#{pane_width}x#{pane_height}"],
];

struct History {
    test: &'static str,
    pair: Pair,
    clients: Option<(RawClient, RawClient)>,
    rows: u16,
}

impl History {
    fn start(test: &'static str, columns: u16, rows: u16, attached: bool) -> Self {
        let pair = reference_pair("unicode");
        for mux in [&pair.tm, &pair.reference] {
            mux.ok(&["new-session", "-d", "-s", "init", "--", "sleep", "300"]);
            mux.ok(&["set-option", "-g", "history-limit", HISTORY_LIMIT]);
            mux.ok(&[
                "new-session",
                "-d",
                "-s",
                "S",
                "-x",
                &columns.to_string(),
                "-y",
                &rows.to_string(),
                "--",
                "sh",
                "-c",
                "stty -echo; printf 'READY\\n'; while IFS= read -r line; do eval \"$line\"; done",
            ]);
            mux.capture_until("S", "READY", TIMEOUT);
        }
        let clients = attached.then(|| {
            let arguments = ["attach-session", "-t", "S"];
            (
                RawClient::spawn(&pair.tm, "tm", &arguments, rows + 1, columns),
                RawClient::spawn(&pair.reference, "reference", &arguments, rows + 1, columns),
            )
        });
        let history = Self { test, pair, clients, rows };
        if history.clients.is_some() {
            history.wait_for_size(columns);
        }
        history
    }

    fn wait_for_size(&self, columns: u16) {
        let want = format!("{columns}x{}", self.rows);
        for mux in [&self.pair.tm, &self.pair.reference] {
            let deadline = Instant::now() + TIMEOUT;
            loop {
                let size = mux.body(&["display-message", "-p", "-t", "S", "#{window_width}x#{window_height}"]);
                if size == want {
                    break;
                }
                assert!(Instant::now() < deadline, "{:?} window size {size}, want {want}", mux.implementation());
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    fn observe(&self, key: &str, step: &str) {
        self.pair
            .observe(key, &OBSERVATIONS)
            .unwrap_or_else(|error| panic!("{}: {step}: {error}", self.test));
    }

    fn run(&self, key: &str, step: &str, line: &str, marker: &str) {
        for mux in [&self.pair.tm, &self.pair.reference] {
            mux.ok(&["send-keys", "-t", "S", "-l", line]);
            mux.ok(&["send-keys", "-t", "S", "Enter"]);
            let captured = mux.capture_until("S", marker, TIMEOUT);
            assert!(captured.contains(marker), "{step}: {:?} never printed {marker}", mux.implementation());
        }
        self.observe(key, step);
    }

    fn resize(&self, key: &str, step: &str, columns: u16) {
        let (tm, reference) = self.clients.as_ref().expect("resize needs attached clients");
        tm.resize(self.rows + 1, columns);
        reference.resize(self.rows + 1, columns);
        self.wait_for_size(columns);
        self.observe(key, step);
    }

    fn command(&self, step: &str, arguments: &[&str]) {
        self.pair
            .both(self.test, arguments)
            .unwrap_or_else(|error| panic!("{}: {step}: {error}", self.test));
    }
}

#[test]
fn wide_characters_wrap_trim_copy_and_capture_like_the_reference() {
    const TEST: &str = "wide_characters_wrap_trim_copy_and_capture";
    const TRIM: &str = "history_limit_trim_chunk";
    const MARGIN: &str = "wide_characters_margin_wrap";
    let history = History::start(TEST, 10, 6, false);
    history.observe(TEST, "start");
    history.run(
        MARGIN,
        "wide at the margin and exactly fitting",
        MARGIN_LINES,
        "M01",
    );
    history.run(
        TEST,
        "trim at history-limit",
        "i=1; while [ $i -le 40 ]; do printf 'L%d abcdef\\344\\270\\255\\360\\237\\230\\200\\n' $i; i=$((i+1)); done; printf 'M02\\n'",
        "M02",
    );
    for (step, arguments) in [
        ("enter copy mode", &["copy-mode", "-t", "S"][..]),
        ("history-top", &["send-keys", "-t", "S", "-X", "history-top"]),
        ("start-of-line", &["send-keys", "-t", "S", "-X", "start-of-line"]),
        ("begin-selection", &["send-keys", "-t", "S", "-X", "begin-selection"]),
        ("down", &["send-keys", "-t", "S", "-X", "cursor-down"]),
        ("down", &["send-keys", "-t", "S", "-X", "cursor-down"]),
        ("right", &["send-keys", "-t", "S", "-X", "cursor-right"]),
        ("right", &["send-keys", "-t", "S", "-X", "cursor-right"]),
        ("end-of-line", &["send-keys", "-t", "S", "-X", "end-of-line"]),
        ("copy-selection", &["send-keys", "-t", "S", "-X", "copy-selection"]),
        ("show-buffer", &["show-buffer"]),
        ("cancel", &["send-keys", "-t", "S", "-X", "cancel"]),
    ] {
        history.command(step, arguments);
    }
    history.observe(TEST, "after copy");
    history.run(TRIM, "print after trimming", "printf 'M03\\n'", "M03");
}

#[test]
fn wide_characters_survive_narrower_and_wider_resizes_like_the_reference() {
    const MARGIN: &str = "wide_characters_margin_wrap";
    const NARROWER: &str = "wide_characters_resize_narrower";
    const WIDER: &str = "wide_characters_resize_wider";
    let history = History::start(
        "wide_characters_survive_narrower_and_wider_resizes",
        10,
        6,
        true,
    );
    history.run(MARGIN, "wide at the margin and exactly fitting", MARGIN_LINES, "M01");
    history.resize(NARROWER, "narrower", 7);
    history.resize(WIDER, "wider", 14);
}

fn copy_mode_actions(history: &History, test: &str, actions: &[&str]) {
    history.command(test, &["copy-mode", "-t", "S"]);
    for action in actions {
        history.command(test, &["send-keys", "-t", "S", "-X", action]);
    }
}

#[test]
fn copy_mode_lines_and_selections_follow_wrapped_rows_like_the_reference() {
    const TEST: &str = "copy_mode_lines_and_selections_follow_wrapped_rows";
    let history = History::start(TEST, 10, 6, false);
    history.run(
        TEST,
        "wrapped line",
        "printf 'abcdefghijklmnopqrstuvwxy\\nsecond\\n'; printf 'M01\\n'",
        "M01",
    );
    copy_mode_actions(
        &history,
        TEST,
        &[
            "history-top",
            "cursor-down",
            "start-of-line",
            "begin-selection",
            "end-of-line",
            "copy-selection",
        ],
    );
    let (tm, reference) = history
        .pair
        .both(TEST, &["show-buffer"])
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(String::from_utf8_lossy(&tm.stdout), "abcdefghijklmnopqrstuvwxy");
    assert_eq!(tm.stdout, reference.stdout);
}

#[test]
fn capture_pane_ignores_the_copy_mode_viewport_like_the_reference() {
    const TEST: &str = "capture_pane_ignores_the_copy_mode_viewport";
    let history = History::start(TEST, 10, 4, false);
    history.run(
        TEST,
        "output beyond one screen",
        "i=1; while [ $i -le 9 ]; do printf 'row%d\\n' $i; i=$((i+1)); done; printf 'M01\\n'",
        "M01",
    );
    let before = history.pair.tm.ok(&["capture-pane", "-p", "-t", "S"]);
    copy_mode_actions(&history, TEST, &["history-top"]);
    let (tm, _) = history
        .pair
        .both(TEST, &["capture-pane", "-p", "-t", "S"])
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(String::from_utf8_lossy(&tm.stdout), before);
}
