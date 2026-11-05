#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::process::Output;

use parity::{Mux, Scratch};

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn different_protocol_message(mux: &Mux) -> String {
    let socket = mux.socket().display();
    format!("tm: tm server at {socket} runs a different protocol; run tm -S {socket} kill-server\n")
}

fn window_command(count: usize) -> Vec<String> {
    let mut arguments = ["new-window", "-d", "-t", "S", "--", "sh", "-c", "sleep 30"]
        .map(str::to_owned)
        .to_vec();
    arguments.extend((arguments.len() - 5..count).map(|_| "x".to_owned()));
    arguments
}

#[test]
fn server_with_another_protocol_version_is_refused() {
    let scratch = Scratch::new("protocol-mismatch");
    let tm = Mux::tm(&scratch, "tm");
    let listener = UnixListener::bind(tm.socket()).expect("bind fake server");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut version = [0_u8; 4];
        stream.read_exact(&mut version).expect("read preamble");
        let other = u32::from_le_bytes(version) ^ 1;
        stream.write_all(&other.to_le_bytes()).expect("reply");
        let mut rest = Vec::new();
        let _ = stream.read_to_end(&mut rest);
    });
    let output = tm.run(&["list-sessions"]);
    server.join().expect("fake server");
    assert!(!output.status.success());
    assert_eq!(stderr(&output), different_protocol_message(&tm));
}

#[test]
fn daemon_that_closes_before_replying_is_reported_as_another_protocol() {
    let scratch = Scratch::new("protocol-eof");
    let tm = Mux::tm(&scratch, "tm");
    let listener = UnixListener::bind(tm.socket()).expect("bind fake server");
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept");
        drop(stream);
    });
    let output = tm.run(&["list-sessions"]);
    server.join().expect("fake server");
    assert!(!output.status.success());
    assert_eq!(stderr(&output), different_protocol_message(&tm));
}

#[test]
fn over_long_argument_lists_are_refused_before_any_byte_is_sent() {
    let scratch = Scratch::new("protocol-limit");
    let tm = Mux::tm(&scratch, "tm");
    tm.ok(&["new-session", "-d", "-s", "S", "--", "sleep", "30"]);

    let at_limit = window_command(1024);
    let at_limit = at_limit.iter().map(String::as_str).collect::<Vec<_>>();
    tm.ok(&at_limit);
    assert_eq!(tm.body(&["list-windows", "-t", "S", "-F", "x"]).lines().count(), 2);

    let too_many = window_command(1025);
    let too_many = too_many.iter().map(String::as_str).collect::<Vec<_>>();
    let output = tm.run(&too_many);
    assert!(!output.status.success());
    assert_eq!(
        stderr(&output),
        "tm: too many arguments: 1025 (limit 1024)\n"
    );
    assert_eq!(tm.body(&["list-windows", "-t", "S", "-F", "x"]).lines().count(), 2);

    let silent = Mux::tm(&scratch, "silent");
    let listener = UnixListener::bind(silent.socket()).expect("bind fake server");
    listener.set_nonblocking(true).expect("nonblocking");
    let output = silent.run(&too_many);
    assert!(!output.status.success());
    assert_eq!(
        listener.accept().expect_err("client must not connect").kind(),
        std::io::ErrorKind::WouldBlock
    );
    tm.ok(&["kill-server"]);
}
