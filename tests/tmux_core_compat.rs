#![cfg(unix)]

//! Safe ports of the core portions of tmux's `regress/` suite.
//!
//! The upstream shell scripts are an excellent behavioral specification, but
//! they assume tmux's complete command surface and often use control mode,
//! formats, hooks, or nested tmux clients. These tests keep the session,
//! target, pane, window, PTY, and VT behavior that tm claims today. Every
//! test owns a private TM_SOCKET and cleans it up through `kill-server`; no
//! tmux executable or tmux socket is consulted.

use std::fs;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(0);

struct Tm {
    socket: String,
}

impl Tm {
    fn new() -> Self {
        let number = NEXT_SOCKET.fetch_add(1, Ordering::Relaxed);
        let socket = std::env::temp_dir().join(format!(
            "tm-tmux-compat-{}-{number}.sock",
            std::process::id()
        ));
        Self {
            socket: socket.to_string_lossy().into_owned(),
        }
    }

    fn run(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_tm"))
            .env("TM_SOCKET", &self.socket)
            .args(arguments)
            .output()
            .expect("run tm compatibility command")
    }

    fn ok(&self, arguments: &[&str]) -> String {
        let output = self.run(arguments);
        assert!(
            output.status.success(),
            "tm command failed: {:?}\n{}",
            arguments,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn fail(&self, arguments: &[&str]) {
        let output = self.run(arguments);
        assert!(
            !output.status.success(),
            "command unexpectedly succeeded: {arguments:?}"
        );
    }

    fn capture(&self, target: &str) -> String {
        self.ok(&["capture-pane", "-t", target])
    }

    fn capture_until_contains(&self, target: &str, expected: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let captured = self.capture(target);
            if captured.contains(expected) || Instant::now() >= deadline {
                return captured;
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Tm {
    fn drop(&mut self) {
        // kill-server does not auto-start a daemon, so cleanup cannot create
        // a new server if setup failed before the first command.
        let _ = self.run(&["kill-server"]);
    }
}

#[test]
fn session_ops_port_the_tmux_session_lifecycle() {
    let tm = Tm::new();

    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "S1",
        "-n",
        "first",
        "--",
        "sh",
        "-c",
        "printf ready; sleep 30",
    ]);
    assert!(tm.ok(&["list-sessions"]).contains("S1: 1 windows"));
    assert_eq!(
        tm.ok(&[
            "new-session",
            "-d",
            "-P",
            "-F",
            "#{session_id}:#{session_name}:#{session_windows}",
            "-s",
            "printed",
        ]),
        "$1:printed:1\n"
    );
    assert_eq!(
        tm.ok(&["new-session", "-A", "-d", "-s", "printed"]),
        "printed\n"
    );
    assert!(tm.ok(&["has-session", "-t", "S1"]).is_empty());
    tm.fail(&["new-session", "-d", "-s", "S1"]);

    tm.ok(&["rename-session", "-t", "S1", "renamed"]);
    assert!(tm.ok(&["has-session", "-t", "renamed"]).is_empty());
    tm.fail(&["has-session", "-t", "S1"]);
    tm.ok(&["kill-session", "-t", "renamed"]);
    tm.fail(&["has-session", "-t", "renamed"]);
}

#[test]
fn session_and_window_option_inheritance_preserves_base_indices_headlessly() {
    let tm = Tm::new();
    tm.ok(&["new-session", "-d", "-E", "-s", "bootstrap"]);
    tm.ok(&["set-option", "-g", "base-index", "100"]);
    tm.ok(&["new-session", "-d", "-s", "indexed", "--", "sleep", "30"]);
    assert_eq!(
        tm.ok(&["list-windows", "-t", "indexed", "-F", "#{window_index}"]),
        "100\n"
    );

    tm.ok(&["set-option", "-t", "indexed", "base-index", "200"]);
    tm.ok(&["new-window", "-d", "-t", "indexed:", "--", "sleep", "30"]);
    assert_eq!(
        tm.ok(&["list-windows", "-t", "indexed", "-F", "#{window_index}"]),
        "100\n200\n"
    );
}

#[test]
fn kill_all_keeps_only_the_explicit_headless_target() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "keep",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "drop",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    tm.ok(&["kill-session", "-a", "-t", "keep"]);
    assert_eq!(tm.ok(&["list-sessions", "-F", "#{session_name}"]), "keep\n");

    tm.ok(&[
        "new-window",
        "-d",
        "-t",
        "keep",
        "-n",
        "one",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    tm.ok(&[
        "new-window",
        "-d",
        "-t",
        "keep",
        "-n",
        "two",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    tm.ok(&["kill-window", "-a", "-t", "keep:1"]);
    assert_eq!(tm.ok(&["list-windows", "-t", "keep"]).lines().count(), 1);
    assert!(tm.ok(&["list-windows", "-t", "keep"]).contains("1: one"));
}

#[test]
fn targets_and_pane_ops_port_the_stable_pane_target_flow() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "P",
        "--",
        "sh",
        "-c",
        "printf first; sleep 30",
    ]);
    tm.ok(&[
        "split-window",
        "-h",
        "-t",
        "P:0.0",
        "--",
        "sh",
        "-c",
        "printf second; sleep 30",
    ]);

    let panes = tm.ok(&["list-panes", "-t", "P:0"]);
    let pane_ids = panes
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(id, _)| id)
        .collect::<Vec<_>>();
    assert_eq!(pane_ids.len(), 2, "unexpected pane listing: {panes:?}");
    assert!(tm.capture(pane_ids[0]).contains("first"));
    assert!(tm.capture(pane_ids[1]).contains("second"));

    tm.ok(&["select-pane", "-R", "-t", pane_ids[0]]);
    assert!(
        tm.ok(&["list-panes", "-t", "P:0"])
            .contains(&format!("{}: 1 *", pane_ids[1].trim_start_matches('%')))
    );
    tm.ok(&["select-pane", "-m", "-T", "marked", "-t", pane_ids[1]]);
    assert_eq!(
        tm.ok(&[
            "display-message",
            "-p",
            "-t",
            pane_ids[1],
            "#{pane_marked}:#{pane_title}"
        ]),
        "1:marked\n"
    );
    tm.ok(&["select-pane", "-M", "-t", pane_ids[1]]);
    assert_eq!(
        tm.ok(&["display-message", "-p", "-t", pane_ids[1], "#{pane_marked}"]),
        "0\n"
    );
    tm.ok(&["swap-pane", "-d", "-s", pane_ids[0], "-t", pane_ids[1]]);
    let swapped = tm.ok(&["list-panes", "-t", "P:0", "-F", "#{pane_index}:#{pane_id}"]);
    assert!(swapped.contains(&format!("0:{}", pane_ids[1])));
    assert!(swapped.contains(&format!("1:{}", pane_ids[0])));
    tm.ok(&["kill-pane", "-t", pane_ids[1]]);
    assert_eq!(tm.ok(&["list-panes", "-t", "P:0"]).lines().count(), 1);
}

#[test]
fn pane_target_tokens_match_headless_tmux_geometry() {
    let tm = Tm::new();
    tm.ok(&["new-session", "-d", "-s", "T", "-x", "40", "-y", "12"]);
    tm.ok(&["split-window", "-h", "-t", "T:0"]);
    tm.ok(&["split-window", "-v", "-t", "T:0.0"]);
    tm.ok(&["split-window", "-v", "-t", "T:0.1"]);

    let pane = |target: &str| {
        tm.ok(&["display-message", "-p", "-t", target, "#{pane_id}"])
            .trim()
            .to_owned()
    };
    let top_left = pane("T:0.{top-left}");
    let top_right = pane("T:0.{top-right}");
    let bottom_left = pane("T:0.{bottom-left}");
    let bottom_right = pane("T:0.{bottom-right}");
    assert_ne!(top_left, top_right);
    assert_ne!(bottom_left, bottom_right);
    assert_eq!(pane("T:0.{top}"), top_left);
    assert_eq!(pane("T:0.{left}"), top_left);
    assert_eq!(pane("T:0.{bottom}"), bottom_left);
    assert_eq!(pane("T:0.{right}"), top_right);

    tm.ok(&["select-pane", "-t", "T:0.0"]);
    assert_eq!(pane("T:0.+"), top_right);
    assert_eq!(pane("T:0.{down-of}"), bottom_left);
    assert_eq!(pane("T:0.{right-of}"), top_right);
    tm.ok(&["select-pane", "-t", "T:0.2"]);
    tm.ok(&["select-pane", "-t", "T:0.0"]);
    assert_eq!(pane("T:0.!"), bottom_left);

    tm.ok(&["select-pane", "-m", "-t", &top_right]);
    assert_eq!(pane("~"), top_right);
    assert_eq!(pane("{marked}"), top_right);
}

#[test]
fn session_and_window_target_tokens_match_headless_tmux() {
    let tm = Tm::new();
    tm.ok(&["new-session", "-d", "-s", "targets", "-x", "40", "-y", "12"]);
    for name in ["editing", "shell", "logs"] {
        tm.ok(&[
            "new-window",
            "-d",
            "-t",
            "targets:",
            "-n",
            name,
            "--",
            "sh",
            "-c",
            "sleep 30",
        ]);
    }
    let window_id = tm
        .ok(&[
            "display-message",
            "-p",
            "-t",
            "targets:editing",
            "#{window_id}",
        ])
        .trim()
        .to_owned();
    let session_id = tm
        .ok(&["display-message", "-p", "-t", "targets:", "#{session_id}"])
        .trim()
        .to_owned();
    let window = |target: &str| {
        tm.ok(&["display-message", "-p", "-t", target, "#{window_name}"])
            .trim()
            .to_owned()
    };

    assert_eq!(window(&format!("{session_id}:")), "0");
    assert_eq!(window(&window_id.to_string()), "editing");
    assert_eq!(window("targets:editi"), "editing");
    assert_eq!(window("targets:sh*"), "shell");
    assert_eq!(window("targets:^"), "0");
    assert_eq!(window("targets:$"), "logs");
    assert_eq!(window("targets:+"), "editing");
    assert_eq!(window("targets:{next}"), "editing");
    assert_eq!(window("targets:-"), "logs");
    assert_eq!(window("targets:{previous}"), "logs");

    tm.ok(&["select-window", "-t", "targets:2"]);
    tm.ok(&["select-window", "-t", "targets:0"]);
    assert_eq!(window("targets:!"), "shell");
    assert_eq!(window("targets:{last}"), "shell");
}

#[test]
fn linked_windows_share_headless_identity_and_unlink_cleanly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "source",
        "--",
        "sh",
        "-c",
        "printf shared; sleep 30",
    ]);
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "destination",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    tm.ok(&["link-window", "-d", "-s", "source:0", "-t", "destination:5"]);
    assert!(tm.capture("destination:5.0").contains("shared"));
    assert_eq!(
        tm.ok(&[
            "display-message",
            "-p",
            "-t",
            "destination:5",
            "#{window_linked}:#{window_linked_sessions}",
        ]),
        "1:2\n"
    );
    tm.ok(&["rename-window", "-t", "destination:5", "shared"]);
    assert_eq!(
        tm.ok(&["display-message", "-p", "-t", "source:0", "#{window_name}"]),
        "shared\n"
    );
    tm.ok(&["unlink-window", "-t", "destination:5"]);
    assert!(tm.ok(&["has-session", "-t", "source"]).is_empty());
    tm.fail(&["unlink-window", "-t", "source:0"]);
    tm.ok(&["unlink-window", "-k", "-t", "source:0"]);
    tm.fail(&["has-session", "-t", "source"]);
}

#[test]
fn grouped_sessions_share_headless_windows_but_track_active_window() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "group-one",
        "-n",
        "shared",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    tm.ok(&["new-session", "-d", "-s", "group-two", "-t", "group-one"]);
    assert_eq!(
        tm.ok(&[
            "list-sessions",
            "-F",
            "#{session_name}:#{session_grouped}:#{session_group_size}:#{session_group_list}",
        ]),
        "group-one:1:2:group-one,group-two\ngroup-two:1:2:group-one,group-two\n"
    );
    tm.ok(&[
        "new-window",
        "-d",
        "-t",
        "group-two:",
        "-n",
        "second",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    assert_eq!(
        tm.ok(&["list-windows", "-t", "group-one", "-F", "#{window_name}"]),
        "shared\nsecond\n"
    );
    tm.ok(&["select-window", "-t", "group-two:1"]);
    assert!(
        tm.ok(&["list-windows", "-t", "group-two"])
            .contains("1: second *")
    );
    assert!(
        tm.ok(&["list-windows", "-t", "group-one"])
            .contains("0: shared *")
    );
    tm.ok(&["kill-session", "-t", "group-two"]);
    assert_eq!(
        tm.ok(&["list-windows", "-t", "group-one"]).lines().count(),
        2
    );
    assert!(
        tm.ok(&[
            "list-sessions",
            "-F",
            "#{session_grouped}:#{session_group_size}"
        ])
        .contains("0:1")
    );
}

#[test]
fn bell_output_sets_and_selection_clears_the_window_bell_flag_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "bell",
        "--",
        "sh",
        "-c",
        "printf '\\007'; sleep 30",
    ]);
    let deadline = Instant::now() + Duration::from_secs(2);
    let bell = loop {
        let value = tm.ok(&["display-message", "-p", "-t", "bell", "#{window_bell_flag}"]);
        if value.trim() == "1" || Instant::now() >= deadline {
            break value;
        }
        thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(bell.trim(), "1");
    assert!(
        tm.ok(&["display-message", "-p", "-t", "bell", "#{window_flags}"])
            .contains('!')
    );
    tm.ok(&["select-window", "-t", "bell:0"]);
    assert_eq!(
        tm.ok(&["display-message", "-p", "-t", "bell", "#{window_bell_flag}"])
            .trim(),
        "0"
    );
    tm.ok(&[
        "new-window",
        "-d",
        "-t",
        "bell:",
        "-n",
        "quiet",
        "--",
        "sh",
        "-c",
        "sleep 0.2; printf '\\007'; sleep 30",
    ]);
    tm.ok(&[
        "set-window-option",
        "-t",
        "bell:quiet",
        "monitor-bell",
        "off",
    ]);
    thread::sleep(Duration::from_millis(500));
    assert_eq!(
        tm.ok(&[
            "display-message",
            "-p",
            "-t",
            "bell:quiet",
            "#{window_bell_flag}",
        ])
        .trim(),
        "0"
    );
}

#[test]
fn window_ops_port_new_select_and_kill_window_behavior() {
    let tm = Tm::new();
    tm.ok(&["new-session", "-d", "-s", "W", "--", "sh", "-c", "sleep 30"]);
    tm.ok(&[
        "new-window",
        "-d",
        "-t",
        "W",
        "-n",
        "second",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    let windows = tm.ok(&["list-windows", "-t", "W"]);
    assert!(windows.contains("0: 0"));
    assert!(windows.contains("1: second"));
    tm.ok(&["select-window", "-t", "W:1"]);
    assert!(tm.ok(&["list-windows", "-t", "W"]).contains("1: second *"));
    tm.ok(&["next-window", "-t", "W"]);
    assert!(tm.ok(&["list-windows", "-t", "W"]).contains("0: 0 *"));
    tm.ok(&["kill-window", "-t", "W:1"]);
    assert_eq!(tm.ok(&["list-windows", "-t", "W"]).lines().count(), 1);
}

#[test]
fn move_window_preserves_headless_window_order_and_renumbering() {
    let tm = Tm::new();
    tm.ok(&["new-session", "-d", "-s", "M", "--", "sh", "-c", "sleep 30"]);
    for name in ["one", "two"] {
        tm.ok(&[
            "new-window",
            "-d",
            "-t",
            "M",
            "-n",
            name,
            "--",
            "sh",
            "-c",
            "sleep 30",
        ]);
    }
    tm.ok(&["move-window", "-d", "-s", "M:2", "-t", "M:7"]);
    assert_eq!(
        tm.ok(&[
            "list-windows",
            "-t",
            "M",
            "-F",
            "#{window_index}:#{window_name}"
        ]),
        "0:0\n1:one\n7:two\n"
    );
    tm.ok(&["move-window", "-d", "-a", "-s", "M:7", "-t", "M:0"]);
    assert_eq!(
        tm.ok(&[
            "list-windows",
            "-t",
            "M",
            "-F",
            "#{window_index}:#{window_name}"
        ]),
        "0:0\n1:two\n2:one\n"
    );
    tm.ok(&["move-window", "-r", "-t", "M:"]);
    assert_eq!(
        tm.ok(&[
            "list-windows",
            "-t",
            "M",
            "-F",
            "#{window_index}:#{window_name}"
        ]),
        "0:0\n1:two\n2:one\n"
    );
    tm.ok(&["set-option", "-t", "M", "base-index", "5"]);
    tm.ok(&["move-window", "-r", "-t", "M:"]);
    assert_eq!(
        tm.ok(&[
            "list-windows",
            "-t",
            "M",
            "-F",
            "#{window_index}:#{window_name}",
        ]),
        "5:0\n6:two\n7:one\n"
    );
    tm.ok(&["set-option", "-t", "M", "renumber-windows", "on"]);
    tm.ok(&["kill-window", "-t", "M:6"]);
    assert_eq!(
        tm.ok(&[
            "list-windows",
            "-t",
            "M",
            "-F",
            "#{window_index}:#{window_name}",
        ]),
        "5:0\n6:one\n"
    );
    tm.ok(&["set-option", "-t", "M", "base-index", "0"]);
    tm.ok(&["move-window", "-r", "-t", "M:"]);
    assert_eq!(
        tm.ok(&[
            "list-windows",
            "-t",
            "M",
            "-F",
            "#{window_index}:#{window_name}",
        ]),
        "0:0\n1:one\n"
    );
}

#[test]
fn input_regressions_port_vt100_capture_expectations() {
    let tm = Tm::new();
    let cases = [
        ("backspace", "printf 'abc\\bd;'; sleep 30", "abd;"),
        (
            "erase",
            "printf 'ABCDEFGH\\r\\033[3C\\033[2X'; sleep 30",
            "ABC  FGH",
        ),
        (
            "cursor",
            "printf 'A\\033[3;5HB\\033[2GC\\033[2D!'; sleep 30",
            "A",
        ),
        ("unicode", "printf 'AあB'; sleep 30", "AあB"),
        (
            "alternate",
            "printf 'MAIN\\033[?1049hALT\\033[?1049lZ\\n'; sleep 30",
            "MAINZ",
        ),
    ];
    for (name, command, expected) in cases {
        tm.ok(&["new-session", "-d", "-s", name, "--", "sh", "-c", command]);
        let captured = tm.capture_until_contains(name, expected);
        assert!(
            captured.contains(expected),
            "capture for {name:?} did not contain {expected:?}: {:?}",
            captured
        );
        tm.ok(&["kill-session", "-t", name]);
    }
}

#[test]
fn input_edit_regressions_match_tmux_headlessly() {
    let tm = Tm::new();
    let cases = [
        (
            "delete-character",
            10,
            3,
            "printf 'abcdef\\r\\033[3C\\033[2PXY\\n'; sleep 30",
            "abcXY",
        ),
        (
            "insert-character",
            10,
            3,
            "printf 'abcdef\\r\\033[3C\\033[2@XY\\n'; sleep 30",
            "abcXYdef",
        ),
        (
            "erase-line",
            10,
            3,
            "printf 'abcdef\\r\\033[3C\\033[KZ\\n'; sleep 30",
            "abcZ",
        ),
        (
            "erase-display",
            10,
            3,
            "printf 'one\\ntwo\\033[2;2H\\033[JX\\n'; sleep 30",
            "tX",
        ),
        (
            "insert-line",
            8,
            4,
            "printf '111\\n222\\n333\\033[2;1H\\033[LAAA\\n'; sleep 30",
            "AAA",
        ),
        (
            "repeat-character",
            10,
            3,
            "printf 'A\\033[4bB\\n'; sleep 30",
            "AAAAAB",
        ),
        (
            "screen-alignment",
            6,
            3,
            "printf '\\033#8'; sleep 30",
            "EEEEEE",
        ),
    ];
    for (name, cols, rows, command, expected) in cases {
        tm.ok(&[
            "new-session",
            "-d",
            "-s",
            name,
            "-x",
            &cols.to_string(),
            "-y",
            &rows.to_string(),
            "--",
            "sh",
            "-c",
            command,
        ]);
        let captured = tm.capture_until_contains(name, expected);
        assert!(
            captured.contains(expected),
            "capture for {name:?} did not contain {expected:?}: {captured:?}"
        );
    }
}

#[test]
fn input_scroll_regressions_match_tmux_headlessly() {
    let tm = Tm::new();
    let cases = [
        (
            "wrap-regression",
            5,
            3,
            "printf 'abcdeF'; sleep 30",
            "abcde",
        ),
        (
            "wrap-last-regression",
            5,
            3,
            "printf 'abcd\\033[5GZQ'; sleep 30",
            "abcdZ",
        ),
        (
            "no-wrap-regression",
            5,
            3,
            "printf '\\033[?7labcdeF'; sleep 30",
            "abcdF",
        ),
        (
            "scroll-up-regression",
            5,
            4,
            "printf '11111\\n22222\\n33333\\n44444\\033[2;3r\\033[3;1HAAAAA\\nBBBBB\\033[r'; sleep 30",
            "AAAAA",
        ),
        (
            "scroll-down-regression",
            5,
            4,
            "printf '11111\\n22222\\n33333\\n44444\\033[2;3r\\033[2;1H\\033[TZZZZZ\\033[r'; sleep 30",
            "ZZZZZ",
        ),
        (
            "reverse-index-regression",
            5,
            4,
            "printf '11111\\n22222\\n33333\\n44444\\033[2;3r\\033[2;1H\\033MZZZZZ\\033[r'; sleep 30",
            "ZZZZZ",
        ),
    ];
    for (name, cols, rows, command, expected) in cases {
        tm.ok(&[
            "new-session",
            "-d",
            "-s",
            name,
            "-x",
            &cols.to_string(),
            "-y",
            &rows.to_string(),
            "--",
            "sh",
            "-c",
            command,
        ]);
        let captured = tm.capture_until_contains(name, expected);
        assert!(
            captured.contains(expected),
            "capture for {name:?} did not contain {expected:?}: {captured:?}"
        );
    }
}

#[test]
fn input_unicode_regressions_match_tmux_headlessly() {
    let tm = Tm::new();
    let cases = [
        (
            "wide-regression",
            10,
            3,
            "printf 'あB\\rX\\n'; sleep 30",
            "X B",
        ),
        (
            "wide-edge-regression",
            5,
            3,
            "printf 'abcあZ\\n'; sleep 30",
            "abcあ",
        ),
        (
            "combining-regression",
            10,
            3,
            "printf 'é\\n'; sleep 30",
            "é",
        ),
        (
            "variation-regression",
            10,
            3,
            "printf '✔️X\\n'; sleep 30",
            "✔️X",
        ),
        ("flag-regression", 10, 3, "printf '🇬🇧X\\n'; sleep 30", "🇬🇧X"),
    ];
    for (name, cols, rows, command, expected) in cases {
        tm.ok(&[
            "new-session",
            "-d",
            "-s",
            name,
            "-x",
            &cols.to_string(),
            "-y",
            &rows.to_string(),
            "--",
            "sh",
            "-c",
            command,
        ]);
        let captured = tm.capture_until_contains(name, expected);
        assert!(
            captured.contains(expected),
            "capture for {name:?} did not contain {expected:?}: {captured:?}"
        );
    }
}

#[test]
fn input_cursor_regressions_match_tmux_headlessly() {
    let tm = Tm::new();
    let cases = [
        (
            "cursor-regression",
            10,
            3,
            "ABCDE\\r\\033[2Cxy\\033[1D!\\033[4GZ\\n",
            "ABxZE",
            "0,1",
        ),
        (
            "save-cursor-regression",
            10,
            3,
            "abc\\0337\\033[2;5HXY\\0338Z\\n",
            "abcZ",
            "0,1",
        ),
        (
            "hvp-regression",
            10,
            4,
            "A\\033[3dB\\033[5GC\\033[2;2fD\\n",
            " B  C",
            "0,3",
        ),
    ];
    for (name, cols, rows, sequence, expected, cursor) in cases {
        tm.ok(&[
            "new-session",
            "-d",
            "-s",
            name,
            "-x",
            &cols.to_string(),
            "-y",
            &rows.to_string(),
            "--",
            "sh",
            "-c",
            &format!("printf '{sequence}'; sleep 30"),
        ]);
        assert!(
            tm.capture_until_contains(name, expected).contains(expected),
            "capture for {name} did not contain {expected:?}"
        );
        assert_eq!(
            tm.ok(&[
                "display-message",
                "-p",
                "-t",
                name,
                "#{cursor_x},#{cursor_y}"
            ]),
            format!("{cursor}\n"),
            "cursor for {name}"
        );
    }
}

#[test]
fn headless_formats_expose_tmux_target_metadata() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "F",
        "-n",
        "main",
        "-x",
        "40",
        "-y",
        "10",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    let pane = tm.ok(&["list-panes", "-F", "#{pane_id}"]);
    assert!(pane.starts_with('%'));
    assert_eq!(
        tm.ok(&[
            "list-windows",
            "-F",
            "#{session_name}:#{window_index}:#{window_name}:#{window_width}x#{window_height}"
        ]),
        "F:0:main:40x10\n"
    );
    assert_eq!(
        tm.ok(&[
            "display-message",
            "-p",
            "-t",
            "F:0.0",
            "#{pane_id} #{pane_index} #{pane_width}x#{pane_height} #{pane_left},#{pane_top}"
        ]),
        format!("{} 0 40x10 0,0\n", pane.trim())
    );
    assert_eq!(
        tm.ok(&[
            "display-message",
            "-p",
            "-t",
            "F:0.0",
            "#{pane_at_top}:#{pane_at_bottom}:#{pane_at_left}:#{pane_at_right}:#{pane_flags}:#{pane_last}:#{pane_start_command_list}",
        ]),
        "1:1:1:1:*:0:'sh' '-c' 'sleep 30'\n"
    );
    assert_eq!(
        tm.ok(&[
            "display-message",
            "-p",
            "-t",
            "F:0.0",
            "#{?#{==:#{window_index},0},yes,no}:##"
        ]),
        "yes:#\n"
    );
}

#[test]
fn capture_pane_preserves_osc8_hyperlinks_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "hyperlink",
        "-x",
        "40",
        "-y",
        "4",
        "--",
        "sh",
        "-c",
        "printf '\\033]8;id=1;https://example.com\\033\\\\linked\\033]8;;\\033\\\\\\n'; sleep 30",
    ]);
    assert!(
        tm.capture_until_contains("hyperlink", "linked")
            .contains("linked")
    );
    let escaped = tm.ok(&["capture-pane", "-p", "-e", "-S0", "-E1", "-t", "hyperlink"]);
    assert_eq!(
        escaped.trim_end_matches('\n'),
        "\u{1b}]8;id=1;https://example.com\u{1b}\\linked\u{1b}]8;;\u{1b}\\"
    );
}

#[test]
fn capture_pane_preserves_trailing_spaces_with_n_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "trailing",
        "-x",
        "10",
        "-y",
        "3",
        "--",
        "sh",
        "-c",
        "printf 'x   \\n'; sleep 30",
    ]);
    assert!(tm.capture_until_contains("trailing", "x").contains('x'));
    assert_eq!(
        tm.ok(&["capture-pane", "-pN", "-S0", "-E0", "-t", "trailing"]),
        "x         \n"
    );
}

#[test]
fn capture_pane_supports_ranges_and_escaped_cell_attributes() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "C",
        "-x",
        "40",
        "-y",
        "4",
        "--",
        "sh",
        "-c",
        "printf '\\033[31;42;1mabc\\033[0;31mdef\\nplain\\n'; sleep 30",
    ]);
    assert!(tm.capture_until_contains("C", "plain").contains("plain"));
    let plain = tm.ok(&["capture-pane", "-p", "-S", "0", "-E", "1", "-t", "C"]);
    assert!(plain.contains("abcdef\nplain"), "plain capture: {plain:?}");
    let escaped = tm.ok(&["capture-pane", "-p", "-e", "-S0", "-E1", "-t", "C"]);
    assert!(escaped.contains("\u{1b}["), "escaped capture: {escaped:?}");
    assert!(
        escaped.contains("abc") && escaped.contains("def"),
        "escaped capture: {escaped:?}"
    );
}

#[test]
fn capture_pane_ranges_include_scrollback_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "history",
        "-x30",
        "-y4",
        "--",
        "sh",
        "-c",
        "i=0; while [ $i -lt 8 ]; do printf 'row%d\\n' $i; i=$((i + 1)); done; sleep 30",
    ]);
    assert!(
        tm.capture_until_contains("history", "row7")
            .contains("row7")
    );
    assert_eq!(
        tm.ok(&[
            "capture-pane",
            "-p",
            "-S",
            "-3",
            "-E",
            "-1",
            "-t",
            "history",
        ])
        .trim_end(),
        "row2\nrow3\nrow4"
    );
    let all = tm.ok(&["capture-pane", "-p", "-S", "-", "-E", "-", "-t", "history"]);
    assert!(
        all.contains("row0") && all.contains("row7"),
        "full capture: {all:?}"
    );
}

#[test]
fn split_window_honors_cell_and_percent_sizes_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "G",
        "-x80",
        "-y24",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    tm.ok(&[
        "split-window",
        "-d",
        "-h",
        "-l",
        "20",
        "-t",
        "G:0.0",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    let widths = tm.ok(&[
        "list-panes",
        "-t",
        "G:0",
        "-F",
        "#{pane_index}:#{pane_width}x#{pane_height}",
    ]);
    assert!(widths.contains("0:59x24"), "pane widths: {widths:?}");
    assert!(widths.contains("1:20x24"), "pane widths: {widths:?}");

    tm.ok(&[
        "split-window",
        "-d",
        "-v",
        "-l",
        "25%",
        "-t",
        "G:0.0",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    let heights = tm.ok(&[
        "list-panes",
        "-t",
        "G:0",
        "-F",
        "#{pane_index}:#{pane_width}x#{pane_height}",
    ]);
    assert!(heights.contains("2:59x6"), "pane heights: {heights:?}");
    assert!(heights.contains("0:59x17"), "pane heights: {heights:?}");
    tm.ok(&["resize-pane", "-x", "20", "-t", "G:0.0"]);
    assert!(
        tm.ok(&[
            "list-panes",
            "-t",
            "G:0",
            "-F",
            "#{pane_index}:#{pane_width}"
        ])
        .contains("0:20")
    );
    tm.ok(&["resize-pane", "-R", "2", "-t", "G:0.0"]);
    assert!(
        tm.ok(&[
            "list-panes",
            "-t",
            "G:0",
            "-F",
            "#{pane_index}:#{pane_width}"
        ])
        .contains("0:22")
    );
    tm.ok(&["resize-pane", "-Z", "-t", "G:0.0"]);
    assert_eq!(
        tm.ok(&[
            "display-message",
            "-p",
            "-t",
            "G:0.0",
            "#{window_zoomed_flag}:#{pane_width}x#{pane_height}",
        ]),
        "1:80x24\n"
    );
    tm.ok(&["resize-pane", "-Z", "-t", "G:0.0"]);
    assert_eq!(
        tm.ok(&[
            "display-message",
            "-p",
            "-t",
            "G:0.0",
            "#{window_zoomed_flag}"
        ]),
        "0\n"
    );
}

#[test]
fn split_window_full_spans_the_perpendicular_axis_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "F",
        "-x80",
        "-y24",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    tm.ok(&[
        "split-window",
        "-d",
        "-v",
        "-t",
        "F:0.0",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    tm.ok(&[
        "split-window",
        "-d",
        "-h",
        "-b",
        "-f",
        "-l",
        "10",
        "-t",
        "F:0.0",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    let panes = tm.ok(&[
        "list-panes",
        "-t",
        "F:0",
        "-F",
        "#{pane_index}:#{pane_width}x#{pane_height}:#{pane_left},#{pane_top}",
    ]);
    assert!(
        panes.contains("2:10x24:0,0"),
        "full pane geometry: {panes:?}"
    );
    assert!(panes.contains("0:69x11"), "target geometry: {panes:?}");
}

#[test]
fn rotate_and_swap_windows_preserve_headless_layout_identity() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "rotate",
        "-x40",
        "-y8",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    tm.ok(&[
        "split-window",
        "-d",
        "-t",
        "rotate:0",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    tm.ok(&[
        "split-window",
        "-d",
        "-t",
        "rotate:0",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    let panes = tm
        .ok(&["list-panes", "-t", "rotate:0", "-F", "#{pane_id}"])
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert_eq!(panes.len(), 3);
    tm.ok(&["rotate-window", "-U", "-t", "rotate:0"]);
    let rotated = tm.ok(&["list-panes", "-t", "rotate:0", "-F", "#{pane_id}"]);
    assert_eq!(
        rotated.lines().collect::<Vec<_>>(),
        [&panes[1], &panes[2], &panes[0]]
    );
    tm.ok(&["rotate-window", "-D", "-t", "rotate:0"]);
    assert_eq!(
        tm.ok(&["list-panes", "-t", "rotate:0", "-F", "#{pane_id}"]),
        panes.join("\n") + "\n"
    );
    tm.ok(&[
        "new-window",
        "-d",
        "-t",
        "rotate",
        "-n",
        "second",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    tm.ok(&["swap-window", "-d", "-s", "rotate:0", "-t", "rotate:1"]);
    assert_eq!(
        tm.ok(&[
            "list-windows",
            "-t",
            "rotate",
            "-F",
            "#{window_index}:#{window_name}"
        ]),
        "0:second\n1:0\n"
    );
}

#[test]
fn break_and_join_pane_move_ptys_between_windows_headlessly() {
    let tm = Tm::new();
    tm.ok(&["new-session", "-d", "-s", "J", "--", "sh", "-c", "sleep 30"]);
    tm.ok(&[
        "split-window",
        "-d",
        "-h",
        "-t",
        "J:0.0",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    let panes = tm.ok(&["list-panes", "-t", "J:0", "-F", "#{pane_id}"]);
    let source = panes.lines().nth(1).expect("second pane");
    let broken = tm.ok(&[
        "break-pane",
        "-d",
        "-P",
        "-F",
        "#{window_index}:#{pane_id}",
        "-n",
        "broken",
        "-s",
        source,
        "-t",
        "J:",
    ]);
    assert_eq!(broken.trim(), format!("1:{source}"));
    assert!(tm.ok(&["list-windows", "-t", "J"]).contains("broken"));
    tm.ok(&["join-pane", "-d", "-v", "-s", "J:broken.0", "-t", "J:0.0"]);
    assert_eq!(tm.ok(&["list-windows", "-t", "J"]).lines().count(), 1);
    assert_eq!(tm.ok(&["list-panes", "-t", "J:0"]).lines().count(), 2);
}

#[test]
fn dead_panes_can_be_respawned_headlessly() {
    let tm = Tm::new();
    tm.ok(&["new-session", "-d", "-s", "R", "--", "sleep", "30"]);
    tm.ok(&["set-option", "-g", "remain-on-exit", "on"]);
    tm.ok(&["respawn-pane", "-k", "-t", "R:0.0", "--", "true"]);
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if tm
            .ok(&["display-message", "-p", "-t", "R:0.0", "#{pane_dead}"])
            .trim()
            == "1"
        {
            break;
        }
        assert!(Instant::now() < deadline, "pane did not become dead");
        thread::sleep(Duration::from_millis(20));
    }
    tm.ok(&[
        "respawn-pane",
        "-t",
        "R:0.0",
        "--",
        "sh",
        "-c",
        "printf respawned; sleep 30",
    ]);
    assert!(
        tm.capture_until_contains("R:0.0", "respawned")
            .contains("respawned")
    );
}

#[test]
fn exited_panes_are_removed_by_default_headlessly() {
    let tm = Tm::new();
    tm.ok(&["new-session", "-d", "-s", "exit-default", "--", "true"]);
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if tm
            .run(&["has-session", "-t", "exit-default"])
            .status
            .success()
        {
            assert!(
                Instant::now() < deadline,
                "default exited pane still has a session"
            );
            thread::sleep(Duration::from_millis(20));
        } else {
            break;
        }
    }
}

#[test]
fn respawn_window_replaces_all_panes_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "rw",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    tm.ok(&[
        "split-window",
        "-d",
        "-t",
        "rw:0",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    assert_eq!(tm.ok(&["list-panes", "-t", "rw:0"]).lines().count(), 2);
    tm.ok(&[
        "respawn-window",
        "-k",
        "-t",
        "rw:0",
        "--",
        "sh",
        "-c",
        "printf replaced; sleep 30",
    ]);
    assert_eq!(tm.ok(&["list-panes", "-t", "rw:0"]).lines().count(), 1);
    assert_eq!(
        tm.ok(&["display-message", "-p", "-t", "rw:0.0", "#{pane_dead}"]),
        "0\n"
    );
    assert!(
        tm.capture_until_contains("rw:0.0", "replaced")
            .contains("replaced")
    );
}

#[test]
fn new_window_supports_explicit_indices_force_and_print_formats_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "nw",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    assert_eq!(
        tm.ok(&[
            "new-window",
            "-d",
            "-t",
            "nw:9",
            "-n",
            "nine",
            "-P",
            "-F",
            "#{window_index}:#{window_name}",
            "--",
            "sh",
            "-c",
            "sleep 30",
        ]),
        "9:nine\n"
    );
    tm.fail(&[
        "new-window",
        "-d",
        "-t",
        "nw:9",
        "-n",
        "duplicate",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    tm.ok(&[
        "new-window",
        "-d",
        "-k",
        "-t",
        "nw:9",
        "-n",
        "replaced",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    assert!(
        tm.ok(&[
            "list-windows",
            "-t",
            "nw",
            "-F",
            "#{window_index}:#{window_name}"
        ])
        .contains("9:replaced")
    );

    tm.ok(&[
        "new-window",
        "-d",
        "-a",
        "-t",
        "nw:0",
        "-n",
        "after",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    assert!(
        tm.ok(&[
            "list-windows",
            "-t",
            "nw",
            "-F",
            "#{window_index}:#{window_name}"
        ])
        .contains("1:after")
    );
    tm.ok(&["select-window", "-t", "nw:0"]);
    tm.ok(&["new-window", "-S", "-d", "-t", "nw:", "-n", "after"]);
    assert_eq!(
        tm.ok(&[
            "display-message",
            "-p",
            "-t",
            "nw:",
            "#{window_index}:#{window_name}"
        ]),
        "0:0\n"
    );
}

#[test]
fn empty_panes_never_spawn_or_accept_pty_input_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "empty",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    tm.ok(&["new-window", "-d", "-E", "-t", "empty:", "-n", "blank"]);
    assert_eq!(
        tm.ok(&[
            "display-message",
            "-p",
            "-t",
            "empty:blank.0",
            "#{pane_dead}:#{pane_pid}:#{pane_current_command}",
        ]),
        "0::\n"
    );
    tm.ok(&["send-keys", "-t", "empty:blank.0", "unexpected", "Enter"]);
    assert_eq!(tm.ok(&["capture-pane", "-t", "empty:blank.0"]), "");

    tm.ok(&["split-window", "-d", "-E", "-t", "empty:blank.0"]);
    assert_eq!(
        tm.ok(&["list-panes", "-t", "empty:blank"]).lines().count(),
        2
    );
    let split_empty = tm
        .ok(&[
            "display-message",
            "-p",
            "-t",
            "empty:blank.1",
            "#{pane_pid}",
        ])
        .trim()
        .to_owned();
    assert!(split_empty.is_empty());

    tm.ok(&["respawn-pane", "-k", "-E", "-t", "empty:blank.1"]);
    assert_eq!(
        tm.ok(&[
            "display-message",
            "-p",
            "-t",
            "empty:blank.1",
            "#{pane_dead}:#{pane_pid}",
        ]),
        "0:\n"
    );
}

#[test]
fn show_options_round_trips_global_and_window_state_headlessly() {
    let tm = Tm::new();
    tm.ok(&["new-session", "-d", "-s", "options", "--", "sleep", "30"]);

    tm.ok(&["set-option", "-g", "@words", "two words"]);
    assert_eq!(
        tm.ok(&["show-options", "-g", "@words"]),
        "@words \"two words\"\n"
    );
    assert_eq!(tm.ok(&["show", "-gqv", "@missing"]), "");
    tm.ok(&["set-option", "-g", "display-time", "1234"]);
    assert_eq!(tm.ok(&["show", "-gv", "display-time"]), "1234\n");
    tm.ok(&["set-option", "-g", "status-left", "GLOBAL"]);

    tm.ok(&[
        "set-option",
        "-s",
        "-t",
        "options",
        "status-left",
        "SESSION",
    ]);
    assert_eq!(
        tm.ok(&["show", "-t", "options", "-v", "status-left"]),
        "SESSION\n"
    );
    tm.ok(&["set-option", "-u", "-t", "options", "status-left"]);
    assert_eq!(
        tm.ok(&["show", "-t", "options", "-v", "status-left"]),
        "GLOBAL\n"
    );
    tm.ok(&[
        "set-option",
        "-w",
        "-t",
        "options:0",
        "@window-tag",
        "window",
    ]);
    assert_eq!(
        tm.ok(&[
            "show-window-options",
            "-t",
            "options:0",
            "-v",
            "@window-tag",
        ]),
        "window\n"
    );
    tm.ok(&["split-window", "-d", "-t", "options:0", "--", "sleep", "30"]);
    let pane = tm
        .ok(&["list-panes", "-t", "options:0", "-F", "#{pane_id}"])
        .lines()
        .nth(1)
        .expect("second option pane")
        .to_owned();
    tm.ok(&["set-option", "-p", "-t", &pane, "@pane-tag", "pane"]);
    assert_eq!(
        tm.ok(&["show-options", "-p", "-t", &pane, "-v", "@pane-tag"]),
        "pane\n"
    );

    tm.ok(&["set-window-option", "-g", "mode-keys", "vi"]);
    assert_eq!(tm.ok(&["show-window-options", "-v", "mode-keys"]), "vi\n");
    tm.ok(&["set-window-option", "-t", "options:0", "mode-keys", "emacs"]);
    tm.ok(&["set-window-option", "-u", "-t", "options:0", "mode-keys"]);
    assert_eq!(
        tm.ok(&["show-window-options", "-t", "options:0", "-v", "mode-keys",]),
        "vi\n"
    );
    tm.ok(&[
        "set-window-option",
        "-t",
        "options:0",
        "synchronize-panes",
        "on",
    ]);
    assert_eq!(
        tm.ok(&[
            "display-message",
            "-p",
            "-t",
            "options:0.0",
            "#{pane_synchronized}",
        ]),
        "1\n"
    );
    tm.ok(&[
        "new-window",
        "-d",
        "-t",
        "options:",
        "-n",
        "future",
        "--",
        "sleep",
        "30",
    ]);
    assert_eq!(
        tm.ok(&[
            "show-window-options",
            "-t",
            "options:future",
            "-v",
            "mode-keys",
        ]),
        "vi\n"
    );
    assert!(
        tm.ok(&["show-options", "-g"])
            .lines()
            .any(|line| line == "@words \"two words\"")
    );
}

#[test]
fn environment_commands_reach_subsequently_spawned_panes_headlessly() {
    let tm = Tm::new();
    let name = format!("TM_HEADLESS_ENV_{}", std::process::id());
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "environment",
        "--",
        "sleep",
        "30",
    ]);
    tm.ok(&["set-environment", &name, "from-tm"]);
    assert_eq!(
        tm.ok(&["show-environment", &name]),
        format!("{name}=from-tm\n")
    );
    assert_eq!(
        tm.ok(&[
            "show-environment",
            "-F",
            "#{environment_name}=#{environment_value}",
            &name,
        ]),
        format!("{name}=from-tm\n")
    );
    tm.ok(&[
        "new-window",
        "-d",
        "-t",
        "environment:",
        "--",
        "sh",
        "-c",
        &format!("printf \"${name}\"; sleep 30"),
    ]);
    assert!(
        tm.capture_until_contains("environment:1", "from-tm")
            .contains("from-tm")
    );
    tm.ok(&["set-environment", "-r", &name]);
    tm.fail(&["show-environment", &name]);
}

#[test]
fn pane_current_path_tracks_osc7_directory_reports_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "paths",
        "--",
        "sh",
        "-c",
        "printf '\\033]7;file://localhost/tmp/tm-path\\033\\\\ready'; sleep 30",
    ]);
    assert!(
        tm.capture_until_contains("paths:0.0", "ready")
            .contains("ready")
    );
    assert_eq!(
        tm.ok(&[
            "display-message",
            "-p",
            "-t",
            "paths:0.0",
            "#{pane_current_path}",
        ]),
        "/tmp/tm-path\n"
    );
}

#[test]
fn pipe_pane_captures_pty_output_and_toggles_off_headlessly() {
    let tm = Tm::new();
    let path = std::env::temp_dir().join(format!(
        "tm-pipe-pane-{}-{}.txt",
        std::process::id(),
        NEXT_SOCKET.load(Ordering::Relaxed)
    ));
    let path = path.to_string_lossy().into_owned();
    let _ = fs::remove_file(&path);
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "pipe-pane",
        "--",
        "sh",
        "-c",
        "read line; printf 'OUT:%s\\n' \"$line\"; sleep 30",
    ]);
    tm.ok(&[
        "pipe-pane",
        "-t",
        "pipe-pane:0.0",
        "--",
        &format!("cat > {path}"),
    ]);
    tm.ok(&["send-keys", "-t", "pipe-pane:0.0", "piped", "Enter"]);
    let deadline = Instant::now() + Duration::from_secs(2);
    let output = loop {
        let value = fs::read_to_string(&path).unwrap_or_default();
        if value.contains("OUT:piped") || Instant::now() >= deadline {
            break value;
        }
        thread::sleep(Duration::from_millis(20));
    };
    assert!(output.contains("OUT:piped"), "pipe output: {output:?}");
    tm.ok(&["pipe-pane", "-o", "-t", "pipe-pane:0.0"]);
    let _ = fs::remove_file(&path);
}

#[test]
fn format_modifiers_match_tmux_headlessly_without_extra_dependencies() {
    let tm = Tm::new();
    tm.ok(&["new-session", "-d", "-E", "-s", "formats"]);
    for (name, value) in [
        ("@s", "abcdefghij"),
        ("@path", "/usr/local/bin/foo"),
        ("@name", "window-name"),
        ("@greek", "αβγ"),
        ("@cjk", "中文"),
        ("@sp", "a b$c"),
        ("@sq", "a'b"),
        ("@hash", "a#b"),
        ("@v", "foo:bar"),
        ("@sub", "abABab"),
        ("@slash", "foo/bar foo/"),
        ("@ts", "1000000000"),
        ("@emoji", "😀😀"),
    ] {
        tm.ok(&["set-option", "-g", name, value]);
    }
    tm.ok(&["set-option", "-g", "@rec", "#{E:@rec}"]);

    let cases = [
        ("##", "#"),
        ("#,", ","),
        ("#{s/#:/_/:@v}", "foo_bar"),
        ("#{s/o/#:/:@v}", "f:::bar"),
        ("#{==:@name,window-name}", "1"),
        ("#{<:3,5}", "1"),
        ("#{>=:5,5}", "1"),
        ("#{q:@sp}", r"a\ b\$c"),
        ("#{q/s:@sp}", "'a b$c'"),
        ("#{q/s:@sq}", "'a'\\''b'"),
        ("#{q/e:@hash}", "a##b"),
        ("#{q/a:@sp}", "\"a b$c\""),
        ("#{c:red}", "800000"),
        ("#{c:colour4}", "000080"),
        ("#{c:#7f7f7f}", "7f7f7f"),
        ("#{c/f:red}", "\x1b[31m"),
        ("#{c/b:colour4}", "\x1b[48;5;4m"),
        ("#{n:@greek}", "6"),
        ("#{w:@greek}", "3"),
        ("#{b:@path}", "foo"),
        ("#{d:@path}", "/usr/local/bin"),
        ("#{a:98}", "b"),
        ("#{R:ab,2}", "abab"),
        ("#{p12:@name}", "window-name "),
        ("#{p-12:@name}", " window-name"),
        ("#{=5:@s}", "abcde"),
        ("#{=-5:@s}", "fghij"),
        ("#{=/5/...:@s}", "abcde..."),
        ("#{=2:@cjk}", "中"),
        ("#{=/2/x:@cjk}", "中x"),
        ("#{=/1/x:@cjk}", "x"),
        ("#{=3:#{b:@path}}", "foo"),
        ("#{p6:@cjk}", "中文  "),
        ("#{p-6:@cjk}", "  中文"),
        ("#{n:@emoji}", "8"),
        ("#{w:@emoji}", "4"),
        ("#{n:#{R:x,300}}", "300"),
        ("#{=/3/#,:@s}", "abc,"),
        ("#{=/3/#{l:>}:@s}", "abc>"),
        ("#{e|+|:2,3}", "5"),
        ("#{e|*|f|2:2.5,2}", "5.00"),
        ("#{t/f/%Y:@ts}", "2001"),
        ("#{T:@ts}", "1000000000"),
        ("#{E:@rec}", "__EMPTY__"),
        ("#{t/r:@ts}", "__NONEMPTY__"),
        ("#{m:*foo*,barfoobar}", "1"),
        (r"#{m/r:^[0-9]+\$,12345}", "1"),
        (r"#{m/ri:^ab+\$,ABBB}", "1"),
        ("#{m/z:foo,foobar}", "1"),
        ("#{m/p:ac,abc}", "0,2"),
        ("#{s/[bd]/X/:@s}", "aXcXefghij"),
        ("#{s/A/X/i:@sub}", "XbXBXb"),
        ("#{s/a(.)/\\1x/i:@sub}", "bxBxbx"),
        ("#{s/(.)(.)/\\2\\1/:@s}", "badcfehgji"),
        ("#{s|foo/|bar/|:@slash}", "bar/bar bar/"),
    ];
    for (format, expected) in cases {
        if expected == "__NONEMPTY__" {
            assert!(!tm.ok(&["display-message", "-p", format]).trim().is_empty());
            continue;
        }
        if expected == "__EMPTY__" {
            assert_eq!(tm.ok(&["display-message", "-p", format]), "\n");
            continue;
        }
        assert_eq!(
            tm.ok(&["display-message", "-p", format]),
            format!("{expected}\n"),
            "format {format:?}",
        );
    }
}

#[test]
fn format_name_and_content_modifiers_match_tmux_headlessly() {
    let tm = Tm::new();
    tm.ok(&["new-session", "-d", "-E", "-s", "names"]);
    let names_id = tm
        .ok(&["display-message", "-p", "-t", "names:", "#{session_id}"])
        .trim()
        .to_owned();
    tm.ok(&["rename-window", "-t", "names:0", "knownwin"]);
    tm.ok(&["new-session", "-d", "-E", "-s", "other"]);
    tm.ok(&["new-window", "-d", "-t", "other:", "-n", "sibling"]);
    assert_eq!(
        tm.ok(&[
            "display-message",
            "-p",
            "-t",
            "names:",
            "#{N/s:names}:#{N/s:other}:#{N/s:nosuch}:#{N/w:knownwin}:#{N/w:sibling}:#{N:nosuchwindow}",
        ]),
        "1:1:0:1:0:0\n"
    );
    tm.ok(&["new-window", "-d", "-t", "names:", "-n", "sibling"]);
    assert_eq!(
        tm.ok(&["display-message", "-p", "-t", "names:", "#{N/w:sibling}",]),
        "1\n"
    );

    tm.ok(&["set-option", "-g", "@name", "format.#(ok)"]);
    tm.ok(&["rename-session", "-t", "names", "#{@name}"]);
    let names_window = format!("{names_id}:0");
    let names_pane = format!("{names_id}:0.0");
    tm.ok(&["rename-window", "-t", &names_window, "#{@name}"]);
    tm.ok(&["select-pane", "-T", "title#:.ok", "-t", &names_pane]);
    let server_pid = tm
        .ok(&["display-message", "-p", "-t", &names_pane, "#{pid}"])
        .trim()
        .to_owned();
    assert_eq!(
        tm.ok(&[
            "display-message",
            "-p",
            "-t",
            &names_pane,
            "#{session_name}:#{window_name}:#{pane_title}:#{pid}",
        ]),
        format!("format.#(ok):format.#(ok):title#:.ok:{}\n", server_pid)
    );

    tm.ok(&[
        "new-window",
        "-d",
        "-t",
        &format!("{names_id}:"),
        "-n",
        "",
        "--",
        "sh",
        "-c",
        "printf '\\033]2;osc-title#[fg=red]ok\\007'; sleep 30",
    ]);
    let names_second_pane = format!("{names_id}:2.0");
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let title = tm
            .ok(&[
                "display-message",
                "-p",
                "-t",
                &names_second_pane,
                "#{pane_title}",
            ])
            .trim()
            .to_owned();
        if title == "osc-title#[fg=red]ok" || Instant::now() >= deadline {
            assert_eq!(title, "osc-title#[fg=red]ok");
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        tm.ok(&[
            "display-message",
            "-p",
            "-t",
            &names_second_pane,
            "#{window_name}:#{pane_title}"
        ]),
        ":osc-title#[fg=red]ok\n"
    );

    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "search",
        "-x80",
        "-y10",
        "--",
        "sh",
        "-c",
        "printf 'Zebra_Marker_42\\nsecond row\\n'; sleep 30",
    ]);
    assert!(
        tm.capture_until_contains("search:", "Zebra_Marker_42")
            .contains("Zebra_Marker_42")
    );
    assert_eq!(
        tm.ok(&[
            "display-message",
            "-p",
            "-t",
            "search:",
            "#{C:Zebra_Marker_42}:#{C:Absent_String_999}:#{C/r:Zebra_.*_42}:#{C/i:zebra_marker_42}",
        ]),
        "1:0:1:1\n"
    );

    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "search-history",
        "-x",
        "80",
        "-y",
        "4",
        "--",
        "sh",
        "-c",
        "i=0; while [ $i -lt 8 ]; do printf 'history-row-%d\\n' $i; i=$((i + 1)); done; sleep 30",
    ]);
    assert!(
        tm.capture_until_contains("search-history:", "history-row-7")
            .contains("history-row-7")
    );
    assert_eq!(
        tm.ok(&[
            "display-message",
            "-p",
            "-t",
            "search-history:",
            "#{C:history-row-0}:#{C:history-row-7}:#{C:missing-history-row}",
        ]),
        "1:8:0\n"
    );
}

#[test]
fn format_context_loops_iterate_sessions_windows_and_panes_headlessly() {
    let tm = Tm::new();
    for name in ["zeta", "alpha", "mike"] {
        tm.ok(&["new-session", "-d", "-E", "-s", name]);
    }
    assert_eq!(
        tm.ok(&["display-message", "-p", "#{S:#{session_name} }"]),
        "zeta alpha mike \n"
    );
    assert_eq!(
        tm.ok(&["display-message", "-p", "#{S/n:#{session_name} }"]),
        "alpha mike zeta \n"
    );
    assert_eq!(
        tm.ok(&["display-message", "-p", "#{S/nr:#{session_name} }"]),
        "zeta mike alpha \n"
    );

    tm.ok(&["rename-window", "-t", "zeta:0", "charlie"]);
    tm.ok(&["new-window", "-d", "-E", "-t", "zeta:1", "-n", "alpha"]);
    tm.ok(&["new-window", "-d", "-E", "-t", "zeta:2", "-n", "bravo"]);
    assert_eq!(
        tm.ok(&[
            "display-message",
            "-t",
            "zeta:",
            "-p",
            "#{W:#{window_name} }",
        ]),
        "charlie alpha bravo \n"
    );
    assert_eq!(
        tm.ok(&[
            "display-message",
            "-t",
            "zeta:",
            "-p",
            "#{W/n:#{window_name} }"
        ]),
        "alpha bravo charlie \n"
    );

    tm.ok(&["split-window", "-h", "-E", "-t", "zeta:charlie"]);
    tm.ok(&["split-window", "-h", "-E", "-t", "zeta:charlie"]);
    assert_eq!(
        tm.ok(&[
            "display-message",
            "-t",
            "zeta:charlie",
            "-p",
            "#{P:#{pane_index}}"
        ]),
        "012\n"
    );
    assert_eq!(
        tm.ok(&[
            "display-message",
            "-t",
            "zeta:charlie",
            "-p",
            "#{P/r:#{pane_index}}"
        ]),
        "210\n"
    );
}

#[test]
fn run_shell_matches_headless_stdout_and_background_contract() {
    let tm = Tm::new();
    tm.ok(&["new-session", "-d", "-E", "-s", "run"]);
    assert_eq!(
        tm.ok(&["run-shell", "--", "printf", "shell-output"]),
        "shell-output\n"
    );
    assert!(tm.ok(&["run-shell", "-b", "--", "sleep", "0.1"]).is_empty());
    assert!(
        tm.ok(&[
            "run-shell",
            "-t",
            "run:0.0",
            "--",
            "printf",
            "not-client-output"
        ])
        .is_empty()
    );
}
