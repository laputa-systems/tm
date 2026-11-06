#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/raw_pty.rs"]
#[allow(dead_code)]
mod raw_pty;

use std::time::Duration;

use parity::{Mux, Scratch};
use raw_pty::RawClient;

const TIMEOUT: Duration = Duration::from_secs(5);

fn session(mux: &Mux) {
    mux.ok(&[
        "new-session", "-d", "-s", "S", "-x", "40", "-y", "10", "--", "sh", "-c",
        "printf 'SIGNAL_READY\\n'; sleep 300",
    ]);
}

struct Closed {
    status: Option<i32>,
    screen: String,
    termios: raw_pty::Termios,
    modes: raw_pty::ObservedModes,
}

fn attach_and_signal(mux: &Mux, signal: i32) -> Closed {
    session(mux);
    let mut client = RawClient::spawn(mux, "signal", &["attach-session", "-t", "S"], 10, 40);
    client.wait_for_text("SIGNAL_READY", TIMEOUT);
    assert!(client.modes().alternate_screen);
    client.signal(signal);
    let status = client.finish(TIMEOUT);
    Closed {
        status,
        screen: client.screen_text(),
        termios: client.termios(),
        modes: client.modes(),
    }
}

#[test]
fn terminate_and_hangup_signals_restore_the_terminal_like_the_reference() {
    let scratch = Scratch::new("rows-signals");
    for (signal, message) in [(libc::SIGTERM, "[terminated]"), (libc::SIGHUP, "[lost tty]")] {
        let tm = attach_and_signal(&Mux::tm(&scratch, &format!("tm{signal}")), signal);
        let reference = attach_and_signal(&Mux::reference(&scratch, &format!("ref{signal}")), signal);
        assert_eq!(tm.status, Some(1), "signal {signal}");
        assert_eq!(reference.status, Some(1), "signal {signal}");
        assert_eq!(tm.screen, message);
        assert_eq!(reference.screen, message);
        assert_eq!(tm.termios, reference.termios, "termios after signal {signal}");
        assert_eq!(tm.modes, reference.modes, "terminal modes after signal {signal}");
        assert!(!tm.modes.alternate_screen && !tm.modes.bracketed_paste);
    }
}

#[test]
fn external_stop_signal_suspends_tm_and_is_ignored_by_tmux() {
    let scratch = Scratch::new("rows-stop");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");

    session(&tm);
    let mut client = RawClient::spawn(&tm, "stop", &["attach-session", "-t", "S"], 10, 40);
    client.wait_for_text("SIGNAL_READY", TIMEOUT);
    client.signal(libc::SIGTSTP);
    client.wait_for_screen("the alternate screen to close", TIMEOUT, |screen| {
        !screen.alternate_screen()
    });
    client.signal(libc::SIGCONT);
    client.wait_for_screen("the screen to be redrawn", TIMEOUT, |screen| {
        screen.alternate_screen() && screen.contents().contains("SIGNAL_READY")
    });
    assert_eq!(client.detach(), Some(0));

    session(&reference);
    let mut client = RawClient::spawn(&reference, "stop", &["attach-session", "-t", "S"], 10, 40);
    client.wait_for_text("SIGNAL_READY", TIMEOUT);
    client.signal(libc::SIGTSTP);
    client.signal(libc::SIGCONT);
    reference.ok(&["send-keys", "-t", "S", "x"]);
    client.write(b"\x01d");
    assert_eq!(client.finish(TIMEOUT), Some(0));
    assert!(client.screen_text().contains("[detached (from session S)]"));
    assert_eq!(client.count(b"\x1b[?1049l"), 1);
}

fn server_pid(mux: &Mux) -> libc::pid_t {
    mux.body(&["display-message", "-p", "#{pid}"])
        .trim()
        .parse()
        .expect("server pid")
}

#[test]
fn killed_server_leaves_a_socket_that_the_next_command_replaces_like_the_reference() {
    let scratch = Scratch::new("rows-stale");
    let outcomes: Vec<_> = [Mux::tm(&scratch, "tm"), Mux::reference(&scratch, "reference")]
        .iter()
        .map(|mux| {
            mux.ok(&["new-session", "-d", "-s", "old", "--", "sleep", "300"]);
            let pid = server_pid(mux);
            // SAFETY: pid is the server this test just started on its private socket.
            assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);
            let deadline = std::time::Instant::now() + TIMEOUT;
            while unsafe { libc::kill(pid, 0) } == 0 && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(mux.socket().exists(), "the killed server should leave its socket");
            let listing = mux.outcome(&["list-sessions"]);
            mux.ok(&["new-session", "-d", "-s", "new", "--", "sleep", "300"]);
            (
                listing.status,
                mux.body(&["list-sessions", "-F", "#{session_name}"]),
            )
        })
        .collect();
    assert_eq!(outcomes[0], outcomes[1]);
    assert_eq!(outcomes[0].1, "new");
}

#[test]
fn concurrent_first_commands_converge_on_one_server_like_the_reference() {
    let scratch = Scratch::new("rows-concurrent");
    let sessions: Vec<_> = [Mux::tm(&scratch, "tm"), Mux::reference(&scratch, "reference")]
        .iter()
        .map(|mux| {
            let children: Vec<_> = ["one", "two", "three"]
                .iter()
                .map(|name| {
                    mux.command(&["new-session", "-d", "-s", name, "--", "sleep", "300"])
                        .spawn()
                        .expect("spawn client")
                })
                .collect();
            for child in children {
                let output = child.wait_with_output().expect("wait for client");
                assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            }
            let mut names: Vec<_> = mux
                .body(&["list-sessions", "-F", "#{session_name}"])
                .lines()
                .map(str::to_owned)
                .collect();
            names.sort();
            names
        })
        .collect();
    assert_eq!(sessions[0], sessions[1]);
    assert_eq!(sessions[0], ["one", "three", "two"]);
}

#[test]
fn socket_is_private_and_only_tm_writes_a_diagnostic_log() {
    use std::os::unix::fs::PermissionsExt;

    let scratch = Scratch::new("rows-socket");
    let tm = Mux::tm(&scratch, "tm");
    let reference = Mux::reference(&scratch, "reference");
    for mux in [&tm, &reference] {
        std::fs::set_permissions(mux.root(), std::fs::Permissions::from_mode(0o700))
            .expect("private scratch root");
        mux.ok(&["new-session", "-d", "-s", "S", "--", "sleep", "300"]);
        let mode = std::fs::metadata(mux.socket()).expect("socket metadata").permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "{:?} socket mode", mux.implementation());
    }
    let log = |mux: &Mux| {
        let mut path = mux.socket().as_os_str().to_owned();
        path.push(".log");
        std::path::PathBuf::from(path)
    };
    let contents = std::fs::read_to_string(log(&tm)).expect("tm daemon log");
    assert!(contents.contains("listening on"), "{contents:?}");
    assert_eq!(
        std::fs::metadata(log(&tm)).expect("log metadata").permissions().mode() & 0o777,
        0o600
    );
    assert!(!log(&reference).exists());
}

#[test]
fn buffer_limit_drops_the_oldest_buffers_like_the_reference() {
    let scratch = Scratch::new("rows-buffers");
    let listings: Vec<_> = [Mux::tm(&scratch, "tm"), Mux::reference(&scratch, "reference")]
        .iter()
        .map(|mux| {
            mux.ok(&["new-session", "-d", "-s", "S", "--", "sleep", "300"]);
            for index in 0..53 {
                mux.ok(&["set-buffer", &format!("value {index}")]);
            }
            mux.body(&["list-buffers", "-F", "#{buffer_name} #{buffer_size} #{buffer_sample}"])
        })
        .collect();
    assert_eq!(listings[0], listings[1]);
    assert_eq!(listings[0].lines().count(), 50);
}
