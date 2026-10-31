# Testing

Run the host test suite with `make test`. `make test-ci` runs it in the release
profile.

The integration tests use private `TM_SOCKET` paths and do not invoke tmux or
connect to an existing tmux server. [`tests/tmux_core_compat.rs`](../tests/tmux_core_compat.rs)
covers command, session, window, pane, PTY, capture, format, option, and
`run-shell` behavior. Copy mode, buffers, and compiled startup settings have
focused suites in [`tests/tmux_copy_mode_compat.rs`](../tests/tmux_copy_mode_compat.rs),
[`tests/tmux_buffers_compat.rs`](../tests/tmux_buffers_compat.rs), and
[`tests/tmux_config_compat.rs`](../tests/tmux_config_compat.rs). The daemon
lifecycle smoke test is [`tests/core_smoke.rs`](../tests/core_smoke.rs).

[`tests/attached_client_pty.rs`](../tests/attached_client_pty.rs) covers the
real terminal boundary; see [PTY integration tests](ptytest.md). Passing the
suite establishes compatibility only for the exercised flows, not for the
whole tmux command or configuration surface.

Input timing follows the compiled profile. With `escape-time=0`, a partial
prefix or terminal sequence gets a 1 ms deadline before it is forwarded, while
repeat mode uses tmux's 500 ms `repeat-time` window and refreshes on each
repeat. Paste payloads and repeat deadlines are covered by their focused
suites; tests wait for pane bytes or terminal state instead of assuming a
settle delay.
