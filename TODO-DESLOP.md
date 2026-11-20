Prefix: DSLP
Next: 11

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

### DSLP-3 · P2 · Types: command and protocol component

- **Area:** deslop · types
- **Where:** src/command.rs, src/protocol.rs; their unit tests
- **Problem:** Strict-profile candidates are unmeasured for the command and protocol component (about 4.1k lines). CLI clients and the server consume its request and wire contracts.
- **Recommendation:** Run the deslop Types step on this component; preserve CLI parsing, errors, protocol version and wire bytes. Accept: adopted profile clean, escape hatches not increased, witnesses recorded; run cargo test --bin tm command::tests, cargo test --bin tm protocol::tests, cargo test --test protocol_version, make test and make lint through the isolation prefix.
- **Fix with:** deslop
- **Effort:** M

### DSLP-4 · P2 · Tests: command and protocol component

- **Area:** deslop · tests
- **Where:** src/command.rs, src/protocol.rs, tests/tmux_cli_flags.rs, tests/protocol_version.rs
- **Problem:** Coverage and mutation of CLI parsing and protocol contracts are unmeasured for this bounded component (about 9k lines). CLI users, clients and the server consume these contracts.
- **Recommendation:** Run the deslop Tests step on this component, covering flags, exit behavior, protocol version and wire bytes. Accept: critical survivors dispositioned and cost budget met; run cargo test --test tmux_cli_flags, cargo test --test protocol_version, make test and make lint through the isolation prefix.
- **Fix with:** deslop
- **Effort:** L

### DSLP-5 · P2 · Dead code: command and protocol component

- **Area:** deslop · dead code
- **Where:** src/command.rs, src/protocol.rs; their callers and tests
- **Problem:** No dead-code scan has settled the command and protocol component (about 4.1k lines). CLI users, clients and the server consume its parsing and wire contracts.
- **Recommendation:** Run the deslop Dead code step on this component, preserving CLI flags, errors, protocol version and wire bytes. Accept: findings clean or allowlisted with reasons and walk-backs filed; run cargo test --test tmux_cli_flags, cargo test --test protocol_version, make test and make lint through the isolation prefix.
- **Fix with:** deslop
- **Effort:** M

### DSLP-8 · P2 · Daemon exits and drops every session when accept fails on descriptor exhaustion

- **Area:** deslop · diagnose · server sockets
- **Where:** src/server.rs serve, spawn_connection, handle_connection
- **Problem:** Any accept error other than Interrupted or WouldBlock, and any failed connection-thread spawn, ends the daemon; connections have no read deadline or limit. Reproduced at 6cf0b94: (ulimit -n 256; tm -S $S new-session -d 'sleep 60'), then hold about 250 idle AF_UNIX connections to $S; tm -S $S list-sessions then reports no server running and the log ends 'daemon: Too many open files (os error 24)'. All sessions and panes are lost. See docs/deslop-diagnose.md.
- **Recommendation:** Treat EMFILE, ENFILE, ENOBUFS, ENOMEM and ECONNABORTED as transient (log, back off, keep serving); give the handshake and first request read a deadline and cap concurrent connection threads. Accept: a test that exhausts descriptors with idle connections and still lists the session afterwards, and a test that a silent connection is dropped; make test and make lint.
- **Fix with:** lifecycle-concurrency
- **Effort:** M

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

