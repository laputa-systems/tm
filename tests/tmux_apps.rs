#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;
#[path = "support/pty_harness.rs"]
#[allow(dead_code)]
mod pty_harness;

use std::path::Path;
use std::time::{Duration, Instant};

use parity::{Mux, Scratch};
use ptytest::{PtyTest, Size};

fn create(mux: &Mux, command: &[&str]) {
    let mut arguments = vec![
        "new-session", "-d", "-s", "S", "-x", "80", "-y", "24", "--",
    ];
    arguments.extend_from_slice(command);
    mux.ok(&arguments);
}

fn attach(mux: &Mux, name: &str, width: u16, height: u16) -> (PtyTest, ptytest::TerminalBaseline) {
    let terminal = pty_harness::spawn_attached(
        mux,
        name,
        &["attach-session", "-t", "S"],
        Size::new(width, height).expect("size"),
    );
    let baseline = terminal.terminal_baseline();
    (terminal, baseline)
}

fn send(terminal: &mut PtyTest, bytes: &[u8]) {
    terminal
        .send_bytes(terminal.deadline(Duration::from_secs(5)), bytes)
        .expect("send terminal input");
}

fn wait_file(path: &Path, expected: &[u8]) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if std::fs::read(path).ok().as_deref() == Some(expected) {
            return;
        }
        if Instant::now() >= deadline {
            let actual = std::fs::read(path).unwrap_or_default();
            assert_eq!(actual, expected, "file {}", path.display());
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn shell_editing_and_multiline_paste_write_the_file() {
    let scratch = Scratch::new("apps-shell");
    let tm = Mux::tm(&scratch, "tm");
    let output = scratch.root().join("shell.txt");
    create(
        &tm,
        &[
            "sh",
            "-c",
            "printf 'SH_READY\\n'; cat > \"$1\"; printf 'SH_DONE\\n'; sleep 30",
            "sh",
            output.to_str().expect("path"),
        ],
    );
    let (mut client, baseline) = attach(&tm, "shell", 80, 24);
    pty_harness::wait_for_text(&mut client, "SH_READY", Duration::from_secs(5));
    let payload = b"first line\nsecond line\n";
    let mut paste = b"\x1b[200~".to_vec();
    paste.extend_from_slice(payload);
    paste.extend_from_slice(b"\x1b[201~");
    send(&mut client, &paste);
    send(&mut client, b"\x04");
    wait_file(&output, payload);
    pty_harness::wait_for_text(&mut client, "SH_DONE", Duration::from_secs(5));
    pty_harness::detach(&mut client, &baseline);
}

#[test]
fn vim_edits_and_restores_the_attached_screen() {
    let scratch = Scratch::new("apps-vim");
    let tm = Mux::tm(&scratch, "tm");
    let output = scratch.root().join("vim.txt");
    std::fs::write(&output, b"").expect("create vim file");
    create(
        &tm,
        &[
            "vim",
            "-u",
            "NONE",
            "-N",
            "-n",
            output.to_str().expect("path"),
        ],
    );
    let (mut client, baseline) = attach(&tm, "vim", 80, 24);
    client
        .wait_for_quiescence(client.deadline(Duration::from_secs(5)), Duration::from_millis(200))
        .expect("vim ready");
    send(&mut client, b"ihello from vim\x1b:wq\r");
    wait_file(&output, b"hello from vim\n");
    assert_eq!(
        client
            .wait_for_exit(client.deadline(Duration::from_secs(5)))
            .expect("vim exit"),
        ptytest::ExitStatus::Code(0)
    );
    client
        .assert_terminal_restored(&baseline)
        .expect("restore after vim");
    client
        .finish(client.deadline(Duration::from_secs(5)))
        .expect("reap vim client");
}

#[test]
fn less_survives_resize_detach_and_reattach_without_idle_frames() {
    let scratch = Scratch::new("apps-less");
    let tm = Mux::tm(&scratch, "tm");
    let output = scratch.root().join("less.txt");
    let contents = (1..=100)
        .map(|line| format!("LINE_{line:03}\n"))
        .collect::<String>();
    std::fs::write(&output, contents).expect("create less file");
    create(
        &tm,
        &[
            "sh",
            "-c",
            "less \"$1\"; printf 'LESS_DONE\\n'; sleep 30",
            "sh",
            output.to_str().expect("path"),
        ],
    );
    let (mut client, baseline) = attach(&tm, "less", 80, 24);
    pty_harness::wait_for_text(&mut client, "LINE_001", Duration::from_secs(5));
    client
        .resize(Size::new(100, 30).expect("size"))
        .expect("resize attached client");
    client
        .wait_for_quiescence(client.deadline(Duration::from_secs(3)), Duration::from_millis(150))
        .expect("resize frame");
    let stable = client.raw_output().len();
    client
        .wait_for_quiescence(client.deadline(Duration::from_secs(2)), Duration::from_millis(200))
        .expect("idle frame");
    assert_eq!(client.raw_output().len(), stable, "idle client kept rendering");
    pty_harness::detach(&mut client, &baseline);

    let (mut client, baseline) = attach(&tm, "less-again", 80, 24);
    pty_harness::wait_for_text(&mut client, "LINE_001", Duration::from_secs(5));
    send(&mut client, b"q");
    pty_harness::wait_for_text(&mut client, "LESS_DONE", Duration::from_secs(5));
    pty_harness::detach(&mut client, &baseline);
}
