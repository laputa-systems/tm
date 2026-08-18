# PTY integration tests

`tests/attached_client_pty.rs` uses `ptytest` for the outer kernel PTY. Keep
tm daemon/socket setup and fixture protocol barriers in this repository; do not
add a second PTY spawn, polling, or cleanup helper.

Run the focused case with `cargo test --test attached_client_pty`. It uses a
hermetic test environment, an audited `xterm-minimal-v1` profile, and named
semantic screen barriers. Do not add settle sleeps: wait for the fixture marker
or visible terminal state instead.

On a crate-owned failure, inspect `target/ptytest-failures/<scenario>-*/` for
exact input/output, events, semantic screen, and redacted configuration.
Snapshots, if a future attached-client scenario needs one, live beside that
scenario and are updated only with `PTYTEST_UPDATE_SNAPSHOTS=1`.
