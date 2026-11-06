#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;

use std::fs::File;
use std::os::fd::FromRawFd;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

#[path = "support/raw_pty.rs"]
#[allow(dead_code)]
mod raw_pty;

use parity::{Mux, Scratch};
use raw_pty::RawClient;

fn create_session(mux: &Mux, name: &str, command: &str) {
    mux.ok(&[
        "new-session",
        "-d",
        "-s",
        name,
        "-x",
        "40",
        "-y",
        "8",
        "--",
        "sh",
        "-c",
        command,
    ]);
}

fn assert_responsive(mux: &Mux, label: &str) {
    let started = Instant::now();
    mux.ok(&["display-message", "-p", "-t", "S", "responsive"]);
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "{label} blocked commands for {:?}",
        started.elapsed()
    );
}

fn wait_for_path(path: &Path, deadline: Duration) -> bool {
    let deadline = Instant::now() + deadline;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    path.exists()
}

fn wait_for_file_contents(path: &Path, expected: &str, deadline: Duration) -> bool {
    let deadline = Instant::now() + deadline;
    while Instant::now() < deadline {
        if std::fs::read_to_string(path).ok().as_deref() == Some(expected) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    std::fs::read_to_string(path).ok().as_deref() == Some(expected)
}

#[test]
fn foreground_run_shell_releases_the_state_lock() {
    let scratch = Scratch::new("liveness-run-shell");
    let tm = Mux::tm(&scratch, "tm");
    create_session(&tm, "S", "sleep 30");
    create_session(&tm, "OTHER", "sleep 30");
    let marker = scratch.root().join("run-shell.done");
    let command = format!("sleep 1; printf done > {}", marker.display());
    let mut child = tm
        .command(&["run-shell", "--", &command])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn run-shell");
    std::thread::sleep(Duration::from_millis(100));
    assert_responsive(&tm, "foreground run-shell");
    assert!(tm.ok(&["has-session", "-t", "OTHER"]).is_empty());
    assert!(wait_for_path(&marker, Duration::from_secs(5)));
    assert!(child.wait().expect("run-shell status").success());

    let marker = scratch.root().join("run-shell-target.done");
    let command = format!("sleep 1; printf done > {}", marker.display());
    let mut child = tm
        .command(&["run-shell", "-t", "S", "--", &command])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn targeted run-shell");
    std::thread::sleep(Duration::from_millis(100));
    assert_responsive(&tm, "targeted run-shell");
    assert!(wait_for_path(&marker, Duration::from_secs(5)));
    assert!(child.wait().expect("targeted run-shell status").success());
}

#[test]
fn nonreading_pipe_pane_does_not_block_commands() {
    let scratch = Scratch::new("liveness-pipe");
    let tm = Mux::tm(&scratch, "tm");
    create_session(&tm, "S", "read line; yes X | head -c 3000000; sleep 30");
    tm.ok(&["pipe-pane", "-t", "S", "--", "sleep 30"]);
    tm.ok(&["send-keys", "-t", "S", "go", "Enter"]);
    std::thread::sleep(Duration::from_millis(300));
    assert_responsive(&tm, "nonreading pipe-pane");
    create_session(&tm, "OTHER", "sleep 30");
    assert!(tm.ok(&["has-session", "-t", "OTHER"]).is_empty());
}

#[test]
fn copy_pipe_child_does_not_block_commands() {
    let scratch = Scratch::new("liveness-copy-pipe");
    let tm = Mux::tm(&scratch, "tm");
    create_session(&tm, "S", "printf 'first line\\nsecond line\\n'; sleep 30");
    tm.capture_until("S", "second line", Duration::from_secs(3));
    let marker = scratch.root().join("copy-pipe.txt");
    tm.ok(&["copy-mode", "-t", "S"]);
    tm.ok(&["send-keys", "-t", "S", "-X", "history-top"]);
    let command = format!("sleep 1; cat > {}", marker.display());
    tm.ok(&[
        "send-keys",
        "-t",
        "S",
        "-X",
        "copy-pipe-line",
        "--",
        &command,
    ]);
    assert_responsive(&tm, "copy-pipe child");
    assert!(
        wait_for_file_contents(&marker, "first line", Duration::from_secs(5)),
        "copy-pipe output: {:?}",
        std::fs::read_to_string(&marker)
    );
}

#[test]
fn nonreading_pane_keeps_ordered_input_without_drops() {
    let scratch = Scratch::new("liveness-pane-input");
    let tm = Mux::tm(&scratch, "tm");
    let output = scratch.root().join("pane-input.bin");
    let input = scratch.root().join("input.bin");
    let payload = (0..200_000u32)
        .map(|index| b'a' + (index % 26) as u8)
        .collect::<Vec<_>>();
    std::fs::write(&input, &payload).expect("write pane input");
    let total = payload.len() + 4;
    create_session(
        &tm,
        "S",
        &format!(
            "sleep 1; stty raw -echo; printf RAW_READY\\n; sleep 1; head -c {} > {}; printf done > {}.done; sleep 30",
            total,
            output.display(),
            output.display()
        ),
    );
    create_session(&tm, "OTHER", "sleep 30");
    tm.capture_until("S", "RAW_READY", Duration::from_secs(5));
    tm.ok(&["load-buffer", "-b", "large", input.to_str().expect("path")]);
    tm.ok(&["paste-buffer", "-b", "large", "-t", "S"]);
    tm.ok(&["send-keys", "-l", "-t", "S", "TAIL"]);
    assert_responsive(&tm, "nonreading pane input");
    assert!(tm.ok(&["has-session", "-t", "OTHER"]).is_empty());
    assert!(wait_for_path(
        Path::new(&format!("{}.done", output.display())),
        Duration::from_secs(10)
    ));
    let received = std::fs::read(&output).expect("pane input output");
    assert_eq!(received.len(), payload.len() + 4);
    assert!(received.starts_with(&payload));
    assert!(received.ends_with(b"TAIL"));
}

#[test]
fn a_stalled_attach_client_does_not_block_other_sessions() {
    let scratch = Scratch::new("liveness-slow-client");
    let tm = Mux::tm(&scratch, "tm");
    create_session(
        &tm,
        "S",
        "printf 'READY\\n'; seq 1 100000; sleep 30",
    );
    create_session(&tm, "OTHER", "sleep 30");
    let mut master = -1;
    let mut slave = -1;
    // SAFETY: openpty initializes both descriptors on success; the test owns
    // the returned master and slave until the child is reaped.
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        },
        0
    );
    // SAFETY: both descriptors were initialized by the successful openpty.
    let master_file = unsafe { File::from_raw_fd(master) };
    let slave_input = unsafe { File::from_raw_fd(slave) };
    let slave_output = slave_input.try_clone().expect("clone attach pty");
    let slave_error = slave_input.try_clone().expect("clone attach pty");
    let mut child = tm
        .command(&["attach-session", "-t", "S"])
        .stdin(Stdio::from(slave_input))
        .stdout(Stdio::from(slave_output))
        .stderr(Stdio::from(slave_error))
        .spawn()
        .expect("spawn stalled attach client");
    let _master_file = master_file;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let clients = tm.ok(&["list-clients", "-F", "#{client_name}"]);
        if !clients.trim().is_empty() || Instant::now() >= deadline {
            assert!(!clients.trim().is_empty(), "attach client did not register");
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_responsive(&tm, "stalled attach client");
    assert!(tm.ok(&["has-session", "-t", "OTHER"]).is_empty());
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn pane_programs_start_with_default_signal_dispositions() {
    let scratch = Scratch::new("liveness-sigign");
    let tm = Mux::tm(&scratch, "tm");
    let output = scratch.root().join("sigign.txt");
    create_session(
        &tm,
        "S",
        &format!(
            "sed -n 's/^SigIgn:[[:space:]]*//p' /proc/self/status > {0}.tmp; mv {0}.tmp {0}; sleep 30",
            output.display()
        ),
    );
    assert!(wait_for_path(&output, Duration::from_secs(5)));
    assert_eq!(
        std::fs::read_to_string(&output).expect("sigign").trim(),
        "0000000000000000"
    );
}

#[test]
fn out_of_range_session_sizes_are_rejected_like_tmux() {
    let scratch = Scratch::new("liveness-size");
    let tm = Mux::tm(&scratch, "tm");
    create_session(&tm, "S", "sleep 30");
    for (arguments, message) in [
        (["-x", "65536"], "width too large"),
        (["-x", "0"], "width too small"),
        (["-y", "65536"], "height too large"),
        (["-y", "0"], "height too small"),
        (["-x", "wide"], "width invalid"),
    ] {
        let mut command = vec!["new-session", "-d", "-s", "BAD"];
        command.extend(arguments);
        let outcome = tm.outcome(&command);
        assert_ne!(outcome.status, parity::ExitStatus::Code(0), "{arguments:?}");
        assert!(
            String::from_utf8_lossy(&outcome.stderr).contains(message),
            "{arguments:?}: {:?}",
            String::from_utf8_lossy(&outcome.stderr)
        );
    }
    assert_responsive(&tm, "rejected sizes");
    assert!(tm.ok(&["has-session", "-t", "S"]).is_empty());
}

#[test]
fn the_largest_session_size_keeps_one_pane_grid_affordable() {
    let scratch = Scratch::new("liveness-area");
    let tm = Mux::tm(&scratch, "tm");
    create_session(&tm, "S", "sleep 30");
    tm.ok(&["new-session", "-d", "-s", "BIG", "-x", "10000", "-y", "10000", "--", "sleep", "30"]);
    assert_eq!(
        tm.body(&["display-message", "-p", "-t", "BIG", "#{window_width}x#{window_height}"]),
        "10000x400"
    );
    assert_responsive(&tm, "largest session");
    assert!(tm.ok(&["has-session", "-t", "S"]).is_empty());
}

fn process_has_exited(pid: &str) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => stat
            .rsplit(')')
            .next()
            .is_some_and(|rest| rest.trim_start().starts_with('Z')),
        Err(_) => true,
    }
}

fn wait_for_exit(pid: &str, deadline: Duration) -> bool {
    let deadline = Instant::now() + deadline;
    while Instant::now() < deadline {
        if process_has_exited(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    process_has_exited(pid)
}

#[test]
fn a_daemon_exits_after_its_last_session_when_the_socket_file_is_gone() {
    let scratch = Scratch::new("liveness-socket-removed");
    let tm = Mux::tm(&scratch, "tm");
    create_session(&tm, "S", "sleep 1");
    let pid = tm.ok(&["display-message", "-p", "-t", "S", "#{pid}"]).trim().to_owned();
    std::fs::remove_file(tm.socket()).expect("remove socket file");
    assert!(
        wait_for_exit(&pid, Duration::from_secs(5)),
        "daemon {pid} stayed alive after its last session ended"
    );
}

#[test]
fn a_daemon_exits_on_sigterm_when_the_socket_file_is_gone() {
    let scratch = Scratch::new("liveness-socket-removed-term");
    let tm = Mux::tm(&scratch, "tm");
    create_session(&tm, "S", "sleep 30");
    let pid = tm.ok(&["display-message", "-p", "-t", "S", "#{pid}"]).trim().to_owned();
    std::fs::remove_file(tm.socket()).expect("remove socket file");
    // SAFETY: signalling the daemon pid reported by the daemon under test.
    assert_eq!(unsafe { libc::kill(pid.parse().expect("pid"), libc::SIGTERM) }, 0);
    assert!(
        wait_for_exit(&pid, Duration::from_secs(5)),
        "daemon {pid} ignored SIGTERM"
    );
}

#[test]
fn prompted_run_shell_does_not_block_other_commands() {
    let scratch = Scratch::new("liveness-prompt-run-shell");
    let tm = Mux::tm(&scratch, "tm");
    tm.ok(&[
        "new-session", "-d", "-s", "S", "-x", "40", "-y", "10", "-c",
        scratch.root().to_str().expect("path"), "--", "sh", "-c", "printf 'PANE_READY\\n'; sleep 30",
    ]);
    let marker = scratch.root().join("prompted.done");
    let mut client = RawClient::spawn(&tm, "tm", &["attach-session", "-t", "S"], 10, 40);
    client.wait_for_text("PANE_READY", Duration::from_secs(5));
    client.write(b"\x01");
    std::thread::sleep(Duration::from_millis(100));
    client.write(b"/");
    client.write(format!("run-shell \"sleep 2; touch {}\"", marker.display()).as_bytes());
    client.wait_for_text("run-shell", Duration::from_secs(5));
    std::thread::sleep(Duration::from_millis(300));
    client.write(b"\r");
    std::thread::sleep(Duration::from_millis(500));
    assert!(!marker.exists(), "prompt did not start run-shell before the check");
    let started = Instant::now();
    tm.ok(&["list-sessions"]);
    tm.ok(&["display-message", "-p", "x"]);
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "commands blocked for {:?} while a prompted run-shell ran",
        started.elapsed()
    );
    assert!(
        wait_for_path(&marker, Duration::from_secs(6)),
        "screen: {:?}",
        client.screen_text()
    );
}

fn peak_resident_kib(pid: &str) -> u64 {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .expect("read daemon status")
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|value| value.trim().trim_end_matches("kB").trim().parse().ok())
        .expect("VmHWM line")
}

#[test]
fn run_shell_output_does_not_accumulate_in_the_daemon() {
    let scratch = Scratch::new("liveness-run-shell-memory");
    let tm = Mux::tm(&scratch, "tm");
    create_session(&tm, "S", "sleep 30");
    let pid = tm.ok(&["display-message", "-p", "-t", "S", "#{pid}"]).trim().to_owned();
    let baseline = peak_resident_kib(&pid);
    let marker = scratch.root().join("flood.done");
    let flood = format!("head -c 400000000 /dev/zero; touch {}", marker.display());
    tm.ok(&["run-shell", "-b", "--", &flood]);
    assert!(wait_for_path(&marker, Duration::from_secs(30)));
    let output = tm.ok(&["run-shell", "--", "head -c 400000000 /dev/zero"]);
    assert!(output.len() <= 2 * 1024 * 1024, "foreground output was {} bytes", output.len());
    let growth = peak_resident_kib(&pid).saturating_sub(baseline);
    assert!(growth < 64 * 1024, "daemon peak RSS grew by {growth} KiB");
}
