#![cfg(unix)]

use std::process::{Command, Output};

struct TmGuard(String);

impl Drop for TmGuard {
    fn drop(&mut self) {
        let _ = run(&self.0, &["kill-server"]);
    }
}

fn run(socket: &str, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tm"))
        .env("TM_SOCKET", socket)
        .args(arguments)
        .output()
        .expect("run tm")
}

#[test]
fn core_session_pane_and_pty_lifecycle_survives_the_daemon_boundary() {
    let socket = format!(
        "/tmp/tm-test-{}-{}.sock",
        std::process::id(),
        std::thread::current().name().unwrap_or("core")
    );
    let _guard = TmGuard(socket.clone());

    let created = run(
        &socket,
        &[
            "new-session",
            "-d",
            "-s",
            "smoke",
            "--",
            "sh",
            "-c",
            "printf ready; sleep 30",
        ],
    );
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );

    let mut captured = run(&socket, &["capture-pane", "-t", "smoke"]);
    for _ in 0..50 {
        if String::from_utf8_lossy(&captured.stdout).contains("ready") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        captured = run(&socket, &["capture-pane", "-t", "smoke"]);
    }
    assert!(captured.status.success());
    assert!(String::from_utf8_lossy(&captured.stdout).contains("ready"));

    let split = run(
        &socket,
        &[
            "split-window",
            "-h",
            "-t",
            "smoke",
            "--",
            "sh",
            "-c",
            "printf second; sleep 30",
        ],
    );
    assert!(
        split.status.success(),
        "{}",
        String::from_utf8_lossy(&split.stderr)
    );

    let panes = run(&socket, &["list-panes", "-t", "smoke"]);
    let panes = String::from_utf8_lossy(&panes.stdout);
    assert!(panes.contains("%0"));
    assert!(panes.contains("%1"));

    let io_session = run(&socket, &["new-session", "-d", "-s", "io", "--", "sh"]);
    assert!(io_session.status.success());
    let sent = run(&socket, &["send-keys", "-t", "io", "printf hello", "Enter"]);
    assert!(
        sent.status.success(),
        "{}",
        String::from_utf8_lossy(&sent.stderr)
    );
    std::thread::sleep(std::time::Duration::from_millis(100));
    let io_capture = run(&socket, &["capture-pane", "-t", "io"]);
    assert!(String::from_utf8_lossy(&io_capture.stdout).contains("hello"));

    let renamed = run(&socket, &["rename-session", "-t", "io", "renamed"]);
    assert!(renamed.status.success());
    let sessions = run(&socket, &["list-sessions"]);
    let sessions = String::from_utf8_lossy(&sessions.stdout);
    assert!(sessions.contains("smoke: 1 windows"));
    assert!(sessions.contains("renamed: 1 windows"));

    let killed = run(&socket, &["kill-server"]);
    assert!(
        killed.status.success(),
        "{}",
        String::from_utf8_lossy(&killed.stderr)
    );
}
