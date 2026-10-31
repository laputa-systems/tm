#![cfg(unix)]
#![cfg(feature = "parity")]

#[path = "support/parity.rs"]
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
fn even_horizontal_geometry_documents_the_reference_gap() {
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
    assert_eq!(tm_geometry, "40\n19\n19");
    assert_eq!(reference_geometry, "26\n26\n26");
    assert_ne!(
        tm_geometry, reference_geometry,
        "the initial parity check must reproduce the known even-horizontal mismatch"
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
            "stty -echo raw; od -An -tx1 -N3; sleep 30",
        ]);
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
