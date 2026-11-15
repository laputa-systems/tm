#[path = "tm_bin.rs"]
pub mod tm_bin;

use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub const SILENT_SHELL: [&str; 3] = ["/bin/sh", "-c", "stty -echo; PS1= exec /bin/sh +m"];

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
            binary: tm_bin::tm_bin(),
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
        let binary = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".work/ref/tmux-3.8/bin/tmux");
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

    pub fn with_term(mut self, term: &str) -> Self {
        self.term = term.to_owned();
        self
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
            status: ExitStatus::of(output.status),
            stdout: normalize(&output.stdout, self),
            stderr: normalize(&output.stderr, self),
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

    pub fn type_in_pane(&self, pane: &str, words: &[&str], held_until_released: bool) {
        let quote = |word: &str| format!("'{word}'");
        let mut invocation = vec![quote(&self.binary.to_string_lossy())];
        if let Some(config) = &self.config {
            invocation.push("-f".to_owned());
            invocation.push(quote(&config.to_string_lossy()));
        }
        invocation.extend(words.iter().map(|word| quote(word)));
        let output = self.root.join("in-pane.out");
        let partial = self.root.join("in-pane.partial");
        let release = self.root.join("in-pane.go");
        let _ = std::fs::remove_file(&output);
        let _ = std::fs::remove_file(&release);
        let run = format!(
            "{{ {} </dev/null 2>&1; echo status=$?; }} > '{}'; mv '{}' '{}'",
            invocation.join(" "),
            partial.display(),
            partial.display(),
            output.display()
        );
        let line = if held_until_released {
            format!(
                "( trap '' HUP TERM; until [ -e '{}' ]; do sleep 0.05; done; {run} ) </dev/null >/dev/null 2>&1 &",
                release.display()
            )
        } else {
            run
        };
        self.ok(&["send-keys", "-t", pane, "-l", &line]);
        self.ok(&["send-keys", "-t", pane, "Enter"]);
    }

    pub fn release_in_pane(&self) {
        std::fs::write(self.root.join("in-pane.go"), b"").expect("release the held pane command");
    }

    pub fn in_pane_output(&self, timeout: Duration) -> Result<String, String> {
        let output = self.root.join("in-pane.out");
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(text) = std::fs::read_to_string(&output) {
                let text = text
                    .strip_prefix("tmux: ")
                    .or_else(|| text.strip_prefix("tm: "))
                    .map_or(text.clone(), str::to_owned);
                return Ok(text);
            }
            if Instant::now() >= deadline {
                return Err(format!("{:?} in-pane command never finished", self.implementation));
            }
            std::thread::sleep(Duration::from_millis(15));
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
        // kill-server is best effort: under load the server can outlive the
        // deadline and keep its sessions' ptys open, and leaked servers
        // accumulate until new pane spawns fail with ENXIO.
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if std::os::unix::net::UnixStream::connect(&self.socket).is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = std::fs::remove_file(&self.socket);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitStatus {
    Code(i32),
    Signal(i32),
}

impl ExitStatus {
    fn of(status: std::process::ExitStatus) -> Self {
        match (status.code(), status.signal()) {
            (Some(code), _) => Self::Code(code),
            (None, Some(signal)) => Self::Signal(signal),
            (None, None) => panic!("exit status {status:?} has neither code nor signal"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Outcome {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Field {
    Status,
    Stdout,
    Stderr,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Difference {
    pub field: Field,
    pub tm: String,
    pub reference: String,
}

fn lossy(bytes: &[u8]) -> String {
    format!("{:?}", String::from_utf8_lossy(bytes))
}

pub fn first_difference(tm: &Outcome, reference: &Outcome) -> Option<Difference> {
    if tm.status != reference.status {
        return Some(Difference {
            field: Field::Status,
            tm: format!("{:?}", tm.status),
            reference: format!("{:?}", reference.status),
        });
    }
    if tm.stdout != reference.stdout {
        return Some(Difference {
            field: Field::Stdout,
            tm: lossy(&tm.stdout),
            reference: lossy(&reference.stdout),
        });
    }
    if tm.stderr != reference.stderr {
        return Some(Difference {
            field: Field::Stderr,
            tm: lossy(&tm.stderr),
            reference: lossy(&reference.stderr),
        });
    }
    None
}

pub fn observation_difference<T: std::fmt::Debug + PartialEq>(
    what: &str,
    tm: &T,
    reference: &T,
) -> Option<String> {
    (tm != reference).then(|| format!("{what} differ\n tm: {tm:?}\nref: {reference:?}"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridLimit {
    ErasedCellExtent,
    WideCharacterMarginWrap,
    NoResizeReflow,
    HistoryTrimGranularity,
}

impl GridLimit {
    pub const fn parity_row(self) -> &'static str {
        match self {
            Self::ErasedCellExtent => "capture of erased cells",
            Self::WideCharacterMarginWrap => "wide character at the margin",
            Self::NoResizeReflow => "resize reflow",
            Self::HistoryTrimGranularity => "history trim granularity",
        }
    }
}

pub struct IntentionalDifference {
    pub test: &'static str,
    pub command: &'static [&'static str],
    pub limit: GridLimit,
    pub field: Field,
    pub tm: &'static str,
    pub reference: &'static str,
}

pub const INTENTIONAL_DIFFERENCES: &[IntentionalDifference] = &[
    IntentionalDifference {
        test: "capture_keeps_cells_erased_after_being_written",
        command: &["capture-pane", "-p", "-J", "-t", "S"],
        limit: GridLimit::ErasedCellExtent,
        field: Field::Stdout,
        tm: "e\nEND\n\n\n",
        reference: "e     \nEND\n\n\n",
    },
    IntentionalDifference {
        test: "capture_keeps_cells_erased_after_being_written",
        command: &["capture-pane", "-p", "-N", "-t", "S"],
        limit: GridLimit::ErasedCellExtent,
        field: Field::Stdout,
        tm: "e    \nEND  \n\n\n",
        reference: "e         \nEND  \n\n\n",
    },
    IntentionalDifference {
        test: "wide_characters_margin_wrap",
        command: &["capture-pane", "-p", "-e", "-J", "-S", "-", "-t", "S"],
        limit: GridLimit::WideCharacterMarginWrap,
        field: Field::Stdout,
        tm: "READY\nabcdefghi\n中Z\nabcdefgh中Y\nabcdefghi\n😀Z\nabcdefgh😀Y\n0123456789\ne\u{301}中\nM01\n\n",
        reference: "READY\nabcdefghi中Z\nabcdefgh中Y\nabcdefghi😀Z\nabcdefgh😀Y\n0123456789\ne\u{301}中\nM01\n\n",
    },
    IntentionalDifference {
        test: "wide_characters_margin_wrap",
        command: &["capture-pane", "-p", "-J", "-S", "-3", "-E", "2", "-t", "S"],
        limit: GridLimit::WideCharacterMarginWrap,
        field: Field::Stdout,
        tm: "Y\nabcdefghi\n😀Z\nabcdefgh😀Y\n0123456789\n",
        reference: "Y\nabcdefghi😀Z\nabcdefgh😀Y\n0123456789\n",
    },
    IntentionalDifference {
        test: "history_limit_trim_chunk",
        command: &["capture-pane", "-p", "-S", "-", "-t", "S"],
        limit: GridLimit::HistoryTrimGranularity,
        field: Field::Stdout,
        tm: "中😀\nL30 abcdef\n中😀\nL31 abcdef\n中😀\nL32 abcdef\n中😀\nL33 abcdef\n中😀\nL34 abcdef\n中😀\nL35 abcdef\n中😀\nL36 abcdef\n中😀\nL37 abcdef\n中😀\nL38 abcdef\n中😀\nL39 abcdef\n中😀\nL40 abcdef\n中😀\nM02\nM03\n\n",
        reference: "L30 abcdef\n中😀\nL31 abcdef\n中😀\nL32 abcdef\n中😀\nL33 abcdef\n中😀\nL34 abcdef\n中😀\nL35 abcdef\n中😀\nL36 abcdef\n中😀\nL37 abcdef\n中😀\nL38 abcdef\n中😀\nL39 abcdef\n中😀\nL40 abcdef\n中😀\nM02\nM03\n\n",
    },
    IntentionalDifference {
        test: "history_limit_trim_chunk",
        command: &["capture-pane", "-p", "-S", "-", "-E", "-", "-t", "S"],
        limit: GridLimit::HistoryTrimGranularity,
        field: Field::Stdout,
        tm: "中😀\nL30 abcdef\n中😀\nL31 abcdef\n中😀\nL32 abcdef\n中😀\nL33 abcdef\n中😀\nL34 abcdef\n中😀\nL35 abcdef\n中😀\nL36 abcdef\n中😀\nL37 abcdef\n中😀\nL38 abcdef\n中😀\nL39 abcdef\n中😀\nL40 abcdef\n中😀\nM02\nM03\n\n",
        reference: "L30 abcdef\n中😀\nL31 abcdef\n中😀\nL32 abcdef\n中😀\nL33 abcdef\n中😀\nL34 abcdef\n中😀\nL35 abcdef\n中😀\nL36 abcdef\n中😀\nL37 abcdef\n中😀\nL38 abcdef\n中😀\nL39 abcdef\n中😀\nL40 abcdef\n中😀\nM02\nM03\n\n",
    },
    IntentionalDifference {
        test: "history_limit_trim_chunk",
        command: &["capture-pane", "-p", "-e", "-J", "-S", "-", "-t", "S"],
        limit: GridLimit::HistoryTrimGranularity,
        field: Field::Stdout,
        tm: "中😀\nL30 abcdef中😀\nL31 abcdef中😀\nL32 abcdef中😀\nL33 abcdef中😀\nL34 abcdef中😀\nL35 abcdef中😀\nL36 abcdef中😀\nL37 abcdef中😀\nL38 abcdef中😀\nL39 abcdef中😀\nL40 abcdef中😀\nM02\nM03\n\n",
        reference: "L30 abcdef中😀\nL31 abcdef中😀\nL32 abcdef中😀\nL33 abcdef中😀\nL34 abcdef中😀\nL35 abcdef中😀\nL36 abcdef中😀\nL37 abcdef中😀\nL38 abcdef中😀\nL39 abcdef中😀\nL40 abcdef中😀\nM02\nM03\n\n",
    },
    IntentionalDifference {
        test: "wide_characters_resize_narrower",
        command: &["capture-pane", "-p", "-t", "S"],
        limit: GridLimit::NoResizeReflow,
        field: Field::Stdout,
        tm: "abcdefg\nY\n0123456\ne\u{301}中\nM01\n\n",
        reference: "h😀Y\n0123456\n789\ne\u{301}中\nM01\n\n",
    },
    IntentionalDifference {
        test: "wide_characters_resize_narrower",
        command: &["capture-pane", "-p", "-J", "-t", "S"],
        limit: GridLimit::NoResizeReflow,
        field: Field::Stdout,
        tm: "abcdefg\nY\n0123456\ne\u{301}中\nM01\n\n",
        reference: "h😀Y\n0123456789\ne\u{301}中\nM01\n\n",
    },
    IntentionalDifference {
        test: "wide_characters_resize_narrower",
        command: &["capture-pane", "-p", "-e", "-t", "S"],
        limit: GridLimit::NoResizeReflow,
        field: Field::Stdout,
        tm: "abcdefg\nY\n0123456\ne\u{301}中\nM01\n\n",
        reference: "h😀Y\n0123456\n789\ne\u{301}中\nM01\n\n",
    },
    IntentionalDifference {
        test: "wide_characters_resize_narrower",
        command: &["capture-pane", "-p", "-e", "-J", "-t", "S"],
        limit: GridLimit::NoResizeReflow,
        field: Field::Stdout,
        tm: "abcdefg\nY\n0123456\ne\u{301}中\nM01\n\n",
        reference: "h😀Y\n0123456789\ne\u{301}中\nM01\n\n",
    },
    IntentionalDifference {
        test: "wide_characters_resize_narrower",
        command: &["capture-pane", "-p", "-N", "-t", "S"],
        limit: GridLimit::NoResizeReflow,
        field: Field::Stdout,
        tm: "abcdefg\nY  \n0123456\ne\u{301}中    \nM01    \n\n",
        reference: "h😀Y   \n0123456\n789\ne\u{301}中  \nM01  \n\n",
    },
    IntentionalDifference {
        test: "wide_characters_resize_narrower",
        command: &["capture-pane", "-p", "-S", "-", "-t", "S"],
        limit: GridLimit::NoResizeReflow,
        field: Field::Stdout,
        tm: "READY\nabcdefg\n中Z\nabcdefg\nY\nabcdefg\n😀Z\nabcdefg\nY\n0123456\ne\u{301}中\nM01\n\n",
        reference: "READY\nabcdefg\nhi中Z\nabcdefg\nh中Y\nabcdefg\nhi😀Z\nabcdefg\nh😀Y\n0123456\n789\ne\u{301}中\nM01\n\n",
    },
    IntentionalDifference {
        test: "wide_characters_resize_narrower",
        command: &["capture-pane", "-p", "-S", "-", "-E", "-", "-t", "S"],
        limit: GridLimit::NoResizeReflow,
        field: Field::Stdout,
        tm: "READY\nabcdefg\n中Z\nabcdefg\nY\nabcdefg\n😀Z\nabcdefg\nY\n0123456\ne\u{301}中\nM01\n\n",
        reference: "READY\nabcdefg\nhi中Z\nabcdefg\nh中Y\nabcdefg\nhi😀Z\nabcdefg\nh😀Y\n0123456\n789\ne\u{301}中\nM01\n\n",
    },
    IntentionalDifference {
        test: "wide_characters_resize_narrower",
        command: &["capture-pane", "-p", "-e", "-J", "-S", "-", "-t", "S"],
        limit: GridLimit::NoResizeReflow,
        field: Field::Stdout,
        tm: "READY\nabcdefg\n中Z\nabcdefgY\nabcdefg\n😀Z\nabcdefg\nY\n0123456\ne\u{301}中\nM01\n\n",
        reference: "READY\nabcdefghi中Z\nabcdefgh中Y\nabcdefghi😀Z\nabcdefgh😀Y\n0123456789\ne\u{301}中\nM01\n\n",
    },
    IntentionalDifference {
        test: "wide_characters_resize_narrower",
        command: &["capture-pane", "-p", "-J", "-S", "-3", "-E", "2", "-t", "S"],
        limit: GridLimit::NoResizeReflow,
        field: Field::Stdout,
        tm: "Y\nabcdefg\n😀Z\nabcdefg\nY\n0123456\n",
        reference: "abcdefghi😀Z\nabcdefgh😀Y\n0123456789\n",
    },
    IntentionalDifference {
        test: "wide_characters_resize_wider",
        command: &["capture-pane", "-p", "-t", "S"],
        limit: GridLimit::NoResizeReflow,
        field: Field::Stdout,
        tm: "abcdefg\nY\n0123456\ne\u{301}中\nM01\n\n",
        reference: "abcdefghi😀Z\nabcdefgh😀Y\n0123456789\ne\u{301}中\nM01\n\n",
    },
    IntentionalDifference {
        test: "wide_characters_resize_wider",
        command: &["capture-pane", "-p", "-J", "-t", "S"],
        limit: GridLimit::NoResizeReflow,
        field: Field::Stdout,
        tm: "abcdefg\nY\n0123456\ne\u{301}中\nM01\n\n",
        reference: "abcdefghi😀Z\nabcdefgh😀Y\n0123456789\ne\u{301}中\nM01\n\n",
    },
    IntentionalDifference {
        test: "wide_characters_resize_wider",
        command: &["capture-pane", "-p", "-e", "-t", "S"],
        limit: GridLimit::NoResizeReflow,
        field: Field::Stdout,
        tm: "abcdefg\nY\n0123456\ne\u{301}中\nM01\n\n",
        reference: "abcdefghi😀Z\nabcdefgh😀Y\n0123456789\ne\u{301}中\nM01\n\n",
    },
    IntentionalDifference {
        test: "wide_characters_resize_wider",
        command: &["capture-pane", "-p", "-e", "-J", "-t", "S"],
        limit: GridLimit::NoResizeReflow,
        field: Field::Stdout,
        tm: "abcdefg\nY\n0123456\ne\u{301}中\nM01\n\n",
        reference: "abcdefghi😀Z\nabcdefgh😀Y\n0123456789\ne\u{301}中\nM01\n\n",
    },
    IntentionalDifference {
        test: "wide_characters_resize_wider",
        command: &["capture-pane", "-p", "-N", "-t", "S"],
        limit: GridLimit::NoResizeReflow,
        field: Field::Stdout,
        tm: "abcdefg       \nY  \n0123456       \ne\u{301}中    \nM01    \n\n",
        reference: "abcdefghi😀Z  \nabcdefgh😀Y   \n0123456789    \ne\u{301}中  \nM01  \n\n",
    },
    IntentionalDifference {
        test: "wide_characters_resize_wider",
        command: &["capture-pane", "-p", "-S", "-", "-t", "S"],
        limit: GridLimit::NoResizeReflow,
        field: Field::Stdout,
        tm: "READY\nabcdefghi\n中Z\nabcdefgh中\nY\nabcdefghi\n😀Z\nabcdefg\nY\n0123456\ne\u{301}中\nM01\n\n",
        reference: "READY\nabcdefghi中Z\nabcdefgh中Y\nabcdefghi😀Z\nabcdefgh😀Y\n0123456789\ne\u{301}中\nM01\n\n",
    },
    IntentionalDifference {
        test: "wide_characters_resize_wider",
        command: &["capture-pane", "-p", "-S", "-", "-E", "-", "-t", "S"],
        limit: GridLimit::NoResizeReflow,
        field: Field::Stdout,
        tm: "READY\nabcdefghi\n中Z\nabcdefgh中\nY\nabcdefghi\n😀Z\nabcdefg\nY\n0123456\ne\u{301}中\nM01\n\n",
        reference: "READY\nabcdefghi中Z\nabcdefgh中Y\nabcdefghi😀Z\nabcdefgh😀Y\n0123456789\ne\u{301}中\nM01\n\n",
    },
    IntentionalDifference {
        test: "wide_characters_resize_wider",
        command: &["capture-pane", "-p", "-e", "-J", "-S", "-", "-t", "S"],
        limit: GridLimit::NoResizeReflow,
        field: Field::Stdout,
        tm: "READY\nabcdefghi\n中Z\nabcdefgh中\nY\nabcdefghi\n😀Z\nabcdefg\nY\n0123456\ne\u{301}中\nM01\n\n",
        reference: "READY\nabcdefghi中Z\nabcdefgh中Y\nabcdefghi😀Z\nabcdefgh😀Y\n0123456789\ne\u{301}中\nM01\n\n",
    },
    IntentionalDifference {
        test: "wide_characters_resize_wider",
        command: &["capture-pane", "-p", "-J", "-S", "-3", "-E", "2", "-t", "S"],
        limit: GridLimit::NoResizeReflow,
        field: Field::Stdout,
        tm: "Y\nabcdefghi\n😀Z\nabcdefg\nY\n0123456\n",
        reference: "READY\nabcdefghi中Z\nabcdefgh中Y\nabcdefghi😀Z\nabcdefgh😀Y\n0123456789\n",
    },
];

pub fn check_command(
    known: &[IntentionalDifference],
    test: &str,
    arguments: &[&str],
    tm: &Outcome,
    reference: &Outcome,
) -> Result<(), String> {
    let listed = known
        .iter()
        .find(|entry| entry.test == test && entry.command == arguments);
    let observed = first_difference(tm, reference);
    match (listed, observed) {
        (None, None) => Ok(()),
        (None, Some(difference)) => Err(format!(
            "{test} {arguments:?}: {:?} differs\n tm: {}\nref: {}",
            difference.field, difference.tm, difference.reference
        )),
        (Some(entry), None) => Err(format!(
            "{test} {arguments:?}: now matches the reference; remove its INTENTIONAL_DIFFERENCES entry ({})",
            entry.limit.parity_row()
        )),
        (Some(entry), Some(difference)) => {
            if difference.field == entry.field
                && difference.tm == format!("{:?}", entry.tm)
                && difference.reference == format!("{:?}", entry.reference)
            {
                Ok(())
            } else {
                Err(format!(
                    "{test} {arguments:?}: difference no longer matches its INTENTIONAL_DIFFERENCES entry ({})\n tm: {}\nref: {}",
                    entry.limit.parity_row(), difference.tm, difference.reference
                ))
            }
        }
    }
}

pub fn compare_commands(test: &str, tm: &Mux, reference: &Mux, commands: &[&[&str]]) {
    for arguments in commands {
        if let Err(message) = check_command(
            INTENTIONAL_DIFFERENCES,
            test,
            arguments,
            &tm.outcome(arguments),
            &reference.outcome(arguments),
        ) {
            panic!("{message}");
        }
    }
}

fn replace_bytes(haystack: &[u8], needle: &[u8], replacement: &[u8]) -> Vec<u8> {
    if needle.is_empty() {
        return haystack.to_vec();
    }
    let mut output = Vec::with_capacity(haystack.len());
    let mut index = 0;
    while index < haystack.len() {
        if haystack[index..].starts_with(needle) {
            output.extend_from_slice(replacement);
            index += needle.len();
        } else {
            output.push(haystack[index]);
            index += 1;
        }
    }
    output
}

pub fn normalize(value: &[u8], mux: &Mux) -> Vec<u8> {
    let value = value
        .strip_prefix(b"tmux: ")
        .or_else(|| value.strip_prefix(b"tm: "))
        .unwrap_or(value);
    let output = replace_bytes(value, mux.socket.as_os_str().as_bytes(), b"<socket>");
    let output = replace_bytes(&output, mux.root.as_os_str().as_bytes(), b"<root>");
    let output = replace_bytes(&output, mux.home.as_os_str().as_bytes(), b"<home>");
    replace_bytes(&output, b"tmux 3.8", b"tmux <version>")
}

pub fn run_with_deadline(mut command: Command, timeout: Duration) -> Result<Output, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("spawn {:?}: {error}", command.get_program()))?;
    let stdout = drain(child.stdout.take().expect("piped stdout"));
    let stderr = drain(child.stderr.take().expect("piped stderr"));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
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
    };
    Ok(Output {
        status,
        stdout: join_drain(stdout)?,
        stderr: join_drain(stderr)?,
    })
}

fn drain<R: Read + Send + 'static>(mut pipe: R) -> std::thread::JoinHandle<std::io::Result<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        pipe.read_to_end(&mut bytes)?;
        Ok(bytes)
    })
}

fn join_drain(
    handle: std::thread::JoinHandle<std::io::Result<Vec<u8>>>,
) -> Result<Vec<u8>, String> {
    handle
        .join()
        .map_err(|_| "output reader panicked".to_owned())?
        .map_err(|error| format!("read command output: {error}"))
}
