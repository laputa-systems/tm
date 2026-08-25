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

use ptytest::{Color, CommandSpec, ExitStatus, ProtocolProfile, PtyTest, Scenario, Size, TestEnv};
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
    fn new(socket: String) -> Self {
        Self { socket }
    }

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
        .wait_for_screen(
            terminal.deadline(Duration::from_secs(3)),
            "fixture ready",
            |screen| screen.contains("VT_FIXTURE_READY"),
        )
        .expect("attached client readiness");

    terminal
        .send_text(terminal.deadline(Duration::from_secs(3)), "input\n")
        .expect("send fixture input through client");
    terminal
        .wait_for_screen(
            terminal.deadline(Duration::from_secs(3)),
            "input acknowledgement",
            |screen| screen.contains("INPUT_ACK"),
        )
        .expect("fixture input acknowledgement");

    terminal
        .send_text(terminal.deadline(Duration::from_secs(3)), "begin-resize\n")
        .expect("start resize barrier");
    terminal
        .wait_for_screen(
            terminal.deadline(Duration::from_secs(3)),
            "resize barrier",
            |screen| screen.contains("RESIZE_WAIT"),
        )
        .expect("fixture resize barrier");
    terminal
        .resize(Size::new(50, 10).expect("constant resized PTY size"))
        .expect("resize outer test PTY");
    terminal
        .wait_for_screen(
            terminal.deadline(Duration::from_secs(3)),
            "resize signal",
            |screen| screen.contains("RESIZE_SIGNAL"),
        )
        .expect("foreground resize signal");
    terminal
        .send_text(terminal.deadline(Duration::from_secs(3)), "measure\n")
        .expect("measure resized fixture PTY");
    terminal
        .wait_for_screen(
            terminal.deadline(Duration::from_secs(3)),
            "resized fixture size",
            |screen| screen.contains("RESIZE_ACK:10x50"),
        )
        .expect("nested PTY resize propagation");

    terminal
        .send_bytes(terminal.deadline(Duration::from_secs(3)), b"\x02d")
        .expect("send detach sequence");
    assert_eq!(
        terminal
            .wait_for_exit(terminal.deadline(Duration::from_secs(3)))
            .expect("wait for clean detach"),
        ExitStatus::Code(0)
    );
    terminal
        .assert_terminal_restored(&baseline)
        .expect("attached client restores applicable terminal modes");
    terminal
        .finish(terminal.deadline(Duration::from_secs(3)))
        .expect("reap attached client");
}

#[test]
fn attached_client_captures_panes_colors_mouse_scroll_and_border_resize() {
    let environment = TestEnv::hermetic().expect("create hermetic test environment");
    let number = NEXT_SOCKET.fetch_add(1, Ordering::Relaxed);
    let socket = environment
        .paths()
        .root()
        .join(format!("tm-attached-panes-{number}.sock"))
        .to_string_lossy()
        .into_owned();
    let server = TestServer::new(socket.clone());
    server.create_fixture_session();
    let fixture = env!("CARGO_BIN_EXE_tm-pty-fixture");
    let mouse = run_tm(&socket, ["set-option", "-g", "mouse", "on"]);
    assert!(
        mouse.status.success(),
        "enable mouse failed: {}",
        String::from_utf8_lossy(&mouse.stderr)
    );
    let split = run_tm(
        &socket,
        ["split-window", "-h", "-d", "-t", "pty-e2e:0", "--", fixture],
    );
    assert!(
        split.status.success(),
        "split panes failed: {}",
        String::from_utf8_lossy(&split.stderr)
    );

    let scenario = Scenario::new("tm attached panes")
        .expect("valid scenario label")
        .command(
            CommandSpec::new(env!("CARGO_BIN_EXE_tm"))
                .env("TM_SOCKET", &socket)
                .args(["attach-session", "-t", "pty-e2e"]),
        )
        .size(Size::new(COLS, ROWS).expect("constant PTY size"))
        .environment(environment)
        .protocol_profile(ProtocolProfile::xterm_minimal_v1());
    let mut terminal = PtyTest::spawn(scenario).expect("spawn attached panes PTY");
    let baseline = terminal.terminal_baseline();

    terminal
        .wait_for_screen(
            terminal.deadline(Duration::from_secs(3)),
            "left pane fixture",
            |screen| screen.cell(0, 0).is_some_and(|cell| cell.contents() == "V"),
        )
        .expect("left pane readiness");
    terminal
        .wait_for_screen(
            terminal.deadline(Duration::from_secs(3)),
            "right pane fixture",
            |screen| {
                screen
                    .cell(0, 21)
                    .is_some_and(|cell| cell.contents() == "V")
            },
        )
        .expect("right pane readiness");

    let initial = terminal.screen();
    assert_eq!(
        initial.cell(0, 20).expect("initial pane border").contents(),
        "│"
    );
    assert_eq!(
        initial
            .cell(0, 20)
            .expect("initial pane border")
            .attributes()
            .foreground,
        Color::Indexed(2),
        "active pane border style was lost"
    );
    assert_eq!(
        initial
            .cell(0, 21)
            .expect("initial right pane cell")
            .contents(),
        "V"
    );

    // Let the attach loop settle, then prove that an unchanged pane state is
    // silent on the wire. This catches regressions that resend non-clearing
    // frames even though the full-screen clear count remains stable.
    assert!(
        terminal
            .wait_for_quiescence(
                terminal.deadline(Duration::from_secs(3)),
                Duration::from_millis(100),
            )
            .expect("settle initial pane frame")
    );
    let idle_output_length = terminal.raw_output().len();
    assert!(
        terminal
            .wait_for_quiescence(
                terminal.deadline(Duration::from_secs(3)),
                Duration::from_millis(100),
            )
            .expect("observe unchanged pane state")
    );
    assert_eq!(
        terminal.raw_output().len(),
        idle_output_length,
        "unchanged pane state must not emit another attached frame"
    );

    let color = run_tm(
        &socket,
        ["send-keys", "-t", "pty-e2e:0.0", "color", "Enter"],
    );
    assert!(
        color.status.success(),
        "send color failed: {}",
        String::from_utf8_lossy(&color.stderr)
    );
    let colored = terminal
        .wait_for_screen(
            terminal.deadline(Duration::from_secs(3)),
            "colored fixture output",
            |screen| screen.contains("COLOR_FIXTURE"),
        )
        .expect("colored fixture output");
    let colored_cell = (0..usize::from(ROWS))
        .flat_map(|row| (0..usize::from(COLS)).map(move |column| (row, column)))
        .filter_map(|(row, column)| colored.cell(row, column))
        .find(|cell| cell.contents() == "C")
        .expect("colored marker cell");
    assert_eq!(
        colored_cell.attributes().foreground,
        Color::Indexed(1),
        "pane foreground color was lost"
    );
    assert_eq!(
        colored_cell.attributes().background,
        Color::Indexed(2),
        "pane background color was lost"
    );
    assert!(colored_cell.attributes().bold, "pane bold style was lost");
    terminal
        .assert_snapshot("tests/snapshots/attached_client_panes_initial.txt")
        .expect("initial pane capture");

    let scroll = run_tm(
        &socket,
        ["send-keys", "-t", "pty-e2e:0.0", "scroll", "Enter"],
    );
    assert!(
        scroll.status.success(),
        "send scroll failed: {}",
        String::from_utf8_lossy(&scroll.stderr)
    );
    terminal
        .wait_for_screen(
            terminal.deadline(Duration::from_secs(3)),
            "scroll fixture output",
            |screen| screen.contains("SCROLL_23"),
        )
        .expect("scroll fixture output");
    terminal
        .send_bytes(terminal.deadline(Duration::from_secs(3)), b"\x1b[<64;5;2M")
        .expect("send wheel event");
    let scrolled = terminal
        .wait_for_screen_change(
            terminal.deadline(Duration::from_secs(3)),
            "mouse wheel scroll",
        )
        .expect("mouse wheel scroll");
    assert!(
        !scrolled.contains("SCROLL_23"),
        "wheel event did not move the copy-mode viewport"
    );

    terminal
        .send_bytes(terminal.deadline(Duration::from_secs(3)), b"\x1b[<0;21;2M")
        .expect("press pane border");
    terminal
        .send_bytes(terminal.deadline(Duration::from_secs(3)), b"\x1b[<32;26;2M")
        .expect("drag pane border");
    terminal
        .send_bytes(terminal.deadline(Duration::from_secs(3)), b"\x1b[<0;26;2m")
        .expect("release pane border");
    let resized = terminal
        .wait_for_screen(
            terminal.deadline(Duration::from_secs(3)),
            "resized pane border",
            |screen| {
                screen
                    .cell(0, 25)
                    .is_some_and(|cell| cell.contents() == "│")
            },
        )
        .expect("resized pane border");
    assert_ne!(
        resized
            .cell(0, 20)
            .expect("old pane border cell")
            .contents(),
        "│",
        "the old border was not returned to pane content"
    );
    terminal
        .assert_snapshot("tests/snapshots/attached_client_panes_resized.txt")
        .expect("resized pane capture");
    let full_clear_count = terminal
        .raw_output()
        .windows(b"\x1b[2J".len())
        .filter(|window| *window == b"\x1b[2J")
        .count();
    assert_eq!(
        full_clear_count, 1,
        "attached updates must not flash-clear the terminal"
    );

    let shutdown = run_tm(&socket, ["kill-server"]);
    assert!(
        shutdown.status.success(),
        "stop pane capture server failed: {}",
        String::from_utf8_lossy(&shutdown.stderr)
    );
    assert_eq!(
        terminal
            .wait_for_exit(terminal.deadline(Duration::from_secs(3)))
            .expect("wait for pane detach"),
        ExitStatus::Code(0)
    );
    terminal
        .assert_terminal_restored(&baseline)
        .expect("restore terminal after pane capture");
    terminal
        .finish(terminal.deadline(Duration::from_secs(3)))
        .expect("reap pane capture client");
}

#[test]
fn opening_pane_from_attached_client_preserves_existing_pane_contents() {
    let environment = TestEnv::hermetic().expect("create hermetic test environment");
    let number = NEXT_SOCKET.fetch_add(1, Ordering::Relaxed);
    let socket = environment
        .paths()
        .root()
        .join(format!("tm-attached-split-{number}.sock"))
        .to_string_lossy()
        .into_owned();
    let server = TestServer::new(socket.clone());
    server.create_fixture_session();

    let scenario = Scenario::new("tm attached split preserves content")
        .expect("valid scenario label")
        .command(
            CommandSpec::new(env!("CARGO_BIN_EXE_tm"))
                .env("TM_SOCKET", &socket)
                .args(["attach-session", "-t", "pty-e2e"]),
        )
        .size(Size::new(COLS, ROWS).expect("constant PTY size"))
        .environment(environment)
        .protocol_profile(ProtocolProfile::xterm_minimal_v1());
    let mut terminal = PtyTest::spawn(scenario).expect("spawn attached split PTY");
    let baseline = terminal.terminal_baseline();

    terminal
        .wait_for_screen(
            terminal.deadline(Duration::from_secs(3)),
            "fixture ready",
            |screen| screen.contains("VT_FIXTURE_READY"),
        )
        .expect("attached client readiness");

    terminal
        .send_text(terminal.deadline(Duration::from_secs(3)), "scroll\n")
        .expect("fill existing pane before split");
    terminal
        .wait_for_screen(
            terminal.deadline(Duration::from_secs(3)),
            "existing pane content",
            |screen| screen.contains("SCROLL_23"),
        )
        .expect("existing pane content");

    terminal
        .send_bytes(terminal.deadline(Duration::from_secs(3)), b"\x02\"")
        .expect("open pane from attached client");
    let split = terminal
        .wait_for_screen(
            terminal.deadline(Duration::from_secs(3)),
            "new pane border",
            |screen| screen.cell(2, 0).is_some_and(|cell| cell.contents() == "─"),
        )
        .expect("new pane became visible");
    assert!(
        split.contains("SCROLL_23"),
        "opening a pane cleared the original pane contents:\n{split}"
    );

    let shutdown = run_tm(&socket, ["kill-server"]);
    assert!(shutdown.status.success(), "stop split server");
    assert_eq!(
        terminal
            .wait_for_exit(terminal.deadline(Duration::from_secs(3)))
            .expect("wait for split detach"),
        ExitStatus::Code(0)
    );
    terminal
        .assert_terminal_restored(&baseline)
        .expect("restore terminal after split");
    terminal
        .finish(terminal.deadline(Duration::from_secs(3)))
        .expect("reap split client");
}
