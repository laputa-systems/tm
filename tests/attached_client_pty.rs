#![cfg(unix)]

//! Deterministic end-to-end coverage for the real attached client.
//!
//! ```text
//! fixture PTY -> daemon reader -> vt100 pane state -> renderer
//!     -> attach client raw stdout -> ptytest semantic screen
//! ```
//!
//! `ptytest` owns the outer kernel PTY, process group, nonblocking I/O, and
//! semantic terminal model. This test retains only tm's daemon/socket setup
//! and its fixture-specific output barriers.

use ptytest::{CommandSpec, ExitStatus, ProtocolProfile, PtyTest, Scenario, Size, TestEnv};
use std::fs;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(0);

const ROWS: u16 = 6;
const COLS: u16 = 40;

struct TestServer {
    socket: String,
}

impl TestServer {
    fn new(socket: String) -> Self { Self { socket } }

    fn create_fixture_session(&self) {
        let fixture = env!("CARGO_BIN_EXE_tm-pty-fixture");
        let created = Command::new(env!("CARGO_BIN_EXE_tm"))
            .env("TM_SOCKET", &self.socket)
            .args([
                "new-session",
                "-d",
                "-s",
                "pty-e2e",
                "-x",
                &COLS.to_string(),
                "-y",
                &ROWS.to_string(),
                "--",
                fixture,
            ])
            .output()
            .expect("create fixture session");
        assert!(
            created.status.success(),
            "create session failed: {}",
            String::from_utf8_lossy(&created.stderr)
        );
        let remain = run_tm(&self.socket, ["set-option", "-g", "remain-on-exit", "on"]);
        assert!(
            remain.status.success(),
            "set remain-on-exit failed: {}",
            String::from_utf8_lossy(&remain.stderr)
        );
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = run_tm(&self.socket, ["kill-server"]);
        let _ = fs::remove_file(&self.socket);
    }
}

fn run_tm<const N: usize>(socket: &str, args: [&str; N]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tm"))
        .env("TM_SOCKET", socket)
        .args(args)
        .output()
        .expect("run tm command")
}

#[test]
fn attached_client_uses_real_pty_and_semantic_screen_barriers() {
    let environment = TestEnv::hermetic().expect("create hermetic test environment");
    let number = NEXT_SOCKET.fetch_add(1, Ordering::Relaxed);
    let socket = environment
        .paths()
        .root()
        .join(format!("tm-attached-{number}.sock"))
        .to_string_lossy()
        .into_owned();

    // Declare the terminal first so the server is cleaned up before the
    // hermetic scratch root is removed if this test unwinds.
    let mut terminal: PtyTest;
    let server = TestServer::new(socket.clone());
    server.create_fixture_session();

    let scenario = Scenario::new("tm attached client")
        .expect("valid scenario label")
        .command(
            CommandSpec::new(env!("CARGO_BIN_EXE_tm"))
                .env("TM_SOCKET", &socket)
                .args(["attach-session", "-t", "pty-e2e"]),
        )
        .size(Size::new(COLS, ROWS).expect("constant PTY size"))
        .environment(environment)
        .protocol_profile(ProtocolProfile::xterm_minimal_v1());
    terminal = PtyTest::spawn(scenario).expect("spawn attached client PTY");
    let baseline = terminal.terminal_baseline();

    terminal
        .wait_for_screen(terminal.deadline(Duration::from_secs(3)), "fixture ready", |screen| {
            screen.contains("VT_FIXTURE_READY")
        })
        .expect("attached client readiness");

    terminal
        .send_text(terminal.deadline(Duration::from_secs(3)), "input\n")
        .expect("send fixture input through client");
    terminal
        .wait_for_screen(terminal.deadline(Duration::from_secs(3)), "input acknowledgement", |screen| {
            screen.contains("INPUT_ACK")
        })
        .expect("fixture input acknowledgement");

    terminal
        .send_text(terminal.deadline(Duration::from_secs(3)), "begin-resize\n")
        .expect("start resize barrier");
    terminal
        .wait_for_screen(terminal.deadline(Duration::from_secs(3)), "resize barrier", |screen| {
            screen.contains("RESIZE_WAIT")
        })
        .expect("fixture resize barrier");
    terminal
        .resize(Size::new(50, 10).expect("constant resized PTY size"))
        .expect("resize outer test PTY");
    terminal
        .wait_for_screen(terminal.deadline(Duration::from_secs(3)), "resize signal", |screen| {
            screen.contains("RESIZE_SIGNAL")
        })
        .expect("foreground resize signal");
    terminal
        .send_text(terminal.deadline(Duration::from_secs(3)), "measure\n")
        .expect("measure resized fixture PTY");
    terminal
        .wait_for_screen(terminal.deadline(Duration::from_secs(3)), "resized fixture size", |screen| {
            screen.contains("RESIZE_ACK:10x50")
        })
        .expect("nested PTY resize propagation");

    terminal
        .send_bytes(terminal.deadline(Duration::from_secs(3)), b"\x02d")
        .expect("send detach sequence");
    assert_eq!(
        terminal.wait_for_exit(terminal.deadline(Duration::from_secs(3))).expect("wait for clean detach"),
        ExitStatus::Code(0)
    );
    terminal
        .assert_terminal_restored(&baseline)
        .expect("attached client restores applicable terminal modes");
    terminal.finish(terminal.deadline(Duration::from_secs(3))).expect("reap attached client");
}
