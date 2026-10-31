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
    client
        .finish(client.deadline(Duration::from_secs(3)))
        .expect("reap signalled client");
}

#[test]
fn stop_signal_restores_the_screen_before_stopping() {
    let scratch = Scratch::new("signals-tstp");
    let tm = Mux::tm(&scratch, "tm");
    create_session(&tm);
    let (mut client, _baseline) = attach(&tm, "tstp");
    client.signal(libc::SIGTSTP).expect("send SIGTSTP");
    client
        .wait_for_quiescence(client.deadline(Duration::from_secs(3)), Duration::from_millis(100))
        .expect("read stop restoration");
    assert!(
        !client.screen().modes().alternate_screen,
        "SIGTSTP must leave the alternate screen before stopping"
    );
    client.signal(libc::SIGKILL).expect("send SIGKILL");
    let _ = client.wait_for_exit(client.deadline(Duration::from_secs(3)));
    client
        .finish(client.deadline(Duration::from_secs(3)))
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
