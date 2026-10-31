use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

pub struct Scratch {
    root: PathBuf,
}

impl Scratch {
    pub fn new(label: &str) -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".work/tmp")
            .join(format!(
                "parity-{}-{}-{label}",
                std::process::id(),
                NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed)
            ));
        std::fs::create_dir_all(&root).expect("create parity scratch");
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Implementation {
    Tm,
    Reference,
}

pub struct Mux {
    implementation: Implementation,
    binary: PathBuf,
    socket: PathBuf,
    config: Option<PathBuf>,
    root: PathBuf,
    home: PathBuf,
    term: String,
}

impl Mux {
    pub fn tm(scratch: &Scratch, name: &str) -> Self {
        let root = scratch.root().join(name);
        std::fs::create_dir_all(&root).expect("create tm scratch");
        let home = root.join("home");
        std::fs::create_dir_all(&home).expect("create tm home");
        Self {
            implementation: Implementation::Tm,
            binary: PathBuf::from(env!("CARGO_BIN_EXE_tm")),
            socket: root.join("server.sock"),
            config: None,
            root,
            home,
            term: "tmux-256color".to_owned(),
        }
    }

    pub fn reference(scratch: &Scratch, name: &str) -> Self {
        let root = scratch.root().join(name);
        std::fs::create_dir_all(&root).expect("create reference scratch");
        let home = root.join("home");
        std::fs::create_dir_all(&home).expect("create reference home");
        let binary = std::env::var_os("TMUX_REF")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join(".work/ref/tmux-3.8/bin/tmux")
            });
        assert!(
            binary.is_file(),
            "missing tmux 3.8 reference at {}; run tests/support/install-tmux-ref.sh",
            binary.display()
        );
        let version = Command::new(&binary)
            .arg("-V")
            .output()
            .unwrap_or_else(|error| panic!("run {} -V: {error}", binary.display()));
        assert_eq!(
            String::from_utf8_lossy(&version.stdout).trim(),
            "tmux 3.8",
            "reference must be tmux 3.8"
        );
        Self {
            implementation: Implementation::Reference,
            binary,
            socket: root.join("server.sock"),
            config: Some(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/reference/tmux-3.8.conf"),
            ),
            root,
            home,
            term: "tmux-256color".to_owned(),
        }
    }

    pub fn implementation(&self) -> Implementation {
        self.implementation
    }

    pub fn binary(&self) -> &Path {
        &self.binary
    }

    pub fn config(&self) -> Option<&Path> {
        self.config.as_deref()
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    pub fn term(&self) -> &str {
        &self.term
    }

    pub fn command(&self, arguments: &[&str]) -> Command {
        let mut command = Command::new(&self.binary);
        command
            .current_dir(&self.root)
            .env_remove("TM_SOCKET")
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env_remove("TM_CONFIG")
            .env("HOME", &self.home)
            .env("TMPDIR", &self.root)
            .env("TMUX_TMPDIR", &self.root)
            .env("TERM", &self.term)
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8")
            .env("TZ", "UTC")
            .arg("-S")
            .arg(&self.socket);
        if let Some(config) = &self.config {
            command.arg("-f").arg(config);
        }
        command.args(arguments);
        command
    }

    pub fn try_run(&self, arguments: &[&str]) -> Result<Output, String> {
        run_with_deadline(self.command(arguments), Duration::from_secs(10))
    }

    pub fn run(&self, arguments: &[&str]) -> Output {
        self.try_run(arguments).unwrap_or_else(|error| {
            panic!(
                "{} {:?} failed: {error}",
                match self.implementation {
                    Implementation::Tm => "tm",
                    Implementation::Reference => "tmux",
                },
                arguments
            )
        })
    }

    pub fn ok(&self, arguments: &[&str]) -> String {
        let output = self.run(arguments);
        assert!(
            output.status.success(),
            "{:?} {:?} failed: {}",
            self.implementation,
            arguments,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    pub fn body(&self, arguments: &[&str]) -> String {
        self.ok(arguments).trim_end_matches('\n').to_owned()
    }

    pub fn outcome(&self, arguments: &[&str]) -> Outcome {
        let output = self.run(arguments);
        Outcome {
            success: output.status.success(),
            stdout: normalize(&String::from_utf8_lossy(&output.stdout), self),
            stderr: normalize(&String::from_utf8_lossy(&output.stderr), self),
        }
    }

    pub fn capture_until(&self, target: &str, marker: &str, timeout: Duration) -> String {
        let deadline = Instant::now() + timeout;
        loop {
            let captured = self.ok(&["capture-pane", "-p", "-t", target]);
            if captured.contains(marker) || Instant::now() >= deadline {
                return captured;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn validate_reference_profile(&self) {
        assert_eq!(
            self.implementation,
            Implementation::Reference,
            "profile validation is reference-only"
        );
        assert_eq!(
            self.body(&["show-options", "-s", "-v", "default-terminal"]),
            "tmux-256color"
        );
        assert_eq!(self.body(&["show-options", "-g", "-v", "prefix"]), "C-a");
        let keys = self.ok(&["list-keys", "-T", "prefix"]);
        assert_eq!(
            keys.lines().filter(|line| !line.trim().is_empty()).count(),
            27,
            "reference prefix table does not match the compiled effective table"
        );
    }
}

impl Drop for Mux {
    fn drop(&mut self) {
        let _ = run_with_deadline(
            self.command(&["kill-server"]),
            Duration::from_secs(2),
        );
        let _ = std::fs::remove_file(&self.socket);
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct Outcome {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub fn assert_outcomes_match(label: &str, tm: &Outcome, reference: &Outcome) {
    assert_eq!(
        tm.success, reference.success,
        "{label}: exit status differs\n tm: {:?}\nref: {:?}",
        tm, reference
    );
    assert_eq!(
        tm.stdout, reference.stdout,
        "{label}: stdout differs\n tm: {:?}\nref: {:?}",
        tm, reference
    );
    if !tm.success {
        assert_eq!(
            tm.stderr, reference.stderr,
            "{label}: stderr differs\n tm: {:?}\nref: {:?}",
            tm, reference
        );
    }
}

pub fn compare_commands(tm: &Mux, reference: &Mux, commands: &[&[&str]]) {
    for arguments in commands {
        let label = format!("{arguments:?}");
        assert_outcomes_match(
            &label,
            &tm.outcome(arguments),
            &reference.outcome(arguments),
        );
    }
}

pub fn normalize(value: &str, mux: &Mux) -> String {
    let mut output = value
        .replace(mux.socket.to_string_lossy().as_ref(), "<socket>")
        .replace(mux.root.to_string_lossy().as_ref(), "<root>")
        .replace(mux.home.to_string_lossy().as_ref(), "<home>");
    for (needle, replacement) in [
        ("tmux 3.8", "tmux <version>"),
        ("tm ", "tm "),
    ] {
        output = output.replace(needle, replacement);
    }
    output
}

fn run_with_deadline(mut command: Command, timeout: Duration) -> Result<Output, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("spawn {:?}: {error}", command.get_program()))?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child
                    .wait_with_output()
                    .map_err(|error| format!("collect output: {error}"));
            }
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "command {:?} exceeded {:?}",
                    command.get_program(),
                    timeout
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(error) => return Err(format!("wait for command: {error}")),
        }
    }
}
