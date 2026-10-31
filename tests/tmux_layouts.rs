#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;

use parity::{Mux, Scratch};

fn create_session(mux: &Mux, session: &str, width: &str, height: &str) {
    mux.ok(&[
        "new-session",
        "-d",
        "-s",
        session,
        "-x",
        width,
        "-y",
        height,
        "--",
        "sh",
        "-c",
        "sleep 30",
    ]);
}

fn compare(tm: &Mux, reference: &Mux, commands: &[&[&str]]) {
    parity::compare_commands(tm, reference, commands);
}

#[test]
fn even_layouts_match_reference_for_two_to_five_panes() {
    let scratch = Scratch::new("layouts-even");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    for (width, height) in [("80", "24"), ("79", "23")] {
        for count in 2..=5 {
            let session = format!("H{count}x{width}");
            for mux in [&tm, &reference] {
                create_session(mux, &session, width, height);
                for _ in 1..count {
                    mux.ok(&["split-window", "-h", "-t", &session]);
                }
                mux.ok(&["select-layout", "-t", &session, "even-horizontal"]);
            }
            compare(
                &tm,
                &reference,
                &[
                    &[
                        "list-panes",
                        "-t",
                        &session,
                        "-F",
                        "#{pane_width}:#{pane_left}",
                    ],
                ],
            );
        }
        for mux in [&tm, &reference] {
            mux.ok(&["set-option", "-g", "status", "off"]);
        }
        for count in 2..=5 {
            let session = format!("V{count}x{width}");
            let vertical_height = if height == "24" { "40" } else { "39" };
            for mux in [&tm, &reference] {
                create_session(mux, &session, width, vertical_height);
                for _ in 1..count {
                    mux.ok(&["split-window", "-v", "-t", &session]);
                }
                mux.ok(&["select-layout", "-t", &session, "even-vertical"]);
            }
            compare(
                &tm,
                &reference,
                &[
                    &[
                        "list-panes",
                        "-t",
                        &session,
                        "-F",
                        "#{pane_height}:#{pane_top}",
                    ],
                ],
            );
        }
    }
}

#[test]
fn layout_recomputes_after_border_resize_and_middle_close() {
    let scratch = Scratch::new("layouts-resize");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    for mux in [&tm, &reference] {
        create_session(mux, "R", "80", "24");
        for _ in 1..4 {
            mux.ok(&["split-window", "-h", "-t", "R"]);
        }
        mux.ok(&["select-layout", "-t", "R", "even-horizontal"]);
    }
    for mux in [&tm, &reference] {
        mux.ok(&["resize-pane", "-t", "R:1.1", "-x", "10"]);
        mux.ok(&["select-layout", "-t", "R", "even-horizontal"]);
    }
    compare(
        &tm,
        &reference,
        &[
            &["list-panes", "-t", "R", "-F", "#{pane_width}:#{pane_left}"],
        ],
    );
    for mux in [&tm, &reference] {
        mux.ok(&["kill-pane", "-t", "R:1.1"]);
        mux.ok(&["select-layout", "-t", "R", "even-horizontal"]);
    }
    compare(
        &tm,
        &reference,
        &[
            &["list-panes", "-t", "R", "-F", "#{pane_width}:#{pane_left}"],
        ],
    );
}

#[test]
fn pane_navigation_and_last_targets_match_reference() {
    let scratch = Scratch::new("layouts-navigation");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    for mux in [&tm, &reference] {
        create_session(mux, "N", "80", "24");
        for _ in 1..3 {
            mux.ok(&["split-window", "-h", "-t", "N"]);
        }
        mux.ok(&["select-layout", "-t", "N", "even-horizontal"]);
        mux.ok(&["new-window", "-d", "-t", "N", "-n", "second"]);
    }
    compare(
        &tm,
        &reference,
        &[
            &["select-pane", "-L", "-t", "N:1"],
            &["display-message", "-p", "-t", "N:1", "#{pane_index}"],
            &["select-pane", "-R", "-t", "N:1"],
            &["display-message", "-p", "-t", "N:1", "#{pane_index}"],
            &["select-pane", "-l", "-t", "N:1"],
            &["display-message", "-p", "-t", "N:1", "#{pane_index}"],
            &["select-window", "-t", "N:1"],
            &["select-window", "-t", "N:2"],
            &["last-window", "-t", "N"],
            &["display-message", "-p", "-t", "N", "#{window_index}"],
        ],
    );
}

#[test]
fn zoom_and_unzoom_match_reference_geometry() {
    let scratch = Scratch::new("layouts-zoom");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    for mux in [&tm, &reference] {
        create_session(mux, "Z", "80", "24");
        for _ in 1..3 {
            mux.ok(&["split-window", "-h", "-t", "Z"]);
        }
        mux.ok(&["select-layout", "-t", "Z", "even-horizontal"]);
    }
    compare(
        &tm,
        &reference,
        &[
            &["resize-pane", "-Z", "-t", "Z:1"],
            &[
                "display-message",
                "-p",
                "-t",
                "Z:1",
                "#{window_zoomed_flag}:#{pane_width}",
            ],
            &["resize-pane", "-Z", "-t", "Z:1"],
            &[
                "list-panes",
                "-t",
                "Z",
                "-F",
                "#{pane_width}:#{pane_left}",
            ],
        ],
    );
}
