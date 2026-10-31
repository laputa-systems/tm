#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;

use std::time::Duration;

use parity::{Mux, Scratch};

fn styled_session(mux: &Mux) {
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
        "printf '\\033[31m界X\\033[0m e\\314\\201 AB\\n'; printf '\\033]8;;https://example.com\\033\\\\LINK\\033]8;;\\033\\\\\\n'; printf 'READY\\n'; sleep 30",
    ]);
    mux.capture_until("S", "READY", Duration::from_secs(5));
}

fn copy_line(mux: &Mux) {
    mux.ok(&["copy-mode", "-t", "S"]);
    for action in [
        "history-top",
        "start-of-line",
        "begin-selection",
        "end-of-line",
        "copy-selection",
    ] {
        mux.ok(&["send-keys", "-t", "S", "-X", action]);
    }
}

#[test]
fn wide_combining_and_styled_cells_copy_the_logical_text() {
    let scratch = Scratch::new("copy-cells");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    for mux in [&tm, &reference] {
        styled_session(mux);
        copy_line(mux);
    }
    assert_eq!(tm.body(&["show-buffer"]), "界X e\u{301} AB");
    assert_eq!(tm.body(&["show-buffer"]), reference.body(&["show-buffer"]));

    let tm_capture = tm.ok(&["capture-pane", "-p", "-e", "-t", "S"]);
    let reference_capture = reference.ok(&["capture-pane", "-p", "-e", "-t", "S"]);
    assert_eq!(
        rendered_state(&tm_capture),
        rendered_state(&reference_capture)
    );
}

#[test]
fn hyperlink_cells_copy_text_and_preserve_the_link_in_capture() {
    let scratch = Scratch::new("copy-hyperlink");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    for mux in [&tm, &reference] {
        styled_session(mux);
        mux.ok(&["copy-mode", "-t", "S"]);
        for action in [
            "history-top",
            "cursor-down",
            "start-of-line",
            "begin-selection",
            "end-of-line",
            "copy-selection",
        ] {
            mux.ok(&["send-keys", "-t", "S", "-X", action]);
        }
    }
    assert_eq!(tm.body(&["show-buffer"]), "LINK");
    assert_eq!(tm.body(&["show-buffer"]), reference.body(&["show-buffer"]));
    let tm_capture = tm.ok(&["capture-pane", "-p", "-e", "-t", "S"]);
    let reference_capture = reference.ok(&["capture-pane", "-p", "-e", "-t", "S"]);
    assert!(tm_capture.contains("\u{1b}]8;;https://example.com"));
    assert_eq!(
        tm_capture
            .lines()
            .filter(|line| line.contains("LINK"))
            .count(),
        reference_capture
            .lines()
            .filter(|line| line.contains("LINK"))
            .count()
    );
}

#[test]
fn narrow_resize_keeps_wide_and_combining_cells_aligned() {
    let scratch = Scratch::new("copy-narrow");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    for mux in [&tm, &reference] {
        styled_session(mux);
        mux.ok(&["split-window", "-h", "-d", "-t", "S", "--", "sleep", "30"]);
        mux.ok(&["resize-pane", "-t", "S:1.0", "-x", "12"]);
    }
    let tm_capture = tm.ok(&["capture-pane", "-p", "-t", "S:1.0"]);
    let reference_capture = reference.ok(&["capture-pane", "-p", "-t", "S:1.0"]);
    assert_eq!(
        tm_capture.trim_end_matches('\n'),
        reference_capture.trim_end_matches('\n')
    );
    for mux in [&tm, &reference] {
        mux.ok(&["resize-pane", "-t", "S:1.0", "-x", "40"]);
    }
    let tm_wide = tm.ok(&["capture-pane", "-p", "-t", "S:1.0"]);
    let reference_wide = reference.ok(&["capture-pane", "-p", "-t", "S:1.0"]);
    assert_eq!(
        tm_wide.trim_end_matches('\n'),
        reference_wide.trim_end_matches('\n')
    );
}

#[test]
fn emacs_copy_actions_move_search_and_copy_with_the_cell_model() {
    let scratch = Scratch::new("copy-emacs");
    let tm = Mux::tm(&scratch, "tm");
    tm.ok(&[
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
        "printf 'alpha one\\nwide 界X\\nomega three\\n'; sleep 30",
    ]);
    tm.capture_until("S", "omega three", Duration::from_secs(5));
    tm.ok(&["copy-mode", "-t", "S"]);
    for action in [
        "history-top",
        "cursor-down",
        "start-of-line",
        "begin-selection",
        "end-of-line",
        "copy-selection",
    ] {
        tm.ok(&["send-keys", "-t", "S", "-X", action]);
    }
    assert_eq!(tm.body(&["show-buffer"]), "wide 界X");
}

fn rendered_state(capture: &str) -> String {
    let mut parser = vt100::Parser::new(6, 40, 0);
    parser.process(capture.trim_end_matches('\n').replace('\n', "\r\n").as_bytes());
    let screen = parser.screen();
    let mut cells = Vec::new();
    for row in 0..6 {
        for col in 0..40 {
            let cell = screen.cell(row, col).expect("copy cell");
            cells.push(format!(
                "{:?}/{:?}/{:?}/{}/{}/{}/{}/{}",
                cell.contents(),
                cell.fgcolor(),
                cell.bgcolor(),
                cell.bold(),
                cell.dim(),
                cell.italic(),
                cell.underline(),
                cell.inverse()
            ));
        }
    }
    format!("{:?}", (screen.contents(), cells))
}
