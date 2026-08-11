#![cfg(unix)]

//! Headless ports of the command-side paste-buffer regressions. The daemon is
//! isolated with a per-test socket and no tmux client or socket is consulted.

use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(0);

struct Tm {
    socket: String,
}

impl Tm {
    fn new() -> Self {
        let id = NEXT_SOCKET.fetch_add(1, Ordering::Relaxed);
        Self {
            socket: std::env::temp_dir()
                .join(format!("tm-buffers-{}-{id}.sock", std::process::id()))
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
            "tm {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn body(&self, args: &[&str]) -> String {
        self.ok(args).trim_end_matches('\n').to_owned()
    }

    fn capture_until(&self, target: &str, expected: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let captured = self.body(&["capture-pane", "-t", target]);
            if captured.contains(expected) || Instant::now() >= deadline {
                return captured;
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Tm {
    fn drop(&mut self) {
        let _ = self.run(&["kill-server"]);
    }
}

#[test]
fn buffers_match_the_headless_tmux_command_contract() {
    let tm = Tm::new();
    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "B",
        "-x40",
        "-y10",
        "--",
        "sh",
        "-c",
        "stty raw -echo; exec cat -v",
    ]);

    tm.ok(&["set-buffer", "one"]);
    tm.ok(&["set-buffer", "two"]);
    assert_eq!(
        tm.body(&["list-buffers", "-F", "#{buffer_name}=#{buffer_sample}"]),
        "buffer1=two\nbuffer0=one"
    );
    assert_eq!(
        tm.body(&[
            "list-buffers",
            "-F",
            "#{buffer_name}:#{n:#{buffer_sample}}:#{?#{==:#{buffer_name},buffer1},hit,miss}",
        ]),
        "buffer1:3:hit\nbuffer0:3:miss"
    );
    assert!(tm
        .body(&["list-buffers", "-F", "#{buffer_created}"])
        .lines()
        .all(|value| value.parse::<i64>().is_ok()));
    assert_eq!(tm.body(&["show-buffer"]), "two");
    assert_eq!(tm.body(&["show-buffer", "-b", "buffer0"]), "one");

    tm.ok(&["set-buffer", "-b", "named", "abc"]);
    tm.ok(&["set-buffer", "-a", "-b", "named", "123"]);
    assert_eq!(tm.body(&["show-buffer", "-b", "named"]), "abc123");
    tm.ok(&["set-buffer", "-b", "named", "-n", "other"]);
    assert_eq!(tm.body(&["show-buffer", "-b", "other"]), "abc123");
    assert_eq!(
        tm.body(&[
            "list-buffers",
            "-f",
            "#{==:#{buffer_name},other}",
            "-F",
            "#{buffer_name}"
        ]),
        "other"
    );

    tm.ok(&["set-option", "-g", "buffer-limit", "2"]);
    tm.ok(&["set-buffer", "a1"]);
    tm.ok(&["set-buffer", "a2"]);
    tm.ok(&["set-buffer", "a3"]);
    assert_eq!(
        tm.body(&["list-buffers", "-F", "#{buffer_sample}"]),
        "a3\na2\nabc123"
    );

    tm.ok(&["set-buffer", "-b", "paste", "one\ntwo"]);
    tm.ok(&["paste-buffer", "-b", "paste", "-t", "B:0.0"]);
    assert!(tm.capture_until("B:0.0", "one^Mtwo").contains("one^Mtwo"));

    tm.ok(&["set-buffer", "-b", "paste", "one\ntwo"]);
    tm.ok(&["paste-buffer", "-s", "|", "-b", "paste", "-t", "B:0.0"]);
    assert!(tm.capture_until("B:0.0", "one|two").contains("one|two"));

    tm.ok(&["set-buffer", "-b", "paste", "bracketed"]);
    tm.ok(&["paste-buffer", "-p", "-b", "paste", "-t", "B:0.0"]);
    assert!(tm
        .capture_until("B:0.0", "^[[200~bracketed^[[201~")
        .contains("^[[200~bracketed^[[201~"));

    let source = std::env::temp_dir().join(format!("tm-buffer-source-{}", std::process::id()));
    let saved = std::env::temp_dir().join(format!("tm-buffer-saved-{}", std::process::id()));
    std::fs::write(&source, b"line1\tx\x1b[31m\x01\x02\xc3\xa9\n").expect("write source");
    let source_arg = source.to_string_lossy().into_owned();
    let saved_arg = saved.to_string_lossy().into_owned();
    tm.ok(&["load-buffer", "-b", "file", &source_arg]);
    tm.ok(&["save-buffer", "-b", "file", &saved_arg]);
    assert_eq!(
        std::fs::read(&source).expect("read source"),
        std::fs::read(&saved).expect("read saved")
    );
    let _ = std::fs::remove_file(source);
    let _ = std::fs::remove_file(saved);
}
