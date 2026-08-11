#![cfg(unix)]

//! The real user configuration is the interactive compatibility contract.
//! This test copies it to a private path and starts tm on a private socket;
//! it never connects to or mutates an existing tmux server.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Tm {
    socket: PathBuf,
    config: PathBuf,
}

impl Tm {
    fn from_user_config() -> Option<Self> {
        let home = std::env::var_os("HOME")?;
        let source = PathBuf::from(home).join(".config/tmux/tmux.conf");
        if !source.is_file() {
            return None;
        }
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir();
        let config = root.join(format!("tm-user-config-{}-{id}.conf", std::process::id()));
        let socket = root.join(format!("tm-user-config-{}-{id}.sock", std::process::id()));
        std::fs::copy(source, &config).expect("copy user tmux config");
        Some(Self { socket, config })
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
fn actual_user_config_loads_its_core_contract_on_a_private_daemon() {
    let Some(tm) = Tm::from_user_config() else {
        return;
    };

    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "user-config",
        "--",
        "sleep",
        "30",
    ]);
    assert_eq!(
        tm.ok(&[
            "list-windows",
            "-t",
            "user-config",
            "-F",
            "#{window_index}:#{window_name}"
        ]),
        "1:0\n"
    );
    for (option, expected) in [
        ("prefix", "C-a"),
        ("default-terminal", "tmux-256color"),
        ("history-limit", "10000"),
        ("mouse", "on"),
        ("focus-events", "on"),
        ("extended-keys", "on"),
        ("extended-keys-format", "csi-u"),
        ("set-clipboard", "external"),
        ("base-index", "1"),
        ("renumber-windows", "on"),
        ("monitor-bell", "on"),
        ("bell-action", "any"),
        ("visual-bell", "off"),
        ("escape-time", "0"),
        ("automatic-rename", "off"),
        ("status-position", "bottom"),
        ("status-bg", "black"),
        ("status-fg", "white"),
        ("status-left-length", "32"),
        ("status-right", ""),
        ("monitor-activity", "off"),
        ("set-titles", "off"),
    ] {
        let expected = if expected.is_empty() {
            String::new()
        } else {
            format!("{expected}\n")
        };
        assert_eq!(
            tm.ok(&["show-options", "-g", "-v", option]),
            expected,
            "option {option}"
        );
    }
    assert_eq!(
        tm.ok(&[
            "show-window-options",
            "-t",
            "user-config",
            "-v",
            "mode-keys"
        ]),
        "emacs\n"
    );

    tm.ok(&[
        "new-session",
        "-d",
        "-s",
        "term",
        "--",
        "sh",
        "-c",
        r#"printf "$TERM"; sleep 30"#,
    ]);
    let deadline = Instant::now() + Duration::from_secs(2);
    let term = loop {
        let captured = tm.ok(&["capture-pane", "-t", "term"]);
        if captured.contains("tmux-256color") || Instant::now() >= deadline {
            break captured;
        }
        thread::sleep(Duration::from_millis(20));
    };
    assert!(
        term.contains("tmux-256color"),
        "TERM was not configured: {term:?}"
    );
}
