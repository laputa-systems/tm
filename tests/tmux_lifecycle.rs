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
    let scratch = Scratch::new("lc-perms");
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

#[test]
fn kill_server_then_new_session_succeeds_200_times_on_one_socket() {
    let scratch = Scratch::new("lc-loop");
    let tm = Mux::tm(&scratch, "tm");
    let started = Instant::now();
    for cycle in 0..200 {
        let output = tm.run(&["new-session", "-d", "-s", "S", "--", "sleep", "30"]);
        assert!(
            output.status.success(),
            "cycle {cycle}: new-session failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = tm.run(&["kill-server"]);
        assert!(
            output.status.success(),
            "cycle {cycle}: kill-server failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            std::os::unix::net::UnixStream::connect(tm.socket()).is_err(),
            "cycle {cycle}: socket still accepts after kill-server succeeded"
        );
    }
    assert!(started.elapsed() < Duration::from_secs(9), "{:?}", started.elapsed());
}

#[test]
fn kill_server_restart_matches_the_reference() {
    let scratch = Scratch::new("lc-parity");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    let new_session: &[&str] = &["new-session", "-d", "-s", "S", "--", "sleep", "30"];
    let kill_server: &[&str] = &["kill-server"];
    let list: &[&str] = &["list-sessions", "-F", "#{session_name}"];
    let mut commands = Vec::new();
    for _ in 0..5 {
        commands.push(new_session);
        commands.push(kill_server);
    }
    commands.push(new_session);
    commands.push(list);
    parity::compare_commands("kill_server_restart_matches_the_reference", &tm, &reference, &commands);
}

#[test]
fn attach_racing_kill_server_gets_an_error_or_ends_cleanly() {
    let scratch = Scratch::new("lc-race");
    let tm = Mux::tm(&scratch, "tm");
    for round in 0..8 {
        tm.ok(&["new-session", "-d", "-s", "S", "-x", "40", "-y", "10", "--", "sleep", "30"]);
        let mut terminal = pty_harness::spawn_attached(
            &tm,
            "race",
            &["attach-session", "-t", "S"],
            Size::new(40, 10).expect("size"),
        );
        tm.ok(&["kill-server"]);
        let status = terminal
            .wait_for_exit(terminal.deadline(Duration::from_secs(5)))
            .unwrap_or_else(|error| panic!("round {round}: attach did not exit: {error}"));
        let screen = terminal.screen().to_string();
        match status {
            ptytest::ExitStatus::Code(0) => {}
            ptytest::ExitStatus::Code(1) => {
                assert!(screen.contains("tm:"), "round {round}: silent failure: {screen:?}");
                assert!(
                    !screen.contains("fill whole buffer"),
                    "round {round}: raw EOF surfaced: {screen:?}"
                );
            }
            other => panic!("round {round}: unexpected exit {other:?}: {screen:?}"),
        }
    }
}

#[test]
fn request_sent_after_kill_server_replied_gets_an_explicit_error() {
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};

    let scratch = Scratch::new("lc-shutdown");
    let tm = Mux::tm(&scratch, "tm");
    let recorder_path = tm.root().join("recorder.sock");
    let recorder = UnixListener::bind(&recorder_path).expect("bind request recorder");
    let mut probe = std::process::Command::new(tm.binary())
        .arg("-S")
        .arg(&recorder_path)
        .arg("list-sessions")
        .env_remove("TM_SOCKET")
        .env_remove("TMUX")
        .spawn()
        .expect("spawn recording client");
    let (mut captured, _) = recorder.accept().expect("accept recording client");
    captured
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("read timeout");
    let mut request = vec![0_u8; 4096];
    let length = captured.read(&mut request).expect("read recorded request");
    request.truncate(length);
    drop(captured);
    let _ = probe.wait();

    tm.ok(&["new-session", "-d", "-s", "S", "--", "sleep", "30"]);
    let mut early = UnixStream::connect(tm.socket()).expect("connect before kill-server");
    early
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("read timeout");
    tm.ok(&["kill-server"]);
    assert!(UnixStream::connect(tm.socket()).is_err());
    early.write_all(&request).expect("write request during shutdown");
    let mut response = Vec::new();
    let mut chunk = [0_u8; 256];
    loop {
        match early.read(&mut chunk) {
            Ok(0) => break,
            Ok(length) => response.extend_from_slice(&chunk[..length]),
            Err(error) => panic!("read response: {error}"),
        }
    }
    let text = String::from_utf8_lossy(&response);
    assert!(text.contains("server is shutting down"), "{text:?}");
}
