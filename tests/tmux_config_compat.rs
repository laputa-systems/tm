#![cfg(unix)]

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Tm {
    socket: String,
    root: PathBuf,
    config: PathBuf,
    home: PathBuf,
}

impl Tm {
    fn with_ignored_config() -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".work/tmp")
            .join(format!("tm-config-{}-{id}", std::process::id()));
        let home = root.join("home");
        std::fs::create_dir_all(&home).expect("create isolated home");
        let config = root.join("tmux.conf");
        let contents = "set -g prefix C-z\nset -g base-index 7\nset -g history-limit 3\nset -g status-left CONFLICT\n";
        std::fs::write(&config, contents).expect("write TM_CONFIG");
        std::fs::write(home.join(".tmux.conf"), contents).expect("write home tmux.conf");
        Self {
            socket: root.join("server.sock").to_string_lossy().into_owned(),
            root: root.clone(),
            config,
            home,
        }
    }

    fn run(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_tm"))
            .env_remove("TM_SOCKET")
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env("TM_CONFIG", &self.config)
            .env("HOME", &self.home)
            .env("TMPDIR", &self.root)
            .env("TMUX_TMPDIR", &self.root)
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
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn compiled_defaults_ignore_tm_config_and_home_tmux_conf() {
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
    assert_ne!(
        tm.ok(&["show-options", "-g", "-v", "status-left"]),
        "CONFLICT\n"
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
