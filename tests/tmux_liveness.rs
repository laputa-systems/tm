#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;

use std::fs::File;
use std::os::fd::FromRawFd;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use parity::{Mux, Scratch};

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
    tm.ok(&["pipe-pane", "-t", "S", "--", "sh", "-c", "sleep 30"]);
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
    assert!(wait_for_path(&marker, Duration::from_secs(5)));
    assert_eq!(
        std::fs::read_to_string(&marker).expect("copy-pipe output"),
        "first line"
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
