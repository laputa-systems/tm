#![cfg(unix)]

#[path = "support/parity.rs"]
#[allow(dead_code)]
mod parity;

#[test]
fn descriptive_scenario_names_leave_room_for_private_unix_sockets() {
    let scratch = parity::Scratch::new(&"descriptive-scenario-name-".repeat(5));
    let directory = scratch.root().join("reference");
    std::fs::create_dir(&directory).unwrap();
    let listener = std::os::unix::net::UnixListener::bind(directory.join("server.sock"))
        .expect("private reference socket must fit the platform pathname limit");
    drop(listener);
}
