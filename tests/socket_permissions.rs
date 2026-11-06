#![cfg(unix)]

#[path = "support/tm_bin.rs"]
mod tm_bin;

use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output};

fn run(socket: &str, arguments: &[&str]) -> Output {
    Command::new(tm_bin::tm_bin())
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

fn default_socket_scratch(label: &str) -> std::path::PathBuf {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".work/tmp")
        .join(format!("default-socket-{}-{label}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn run_default(tmpdir: &std::path::Path, arguments: &[&str]) -> Output {
    Command::new(tm_bin::tm_bin())
        .env_remove("TM_SOCKET")
        .env("TMPDIR", tmpdir)
        .args(arguments)
        .output()
        .expect("run tm")
}

fn euid_directory(tmpdir: &std::path::Path) -> std::path::PathBuf {
    tmpdir.join(format!("tm-{}", unsafe { libc::geteuid() }))
}

#[test]
fn default_socket_lives_in_a_private_per_user_directory() {
    let tmpdir = default_socket_scratch("private");
    let created = run_default(
        &tmpdir,
        &["new-session", "-d", "-s", "d", "--", "sh", "-c", "sleep 30"],
    );
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let directory = euid_directory(&tmpdir);
    let mode = std::fs::metadata(&directory).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    let socket = directory.join("default");
    let socket_mode = std::fs::metadata(&socket).unwrap().permissions().mode() & 0o777;
    assert_eq!(socket_mode, 0o600);
    assert!(directory.join("default.lock").exists());
    assert!(directory.join("default.log").exists());
    assert!(!tmpdir.read_dir().unwrap().any(|entry| {
        entry.unwrap().file_name().to_string_lossy().ends_with(".sock")
    }));

    let killed = run_default(&tmpdir, &["kill-server"]);
    assert!(killed.status.success());
    let _ = std::fs::remove_dir_all(&tmpdir);
}

#[test]
fn default_socket_directory_with_group_or_other_access_is_refused() {
    let tmpdir = default_socket_scratch("unsafe");
    let directory = euid_directory(&tmpdir);
    std::fs::create_dir(&directory).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o750)).unwrap();
    let refused = run_default(
        &tmpdir,
        &["new-session", "-d", "-s", "d", "--", "sh", "-c", "sleep 30"],
    );
    assert!(!refused.status.success());
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains(&format!("directory {} has unsafe permissions", directory.display())),
        "{stderr}"
    );
    assert!(!directory.join("default").exists());
    let _ = std::fs::remove_dir_all(&tmpdir);
}

#[test]
fn default_socket_directory_that_is_a_symlink_is_refused() {
    let tmpdir = default_socket_scratch("symlink");
    let target = tmpdir.join("elsewhere");
    std::fs::create_dir(&target).unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).unwrap();
    let directory = euid_directory(&tmpdir);
    std::os::unix::fs::symlink(&target, &directory).unwrap();
    let refused = run_default(
        &tmpdir,
        &["new-session", "-d", "-s", "d", "--", "sh", "-c", "sleep 30"],
    );
    assert!(!refused.status.success());
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains(&format!("{} is not a directory", directory.display())),
        "{stderr}"
    );
    assert!(!target.join("default").exists());
    let _ = std::fs::remove_dir_all(&tmpdir);
}

#[test]
fn explicit_socket_naming_a_regular_file_leaves_the_file_intact() {
    let tmpdir = default_socket_scratch("regular-file");
    let precious = tmpdir.join("precious.txt");
    std::fs::write(&precious, "keep me").unwrap();
    let refused = run(
        precious.to_str().unwrap(),
        &["new-session", "-d", "-s", "d", "--", "sh", "-c", "sleep 30"],
    );
    assert!(!refused.status.success());
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains(&format!(
            "{} exists and is not a socket owned by the current user",
            precious.display()
        )),
        "{stderr}"
    );
    assert_eq!(std::fs::read_to_string(&precious).unwrap(), "keep me");
    let _ = std::fs::remove_dir_all(&tmpdir);
}
