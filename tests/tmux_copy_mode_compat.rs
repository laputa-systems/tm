#![cfg(unix)]

//! Headless ports of the scroll and selection portions of tmux's copy-mode
//! regressions. The tests use private tm sockets and never invoke tmux.

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
        let id = NEXT_SOCKET.fetch_add(1, Ordering::Relaxed);
        Self {
            socket: std::env::temp_dir()
                .join(format!("tm-copy-{}-{id}.sock", std::process::id()))
                .to_string_lossy()
                .into_owned(),
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_tm"))
            .env("TM_SOCKET", &self.socket)
            .args(args)
            .output()
            .expect("run tm")
    }

    fn ok(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "tm {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn body(&self, args: &[&str]) -> String {
        self.ok(args).trim_end_matches('\n').to_owned()
    }

    fn capture_until(&self, target: &str, expected: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let captured = self.body(&["capture-pane", "-t", target]);
            if captured.contains(expected) || Instant::now() >= deadline {
                return captured;
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Tm {
    fn drop(&mut self) {
        let _ = self.run(&["kill-server"]);
    }
}

fn start_history_test(tm: &Tm) {
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "copy",
        "-x40",
        "-y10",
        "--",
        "sh",
        "-c",
        "i=0; while [ $i -lt 80 ]; do printf 'line %02d xxxxxxxxxx\\n' $i; i=$((i + 1)); done; sleep 30",
    ]);
    assert!(tm.capture_until("copy", "line 79").contains("line 79"));
}

#[test]
fn copy_mode_source_pane_drives_selection_in_a_different_target_pane_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "source",
        "-x30",
        "-y6",
        "--",
        "sh",
        "-c",
        "printf 'SOURCE LINE\\n'; sleep 30",
    ]);
    tm.ok(&[
        "split-window",
        "-d",
        "-h",
        "-t",
        "source:0.0",
        "--",
        "sh",
        "-c",
        "printf 'TARGET LINE\\n'; sleep 30",
    ]);
    assert!(tm
        .capture_until("source:0.0", "SOURCE LINE")
        .contains("SOURCE LINE"));
    assert!(tm
        .capture_until("source:0.1", "TARGET LINE")
        .contains("TARGET LINE"));

    tm.ok(&["copy-mode", "-s", "source:0.0", "-t", "source:0.1"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "source:0.1",
            "#{pane_in_mode}"
        ]),
        "1"
    );
    for action in [
        "history-top",
        "start-of-line",
        "begin-selection",
        "end-of-line",
    ] {
        tm.ok(&["send-keys", "-X", action, "-t", "source:0.1"]);
    }
    tm.ok(&["send-keys", "-X", "copy-selection", "-t", "source:0.1"]);
    assert_eq!(tm.body(&["show-buffer"]), "SOURCE LINE");
    assert!(tm
        .body(&["capture-pane", "-t", "source:0.1"])
        .contains("TARGET LINE"));
}

#[test]
fn copy_mode_selection_prefix_creates_a_named_automatic_buffer_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "named-copy",
        "-x30",
        "-y6",
        "--",
        "sh",
        "-c",
        "printf 'named line\\n'; sleep 30",
    ]);
    assert!(tm
        .capture_until("named-copy", "named line")
        .contains("named line"));

    tm.ok(&["copy-mode", "-t", "named-copy"]);
    for action in [
        "history-top",
        "start-of-line",
        "begin-selection",
        "end-of-line",
    ] {
        tm.ok(&["send-keys", "-X", action, "-t", "named-copy"]);
    }
    tm.ok(&[
        "send-keys",
        "-X",
        "copy-selection-and-cancel",
        "-t",
        "named-copy",
        "--",
        "named-",
    ]);
    assert_eq!(tm.body(&["show-buffer", "-b", "named-0"]), "named line");
    assert_eq!(tm.body(&["show-buffer"]), "named line");
}

#[test]
fn copy_mode_copy_paste_flag_suppresses_buffer_storage_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "copy-flags",
        "-x30",
        "-y6",
        "--",
        "sh",
        "-c",
        "printf 'flag line\\n'; sleep 30",
    ]);
    assert!(tm
        .capture_until("copy-flags", "flag line")
        .contains("flag line"));

    tm.ok(&["copy-mode", "-t", "copy-flags"]);
    for action in [
        "history-top",
        "start-of-line",
        "begin-selection",
        "end-of-line",
    ] {
        tm.ok(&["send-keys", "-X", action, "-t", "copy-flags"]);
    }
    tm.ok(&[
        "send-keys",
        "-X",
        "copy-selection",
        "-P",
        "-t",
        "copy-flags",
    ]);
    assert_eq!(tm.body(&["list-buffers"]), "");
}

#[test]
fn copy_mode_scroll_exit_matches_tmux_headlessly() {
    let tm = Tm::new();
    start_history_test(&tm);

    tm.ok(&["copy-mode", "-e", "-t", "copy"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "copy"]);
    tm.ok(&["send-keys", "-X", "start-of-line", "-t", "copy"]);
    tm.ok(&["send-keys", "-X", "begin-selection", "-t", "copy"]);
    tm.ok(&["send-keys", "-X", "cursor-down", "-t", "copy"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "copy",
            "#{selection_present}"
        ]),
        "1"
    );

    tm.ok(&["send-keys", "-N200", "-X", "scroll-down", "-t", "copy"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "copy",
            "#{pane_in_mode} #{scroll_position}"
        ]),
        "1 0"
    );
    tm.ok(&["send-keys", "-X", "clear-selection", "-t", "copy"]);
    tm.ok(&["send-keys", "-X", "scroll-down", "-t", "copy"]);
    assert_eq!(
        tm.body(&["display-message", "-p", "-t", "copy", "#{pane_in_mode}"]),
        "0"
    );
}

#[test]
fn copy_mode_page_and_quiet_options_match_tmux_headlessly() {
    let tm = Tm::new();
    start_history_test(&tm);

    tm.ok(&["copy-mode", "-u", "-t", "copy"]);
    let position = tm
        .body(&["display-message", "-p", "-t", "copy", "#{scroll_position}"])
        .parse::<usize>()
        .expect("scroll position");
    assert!(position > 0, "copy-mode -u did not page into history");

    tm.ok(&["copy-mode", "-q", "-t", "copy"]);
    assert_eq!(
        tm.body(&["display-message", "-p", "-t", "copy", "#{pane_in_mode}"]),
        "0"
    );
}

#[test]
fn copy_mode_selection_survives_scrolling_and_copies_expected_lines() {
    let tm = Tm::new();
    start_history_test(&tm);
    tm.ok(&["copy-mode", "-t", "copy"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "copy"]);
    tm.ok(&["send-keys", "-N10", "-X", "cursor-down", "-t", "copy"]);
    tm.ok(&["send-keys", "-X", "start-of-line", "-t", "copy"]);
    tm.ok(&["send-keys", "-X", "begin-selection", "-t", "copy"]);
    tm.ok(&["send-keys", "-N2", "-X", "cursor-down", "-t", "copy"]);
    tm.ok(&["send-keys", "-X", "copy-selection-no-clear", "-t", "copy"]);

    let initial = "line 10 xxxxxxxxxx\nline 11 xxxxxxxxxx";
    assert_eq!(tm.body(&["show-buffer"]), initial);

    tm.ok(&["send-keys", "-X", "stop-selection", "-t", "copy"]);
    tm.ok(&["send-keys", "-N3", "-X", "scroll-down", "-t", "copy"]);
    tm.ok(&["send-keys", "-X", "copy-selection-no-clear", "-t", "copy"]);
    assert_eq!(tm.body(&["show-buffer"]), initial);

    tm.ok(&["send-keys", "-N2", "-X", "scroll-up", "-t", "copy"]);
    tm.ok(&["send-keys", "-X", "copy-selection-no-clear", "-t", "copy"]);
    assert_eq!(tm.body(&["show-buffer"]), initial);

    for action in [
        "scroll-middle",
        "scroll-bottom",
        "scroll-top",
        "recentre-top-bottom",
    ] {
        tm.ok(&["send-keys", "-X", action, "-t", "copy"]);
        tm.ok(&["send-keys", "-X", "copy-selection-no-clear", "-t", "copy"]);
        assert_eq!(tm.body(&["show-buffer"]), initial, "action {action}");
    }

    tm.ok(&["send-keys", "-X", "other-end", "-t", "copy"]);
    tm.ok(&["send-keys", "-X", "other-end", "-t", "copy"]);
    tm.ok(&["send-keys", "-X", "cursor-up", "-t", "copy"]);
    tm.ok(&["send-keys", "-X", "copy-selection-no-clear", "-t", "copy"]);
    assert_eq!(
        tm.body(&["show-buffer"]),
        "line 09 xxxxxxxxxx\nline 10 xxxxxxxxxx\nline 11 xxxxxxxxxx\nline 12 xxxxxxxxxx"
    );
}

fn start_word_test(tm: &Tm) {
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "words",
        "-x40",
        "-y10",
        "--",
        "sh",
        "-c",
        "printf 'A line of words\\n\\tIndented line\\nAnother line...\\n... @nd then $ym_bols[]{}\\n ?? 500xyz\\n'; sleep 30",
    ]);
    assert!(tm.capture_until("words", "500xyz").contains("500xyz"));
}

fn copy_action(tm: &Tm, action: &str) {
    tm.ok(&["send-keys", "-X", action, "-t", "words"]);
}

#[test]
fn copy_mode_emacs_word_navigation_matches_tmux_headlessly() {
    let tm = Tm::new();
    start_word_test(&tm);
    tm.ok(&["set-window-option", "-g", "mode-keys", "emacs"]);
    tm.ok(&["set-window-option", "-g", "word-separators", ""]);
    tm.ok(&["copy-mode", "-t", "words"]);
    copy_action(&tm, "history-top");
    copy_action(&tm, "start-of-line");

    copy_action(&tm, "begin-selection");
    copy_action(&tm, "previous-word");
    copy_action(&tm, "previous-space");
    copy_action(&tm, "previous-word");
    copy_action(&tm, "copy-selection");
    assert_eq!(tm.body(&["show-buffer"]), "");

    copy_action(&tm, "next-word-end");
    copy_action(&tm, "begin-selection");
    copy_action(&tm, "previous-word");
    copy_action(&tm, "copy-selection");
    assert_eq!(tm.body(&["show-buffer"]), "A");

    for action in [
        "next-word",
        "next-word",
        "next-word",
        "begin-selection",
        "next-word-end",
        "next-word-end",
        "copy-selection",
    ] {
        copy_action(&tm, action);
    }
    assert_eq!(tm.body(&["show-buffer"]), "words\n\tIndented");

    for action in [
        "next-word",
        "begin-selection",
        "next-word",
        "copy-selection",
    ] {
        copy_action(&tm, action);
    }
    assert_eq!(tm.body(&["show-buffer"]), "line");

    for action in [
        "next-word",
        "begin-selection",
        "next-word-end",
        "copy-selection",
    ] {
        copy_action(&tm, action);
    }
    assert_eq!(tm.body(&["show-buffer"]), "line...");

    for action in [
        "previous-word",
        "begin-selection",
        "next-word",
        "copy-selection",
    ] {
        copy_action(&tm, action);
    }
    assert_eq!(tm.body(&["show-buffer"]), "line...");

    for action in [
        "previous-space",
        "begin-selection",
        "next-space",
        "copy-selection",
    ] {
        copy_action(&tm, action);
    }
    assert_eq!(tm.body(&["show-buffer"]), "line...");

    for action in [
        "begin-selection",
        "next-word",
        "next-word",
        "next-word-end",
        "next-word-end",
        "copy-selection",
    ] {
        copy_action(&tm, action);
    }
    assert_eq!(tm.body(&["show-buffer"]), "... @nd then $ym_bols[]{}");

    for action in [
        "previous-word",
        "begin-selection",
        "next-word",
        "copy-selection",
    ] {
        copy_action(&tm, action);
    }
    assert_eq!(tm.body(&["show-buffer"]), "$ym_bols[]{}\n ");

    for action in [
        "next-word-end",
        "begin-selection",
        "next-word-end",
        "copy-selection",
    ] {
        copy_action(&tm, action);
    }
    assert_eq!(tm.body(&["show-buffer"]), " 500xyz");

    copy_action(&tm, "begin-selection");
    copy_action(&tm, "previous-word");
    copy_action(&tm, "copy-selection");
    assert_eq!(tm.body(&["show-buffer"]), "500xyz");

    for action in [
        "begin-selection",
        "next-word",
        "next-word-end",
        "next-word",
        "next-space",
        "next-space-end",
        "copy-selection",
    ] {
        copy_action(&tm, action);
    }
    assert_eq!(tm.body(&["show-buffer"]), "500xyz");
}

#[test]
fn copy_mode_vi_word_navigation_matches_tmux_headlessly() {
    let tm = Tm::new();
    start_word_test(&tm);
    tm.ok(&["set-window-option", "-g", "mode-keys", "vi"]);
    tm.ok(&["copy-mode", "-t", "words"]);
    copy_action(&tm, "history-top");
    copy_action(&tm, "start-of-line");

    for action in [
        "begin-selection",
        "previous-word",
        "previous-space",
        "previous-word",
        "copy-selection",
    ] {
        copy_action(&tm, action);
    }
    assert_eq!(tm.body(&["show-buffer"]), "A");

    for action in [
        "next-word-end",
        "begin-selection",
        "previous-word",
        "copy-selection",
    ] {
        copy_action(&tm, action);
    }
    assert_eq!(tm.body(&["show-buffer"]), "line");

    for action in [
        "next-word",
        "next-word",
        "begin-selection",
        "next-word-end",
        "next-word-end",
        "copy-selection",
    ] {
        copy_action(&tm, action);
    }
    assert_eq!(tm.body(&["show-buffer"]), "words");

    for action in [
        "next-word",
        "next-word",
        "begin-selection",
        "next-word",
        "copy-selection",
    ] {
        copy_action(&tm, action);
    }
    assert_eq!(tm.body(&["show-buffer"]), "line\nA");

    for action in [
        "next-word",
        "begin-selection",
        "next-word-end",
        "copy-selection",
    ] {
        copy_action(&tm, action);
    }
    assert_eq!(tm.body(&["show-buffer"]), "line");

    for action in [
        "previous-word",
        "begin-selection",
        "next-space-end",
        "copy-selection",
    ] {
        copy_action(&tm, action);
    }
    assert_eq!(tm.body(&["show-buffer"]), "line...");

    for action in [
        "previous-space",
        "begin-selection",
        "next-space",
        "copy-selection",
    ] {
        copy_action(&tm, action);
    }
    assert_eq!(tm.body(&["show-buffer"]), "line...\n.");

    for action in [
        "begin-selection",
        "next-word",
        "next-word",
        "next-word-end",
        "next-word-end",
        "copy-selection",
    ] {
        copy_action(&tm, action);
    }
    assert_eq!(tm.body(&["show-buffer"]), "... @nd then");

    for action in [
        "next-space",
        "begin-selection",
        "next-space",
        "copy-selection",
    ] {
        copy_action(&tm, action);
    }
    assert_eq!(tm.body(&["show-buffer"]), "$ym_bols[]{}\n ?");

    for action in [
        "next-word-end",
        "begin-selection",
        "next-word-end",
        "copy-selection",
    ] {
        copy_action(&tm, action);
    }
    assert_eq!(tm.body(&["show-buffer"]), "? 500xyz");

    copy_action(&tm, "begin-selection");
    copy_action(&tm, "previous-word");
    copy_action(&tm, "copy-selection");
    assert_eq!(tm.body(&["show-buffer"]), "500xyz");

    for action in [
        "begin-selection",
        "next-word",
        "next-word-end",
        "next-word",
        "next-space",
        "next-space-end",
        "copy-selection",
    ] {
        copy_action(&tm, action);
    }
    assert_eq!(tm.body(&["show-buffer"]), "500xyz");
}

#[test]
fn copy_mode_vi_cursor_skips_wide_character_padding_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "wide",
        "-x20",
        "-y5",
        "--",
        "sh",
        "-c",
        "printf 'abc中\\nxyz\\n'; sleep 30",
    ]);
    assert!(tm.capture_until("wide", "xyz").contains("xyz"));
    tm.ok(&["set-window-option", "-g", "mode-keys", "vi"]);
    tm.ok(&["copy-mode", "-t", "wide"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "wide"]);
    tm.ok(&["send-keys", "-X", "start-of-line", "-t", "wide"]);
    tm.ok(&["send-keys", "-N3", "-X", "cursor-right", "-t", "wide"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "wide",
            "#{copy_cursor_x},#{copy_cursor_y}"
        ]),
        "3,0"
    );
    tm.ok(&["send-keys", "-X", "cursor-right", "-t", "wide"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "wide",
            "#{copy_cursor_x},#{copy_cursor_y}"
        ]),
        "0,1"
    );
    tm.ok(&["send-keys", "-X", "cursor-left", "-t", "wide"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "wide",
            "#{copy_cursor_x},#{copy_cursor_y}"
        ]),
        "3,0"
    );
    tm.ok(&["send-keys", "-X", "cursor-left", "-t", "wide"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "wide",
            "#{copy_cursor_x},#{copy_cursor_y}"
        ]),
        "2,0"
    );
}

#[test]
fn attached_client_key_bytes_drive_copy_mode_actions_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "keys",
        "-x20",
        "-y5",
        "--",
        "sh",
        "-c",
        "printf 'abcdef\\n'; sleep 30",
    ]);
    assert!(tm.capture_until("keys", "abcdef").contains("abcdef"));
    tm.ok(&["set-window-option", "-g", "mode-keys", "vi"]);
    tm.ok(&["copy-mode", "-t", "keys"]);
    tm.ok(&["send-keys", "-t", "keys", "g", "0", "Space", "Right", "y"]);
    assert_eq!(tm.body(&["show-buffer"]), "ab");
    tm.ok(&["send-keys", "-t", "keys", "q"]);
    assert_eq!(
        tm.body(&["display-message", "-p", "-t", "keys", "#{pane_in_mode}"]),
        "0"
    );

    let pipe_path = "/tmp/tm-vi-enter-copy-command.txt";
    let _ = fs::remove_file(pipe_path);
    tm.ok(&[
        "set-option",
        "-g",
        "copy-command",
        &format!("cat > {pipe_path}"),
    ]);
    tm.ok(&["copy-mode", "-t", "keys"]);
    for action in [
        "history-top",
        "start-of-line",
        "begin-selection",
        "end-of-line",
    ] {
        tm.ok(&["send-keys", "-X", action, "-t", "keys"]);
    }
    tm.ok(&["send-keys", "-t", "keys", "Enter"]);
    assert_eq!(
        fs::read_to_string(pipe_path).expect("vi Enter copy-command output"),
        "abcdef"
    );
    let _ = fs::remove_file(pipe_path);
}

#[test]
fn attached_emacs_copy_mode_bindings_match_tmux_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "emacs-keys",
        "-x20",
        "-y5",
        "--",
        "sh",
        "-c",
        "printf 'abcdef\\n'; sleep 30",
    ]);
    assert!(tm
        .capture_until("emacs-keys", "abcdef")
        .contains("abcdef"));
    tm.ok(&["set-window-option", "-g", "mode-keys", "emacs"]);
    tm.ok(&["copy-mode", "-t", "emacs-keys"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "emacs-keys"]);
    tm.ok(&["send-keys", "-X", "start-of-line", "-t", "emacs-keys"]);
    tm.ok(&["send-keys", "-t", "emacs-keys", "C-Space", "Right", "C-w"]);
    assert_eq!(tm.body(&["show-buffer"]), "a");
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "emacs-keys",
            "#{pane_in_mode}",
        ]),
        "0"
    );
}

#[test]
fn copy_mode_emacs_search_is_incremental_before_enter_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "incremental-search",
        "-x20",
        "-y5",
        "--",
        "sh",
        "-c",
        "printf 'first\\nneedle target\\nlast\\n'; sleep 30",
    ]);
    assert!(tm
        .capture_until("incremental-search", "needle target")
        .contains("needle target"));
    tm.ok(&["set-window-option", "-g", "mode-keys", "emacs"]);
    tm.ok(&["copy-mode", "-t", "incremental-search"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "incremental-search"]);
    tm.ok(&["send-keys", "-t", "incremental-search", "C-s"]);
    tm.ok(&["send-keys", "-l", "-t", "incremental-search", "needle"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "incremental-search",
            "#{copy_cursor_y}:#{pane_in_mode}",
        ]),
        "1:1"
    );
    tm.ok(&["send-keys", "-N", "6", "-t", "incremental-search", "BSpace"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "incremental-search",
            "#{copy_cursor_y}:#{pane_in_mode}",
        ]),
        "0:1"
    );
    tm.ok(&["send-keys", "-l", "-t", "incremental-search", "needle"]);
    tm.ok(&["send-keys", "-t", "incremental-search", "Escape"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "incremental-search",
            "#{copy_cursor_y}:#{pane_in_mode}",
        ]),
        "0:1"
    );
    tm.ok(&[
        "send-keys",
        "-X",
        "history-bottom",
        "-t",
        "incremental-search",
    ]);
    tm.ok(&["send-keys", "-t", "incremental-search", "C-r"]);
    tm.ok(&["send-keys", "-l", "-t", "incremental-search", "needle"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "incremental-search",
            "#{copy_cursor_y}:#{pane_in_mode}",
        ]),
        "1:1"
    );
}

#[test]
fn attached_copy_mode_numeric_prefixes_repeat_navigation_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "repeat-keys",
        "-x20",
        "-y5",
        "--",
        "sh",
        "-c",
        "printf 'zero\\none\\ntwo\\nthree\\n'; sleep 30",
    ]);
    assert!(tm.capture_until("repeat-keys", "three").contains("three"));
    tm.ok(&["set-window-option", "-g", "mode-keys", "vi"]);
    tm.ok(&["copy-mode", "-t", "repeat-keys"]);
    tm.ok(&["send-keys", "-t", "repeat-keys", "g", "3", "j"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "repeat-keys",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "0,3"
    );
    tm.ok(&["send-keys", "-t", "repeat-keys", "2", "k"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "repeat-keys",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "0,1"
    );
}

#[test]
fn copy_mode_kill_on_exit_only_kills_the_entering_pane_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "kill",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    tm.ok(&[
        "split-window",
        "-d",
        "-h",
        "-t",
        "kill:0",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    let active = tm
        .ok(&["display-message", "-p", "-t", "kill:0", "#{pane_id}"])
        .trim()
        .to_owned();
    tm.ok(&["copy-mode", "-k", "-t", &active]);
    assert_eq!(
        tm.body(&["display-message", "-p", "-t", &active, "#{pane_in_mode}"]),
        "1"
    );
    tm.ok(&["send-keys", "-X", "cancel", "-t", &active]);
    assert_eq!(tm.ok(&["list-panes", "-t", "kill:0"]).lines().count(), 1);
}

#[test]
fn copy_mode_redraw_scrolls_long_and_short_rows_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "redraw",
        "-x20",
        "-y5",
        "--",
        "sh",
        "-c",
        "i=0; while [ $i -lt 20 ]; do if [ $((i % 2)) -eq 0 ]; then printf 'LONG-%02d-ABCDEFGHIJKLMNOP\\n' $i; else printf 'S-%02d\\n' $i; fi; i=$((i + 1)); done; sleep 30",
    ]);
    assert!(tm.capture_until("redraw", "S-19").contains("S-19"));
    tm.ok(&["set-option", "-g", "copy-mode-line-numbers", "off"]);
    tm.ok(&["copy-mode", "-H", "-t", "redraw"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "redraw",
            "#{copy_position_hidden}",
        ]),
        "1"
    );
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "redraw",
            "#{copy_line_numbers}"
        ]),
        "0"
    );
    tm.ok(&["send-keys", "-X", "history-top", "-t", "redraw"]);
    assert!(tm.capture_until("redraw", "LONG-00").contains("LONG-00"));
    tm.ok(&["send-keys", "-X", "scroll-down", "-t", "redraw"]);
    assert!(tm.capture_until("redraw", "S-01").contains("S-01"));
    tm.ok(&["send-keys", "-X", "scroll-down", "-t", "redraw"]);
    assert!(tm.capture_until("redraw", "LONG-02").contains("LONG-02"));
}

#[test]
fn copy_mode_search_rectangle_and_append_selection_are_headless() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "advanced",
        "-x30",
        "-y6",
        "--",
        "sh",
        "-c",
        "printf 'alpha one\\nbeta two\\nalpha three\\n'; sleep 30",
    ]);
    assert!(tm
        .capture_until("advanced", "alpha three")
        .contains("alpha three"));

    tm.ok(&["copy-mode", "-t", "advanced"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "advanced"]);
    tm.ok(&[
        "send-keys",
        "-X",
        "search-forward",
        "-t",
        "advanced",
        "--",
        "beta",
    ]);
    tm.ok(&["send-keys", "-X", "start-of-line", "-t", "advanced"]);
    tm.ok(&["send-keys", "-X", "begin-selection", "-t", "advanced"]);
    tm.ok(&["send-keys", "-X", "end-of-line", "-t", "advanced"]);
    tm.ok(&["send-keys", "-X", "copy-selection", "-t", "advanced"]);
    assert_eq!(tm.body(&["show-buffer"]), "beta two");

    tm.ok(&["send-keys", "-X", "history-top", "-t", "advanced"]);
    tm.ok(&["send-keys", "-X", "start-of-line", "-t", "advanced"]);
    tm.ok(&["send-keys", "-X", "begin-selection", "-t", "advanced"]);
    tm.ok(&["send-keys", "-N2", "-X", "cursor-down", "-t", "advanced"]);
    tm.ok(&["send-keys", "-N4", "-X", "cursor-right", "-t", "advanced"]);
    tm.ok(&["send-keys", "-X", "rectangle-toggle", "-t", "advanced"]);
    tm.ok(&["send-keys", "-X", "copy-selection", "-t", "advanced"]);
    assert_eq!(tm.body(&["show-buffer"]), "alpha\nbeta \nalpha");

    tm.ok(&["send-keys", "-X", "rectangle-toggle", "-t", "advanced"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "advanced"]);
    tm.ok(&["send-keys", "-X", "start-of-line", "-t", "advanced"]);
    tm.ok(&["send-keys", "-N2", "-X", "cursor-down", "-t", "advanced"]);
    tm.ok(&["send-keys", "-X", "begin-selection", "-t", "advanced"]);
    tm.ok(&["send-keys", "-N4", "-X", "cursor-right", "-t", "advanced"]);
    tm.ok(&["send-keys", "-X", "append-selection", "-t", "advanced"]);
    assert_eq!(tm.body(&["show-buffer"]), "alphalpha\nbeta \nalpha");
}

#[test]
fn copy_mode_extended_command_table_is_headless() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "commands",
        "-x30",
        "-y6",
        "--",
        "sh",
        "-c",
        "printf 'one (two)\\ntwo three\\nthree four\\n'; sleep 30",
    ]);
    assert!(tm
        .capture_until("commands", "three four")
        .contains("three four"));

    tm.ok(&["copy-mode", "-t", "commands"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "commands"]);
    tm.ok(&["send-keys", "-X", "copy-line", "-t", "commands"]);
    assert_eq!(tm.body(&["show-buffer"]), "one (two)");

    tm.ok(&["send-keys", "-X", "history-top", "-t", "commands"]);
    tm.ok(&["send-keys", "-N2", "-X", "select-line", "-t", "commands"]);
    tm.ok(&["send-keys", "-X", "copy-selection", "-t", "commands"]);
    assert_eq!(tm.body(&["show-buffer"]), "one (two)\ntwo three");

    tm.ok(&["send-keys", "-X", "history-top", "-t", "commands"]);
    tm.ok(&["send-keys", "-X", "start-of-line", "-t", "commands"]);
    tm.ok(&["send-keys", "-X", "begin-selection", "-t", "commands"]);
    tm.ok(&["send-keys", "-X", "cursor-right", "-t", "commands"]);
    tm.ok(&["send-keys", "-X", "stop-selection", "-t", "commands"]);
    tm.ok(&["send-keys", "-N2", "-X", "other-end", "-t", "commands"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "commands",
            "#{selection_active}:#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "0:1,0"
    );

    tm.ok(&["send-keys", "-X", "history-top", "-t", "commands"]);
    tm.ok(&["send-keys", "-X", "start-of-line", "-t", "commands"]);
    tm.ok(&["send-keys", "-X", "begin-selection", "-t", "commands"]);
    tm.ok(&[
        "send-keys",
        "-X",
        "selection-mode",
        "-t",
        "commands",
        "--",
        "line",
    ]);
    tm.ok(&["send-keys", "-X", "cursor-down", "-t", "commands"]);
    tm.ok(&["send-keys", "-X", "copy-selection", "-t", "commands"]);
    assert_eq!(tm.body(&["show-buffer"]), "one (two)\ntwo three");
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "commands",
            "#{selection_mode}:#{selection_active}"
        ]),
        "line:0"
    );

    tm.ok(&["send-keys", "-X", "history-top", "-t", "commands"]);
    tm.ok(&["send-keys", "-X", "rectangle-on", "-t", "commands"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "commands",
            "#{copy_cursor_rectangle}"
        ]),
        "1"
    );
    tm.ok(&["send-keys", "-X", "rectangle-off", "-t", "commands"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "commands",
            "#{copy_cursor_rectangle}"
        ]),
        "0"
    );

    tm.ok(&["send-keys", "-X", "history-top", "-t", "commands"]);
    tm.ok(&["send-keys", "-X", "next-matching-bracket", "-t", "commands"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "commands",
            "#{copy_cursor_x},#{copy_cursor_y}"
        ]),
        "8,0"
    );

    tm.ok(&["send-keys", "-X", "scroll-exit-off", "-t", "commands"]);
    tm.ok(&["send-keys", "-N200", "-X", "scroll-down", "-t", "commands"]);
    assert_eq!(
        tm.body(&["display-message", "-p", "-t", "commands", "#{pane_in_mode}"]),
        "1"
    );
    tm.ok(&["send-keys", "-X", "scroll-exit-on", "-t", "commands"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "commands"]);
    tm.ok(&["send-keys", "-X", "scroll-down", "-t", "commands"]);
    assert_eq!(
        tm.body(&["display-message", "-p", "-t", "commands", "#{pane_in_mode}"]),
        "0"
    );
}

#[test]
fn copy_mode_vi_search_keys_are_headless() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "searchkeys",
        "-x20",
        "-y5",
        "--",
        "sh",
        "-c",
        "printf 'alpha\\nbeta target\\ngamma\\n'; sleep 30",
    ]);
    assert!(tm
        .capture_until("searchkeys", "beta target")
        .contains("beta target"));
    tm.ok(&["set-window-option", "-g", "mode-keys", "vi"]);
    tm.ok(&["copy-mode", "-t", "searchkeys"]);
    tm.ok(&["send-keys", "-t", "searchkeys", "g", "0", "/"]);
    tm.ok(&["send-keys", "-t", "searchkeys", "target"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "searchkeys",
            "#{copy_cursor_y}",
        ]),
        "0"
    );
    tm.ok(&["send-keys", "-t", "searchkeys", "Enter"]);
    tm.ok(&["send-keys", "-X", "start-of-line", "-t", "searchkeys"]);
    tm.ok(&["send-keys", "-X", "begin-selection", "-t", "searchkeys"]);
    tm.ok(&["send-keys", "-X", "end-of-line", "-t", "searchkeys"]);
    tm.ok(&["send-keys", "-X", "copy-selection", "-t", "searchkeys"]);
    assert_eq!(tm.body(&["show-buffer"]), "beta target");
}

#[test]
fn copy_mode_search_wraps_from_both_scrollback_edges_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "search-wrap",
        "-x20",
        "-y5",
        "--",
        "sh",
        "-c",
        "printf 'alpha\\nbeta\\ngamma\\n'; sleep 30",
    ]);
    assert!(tm.capture_until("search-wrap", "gamma").contains("gamma"));
    tm.ok(&["copy-mode", "-t", "search-wrap"]);

    // Copy mode starts at the live edge. Searching forward for the first
    // line must wrap around the retained scrollback.
    tm.ok(&[
        "send-keys",
        "-X",
        "search-forward",
        "-t",
        "search-wrap",
        "--",
        "alpha",
    ]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "search-wrap",
            "#{copy_cursor_y}",
        ]),
        "0"
    );

    tm.ok(&["send-keys", "-X", "history-top", "-t", "search-wrap"]);
    tm.ok(&[
        "send-keys",
        "-X",
        "search-backward",
        "-t",
        "search-wrap",
        "--",
        "gamma",
    ]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "search-wrap",
            "#{copy_cursor_y}",
        ]),
        "2"
    );
}

#[test]
fn copy_mode_regex_and_literal_search_are_distinct_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "search-kinds",
        "-x30",
        "-y6",
        "--",
        "sh",
        "-c",
        "printf 'start\\nα-123\\nα-[0-9]+|α-xyz\\nα-xyz\\n'; sleep 30",
    ]);
    assert!(tm
        .capture_until("search-kinds", "α-xyz")
        .contains("α-xyz"));
    tm.ok(&["copy-mode", "-t", "search-kinds"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "search-kinds"]);
    tm.ok(&[
        "send-keys",
        "-X",
        "search-forward",
        "-t",
        "search-kinds",
        "--",
        "α-[0-9]+|α-xyz",
    ]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "search-kinds",
            "#{copy_cursor_y}",
        ]),
        "1"
    );
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "search-kinds",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "5,1"
    );

    tm.ok(&["send-keys", "-X", "history-top", "-t", "search-kinds"]);
    tm.ok(&[
        "send-keys",
        "-X",
        "search-forward",
        "-t",
        "search-kinds",
        "--",
        "α-(123|xyz)",
    ]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "search-kinds",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "5,1"
    );

    tm.ok(&["send-keys", "-X", "history-top", "-t", "search-kinds"]);
    tm.ok(&[
        "send-keys",
        "-X",
        "search-forward-text",
        "-t",
        "search-kinds",
        "--",
        "α-[0-9]+|α-xyz",
    ]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "search-kinds",
            "#{copy_cursor_y}",
        ]),
        "2"
    );
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "search-kinds",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "14,2"
    );
}

#[test]
fn copy_mode_lowercase_search_is_case_insensitive_like_tmux_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "search-case",
        "-x20",
        "-y5",
        "--",
        "sh",
        "-c",
        "printf 'Alpha\\nbeta\\nALPHA\\n'; sleep 30",
    ]);
    assert!(tm.capture_until("search-case", "ALPHA").contains("ALPHA"));
    tm.ok(&["copy-mode", "-t", "search-case"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "search-case"]);
    tm.ok(&[
        "send-keys",
        "-X",
        "search-forward-text",
        "-t",
        "search-case",
        "--",
        "alpha",
    ]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "search-case",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "5,0"
    );
    tm.ok(&["send-keys", "-X", "history-top", "-t", "search-case"]);
    tm.ok(&[
        "send-keys",
        "-X",
        "search-forward-text",
        "-t",
        "search-case",
        "--",
        "ALPHA",
    ]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "search-case",
            "#{copy_cursor_y}",
        ]),
        "2"
    );
    tm.ok(&["send-keys", "-X", "history-top", "-t", "search-case"]);
    tm.ok(&[
        "send-keys",
        "-X",
        "search-forward",
        "-t",
        "search-case",
        "--",
        "alpha",
    ]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "search-case",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "5,0"
    );
}

#[test]
fn copy_mode_search_respects_wrap_search_off_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "search-no-wrap",
        "-x20",
        "-y5",
        "--",
        "sh",
        "-c",
        "printf 'alpha\\nbeta\\n'; sleep 30",
    ]);
    assert!(tm
        .capture_until("search-no-wrap", "beta")
        .contains("beta"));
    tm.ok(&["set-window-option", "-g", "wrap-search", "off"]);
    tm.ok(&["copy-mode", "-t", "search-no-wrap"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "search-no-wrap"]);
    tm.ok(&[
        "send-keys",
        "-X",
        "search-backward-text",
        "-t",
        "search-no-wrap",
        "--",
        "beta",
    ]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "search-no-wrap",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "0,0"
    );
}

#[test]
fn copy_mode_search_reverse_keeps_reversing_the_original_direction_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "search-reverse",
        "-x20",
        "-y5",
        "--",
        "sh",
        "-c",
        "printf 'x x x\\n'; sleep 30",
    ]);
    assert!(tm
        .capture_until("search-reverse", "x x x")
        .contains("x x x"));
    tm.ok(&["copy-mode", "-t", "search-reverse"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "search-reverse"]);
    tm.ok(&[
        "send-keys",
        "-X",
        "search-forward",
        "-t",
        "search-reverse",
        "--",
        "x",
    ]);
    tm.ok(&["send-keys", "-X", "search-reverse", "-t", "search-reverse"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "search-reverse",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "0,0"
    );
    tm.ok(&["send-keys", "-X", "search-reverse", "-t", "search-reverse"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "search-reverse",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "4,0"
    );
}

#[test]
fn copy_mode_backward_regex_search_preserves_overlapping_matches_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "search-overlap",
        "-x20",
        "-y5",
        "--",
        "sh",
        "-c",
        "printf 'ababa\\n'; sleep 30",
    ]);
    assert!(tm
        .capture_until("search-overlap", "ababa")
        .contains("ababa"));
    tm.ok(&["copy-mode", "-t", "search-overlap"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "search-overlap"]);
    tm.ok(&["send-keys", "-X", "end-of-line", "-t", "search-overlap"]);
    tm.ok(&[
        "send-keys",
        "-X",
        "search-backward",
        "-t",
        "search-overlap",
        "--",
        "aba",
    ]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "search-overlap",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "2,0"
    );
}

#[test]
fn copy_mode_regex_interval_quantifiers_match_tmux_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "search-interval",
        "-x20",
        "-y5",
        "--",
        "sh",
        "-c",
        "printf 'aa\\n'; sleep 30",
    ]);
    assert!(tm.capture_until("search-interval", "aa").contains("aa"));
    tm.ok(&["copy-mode", "-t", "search-interval"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "search-interval"]);
    tm.ok(&[
        "send-keys",
        "-X",
        "search-forward",
        "-t",
        "search-interval",
        "--",
        "a{2}",
    ]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "search-interval",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "2,0"
    );
    for pattern in ["a{2,}", "a{2,2}"] {
        tm.ok(&["send-keys", "-X", "history-top", "-t", "search-interval"]);
        tm.ok(&[
            "send-keys",
            "-X",
            "search-forward",
            "-t",
            "search-interval",
            "--",
            pattern,
        ]);
        assert_eq!(
            tm.body(&[
                "display-message",
                "-p",
                "-t",
                "search-interval",
                "#{copy_cursor_x},#{copy_cursor_y}",
            ]),
            "2,0"
        );
    }
}

#[test]
fn copy_mode_prompts_preserve_utf8_search_input_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "unicode-prompt",
        "-x20",
        "-y5",
        "--",
        "sh",
        "-c",
        "printf 'alpha βeta\\ngamma\\n'; sleep 30",
    ]);
    assert!(tm
        .capture_until("unicode-prompt", "alpha βeta")
        .contains("alpha βeta"));
    tm.ok(&["set-window-option", "-g", "mode-keys", "vi"]);
    tm.ok(&["copy-mode", "-t", "unicode-prompt"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "unicode-prompt"]);
    tm.ok(&["send-keys", "-t", "unicode-prompt", "/"]);
    tm.ok(&["send-keys", "-l", "-t", "unicode-prompt", "β"]);
    tm.ok(&["send-keys", "-t", "unicode-prompt", "Enter"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "unicode-prompt",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "6,0"
    );
}

#[test]
fn copy_mode_vi_word_search_and_goto_line_keys_are_headless() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "vi-keys",
        "-x20",
        "-y5",
        "--",
        "sh",
        "-c",
        "printf 'alpha one\\nbeta two alpha\\ngamma three\\n'; sleep 30",
    ]);
    assert!(tm
        .capture_until("vi-keys", "gamma three")
        .contains("gamma three"));
    tm.ok(&["set-window-option", "-g", "mode-keys", "vi"]);
    tm.ok(&["copy-mode", "-t", "vi-keys"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "vi-keys"]);
    tm.ok(&["send-keys", "-X", "start-of-line", "-t", "vi-keys"]);

    tm.ok(&["send-keys", "-t", "vi-keys", "*"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "vi-keys",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "9,1"
    );

    tm.ok(&["send-keys", "-t", "vi-keys", "#"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "vi-keys",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "0,0"
    );

    tm.ok(&["send-keys", "-t", "vi-keys", ":", "3", "Enter"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "vi-keys",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "0,2"
    );
}

#[test]
fn copy_mode_jump_prompt_keys_are_headless() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "jump-keys",
        "-x20",
        "-y5",
        "--",
        "sh",
        "-c",
        "printf 'abc def ghi\\n'; sleep 30",
    ]);
    assert!(tm
        .capture_until("jump-keys", "abc def ghi")
        .contains("abc def ghi"));
    tm.ok(&["copy-mode", "-t", "jump-keys"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "jump-keys"]);
    tm.ok(&["send-keys", "-X", "start-of-line", "-t", "jump-keys"]);
    tm.ok(&["send-keys", "-t", "jump-keys", "f"]);
    tm.ok(&["send-keys", "-t", "jump-keys", "d"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "jump-keys",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "4,0"
    );
    tm.ok(&["send-keys", "-t", "jump-keys", "T", "a"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "jump-keys",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "1,0"
    );
}

#[test]
fn copy_mode_jump_repeat_prefixes_match_tmux_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "jump-repeat",
        "-x20",
        "-y5",
        "--",
        "sh",
        "-c",
        "printf 'a x b x c x\\n'; sleep 30",
    ]);
    assert!(tm
        .capture_until("jump-repeat", "a x b x c x")
        .contains("a x b x c x"));
    tm.ok(&["copy-mode", "-t", "jump-repeat"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "jump-repeat"]);
    tm.ok(&["send-keys", "-X", "start-of-line", "-t", "jump-repeat"]);
    tm.ok(&[
        "send-keys",
        "-X",
        "jump-forward",
        "-t",
        "jump-repeat",
        "--",
        "x",
    ]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "jump-repeat",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "2,0"
    );
    tm.ok(&["send-keys", "-N2", "-X", "jump-again", "-t", "jump-repeat"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "jump-repeat",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "10,0"
    );
    tm.ok(&[
        "send-keys",
        "-N2",
        "-X",
        "jump-reverse",
        "-t",
        "jump-repeat",
    ]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "jump-repeat",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "2,0"
    );
}

#[test]
fn copy_mode_search_repeat_prefixes_match_tmux_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "search-repeat",
        "-x20",
        "-y5",
        "--",
        "sh",
        "-c",
        "printf 'x x x\\n'; sleep 30",
    ]);
    assert!(tm
        .capture_until("search-repeat", "x x x")
        .contains("x x x"));
    tm.ok(&["copy-mode", "-t", "search-repeat"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "search-repeat"]);
    tm.ok(&[
        "send-keys",
        "-N2",
        "-X",
        "search-forward",
        "-t",
        "search-repeat",
        "--",
        "x",
    ]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "search-repeat",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "3,0"
    );
    tm.ok(&[
        "send-keys",
        "-N2",
        "-X",
        "search-backward",
        "-t",
        "search-repeat",
        "--",
        "x",
    ]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "search-repeat",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "0,0"
    );
}

#[test]
fn copy_mode_paragraph_navigation_matches_tmux_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "paragraphs",
        "-x20",
        "-y6",
        "--",
        "sh",
        "-c",
        "printf 'p0\\np1\\n\\np2\\np3\\n\\np4\\n'; sleep 30",
    ]);
    assert!(tm.capture_until("paragraphs", "p4").contains("p4"));
    tm.ok(&["copy-mode", "-t", "paragraphs"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "paragraphs"]);
    tm.ok(&["send-keys", "-X", "next-paragraph", "-t", "paragraphs"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "paragraphs",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "0,2"
    );
    tm.ok(&["send-keys", "-X", "next-paragraph", "-t", "paragraphs"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "paragraphs",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "0,5"
    );
    tm.ok(&["send-keys", "-X", "previous-paragraph", "-t", "paragraphs"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "paragraphs",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "0,2"
    );
}

#[test]
fn copy_mode_vi_next_bracket_handles_a_closing_bracket_headlessly() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "bracket-vi",
        "-x20",
        "-y5",
        "--",
        "sh",
        "-c",
        "printf 'foo (bar) baz\\n'; sleep 30",
    ]);
    assert!(tm
        .capture_until("bracket-vi", "foo (bar) baz")
        .contains("foo (bar) baz"));
    tm.ok(&["set-window-option", "-g", "mode-keys", "vi"]);
    tm.ok(&["copy-mode", "-t", "bracket-vi"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "bracket-vi"]);
    tm.ok(&["send-keys", "-N8", "-X", "cursor-right", "-t", "bracket-vi"]);
    tm.ok(&[
        "send-keys",
        "-X",
        "next-matching-bracket",
        "-t",
        "bracket-vi",
    ]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "bracket-vi",
            "#{copy_cursor_x},#{copy_cursor_y}",
        ]),
        "4,0"
    );
}

#[test]
fn copy_mode_pipe_and_remaining_command_table_are_headless() {
    let tm = Tm::new();
    let pipe_path = "/tmp/tm-copy-mode-command-table.txt";
    let _ = fs::remove_file(pipe_path);
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "pipe",
        "-x30",
        "-y6",
        "--",
        "sh",
        "-c",
        "printf 'first line\\nsecond line\\n'; sleep 30",
    ]);
    assert!(tm
        .capture_until("pipe", "second line")
        .contains("second line"));

    tm.ok(&["copy-mode", "-t", "pipe"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "pipe"]);
    tm.ok(&[
        "send-keys",
        "-X",
        "copy-pipe-line",
        "-t",
        "pipe",
        "--",
        &format!("cat > {pipe_path}"),
    ]);
    assert_eq!(
        fs::read_to_string(pipe_path).expect("copy-pipe output"),
        "first line"
    );
    assert_eq!(tm.body(&["show-buffer"]), "first line");

    let default_pipe_path = "/tmp/tm-copy-command-default.txt";
    let _ = fs::remove_file(default_pipe_path);
    tm.ok(&[
        "set-option",
        "-g",
        "copy-command",
        &format!("cat > {default_pipe_path}"),
    ]);
    tm.ok(&["copy-mode", "-t", "pipe"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "pipe"]);
    tm.ok(&["send-keys", "-X", "copy-pipe-line", "-t", "pipe"]);
    assert_eq!(
        fs::read_to_string(default_pipe_path).expect("default copy-command output"),
        "first line"
    );

    tm.ok(&["send-keys", "-X", "history-top", "-t", "pipe"]);
    tm.ok(&["send-keys", "-X", "start-of-line", "-t", "pipe"]);
    tm.ok(&["send-keys", "-X", "begin-selection", "-t", "pipe"]);
    tm.ok(&["send-keys", "-X", "end-of-line", "-t", "pipe"]);
    tm.ok(&[
        "send-keys",
        "-X",
        "copy-pipe-and-cancel",
        "-t",
        "pipe",
        "--",
        &format!("tr '[:lower:]' '[:upper:]' > {pipe_path}"),
    ]);
    assert_eq!(
        fs::read_to_string(pipe_path).expect("copy-pipe-and-cancel output"),
        "FIRST LINE"
    );
    assert_eq!(
        tm.body(&["display-message", "-p", "-t", "pipe", "#{pane_in_mode}"]),
        "0"
    );

    tm.ok(&["copy-mode", "-t", "pipe"]);
    for action in [
        "refresh-from-pane",
        "refresh-on",
        "refresh-off",
        "refresh-toggle",
        "toggle-position",
        "scroll-to-mouse",
        "line-numbers-off",
        "next-prompt",
        "previous-prompt",
        "search-forward-text",
    ] {
        if action == "search-forward-text" {
            tm.ok(&["send-keys", "-X", action, "-t", "pipe", "--", "second"]);
        } else {
            tm.ok(&["send-keys", "-X", action, "-t", "pipe"]);
        }
    }
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "pipe",
            "#{copy_line_numbers}",
        ]),
        "0"
    );
    tm.ok(&["send-keys", "-X", "line-numbers-toggle", "-t", "pipe"]);
    assert_eq!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "pipe",
            "#{copy_line_numbers}",
        ]),
        "1"
    );
    tm.ok(&[
        "send-keys",
        "-X",
        "search-backward-text",
        "-t",
        "pipe",
        "--",
        "first",
    ]);
    tm.ok(&["send-keys", "-X", "cancel", "-t", "pipe"]);
    let _ = fs::remove_file(pipe_path);
    let _ = fs::remove_file(default_pipe_path);
}
