#![cfg(unix)]

#[path = "support/tm_bin.rs"]
mod tm_bin;

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
    Command::new(tm_bin::tm_bin())
        .env("TM_SOCKET", socket)
        .args(arguments)
        .output()
        .expect("run tm")
}

fn capture_until(socket: &str, target: &str, marker: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let output = run(socket, &["capture-pane", "-p", "-t", target]);
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

    let mut captured = run(&socket, &["capture-pane", "-p", "-t", "smoke"]);
    for _ in 0..50 {
        if String::from_utf8_lossy(&captured.stdout).contains("ready") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        captured = run(&socket, &["capture-pane", "-p", "-t", "smoke"]);
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

#[test]
fn descriptor_exhaustion_does_not_destroy_existing_sessions() {
    use std::os::unix::net::UnixStream;
    use std::os::unix::process::CommandExt;

    let socket = scratch_root()
        .join(format!("ex-{}-{}.sock", std::process::id(), NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)))
        .to_string_lossy().into_owned();
    let _guard = TmGuard(socket.clone());
    let mut command = Command::new(tm_bin::tm_bin());
    command.env("TM_SOCKET", &socket).args(["new-session", "-d", "-s", "survivor", "--", "sleep", "30"]);
    // SAFETY: setrlimit is async-signal-safe and affects only the child.
    unsafe {
        command.pre_exec(|| {
            let limit = libc::rlimit { rlim_cur: 128, rlim_max: 128 };
            if libc::setrlimit(libc::RLIMIT_NOFILE, &limit) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let created = command.output().expect("create session");
    assert!(created.status.success(), "{}", String::from_utf8_lossy(&created.stderr));
    let mut idle = Vec::new();
    for _ in 0..180 {
        match UnixStream::connect(&socket) {
            Ok(stream) => idle.push(stream),
            Err(_) => break,
        }
    }
    assert!(idle.len() >= 100, "only {} connections opened", idle.len());
    use std::io::Read;
    let last = idle.last_mut().expect("last idle connection");
    last.set_read_timeout(Some(Duration::from_secs(8))).expect("set timeout");
    let mut byte = [0];
    assert_eq!(last.read(&mut byte).expect("connection should be closed"), 0);
    drop(idle);
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let result = run(&socket, &["list-sessions"]);
        if result.status.success() && String::from_utf8_lossy(&result.stdout).contains("survivor:") {
            break;
        }
        assert!(Instant::now() < deadline, "existing session lost: {}", String::from_utf8_lossy(&result.stderr));
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn silent_handshake_expires() {
    use std::io::Read;
    use std::os::unix::net::UnixStream;

    let socket = scratch_root()
        .join(format!("hs-{}-{}.sock", std::process::id(), NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)))
        .to_string_lossy().into_owned();
    let _guard = TmGuard(socket.clone());
    let created = run(&socket, &["new-session", "-d", "-s", "handshake", "--", "sleep", "30"]);
    assert!(created.status.success(), "{}", String::from_utf8_lossy(&created.stderr));
    let mut stream = UnixStream::connect(&socket).expect("connect");
    stream.set_read_timeout(Some(Duration::from_secs(10))).expect("set timeout");
    assert_eq!(stream.read(&mut [0]).expect("handshake closes"), 0);
    assert!(run(&socket, &["has-session", "-t", "handshake"]).status.success());
}
