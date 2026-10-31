#![cfg(unix)]
#![cfg(feature = "parity")]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/pty_harness.rs"]
mod pty_harness;

use std::time::Duration;

use parity::{Mux, Scratch};
use ptytest::Size;

const WIDTH: &str = "80";
const HEIGHT: &str = "24";

fn create_sleep_session(mux: &Mux) {
    mux.ok(&[
        "new-session",
        "-d",
        "-s",
        "parity",
        "-x",
        WIDTH,
        "-y",
        HEIGHT,
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
}

#[test]
fn reference_profile_config_loads_and_lists_the_compiled_table() {
    let scratch = Scratch::new("profile");
    let reference = Mux::reference(&scratch, "reference");
    create_sleep_session(&reference);
    reference.validate_reference_profile();
}

#[test]
fn even_horizontal_geometry_matches_reference() {
    let scratch = Scratch::new("layout-gap");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    create_sleep_session(&tm);
    create_sleep_session(&reference);
    for mux in [&tm, &reference] {
        mux.ok(&["split-window", "-h", "-t", "parity"]);
        mux.ok(&["split-window", "-h", "-t", "parity"]);
        mux.ok(&["select-layout", "-t", "parity", "even-horizontal"]);
    }
    let tm_geometry = tm.body(&[
        "list-panes",
        "-t",
        "parity",
        "-F",
        "#{pane_width}",
    ]);
    let reference_geometry = reference.body(&[
        "list-panes",
        "-t",
        "parity",
        "-F",
        "#{pane_width}",
    ]);
    assert_eq!(tm_geometry, "26\n26\n26");
    assert_eq!(tm_geometry, reference_geometry);
}

#[test]
fn relative_window_targets_match_reference() {
    let scratch = Scratch::new("relative-targets");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    for mux in [&tm, &reference] {
        mux.ok(&[
            "new-session",
            "-d",
            "-s",
            "B",
            "-n",
            "b1",
            "--",
            "sh",
            "-c",
            "sleep 30",
        ]);
        mux.ok(&["new-window", "-d", "-t", "B", "-n", "b2"]);
        mux.ok(&["new-window", "-d", "-t", "B", "-n", "b3"]);
    }
    parity::compare_commands(
        &tm,
        &reference,
        &[
            &["select-window", "-t", "B:+1"],
            &["display-message", "-p", "-t", "B", "#{window_index}:#{window_name}"],
            &["select-window", "-t", "B:-1"],
            &["display-message", "-p", "-t", "B", "#{window_index}:#{window_name}"],
            &["swap-window", "-t", "B:+1"],
            &["select-window", "-t", "B:+1"],
            &["list-windows", "-t", "B", "-F", "#{window_index}:#{window_name}"],
        ],
    );
}

#[test]
fn break_and_join_preflight_matches_reference() {
    let scratch = Scratch::new("break-join-parity");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    for mux in [&tm, &reference] {
        mux.ok(&[
            "new-session",
            "-d",
            "-s",
            "break",
            "-n",
            "one",
            "--",
            "sh",
            "-c",
            "sleep 30",
        ]);
        mux.ok(&["new-window", "-d", "-t", "break", "-n", "two"]);
        mux.ok(&["split-window", "-h", "-t", "break:1"]);
        mux.ok(&[
            "new-session",
            "-d",
            "-s",
            "sole",
            "-n",
            "one",
            "--",
            "sh",
            "-c",
            "sleep 30",
        ]);
        mux.ok(&[
            "new-session",
            "-d",
            "-s",
            "join",
            "-n",
            "one",
            "--",
            "sh",
            "-c",
            "sleep 30",
        ]);
        mux.ok(&["new-window", "-d", "-t", "join", "-n", "two"]);
        mux.ok(&["split-window", "-h", "-t", "join:1"]);
    }
    parity::compare_commands(
        &tm,
        &reference,
        &[
            &["break-pane", "-t", "break:2"],
            &[
                "list-windows",
                "-t",
                "break",
                "-F",
                "#{window_index}:#{window_panes}",
            ],
            &["break-pane", "-s", "sole:1.0", "-t", "sole:"],
            &[
                "list-windows",
                "-t",
                "sole",
                "-F",
                "#{window_index}:#{window_panes}",
            ],
            &["join-pane", "-t", "join:9"],
            &[
                "list-panes",
                "-t",
                "join",
                "-F",
                "#{pane_id}:#{pane_index}",
            ],
        ],
    );
}

#[test]
fn attached_client_renders_normal_profile_fixture() {
    let scratch = Scratch::new("attached-screen");
    let tm = Mux::tm(&scratch, "tm");
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "parity",
        "-x",
        WIDTH,
        "-y",
        HEIGHT,
        "--",
        "sh",
        "-c",
        "printf 'PARITY_READY\\n'; sleep 30",
    ]);
    let size = Size::new(WIDTH.parse().expect("width"), HEIGHT.parse().expect("height"))
        .expect("constant size");
    let mut terminal =
        pty_harness::spawn_attached(&tm, "screen", &["attach-session", "-t", "parity"], size);
    let baseline = terminal.terminal_baseline();
    pty_harness::wait_for_text(&mut terminal, "PARITY_READY", Duration::from_secs(3));
    pty_harness::detach(&mut terminal, &baseline);
}

#[test]
fn pane_input_bytes_match_reference() {
    let scratch = Scratch::new("pane-bytes");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    for mux in [&tm, &reference] {
        mux.ok(&[
            "new-session",
            "-d",
            "-s",
            "parity",
            "-x",
            WIDTH,
            "-y",
            HEIGHT,
            "--",
            "sh",
            "-c",
            "stty -echo raw; printf 'READY\\n'; od -An -tx1 -N3; sleep 30",
        ]);
    }
    for mux in [&tm, &reference] {
        mux.capture_until("parity", "READY", Duration::from_secs(3));
    }
    parity::compare_commands(&tm, &reference, &[&["send-keys", "-t", "parity", "a", "b", "Enter"]]);
    let tm_capture = tm.capture_until("parity", "61", Duration::from_secs(3));
    let reference_capture =
        reference.capture_until("parity", "61", Duration::from_secs(3));
    assert_eq!(
        hex_tokens(&tm_capture),
        "61 62 0d",
        "tm did not deliver the exact pane bytes: {tm_capture:?}"
    );
    assert_eq!(
        hex_tokens(&tm_capture),
        hex_tokens(&reference_capture),
        "pane input bytes differ from the reference"
    );
}

fn hex_tokens(capture: &str) -> String {
    capture
        .split_whitespace()
        .filter(|token| token.len() == 2 && token.chars().all(|c| c.is_ascii_hexdigit()))
        .collect::<Vec<_>>()
        .join(" ")
}
