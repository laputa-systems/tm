#![cfg(unix)]

//! Deterministic end-to-end coverage for the real attached client.
//!
//! The test deliberately crosses every terminal boundary:
//!
//! ```text
//! fixture PTY -> daemon reader -> vt100 pane state -> renderer
//!     -> attach client raw stdout -> test PTY -> vt100 oracle
//! ```
//!
//! It uses an `openpty` pair directly. No `script`, shell, fixed readiness
//! sleeps, or raw transcript comparison is involved.

use std::fs;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use vt100::Parser;

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(0);

const ROWS: u16 = 6;
const COLS: u16 = 40;

struct TmPty {
    master: fs::File,
    child: Child,
    parser: Parser,
    socket: String,
    raw_output: Vec<u8>,
}

impl TmPty {
    fn spawn(socket: &str) -> io::Result<Self> {
        let (master, slave) = openpty(ROWS, COLS)?;
        let stdin = slave.try_clone()?;
        let stdout = slave.try_clone()?;
        let stderr = slave.try_clone()?;
        let mut command = Command::new(env!("CARGO_BIN_EXE_tm"));
        command
            .env("TM_SOCKET", socket)
            .args(["attach-session", "-t", "pty-e2e"])
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        let child = command.spawn()?;
        drop(slave);

        Ok(Self {
            master,
            child,
            parser: Parser::new(ROWS, COLS, 100),
            socket: socket.to_owned(),
            raw_output: Vec::new(),
        })
    }

    fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.master.write_all(bytes)
    }

    fn screen_contains(&mut self, marker: &str, timeout: Duration) -> String {
        let deadline = Instant::now() + timeout;
        loop {
            let contents = self.parser.screen().contents();
            if contents.contains(marker) {
                return contents;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(), "timed out waiting for {marker:?}; screen={contents:?}; raw={:?}", String::from_utf8_lossy(&self.raw_output));
            assert!(
                !self.read_once(remaining),
                "attached PTY closed before {marker:?}; raw={:?}",
                String::from_utf8_lossy(&self.raw_output)
            );
        }
    }

    /// Read one available PTY chunk. Returns true when the peer closed its
    /// side; an attach client is expected to close its PTY immediately after
    /// a successful detach, so the caller decides whether that is terminal.
    fn read_once(&mut self, timeout: Duration) -> bool {
        let timeout_ms = timeout
            .as_millis()
            .min(i32::MAX as u128)
            .max(1) as i32;
        let mut pollfd = libc::pollfd {
            fd: self.master.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `pollfd` references the live PTY master owned by `self`.
        let result = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
        assert!(result >= 0, "poll attached PTY: {}", io::Error::last_os_error());
        if result == 0 {
            return false;
        }
        let mut bytes = [0_u8; 4096];
        match self.master.read(&mut bytes) {
            Ok(0) => true,
            Ok(length) => {
                self.raw_output.extend_from_slice(&bytes[..length]);
                self.parser.process(&bytes[..length]);
                false
            }
            Err(error) if error.raw_os_error() == Some(libc::EIO) => {
                true
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => false,
            Err(error) => panic!("read attached PTY: {error}"),
        }
    }

    fn resize(&self, rows: u16, cols: u16) -> io::Result<()> {
        let size = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: `size` is immutable valid storage and the master is live.
        let result = unsafe {
            libc::ioctl(
                self.master.as_raw_fd(),
                libc::TIOCSWINSZ.into(),
                &size as *const libc::winsize,
            )
        };
        if result == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn detach(&mut self) {
        self.send(b"\x02d").expect("send detach key sequence");
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    assert!(status.success(), "attach client failed: {status}");
                    return;
                }
                Ok(None) => {}
                Err(error) => panic!("wait for attach client: {error}"),
            }
            assert!(
                Instant::now() < deadline,
                "attach client did not exit after detach; raw={:?}",
                String::from_utf8_lossy(&self.raw_output)
            );
            if self.read_once(Duration::from_millis(100)) {
                let status = self.child.wait().expect("wait after PTY close");
                assert!(status.success(), "attach client failed: {status}");
                return;
            }
        }
    }
}

impl Drop for TmPty {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = run_tm(&self.socket, ["kill-server"]);
        let _ = fs::remove_file(&self.socket);
    }
}

fn openpty(rows: u16, cols: u16) -> io::Result<(fs::File, fs::File)> {
    let mut master: RawFd = -1;
    let mut slave: RawFd = -1;
    let mut size = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: openpty initializes both output descriptors on success.
    let result = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if result == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful openpty transferred ownership of both descriptors.
    Ok(unsafe { (fs::File::from_raw_fd(master), fs::File::from_raw_fd(slave)) })
}

fn run_tm<const N: usize>(socket: &str, args: [&str; N]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tm"))
        .env("TM_SOCKET", socket)
        .args(args)
        .output()
        .expect("run tm command")
}

#[test]
fn attached_client_uses_real_pty_and_vt100_screen_barriers() {
    let number = NEXT_SOCKET.fetch_add(1, Ordering::Relaxed);
    let socket = std::env::temp_dir().join(format!(
        "tm-attached-pty-{}-{number}.sock",
        std::process::id()
    ));
    let socket = socket.to_string_lossy().into_owned();
    let fixture = env!("CARGO_BIN_EXE_tm-pty-fixture");

    let created = Command::new(env!("CARGO_BIN_EXE_tm"))
        .env("TM_SOCKET", &socket)
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
    let remain = run_tm(&socket, ["set-option", "-g", "remain-on-exit", "on"]);
    assert!(remain.status.success(), "set remain-on-exit failed: {}", String::from_utf8_lossy(&remain.stderr));

    let mut terminal = TmPty::spawn(&socket).expect("spawn attached client PTY");
    terminal.screen_contains("VT_FIXTURE_READY", Duration::from_secs(3));
    assert!(
        terminal.child.try_wait().expect("check attached client").is_none(),
        "attached client exited before input"
    );

    terminal.send(b"input\n").expect("send fixture input through client");
    terminal.screen_contains("INPUT_ACK", Duration::from_secs(3));

    terminal
        .send(b"begin-resize\n")
        .expect("start resize barrier");
    terminal.screen_contains("RESIZE_WAIT", Duration::from_secs(3));
    terminal.resize(10, 50).expect("resize outer test PTY");
    terminal.screen_contains("RESIZE_SIGNAL", Duration::from_secs(3));
    terminal.send(b"measure\n").expect("measure resized fixture PTY");
    terminal.screen_contains("RESIZE_ACK:10x50", Duration::from_secs(3));

    terminal.detach();
}
