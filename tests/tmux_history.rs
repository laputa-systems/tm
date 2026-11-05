#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;

use std::time::Duration;

use parity::{Mux, Scratch};

fn create_output_session(mux: &Mux, lines: usize) {
    mux.ok(&["new-session", "-d", "-s", "init", "--", "sleep", "30"]);
    mux.ok(&["set-option", "-g", "history-limit", "20"]);
    mux.ok(&[
        "new-session",
        "-d",
        "-s",
        "S",
        "-x",
        "40",
        "-y",
        "6",
        "--",
        "sh",
        "-c",
        &format!(
            "awk 'BEGIN{{for(i=1;i<={lines};i++) print \"LINE_\" i; print \"SEARCH_TARGET\"}}'; sleep 30"
        ),
    ]);
}

#[test]
fn checkpoint_preserves_recent_history_and_later_output() {
    let scratch = Scratch::new("history-checkpoint");
    let tm = Mux::tm(&scratch, "tm");
    create_output_session(&tm, 200_000);
    tm.capture_until("S", "SEARCH_TARGET", Duration::from_secs(30));
    let captured = tm.ok(&["capture-pane", "-p", "-S", "-", "-t", "S"]);
    let lines = captured.lines().collect::<Vec<_>>();
    assert!(lines.contains(&"LINE_200000"), "captured: {captured:?}");
    assert!(lines.contains(&"SEARCH_TARGET"), "captured: {captured:?}");
    assert!(!lines.contains(&"LINE_1"), "history was not bounded");

    tm.ok(&["copy-mode", "-t", "S"]);
    tm.ok(&["send-keys", "-X", "history-top", "-t", "S"]);
    tm.ok(&[
        "send-keys",
        "-X",
        "search-forward",
        "-t",
        "S",
        "--",
        "SEARCH_TARGET",
    ]);
    tm.ok(&["send-keys", "-X", "start-of-line", "-t", "S"]);
    tm.ok(&["send-keys", "-X", "begin-selection", "-t", "S"]);
    tm.ok(&["send-keys", "-X", "end-of-line", "-t", "S"]);
    tm.ok(&["send-keys", "-X", "copy-selection", "-t", "S"]);
    assert_eq!(tm.body(&["show-buffer"]), "SEARCH_TARGET");
}

#[test]
fn history_limit_capture_matches_the_reference() {
    let scratch = Scratch::new("history-reference");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    for mux in [&tm, &reference] {
        mux.ok(&["new-session", "-d", "-s", "init", "--", "sleep", "30"]);
        mux.ok(&["set-option", "-g", "history-limit", "20"]);
        mux.ok(&[
            "new-session",
            "-d",
            "-s",
            "S",
            "-x",
            "40",
            "-y",
            "6",
            "--",
            "sh",
            "-c",
            "awk 'BEGIN{for(i=1;i<=500;i++) print \"LINE_\" i; print \"END\"}'; sleep 30",
        ]);
        mux.capture_until("S", "END", Duration::from_secs(10));
    }
    let tm_capture = tm.ok(&["capture-pane", "-p", "-S", "-", "-t", "S"]);
    let reference_capture = reference.ok(&["capture-pane", "-p", "-S", "-", "-t", "S"]);
    assert_eq!(tm_capture, reference_capture);
    assert_eq!(tm_capture.lines().filter(|line| *line == "LINE_500").count(), 1);
    assert!(!tm_capture.lines().any(|line| line == "LINE_1"));
}

#[test]
fn capture_keeps_trailing_blank_rows_like_the_reference() {
    let scratch = Scratch::new("capture-rows");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    for mux in [&tm, &reference] {
        mux.ok(&[
            "new-session", "-d", "-s", "S", "-x", "20", "-y", "6", "--", "sh", "-c",
            "printf 'one\\n\\033[1mtwo\\033[0m\\n\\nfour\\nabcdefghijklmnopqrstuvwxy\\npad   x  \\n\\033[31mred\\033[0m   \\nEND\\n'; sleep 30",
        ]);
        mux.capture_until("S", "END", Duration::from_secs(10));
    }
    parity::compare_commands(
        "capture_keeps_trailing_blank_rows",
        &tm,
        &reference,
        &[
            &["capture-pane", "-p", "-t", "S"],
            &["capture-pane", "-p", "-t", "S", "-S", "0", "-E", "2"],
            &["capture-pane", "-p", "-t", "S", "-S", "-3", "-E", "1"],
            &["capture-pane", "-p", "-t", "S", "-S", "-", "-E", "-"],
            &["capture-pane", "-p", "-t", "S", "-S", "5"],
            &["capture-pane", "-p", "-t", "S", "-S", "3", "-E", "20"],
            &["capture-pane", "-p", "-J", "-t", "S"],
            &["capture-pane", "-p", "-e", "-t", "S"],
            &["capture-pane", "-p", "-N", "-t", "S"],
            &["capture-pane", "-p", "-e", "-J", "-t", "S"],
            &["capture-pane", "-p", "-N", "-J", "-t", "S"],
            &["capture-pane", "-p", "-e", "-N", "-t", "S"],
        ],
    );
}

fn compare_capture_options(name: &str, rows: &str, script: &str, marker: &str) {
    let scratch = Scratch::new(name);
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    for mux in [&tm, &reference] {
        mux.ok(&[
            "new-session", "-d", "-s", "S", "-x", "20", "-y", rows, "--", "sh", "-c",
            &format!("{script}; sleep 30"),
        ]);
        mux.capture_until("S", marker, Duration::from_secs(10));
    }
    let mut commands: Vec<Vec<&str>> = Vec::new();
    for range in [&[][..], &["-S", "-"][..], &["-S", "-2", "-E", "1"][..]] {
        for flags in [
            &["-p"][..],
            &["-p", "-J"],
            &["-p", "-e"],
            &["-p", "-N"],
            &["-p", "-e", "-J"],
            &["-p", "-N", "-J"],
            &["-p", "-e", "-N"],
            &["-p", "-e", "-N", "-J"],
        ] {
            let mut command = vec!["capture-pane"];
            command.extend_from_slice(flags);
            command.extend_from_slice(&["-t", "S"]);
            command.extend_from_slice(range);
            commands.push(command);
        }
    }
    let commands = commands.iter().map(Vec::as_slice).collect::<Vec<_>>();
    parity::compare_commands(name, &tm, &reference, &commands);
}

#[test]
fn capture_options_join_wrapped_rows_and_keep_written_spaces_like_the_reference() {
    compare_capture_options(
        "capture_options_wrapped",
        "5",
        "printf 'short\\n0123456789012345678901234567890123456789\\nx   \\n%s\\n0123456789012345678\\nabcdefghijklmnopqrst\\nspaced      out      words   \\nEND\\n' \"$(printf 'w%.0s' 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25)\"",
        "END",
    );
}

#[test]
fn capture_options_emit_attribute_transitions_like_the_reference() {
    compare_capture_options(
        "capture_options_styles",
        "8",
        "printf '\\033[1mbold\\033[0m plain\\n\\033[7mrev\\033[27m x\\n\\033[31;1mred bold\\033[22m red\\033[39m\\n\\033[38;5;200mp256\\033[48;2;1;2;3mrgb bg\\033[0m\\n\\033[2;3;4mdim it ul\\033[0m\\n\\033[91mbright\\033[0m \\033[102mbg\\033[0m\\n\\033[44mblue bg\\033[K\\033[0m\\nEND\\n'",
        "END",
    );
}

#[test]
fn capture_options_treat_cleared_cells_like_the_reference() {
    compare_capture_options(
        "capture_options_cleared",
        "6",
        "printf 'ab\\033[K\\ncd  \\033[K\\nkeep\\033[2K\\n\\033[3Cz\\033[1;3H\\033[K\\n\\033[2;1H\\033[1Kx\\nEND\\n'",
        "END",
    );
}

#[test]
fn capture_keeps_cells_erased_after_being_written_like_the_reference() {
    let scratch = Scratch::new("capture-erased");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    for mux in [&tm, &reference] {
        mux.ok(&[
            "new-session", "-d", "-s", "S", "-x", "20", "-y", "4", "--", "sh", "-c",
            "printf 'erased\\033[5D\\033[K\\nEND\\n'; sleep 30",
        ]);
        mux.capture_until("S", "END", Duration::from_secs(10));
    }
    parity::compare_commands(
        "capture_keeps_cells_erased_after_being_written",
        &tm,
        &reference,
        &[
            &["capture-pane", "-p", "-J", "-t", "S"],
            &["capture-pane", "-p", "-N", "-t", "S"],
        ],
    );
}
