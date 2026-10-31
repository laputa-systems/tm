#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/pty_harness.rs"]
mod pty_harness;

use std::time::Duration;

use parity::{Mux, Scratch};
use ptytest::Size;

fn directory(scratch: &Scratch, name: &str) -> String {
    let path = scratch.root().join(name);
    std::fs::create_dir_all(&path).expect("create cwd directory");
    path.to_string_lossy().into_owned()
}

fn create_cwd_session(mux: &Mux, session: &str, directory: &str, osc7: Option<&str>) {
    let command = match osc7 {
        Some(reported) => format!(
            "printf '\\033]7;file://localhost{reported}\\007'; cd '{directory}'; exec sleep 30"
        ),
        None => format!("cd '{directory}'; exec sleep 30"),
    };
    mux.ok(&[
        "new-session",
        "-d",
        "-s",
        session,
        "--",
        "/bin/sh",
        "-c",
        &command,
    ]);
}

fn wait_path(mux: &Mux, target: &str, expected: &str) -> String {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let path = mux.body(&[
            "display-message",
            "-p",
            "-t",
            target,
            "#{pane_current_path}",
        ]);
        if path == expected || std::time::Instant::now() >= deadline {
            return path;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn foreground_shell_cwd_matches_reference() {
    let scratch = Scratch::new("cwd-foreground");
    let directory = directory(&scratch, "shell-cwd");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    create_cwd_session(&tm, "S", &directory, None);
    create_cwd_session(&reference, "S", &directory, None);
    assert_eq!(wait_path(&tm, "S", &directory), directory);
    parity::compare_commands(
        &tm,
        &reference,
        &[&[
            "display-message",
            "-p",
            "-t",
            "S",
            "#{pane_current_path}",
        ]],
    );
}

#[test]
fn osc7_path_takes_precedence_over_the_process_cwd() {
    let scratch = Scratch::new("cwd-osc7");
    let reported = directory(&scratch, "reported");
    let actual = directory(&scratch, "actual");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    create_cwd_session(&tm, "S", &actual, Some(&reported));
    create_cwd_session(&reference, "S", &actual, Some(&reported));
    assert_eq!(wait_path(&tm, "S", &reported), reported);
    assert_eq!(wait_path(&reference, "S", &actual), actual);
    assert_ne!(
        tm.body(&[
            "display-message",
            "-p",
            "-t",
            "S",
            "#{pane_current_path}"
        ]),
        reference.body(&[
            "display-message",
            "-p",
            "-t",
            "S",
            "#{pane_current_path}"
        ]),
        "tmux 3.8 ignores OSC 7; tm intentionally prefers the shell report"
    );
}

#[test]
fn split_binding_inherits_the_pane_current_path() {
    let scratch = Scratch::new("cwd-split");
    let directory = directory(&scratch, "split-cwd");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    create_cwd_session(&tm, "S", &directory, None);
    create_cwd_session(&reference, "S", &directory, None);
    assert_eq!(wait_path(&tm, "S", &directory), directory);
    assert_eq!(wait_path(&reference, "S", &directory), directory);

    let size = Size::new(40, 8).expect("constant size");
    let mut client = pty_harness::spawn_attached(
        &tm,
        "cwd-split",
        &["attach-session", "-t", "S"],
        size,
    );
    pty_harness::wait_for_text(&mut client, "(S)", Duration::from_secs(3));
    let baseline = client.terminal_baseline();
    client
        .send_bytes(client.deadline(Duration::from_secs(3)), b"\x01\\")
        .expect("split through binding");
    pty_harness::detach(&mut client, &baseline);

    reference.ok(&["split-window", "-h", "-c", &directory, "-t", "S"]);
    parity::compare_commands(
        &tm,
        &reference,
        &[&["list-panes", "-t", "S", "-F", "#{pane_current_path}"]],
    );
}
