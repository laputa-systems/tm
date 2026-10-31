#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/pty_harness.rs"]
#[allow(dead_code)]
mod pty_harness;

use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

use parity::{Mux, Scratch};
use ptytest::Size;

fn client_count(mux: &Mux) -> usize {
    mux.ok(&["list-clients", "-F", "#{client_name}"])
        .lines()
        .filter(|line| !line.is_empty())
        .count()
}

fn wait_clients(mux: &Mux, expected: usize) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let count = client_count(mux);
        if count == expected || Instant::now() >= deadline {
            assert_eq!(count, expected);
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn concurrent_startup_converges_and_replaces_a_stale_socket() {
    let scratch = Scratch::new("lifecycle-startup");
    let tm = Mux::tm(&scratch, "tm");
    std::fs::write(tm.socket(), b"stale").expect("write stale socket path");
    let mut first = tm.command(&["new-session", "-d", "-s", "one", "--", "sleep", "30"]);
    let mut second = tm.command(&["new-session", "-d", "-s", "two", "--", "sleep", "30"]);
    let first = first.spawn().expect("spawn first client");
    let second = second.spawn().expect("spawn second client");
    let first = first.wait_with_output().expect("wait first client");
    let second = second.wait_with_output().expect("wait second client");
    assert!(first.status.success(), "{}", String::from_utf8_lossy(&first.stderr));
    assert!(second.status.success(), "{}", String::from_utf8_lossy(&second.stderr));
    let sessions = tm.ok(&["list-sessions", "-F", "#{session_name}"]);
    assert!(sessions.lines().any(|line| line == "one"), "{sessions:?}");
    assert!(sessions.lines().any(|line| line == "two"), "{sessions:?}");
    wait_clients(&tm, 0);
}

#[test]
fn socket_and_diagnostics_keep_private_permissions() {
    let scratch = Scratch::new("lifecycle-permissions");
    let tm = Mux::tm(&scratch, "tm");
    std::fs::set_permissions(tm.root(), std::fs::Permissions::from_mode(0o700))
        .expect("private scratch root");
    tm.ok(&["new-session", "-d", "-s", "S", "--", "sleep", "30"]);
    let socket = std::fs::metadata(tm.socket()).expect("socket metadata");
    assert_eq!(socket.permissions().mode() & 0o777, 0o600);
    let parent = std::fs::metadata(tm.socket().parent().expect("socket parent"))
        .expect("parent metadata");
    assert_eq!(parent.permissions().mode() & 0o777, 0o700);
    let mut log = tm.socket().as_os_str().to_owned();
    log.push(".log");
    let log = std::path::PathBuf::from(log);
    let metadata = std::fs::metadata(&log).expect("daemon log");
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    let contents = std::fs::read_to_string(&log).expect("read daemon log");
    assert!(contents.contains("listening on"), "{contents:?}");
}

#[test]
fn idle_disconnect_releases_the_client_registration() {
    let scratch = Scratch::new("lifecycle-idle");
    let tm = Mux::tm(&scratch, "tm");
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "S",
        "-x",
        "40",
        "-y",
        "10",
        "--",
        "sleep",
        "30",
    ]);
    let terminal = pty_harness::spawn_attached(
        &tm,
        "idle",
        &["attach-session", "-t", "S"],
        Size::new(40, 10).expect("size"),
    );
    wait_clients(&tm, 1);
    drop(terminal);
    wait_clients(&tm, 0);
    assert!(tm.ok(&["has-session", "-t", "S"]).is_empty());
}
