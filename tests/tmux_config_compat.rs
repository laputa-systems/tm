#![cfg(unix)]

//! Interactive behavior is compiled into tm rather than loaded from a file.
//! This test proves the daemon ignores a `TM_CONFIG` path even when one is
//! present in its environment.

use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Tm {
    socket: String,
    ignored_config_path: String,
}

impl Tm {
    fn with_ignored_config() -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir();
        Self {
            socket: root
                .join(format!("tm-config-{}-{id}.sock", std::process::id()))
                .to_string_lossy()
                .into_owned(),
            ignored_config_path: root
                .join(format!("tm-config-ignored-{}-{id}.conf", std::process::id()))
                .to_string_lossy()
                .into_owned(),
        }
    }

    fn run(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_tm"))
            .env("TM_CONFIG", &self.ignored_config_path)
            .args(["-S", self.socket.as_str()])
            .args(arguments)
            .output()
            .expect("run tm")
    }

    fn ok(&self, arguments: &[&str]) -> String {
        let output = self.run(arguments);
        assert!(
            output.status.success(),
            "tm {:?} failed: {}",
            arguments,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

impl Drop for Tm {
    fn drop(&mut self) {
        let _ = self.run(&["kill-server"]);
        let _ = std::fs::remove_file(&self.socket);
    }
}

#[test]
fn compiled_defaults_ignore_tm_config_before_the_first_session_exists() {
    let tm = Tm::with_ignored_config();
    tm.ok(&["new-session", "-d", "-s", "configured", "--", "sleep", "30"]);
    assert_eq!(
        tm.ok(&[
            "list-windows",
            "-t",
            "configured",
            "-F",
            "#{window_index}:#{window_name}"
        ]),
        "1:0\n"
    );
    assert_eq!(tm.ok(&["show-options", "-g", "-v", "prefix"]), "C-a\n");
    assert_eq!(
        tm.ok(&["show-options", "-g", "-v", "history-limit"]),
        "10000\n"
    );
    assert_eq!(
        tm.ok(&[
            "display-message",
            "-p",
            "-t",
            "configured:1.0",
            "#{pane_id}"
        ]),
        "%0\n"
    );
}
