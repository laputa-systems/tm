#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output};

fn run(socket: &str, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tm"))
        .env("TM_SOCKET", socket)
        .args(arguments)
        .output()
        .expect("run tm")
}

#[test]
fn daemon_socket_is_private_and_missing_parent_is_private() {
    let base = std::env::temp_dir().join(format!(
        "tm-perm-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&base).unwrap();
    let socket = base
        .join("nested")
        .join("tm.sock")
        .to_string_lossy()
        .into_owned();

    let created = run(
        &socket,
        &["new-session", "-d", "-s", "perm", "--", "sh", "-c", "sleep 30"],
    );
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );

    let parent = base.join("nested");
    let parent_mode = std::fs::metadata(&parent).unwrap().permissions().mode() & 0o777;
    assert_eq!(parent_mode, 0o700, "missing socket parent should be 0700");

    let socket_mode = std::fs::metadata(&socket).unwrap().permissions().mode() & 0o777;
    assert_eq!(socket_mode, 0o600, "listening socket should be 0600");

    let killed = run(&socket, &["kill-server"]);
    assert!(
        killed.status.success(),
        "{}",
        String::from_utf8_lossy(&killed.stderr)
    );

    let _ = std::fs::remove_dir_all(&base);
}
