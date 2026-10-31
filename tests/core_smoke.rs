#![cfg(unix)]

use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(0);

struct TmGuard(String);

impl Drop for TmGuard {
    fn drop(&mut self) {
        let _ = run(&self.0, &["kill-server"]);
    }
}

fn scratch_root() -> std::path::PathBuf {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".work/tmp");
    std::fs::create_dir_all(&root).expect("create scratch root");
    root
}

fn run(socket: &str, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tm"))
        .env("TM_SOCKET", socket)
        .args(arguments)
        .output()
        .expect("run tm")
}

fn capture_until(socket: &str, target: &str, marker: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let output = run(socket, &["capture-pane", "-t", target]);
        let captured = String::from_utf8_lossy(&output.stdout).into_owned();
        if captured.contains(marker) || Instant::now() >= deadline {
            return captured;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn core_session_pane_and_pty_lifecycle_survives_the_daemon_boundary() {
    let socket = scratch_root()
        .join(format!(
            "tm-core-{}-{}.sock",
            std::process::id(),
            NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
        ))
        .to_string_lossy()
        .into_owned();
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

    let io_session = run(
        &socket,
        &[
            "new-session",
            "-d",
            "-s",
            "io",
            "--",
            "sh",
            "-c",
            "read line; printf 'SMOKE_ACK_%s\\n' \"$line\"; sleep 30",
        ],
    );
    assert!(io_session.status.success());
    let sent = run(&socket, &["send-keys", "-t", "io", "42", "Enter"]);
    assert!(
        sent.status.success(),
        "{}",
        String::from_utf8_lossy(&sent.stderr)
    );
    assert!(
        capture_until(&socket, "io", "SMOKE_ACK_42").contains("SMOKE_ACK_42"),
        "the pane command did not prove its own output"
    );

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
