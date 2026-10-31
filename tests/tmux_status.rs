#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/pty_harness.rs"]
#[allow(dead_code)]
mod pty_harness;

use std::time::Duration;

use parity::{Mux, Scratch};
use ptytest::{Color, PtyTest, Size};

fn session(mux: &Mux, width: u16, height: u16) {
    mux.ok(&[
        "new-session",
        "-d",
        "-s",
        "S",
        "-x",
        &width.to_string(),
        "-y",
        &height.to_string(),
        "--",
        "sh",
        "-c",
        "printf 'PANE_READY\\n'; sleep 30",
    ]);
}

fn attach(mux: &Mux, name: &str, width: u16, height: u16) -> (PtyTest, ptytest::TerminalBaseline) {
    let mut terminal = pty_harness::spawn_attached(
        mux,
        name,
        &["attach-session", "-t", "S"],
        Size::new(width, height).expect("size"),
    );
    pty_harness::wait_for_text(&mut terminal, "PANE_READY", Duration::from_secs(3));
    let baseline = terminal.terminal_baseline();
    (terminal, baseline)
}

fn status_row(terminal: &PtyTest, height: u16) -> String {
    terminal
        .screen()
        .row(usize::from(height - 1))
        .expect("status row")
}

fn add_windows(mux: &Mux) {
    for name in ["A-first", "B-second", "C-third", "D-middle", "E-fifth", "F-last"] {
        mux.ok(&[
            "new-window",
            "-d",
            "-t",
            "S",
            "-n",
            name,
            "--",
            "sh",
            "-c",
            "sleep 30",
        ]);
    }
}

#[test]
fn status_position_and_colors_bound_the_bottom_row() {
    let scratch = Scratch::new("status-colors");
    let tm = Mux::tm(&scratch, "tm");
    session(&tm, 40, 8);
    let (mut client, baseline) = attach(&tm, "colors", 40, 8);
    client
        .wait_for_quiescence(client.deadline(Duration::from_secs(3)), Duration::from_millis(50))
        .expect("status frame");
    let screen = client.screen();
    let cell = screen.cell(7, 0).expect("status cell");
    assert_eq!(cell.attributes().foreground, Color::Indexed(7));
    assert_eq!(cell.attributes().background, Color::Indexed(0));
    assert!(status_row(&client, 8).contains("(S)"));
    assert_eq!(screen.row_wrapped(7), Some(false));
    pty_harness::detach(&mut client, &baseline);

    let scratch = Scratch::new("status-top");
    let tm = Mux::tm(&scratch, "tm");
    session(&tm, 40, 8);
    tm.ok(&["set-option", "-g", "status-position", "top"]);
    let (mut client, baseline) = attach(&tm, "top", 40, 8);
    client
        .wait_for_quiescence(client.deadline(Duration::from_secs(3)), Duration::from_millis(50))
        .expect("top status frame");
    assert!(client.screen().row(0).is_some_and(|row| row.contains("(S)")));
    assert!(
        client
            .screen()
            .cell(0, 0)
            .is_some_and(|cell| cell.attributes().background == Color::Indexed(0))
    );
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn window_list_truncation_keeps_first_middle_and_last_active_visible() {
    let scratch = Scratch::new("status-windows");
    let tm = Mux::tm(&scratch, "tm");
    session(&tm, 30, 8);
    add_windows(&tm);
    let (mut client, baseline) = attach(&tm, "windows", 30, 8);
    for (index, name) in [(1, "A-first"), (4, "D-middle"), (7, "F-last")] {
        tm.ok(&["select-window", "-t", &format!("S:{index}")]);
        client
            .wait_for_quiescence(client.deadline(Duration::from_secs(3)), Duration::from_millis(50))
            .expect("window status frame");
        let row = status_row(&client, 8);
        assert!(row.contains(name), "active {name} missing from {row:?}");
        assert!(row.chars().count() <= 30, "status row too wide: {row:?}");
        assert_eq!(client.screen().row_wrapped(7), Some(false));
    }
    tm.ok(&["select-window", "-t", "S:1"]);
    tm.ok(&["resize-pane", "-Z", "-t", "S"]);
    client
        .wait_for_quiescence(client.deadline(Duration::from_secs(3)), Duration::from_millis(50))
        .expect("zoom status frame");
    assert!(status_row(&client, 8).contains("(Z)"));
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn status_left_prefix_highlight_and_length_are_bounded() {
    let scratch = Scratch::new("status-left");
    let tm = Mux::tm(&scratch, "tm");
    session(&tm, 30, 8);
    tm.ok(&[
        "set-option",
        "-g",
        "status-left",
        "LEFT0123456789TAIL",
    ]);
    tm.ok(&["set-option", "-g", "status-left-length", "10"]);
    let (mut client, baseline) = attach(&tm, "left", 30, 8);
    let row = status_row(&client, 8);
    assert!(row.contains("LEFT012345"), "{row:?}");
    assert!(!row.contains("TAIL"), "{row:?}");
    assert!(row.chars().count() <= 30, "{row:?}");
    tm.ok(&["set-option", "-g", "status-left", "#{?client_prefix,#[fg=yellow],}(#S) "]);
    tm.ok(&["set-option", "-g", "status-left-length", "32"]);
    client
        .wait_for_quiescence(client.deadline(Duration::from_secs(3)), Duration::from_millis(50))
        .expect("left status frame");
    assert_eq!(
        client
            .screen()
            .cell(7, 0)
            .expect("left cell")
            .attributes()
            .foreground,
        Color::Indexed(7)
    );
    client
        .send_bytes(client.deadline(Duration::from_secs(3)), b"\x01")
        .expect("send prefix");
    client
        .wait_for_quiescence(client.deadline(Duration::from_secs(3)), Duration::from_millis(50))
        .expect("prefix status frame");
    assert_eq!(
        client
            .screen()
            .cell(7, 0)
            .expect("prefix cell")
            .attributes()
            .foreground,
        Color::Indexed(3)
    );
    client
        .send_bytes(client.deadline(Duration::from_secs(3)), b"q")
        .expect("clear prefix");
    client
        .wait_for_quiescence(client.deadline(Duration::from_secs(3)), Duration::from_millis(50))
        .expect("cleared prefix frame");
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn long_messages_prompts_and_unicode_names_never_wrap_the_status_row() {
    let scratch = Scratch::new("status-bounds");
    let tm = Mux::tm(&scratch, "tm");
    session(&tm, 24, 8);
    tm.ok(&[
        "new-window",
        "-d",
        "-t",
        "S",
        "-n",
        "界🙂",
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
    let (mut client, baseline) = attach(&tm, "bounds", 24, 8);
    let long = "LONG_MESSAGE_".repeat(8);
    tm.ok(&["display-message", "-t", "S", &long]);
    client
        .wait_for_quiescence(client.deadline(Duration::from_secs(3)), Duration::from_millis(50))
        .expect("message frame");
    let row = status_row(&client, 8);
    assert!(row.chars().count() <= 24, "{row:?}");
    assert_eq!(client.screen().row_wrapped(7), Some(false));

    client
        .send_bytes(client.deadline(Duration::from_secs(3)), b"\x01/")
        .expect("open prompt");
    client
        .wait_for_quiescence(client.deadline(Duration::from_secs(3)), Duration::from_millis(50))
        .expect("prompt frame");
    let input = "x".repeat(80);
    client
        .send_bytes(client.deadline(Duration::from_secs(3)), input.as_bytes())
        .expect("fill prompt");
    client
        .wait_for_quiescence(client.deadline(Duration::from_secs(3)), Duration::from_millis(50))
        .expect("long prompt frame");
    let screen = client.screen();
    let row = status_row(&client, 8);
    assert!(row.chars().count() <= 24, "{row:?}");
    assert_eq!(screen.row_wrapped(7), Some(false));
    assert!(screen.cursor().column < 24, "cursor escaped the status row");
    client
        .send_bytes(client.deadline(Duration::from_secs(3)), b"\x1b")
        .expect("close prompt");
    client
        .wait_for_quiescence(client.deadline(Duration::from_secs(3)), Duration::from_millis(50))
        .expect("closed prompt frame");

    tm.ok(&["select-window", "-t", "S:1"]);
    tm.ok(&["select-window", "-t", "S:2"]);
    client
        .wait_for_quiescence(client.deadline(Duration::from_secs(3)), Duration::from_millis(50))
        .expect("unicode status frame");
    let row = status_row(&client, 8);
    assert!(row.contains("界🙂"), "{row:?}");
    assert!(row.chars().count() <= 24, "{row:?}");
    pty_harness::detach(&mut client, &baseline);
}
