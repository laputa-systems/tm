# PTY integration tests

Run the real attached-client tests with `cargo test --test attached_client_pty`.
[`tests/attached_client_pty.rs`](../tests/attached_client_pty.rs) uses a
hermetic environment, a real outer PTY managed by `ptytest`, and the pinned
`xterm-minimal-v1` protocol profile. Keep daemon/socket setup and fixture
barriers in this repository; use fixture markers or visible terminal state
instead of settle sleeps.

The scenarios cover initial attachment, pane rendering and colors, copy-mode
scrolling, border resize, forwarding mouse reports to a pane application, and
opening a split without losing existing pane contents. Stable captures live in
[`tests/snapshots/`](../tests/snapshots/). Snapshot updates require
`PTYTEST_UPDATE_SNAPSHOTS=1`.

On failure, inspect `target/ptytest-failures/<scenario>-*/` for captured
input/output, events, semantic screen state, and scenario metadata in
`configuration.txt`.
