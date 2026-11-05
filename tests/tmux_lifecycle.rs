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

fn process_table() -> Vec<(u32, u32, String)> {
    let output = std::process::Command::new("ps")
        .args(["-A", "-o", "pid=,ppid=,stat="])
        .output()
        .expect("run ps");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some((
                fields.next()?.parse().ok()?,
                fields.next()?.parse().ok()?,
                fields.next()?.to_owned(),
            ))
        })
        .collect()
}

fn defunct_children(server: u32) -> Vec<u32> {
    process_table()
        .into_iter()
        .filter(|(_, parent, state)| *parent == server && state.starts_with('Z'))
        .map(|(pid, _, _)| pid)
        .collect()
}

fn server_pid(mux: &Mux) -> u32 {
    let pane: u32 = mux
        .body(&["display-message", "-p", "-t", "S", "#{pane_pid}"])
        .parse()
        .expect("pane pid");
    process_table()
        .into_iter()
        .find(|(pid, _, _)| *pid == pane)
        .map(|(_, parent, _)| parent)
        .expect("pane process has a parent")
}

fn wait_pane_pipe(mux: &Mux, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let value = mux.body(&["display-message", "-p", "-t", "S", "#{pane_pipe}"]);
        if value == expected || Instant::now() >= deadline {
            assert_eq!(value, expected, "{:?} pane_pipe", mux.implementation());
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn exited_pipe_helper_is_reaped_and_pane_pipe_follows_the_reference() {
    let scratch = Scratch::new("lc-pipe");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "ref");
    for mux in [&tm, &reference] {
        mux.ok(&["new-session", "-d", "-s", "S", "-x", "80", "-y", "8", "--", "cat"]);
        let server = server_pid(mux);
        mux.ok(&["pipe-pane", "-t", "S", "--", "true"]);
        let reaped_by = Instant::now() + Duration::from_secs(1);
        loop {
            let defunct = defunct_children(server);
            if defunct.is_empty() {
                break;
            }
            assert!(
                Instant::now() < reaped_by,
                "{:?} left exited pipe helpers {defunct:?} unreaped",
                mux.implementation()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    parity::compare_commands(
        "exited_pipe_helper_is_reaped_and_pane_pipe_follows_the_reference",
        &tm,
        &reference,
        &[&["display-message", "-p", "-t", "S", "#{pane_pipe}"]],
    );
    for mux in [&tm, &reference] {
        wait_pane_pipe(mux, "1");
        mux.ok(&["send-keys", "-t", "S", "hello", "Enter"]);
        wait_pane_pipe(mux, "0");
    }
    for mux in [&tm, &reference] {
        mux.ok(&["pipe-pane", "-t", "S", "--", "cat > /dev/null"]);
        wait_pane_pipe(mux, "1");
        mux.ok(&["pipe-pane", "-t", "S"]);
        wait_pane_pipe(mux, "0");
    }
}

fn daemon_running(mux: &Mux) -> bool {
    let socket = mux.socket().to_string_lossy().into_owned();
    let output = std::process::Command::new("ps")
        .args(["-A", "-o", "args="])
        .output()
        .expect("run ps");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line.contains("--daemon") && line.contains(&socket))
}

fn wait_server_gone(mux: &Mux, route: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while mux.run(&["has-session"]).status.success() {
        assert!(Instant::now() < deadline, "{route}: server still answers after the last session went away");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn removing_the_last_session_ends_the_server_like_the_reference() {
    let test = "removing_the_last_session_ends_the_server_like_the_reference";
    let routes: [(&str, &[&str]); 4] = [
        ("kill-session", &["kill-session", "-t", "S"]),
        ("kill-window", &["kill-window", "-t", "S"]),
        ("kill-pane", &["kill-pane", "-t", "S"]),
        ("pane exit", &["send-keys", "-t", "S", "Enter"]),
    ];
    for (route, arguments) in routes {
        let scratch = Scratch::new("lc-empty");
        let tm = Mux::tm(&scratch, "tm");
        let reference = Mux::reference(&scratch, "reference");
        for mux in [&tm, &reference] {
            mux.ok(&["new-session", "-d", "-s", "S", "--", "sh", "-c", "read line"]);
            assert!(mux.run(&["has-session", "-t", "S"]).status.success(), "{route}");
        }
        for mux in [&tm, &reference] {
            let outcome = mux.outcome(arguments);
            assert_eq!(outcome.status, parity::ExitStatus::Code(0), "{route}: {outcome:?}");
        }
        for mux in [&tm, &reference] {
            wait_server_gone(mux, route);
        }
        parity::compare_commands(
            test,
            &tm,
            &reference,
            &[&["has-session"], &["list-sessions"], &["kill-server"]],
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while daemon_running(&tm) {
            assert!(Instant::now() < deadline, "{route}: tm daemon still running");
            std::thread::sleep(Duration::from_millis(20));
        }
        parity::compare_commands(
            test,
            &tm,
            &reference,
            &[
                &["new-session", "-d", "-s", "again", "--", "sleep", "300"],
                &["list-sessions", "-F", "#{session_name} #{session_windows}"],
            ],
        );
    }
}

#[test]
fn commands_without_a_server_fail_like_the_reference() {
    let test = "commands_without_a_server_fail_like_the_reference";
    let commands: [&[&str]; 8] = [
        &["has-session"],
        &["has-session", "-t", "a"],
        &["list-sessions"],
        &["list-windows", "-t", "a"],
        &["kill-server"],
        &["send-keys", "-t", "a", "x"],
        &["display-message", "-p", "x"],
        &["kill-session", "-t", "a"],
    ];
    let scratch = Scratch::new("lc-noserver");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    parity::compare_commands(test, &tm, &reference, &commands);
    for mux in [&tm, &reference] {
        drop(std::os::unix::net::UnixListener::bind(mux.socket()).expect("bind stale socket"));
    }
    parity::compare_commands(test, &tm, &reference, &commands);
}
