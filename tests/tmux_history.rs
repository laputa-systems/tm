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
    assert_eq!(
        tm_capture.trim_end_matches('\n'),
        reference_capture.trim_end_matches('\n')
    );
    assert_eq!(tm_capture.lines().filter(|line| *line == "LINE_500").count(), 1);
    assert!(!tm_capture.lines().any(|line| line == "LINE_1"));
}
