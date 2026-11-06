#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/pty_harness.rs"]
#[allow(dead_code)]
mod pty_harness;

use std::time::{Duration, Instant};

use parity::{Mux, Scratch};
use ptytest::{ExitStatus, Size};

fn create_session(mux: &Mux) {
    mux.ok(&[
        "new-session",
        "-d",
        "-s",
        "S",
        "-x",
        "40",
        "-y",
        "10",
        "--",
        "sh",
        "-c",
        "printf 'SIGNAL_READY\\n'; sleep 30",
    ]);
}

fn attach(mux: &Mux, name: &str) -> (ptytest::PtyTest, ptytest::TerminalBaseline) {
    let mut terminal = pty_harness::spawn_attached(
        mux,
        name,
        &["attach-session", "-t", "S"],
        Size::new(40, 10).expect("size"),
    );
    pty_harness::wait_for_text(&mut terminal, "SIGNAL_READY", Duration::from_secs(3));
    let baseline = terminal.terminal_baseline();
    (terminal, baseline)
}

fn attached_client_pid(mux: &Mux) -> String {
    let pattern = format!("{} attach-session", mux.socket().display());
    let output = std::process::Command::new("pgrep")
        .args(["-f", &pattern])
        .output()
        .expect("run pgrep");
    let pids = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert_eq!(pids.len(), 1, "expected one attached client, found {pids:?}");
    pids.into_iter().next().expect("attached client pid")
}

fn wait_until_stopped(pid: &str, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        let output = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", pid])
            .output()
            .expect("run ps");
        let state = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if state.starts_with('T') {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "client {pid} did not stop; state {state:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn terminate_signal_restores_termios_and_screen() {
    let scratch = Scratch::new("signals-term");
    let tm = Mux::tm(&scratch, "tm");
    create_session(&tm);
    let (mut client, baseline) = attach(&tm, "term");
    client.signal(libc::SIGTERM).expect("send SIGTERM");
    assert_eq!(
        client
            .wait_for_exit(client.deadline(Duration::from_secs(3)))
            .expect("wait for signal exit"),
        ExitStatus::Code(143)
    );
    client
        .assert_terminal_restored(&baseline)
        .expect("restore after SIGTERM");
    pty_harness::reap(&mut client, Duration::from_secs(3))
        .expect("reap signalled client");
}

#[test]
fn stop_signal_restores_the_screen_before_stopping() {
    let scratch = Scratch::new("signals-tstp");
    let tm = Mux::tm(&scratch, "tm");
    create_session(&tm);
    let (mut client, _baseline) = attach(&tm, "tstp");
    let pid = attached_client_pid(&tm);
    client.signal(libc::SIGTSTP).expect("send SIGTSTP");
    client
        .wait_for_screen(
            client.deadline(Duration::from_secs(3)),
            "SIGTSTP leaves the alternate screen",
            |screen| !screen.modes().alternate_screen,
        )
        .expect("read stop restoration");
    wait_until_stopped(&pid, Duration::from_secs(3));
    client
        .drain(client.deadline(Duration::from_millis(1)))
        .expect("drain output written before the stop");
    assert!(
        !client.screen().modes().alternate_screen,
        "the stopped client must not re-enter the alternate screen"
    );
    client.signal(libc::SIGKILL).expect("send SIGKILL");
    let _ = client.wait_for_exit(client.deadline(Duration::from_secs(3)));
    pty_harness::reap(&mut client, Duration::from_secs(3))
        .expect("reap stopped client");
}

#[test]
fn daemon_termination_removes_its_private_socket() {
    let scratch = Scratch::new("signals-daemon");
    let tm = Mux::tm(&scratch, "tm");
    create_session(&tm);
    let pattern = format!("tm --daemon {}", tm.socket().display());
    let output = std::process::Command::new("pgrep")
        .args(["-f", &pattern])
        .output()
        .expect("run pgrep");
    let pid = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .and_then(|line| line.parse::<libc::pid_t>().ok())
        .expect("daemon pid");
    // SAFETY: pid came from the private daemon's command line and is not reused yet.
    assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
    let deadline = Instant::now() + Duration::from_secs(3);
    while tm.socket().exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!tm.socket().exists(), "daemon left its socket behind");
    let output = tm.run(&["has-session", "-t", "S"]);
    assert!(!output.status.success());
}
