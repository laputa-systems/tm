Prefix: DSLP
Next: 26

Deslop campaign backlog ([plan](https://github.com/laputa-systems/stuff/blob/main/docs/campaigns/deslop.md), tier C). One agent works this file with the `deslop` skill: take the lowest-numbered open ticket whose area does not start with `Needs owner ·`. After DSLP-5, work the open P1 and P2 findings, then fill in `## Report` below.

## Report

The broader pass remains open. DSLP-6's navigation fixture now delivers each
prefix/repeat sequence before state probes, keeps all nine full-state
observations and checks literal window states. A delayed-probe control fails
the old test and passes the new one. The expiry barrier now detects a five-second
runtime expiry fault that the old barrier missed. A real Unix-socket regression
guards short, exclusive fixture paths. Native full tests and lint pass; the
historical CI log lacks timestamps, and quiet-host test cost remains unqualified.
See [the repeat qualification](docs/deslop-repeat-flake.md) for exact proof.

## Tickets

### DSLP-13 · P2 · Dead code: server lifecycle and dispatch component

- **Area:** deslop · dead code
- **Where:** src/server.rs:1 through ServerState::window_index_for_creation (ending before ServerState::window_index), src/model.rs
- **Problem:** Dead code work is unmeasured for this bounded slice; CLI and attached clients consume session lifecycle, command dispatch, and layouts.
- **Recommendation:** Run the deslop Dead code step on this slice, preserving its consumer contracts. Accept: findings removed or allowlisted with reasons and walk-backs filed; run cargo test --test tmux_core_compat; cargo test --test tmux_lifecycle; make test; make lint, all through the isolation prefix. Restrict server.rs work to the named functions, at function boundaries.
- **Fix with:** deslop
- **Effort:** L

### DSLP-14 · P2 · Types: server targeting and remaining functions component

- **Area:** deslop · types
- **Where:** src/server.rs: ServerState::window_index through end, at function boundaries, src/model.rs
- **Problem:** Types work is unmeasured for this bounded slice; CLI and attached clients consume targeting, topology, input, and lifecycle behavior.
- **Recommendation:** Run the deslop Types step on this slice, preserving its consumer contracts. Accept: adopted profile clean, escape hatches not increased, and witnesses recorded; run cargo test --test tmux_targeting; cargo test --test tmux_lifecycle; make test; make lint, all through the isolation prefix. Restrict server.rs work to the named functions, at function boundaries.
- **Fix with:** deslop
- **Effort:** L

### DSLP-15 · P2 · Tests: server targeting and remaining functions component

- **Area:** deslop · tests
- **Where:** src/server.rs: ServerState::window_index through end, at function boundaries, src/model.rs
- **Problem:** Tests work is unmeasured for this bounded slice; CLI and attached clients consume targeting, topology, input, and lifecycle behavior.
- **Recommendation:** Run the deslop Tests step on this slice, preserving its consumer contracts. Accept: critical mutation survivors dispositioned and cost budget met; run cargo test --test tmux_targeting; cargo test --test tmux_lifecycle; make test; make lint, all through the isolation prefix. Restrict server.rs work to the named functions, at function boundaries.
- **Fix with:** deslop
- **Effort:** L

### DSLP-16 · P2 · Dead code: server targeting and remaining functions component

- **Area:** deslop · dead code
- **Where:** src/server.rs: ServerState::window_index through end, at function boundaries, src/model.rs
- **Problem:** Dead code work is unmeasured for this bounded slice; CLI and attached clients consume targeting, topology, input, and lifecycle behavior.
- **Recommendation:** Run the deslop Dead code step on this slice, preserving its consumer contracts. Accept: findings removed or allowlisted with reasons and walk-backs filed; run cargo test --test tmux_targeting; cargo test --test tmux_lifecycle; make test; make lint, all through the isolation prefix. Restrict server.rs work to the named functions, at function boundaries.
- **Fix with:** deslop
- **Effort:** L

### DSLP-17 · P2 · Types: terminal and rendering component

- **Area:** deslop · types
- **Where:** src/pty.rs, src/terminal.rs, src/cells.rs, src/capture.rs; rendering functions in src/server.rs
- **Problem:** Types work is unmeasured for this bounded slice; pane processes and attached clients consume terminal bytes, screen state, and render budgets.
- **Recommendation:** Run the deslop Types step on this slice, preserving its consumer contracts. Accept: adopted profile clean, escape hatches not increased, and witnesses recorded; run cargo test --test attached_client_pty; cargo test --test terminal_corpus; make perf if the hot path changes; make test; make lint, all through the isolation prefix. Restrict server.rs work to the named functions, at function boundaries.
- **Fix with:** deslop
- **Effort:** L

### DSLP-18 · P2 · Tests: terminal and rendering component

- **Area:** deslop · tests
- **Where:** src/pty.rs, src/terminal.rs, src/cells.rs, src/capture.rs; rendering functions in src/server.rs
- **Problem:** Tests work is unmeasured for this bounded slice; pane processes and attached clients consume terminal bytes, screen state, and render budgets.
- **Recommendation:** Run the deslop Tests step on this slice, preserving its consumer contracts. Accept: critical mutation survivors dispositioned and cost budget met; run cargo test --test attached_client_pty; cargo test --test terminal_corpus; make perf if the hot path changes; make test; make lint, all through the isolation prefix. Restrict server.rs work to the named functions, at function boundaries.
- **Fix with:** deslop
- **Effort:** L

### DSLP-19 · P2 · Dead code: terminal and rendering component

- **Area:** deslop · dead code
- **Where:** src/pty.rs, src/terminal.rs, src/cells.rs, src/capture.rs; rendering functions in src/server.rs
- **Problem:** Dead code work is unmeasured for this bounded slice; pane processes and attached clients consume terminal bytes, screen state, and render budgets.
- **Recommendation:** Run the deslop Dead code step on this slice, preserving its consumer contracts. Accept: findings removed or allowlisted with reasons and walk-backs filed; run cargo test --test attached_client_pty; cargo test --test terminal_corpus; make perf if the hot path changes; make test; make lint, all through the isolation prefix. Restrict server.rs work to the named functions, at function boundaries.
- **Fix with:** deslop
- **Effort:** L

### DSLP-20 · P2 · Types: copy and input component

- **Area:** deslop · types
- **Where:** src/copy_mode.rs, src/config.rs, src/options.rs; input functions in src/server.rs
- **Problem:** Types work is unmeasured for this bounded slice; server and attached clients consume bindings, selections, buffers, and compiled settings.
- **Recommendation:** Run the deslop Types step on this slice, preserving its consumer contracts. Accept: adopted profile clean, escape hatches not increased, and witnesses recorded; run cargo test --test tmux_copy_mode_compat; cargo test --features parity --test tmux_bindings; cargo test --test tmux_config_compat; make test; make lint, all through the isolation prefix. Restrict server.rs work to the named functions, at function boundaries.
- **Fix with:** deslop
- **Effort:** L

### DSLP-21 · P2 · Tests: copy and input component

- **Area:** deslop · tests
- **Where:** src/copy_mode.rs, src/config.rs, src/options.rs; input functions in src/server.rs
- **Problem:** Tests work is unmeasured for this bounded slice; server and attached clients consume bindings, selections, buffers, and compiled settings.
- **Recommendation:** Run the deslop Tests step on this slice, preserving its consumer contracts. Accept: critical mutation survivors dispositioned and cost budget met; run cargo test --test tmux_copy_mode_compat; cargo test --features parity --test tmux_bindings; cargo test --test tmux_config_compat; make test; make lint, all through the isolation prefix. Restrict server.rs work to the named functions, at function boundaries.
- **Fix with:** deslop
- **Effort:** L

### DSLP-22 · P2 · Dead code: copy and input component

- **Area:** deslop · dead code
- **Where:** src/copy_mode.rs, src/config.rs, src/options.rs; input functions in src/server.rs
- **Problem:** Dead code work is unmeasured for this bounded slice; server and attached clients consume bindings, selections, buffers, and compiled settings.
- **Recommendation:** Run the deslop Dead code step on this slice, preserving its consumer contracts. Accept: findings removed or allowlisted with reasons and walk-backs filed; run cargo test --test tmux_copy_mode_compat; cargo test --features parity --test tmux_bindings; cargo test --test tmux_config_compat; make test; make lint, all through the isolation prefix. Restrict server.rs work to the named functions, at function boundaries.
- **Fix with:** deslop
- **Effort:** L

### DSLP-23 · P2 · Types: harness and parity component

- **Area:** deslop · types
- **Where:** tests/support/*.rs and their integration-suite callers
- **Problem:** Types work is unmeasured for this bounded slice; integration suites consume private sockets, PTY lifecycle, cleanup, and reference comparisons.
- **Recommendation:** Run the deslop Types step on this slice, preserving its consumer contracts. Accept: adopted profile clean, escape hatches not increased, and witnesses recorded; run cargo test --test parity_contract; cargo test --test parity_scratch; make parity; make test; make lint, all through the isolation prefix. Restrict server.rs work to the named functions, at function boundaries.
- **Fix with:** deslop
- **Effort:** L

### DSLP-24 · P2 · Tests: harness and parity component

- **Area:** deslop · tests
- **Where:** tests/support/*.rs and their integration-suite callers
- **Problem:** Tests work is unmeasured for this bounded slice; integration suites consume private sockets, PTY lifecycle, cleanup, and reference comparisons.
- **Recommendation:** Run the deslop Tests step on this slice, preserving its consumer contracts. Accept: critical mutation survivors dispositioned and cost budget met; run cargo test --test parity_contract; cargo test --test parity_scratch; make parity; make test; make lint, all through the isolation prefix. Restrict server.rs work to the named functions, at function boundaries.
- **Fix with:** deslop
- **Effort:** L

### DSLP-25 · P2 · Dead code: harness and parity component

- **Area:** deslop · dead code
- **Where:** tests/support/*.rs and their integration-suite callers
- **Problem:** Dead code work is unmeasured for this bounded slice; integration suites consume private sockets, PTY lifecycle, cleanup, and reference comparisons.
- **Recommendation:** Run the deslop Dead code step on this slice, preserving its consumer contracts. Accept: findings removed or allowlisted with reasons and walk-backs filed; run cargo test --test parity_contract; cargo test --test parity_scratch; make parity; make test; make lint, all through the isolation prefix. Restrict server.rs work to the named functions, at function boundaries.
- **Fix with:** deslop
- **Effort:** L

### DSLP-7 · P3 · Linux CI failure in commands_from_a_killed_pane_fall_back_like_the_reference is unexplained

- **Area:** Needs environment · deslop · ground
- **Where:** tests/tmux_core_compat.rs; .github/workflows/ci.yml (ubuntu-26.04-arm)
- **Problem:** The base push CI run 43983198066 failed on Ubuntu in this test (tm's in-pane output did not finish within six seconds); macOS passed and the log does not give the cause. It does not reproduce on macOS arm64 at base 6cf0b94 (the test passed in every full run during Ground). No runtime defect is established.
- **Recommendation:** On a Linux arm64 host, run cargo test --test tmux_core_compat commands_from_a_killed_pane_fall_back_like_the_reference 50 times, capture the pane and daemon log on failure, and decide between a fixture wait and a runtime defect. Accept: root cause named, and a test or fix lands.
- **Fix with:** root-cause
- **Effort:** M

### DSLP-9 · P3 · Attached client's unterminated SGR mouse prefix grows daemon memory without bound

- **Area:** deslop · diagnose · input
- **Where:** src/server.rs feed_client_mouse, ClientState.mouse_buffer
- **Problem:** After input begins with ESC [ < the daemon appends every byte to mouse_buffer until an M or m arrives; real reports are under 20 bytes. Reproduced at 6cf0b94 with mouse on and a PTY-attached client sending ESC [ < then 64 MiB of '1': daemon RSS 4.7 MiB to 117 MiB, still growing. See docs/deslop-diagnose.md.
- **Recommendation:** Cap the buffer at a small multiple of the longest valid report (for example 32 bytes) and flush the bytes as ordinary input when it overflows, as an invalid prefix already is. Accept: a unit test that feeds 1 MiB after ESC [ < and asserts the buffer stays at the cap and the bytes reach the pane; make test and make lint.
- **Fix with:** lifecycle-concurrency
- **Effort:** S

### DSLP-10 · P3 · Daemon log and lock files follow symlinks: truncation and chmod of the target

- **Area:** deslop · diagnose · socket sidecars
- **Where:** src/server.rs DaemonLog::create and DaemonLog::write rotation, run_daemon_inner lock file
- **Problem:** The .log and .lock sidecars are opened without O_NOFOLLOW and then chmodded by path. Reproduced at 6cf0b94: with $D/s.sock.log a symlink to a file, tm -S $D/s.sock new-session -d overwrote that file with the daemon log; with $D/s.sock.lock a symlink to a mode 644 file, the target became mode 600. It matters when -S names a path in a shared writable directory; the default directory is private. See docs/deslop-diagnose.md.
- **Recommendation:** Open both sidecars with O_CREAT|O_NOFOLLOW and mode 0600, set permissions through the open descriptor, and reopen the log for rotation the same way, refusing a symlink with a clear error. Accept: socket_permissions tests that a symlinked .log and .lock are refused and their targets unchanged; make test and make lint.
- **Fix with:** os-boundaries
- **Effort:** S
