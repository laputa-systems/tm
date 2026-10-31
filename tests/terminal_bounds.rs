#![cfg(unix)]

use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(0);

struct Tm {
    socket: String,
}

impl Tm {
    fn new() -> Self {
        let id = NEXT_SOCKET.fetch_add(1, Ordering::Relaxed);
        Self {
            socket: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join(".work/tmp")
                .join(format!("tm-bounds-{}-{id}.sock", std::process::id()))
                .to_string_lossy()
                .into_owned(),
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_tm"))
            .env("TM_SOCKET", &self.socket)
            .args(args)
            .output()
            .expect("run tm")
    }

    fn ok(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "tm {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn capture_until(&self, target: &str, expected: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let captured = self.ok(&["capture-pane", "-p", "-t", target]);
            if captured.contains(expected) || Instant::now() >= deadline {
                return captured;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Tm {
    fn drop(&mut self) {
        let _ = self.run(&["kill-server"]);
        let _ = std::fs::remove_file(&self.socket);
    }
}

#[test]
fn huge_dec_repeat_is_clamped_and_does_not_stall() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "S",
        "-x",
        "20",
        "-y",
        "4",
        "--",
        "sh",
        "-c",
        "printf 'X\\033[50000000b'; sleep 30",
    ]);
    let start = Instant::now();
    let captured = tm.capture_until("S", "X");
    assert!(start.elapsed() < Duration::from_secs(2));
    for line in captured.lines() {
        assert!(line.len() <= 20, "line was not clamped: {line:?}");
    }
    assert!(captured.lines().any(|line| line.starts_with("XXXXX")));
}

#[test]
fn oversized_control_sequences_recover_at_the_next_boundary() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "S",
        "-x",
        "40",
        "-y",
        "6",
        "--",
        "sh",
        "-c",
        "printf '\\033]52;c;'; head -c 1050000 /dev/zero | tr '\\0' A; printf '\\007RECOVERED\\n'; sleep 30",
    ]);
    let captured = tm.capture_until("S", "RECOVERED");
    assert!(
        captured.contains("RECOVERED"),
        "the parser did not recover after an oversized control sequence"
    );
}
