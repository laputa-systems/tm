#![cfg(unix)]

//! The user's tmux.conf is the compatibility contract for interactive tm.
//! This test exercises the real daemon startup path with an isolated config
//! and socket, so it cannot alter an existing tmux server or the user's state.

use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Tm {
    socket: String,
    config: String,
}

impl Tm {
    fn new() -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir();
        let config = root.join(format!("tm-config-{}-{id}.conf", std::process::id()));
        std::fs::write(
            &config,
            "set -g prefix C-a\n\
             set -g base-index 1\n\
             set -g renumber-windows on\n\
             set -g history-limit 77\n\
             setw -g mode-keys emacs\n\
             bind C-s send -N 2 C-a\n",
        )
        .expect("write isolated tm config");
        Self {
            socket: root
                .join(format!("tm-config-{}-{id}.sock", std::process::id()))
                .to_string_lossy()
                .into_owned(),
            config: config.to_string_lossy().into_owned(),
        }
    }

    fn run(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_tm"))
            .env("TM_SOCKET", &self.socket)
            .env("TM_CONFIG", &self.config)
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
        let _ = std::fs::remove_file(&self.config);
        let _ = std::fs::remove_file(&self.socket);
    }
}

#[test]
fn configured_defaults_apply_before_the_first_session_exists() {
    let tm = Tm::new();
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
        "77\n"
    );
    assert_eq!(
        tm.ok(&[
            "display-message",
            "-p",
            "-t",
            "configured:0.0",
            "#{pane_id}"
        ]),
        "%0\n"
    );
}
