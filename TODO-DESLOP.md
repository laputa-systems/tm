Prefix: DSLP
Next: 28

Deslop campaign backlog ([plan](https://github.com/laputa-systems/stuff/blob/main/docs/campaigns/deslop.md), tier C). One agent works this file with the `deslop` skill: take the lowest-numbered open ticket whose area does not start with `Needs owner ·`. After DSLP-5, work the open P1 and P2 findings, then fill in `## Report` below.

## Report

**Scope and status:** The broader pass was the Types, Tests and Dead code
work across the remaining components mapped in [Ground](docs/deslop-ground.md).
PRs #42, #43 and #44 completed the bounded audits for command and protocol,
server lifecycle and targeting, terminal and rendering, copy and input, and
harness and parity. The manual audits are complete; automated coverage,
mutation and dependency scans remain **Needs environment**. DSLP-7 needs a
Linux arm64 host and is owner/environment-gated. DSLP-26 and DSLP-27 need
the affected CI environment. All five remaining tickets are P3.

**Scorecard:** Rust lines in tracked `src/` and `tests/` at Ground base
`6cf0b94` versus `4cc4bd1`: source 32,621 → 32,717; tests 21,876 →
21,951. The deslop escape-hatch expression matched 1,116 → 1,123 lines
across those Rust files; the count does not establish soundness. Ground's warm
`make test` was 436–445 s under load; the current cold attempt exited 2 after 289.41 s when rustc received SIGKILL, and its retry exited 2 after 11.72 s with SIGKILL and SIGTERM. With `RUSTC_WRAPPER=` and two build jobs, subsequent attempts exited 2 after 711.01 s (147 CLI cases passed, 2 failed; load 38.05 → 30.00) and 973.05 s (139 passed, 10 failed; load 30.16 → 29.83). Failures included startup failures and tm and tmux timeouts; causes are unestablished. A passing current result is **Needs environment**. Line and branch coverage,
and systematic mutants caught, missed, timed out and unviable: unmeasured
before and after, **Needs environment**. The dead-code audits removed zero
lines and zero dependencies. Findings: P1 zero; P2 DSLP-6 and DSLP-8 fixed;
P3 DSLP-7, DSLP-9, DSLP-10, DSLP-26 and DSLP-27 remain open; P4 zero.
The broader P2 step tickets were completed as bounded audits. No controlled
runtime performance comparison was made; no hot rendering path changed in
those audits. Quiet-host test cost remains **Needs environment**. Escaped
regressions: CI flakes DSLP-26 and DSLP-27 remain unexplained; no runtime
defect is established for either.

**Per step:** Ground mapped boundaries and fixed socket-path and timing
fixtures; cold test took 447 s, warm tests 436–445 s, and lint 200–233 s on
the busy host. Diagnose reproduced descriptor exhaustion, unbounded mouse
input and sidecar symlink behavior; DSLP-8 was fixed. Types found no candidate
lint meeting the adoption rule. Tests inventoried boundary contracts and
added the protocol trailing-data assertion, with injected controls across
components. Dead code found no justified removal; five server warnings are
live benchmark consumers. Per-step wall time and CI minutes were not recorded,
except the Ground command observations above; current lint passed in 138.66 s on retry after a compiler interruption at 85.40 s. Other step costs cannot be reconstructed
from these records. See [command and protocol](docs/deslop-command-protocol.md),
[server](docs/deslop-server.md), and [terminal, input and harness](docs/deslop-terminal-input-harness.md).

**Method feedback:** State probes between repeat keys could exhaust the
reference's repeat window; send the input sequence before probing state. A
fault control also exposed an expiry barrier that hid an incorrect selection.
The compiler dead-code scan found no removable component code; unavailable
coverage and mutation tools prevented systematic survivor analysis. See
[repeat qualification](docs/deslop-repeat-flake.md).

**Shared-file gaps:** The deslop tool recipes require available
`cargo-llvm-cov`, `cargo-mutants`, `cargo-machete` and `cargo-udeps` to close
the recorded automated gaps. The `gha-test-workflow` and test-pyramid tools
were also unavailable. Qualifying and supplying those tools through the
shared environment remains open; no shared-file change is established here.

## Tickets

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

### DSLP-26 · P3 · Oversized control sequence recovery test flakes in CI

- **Area:** Needs environment · deslop · terminal
- **Where:** tests/terminal_bounds.rs:92
- **Problem:** CI has intermittently failed oversized_control_sequences_recover_at_the_next_boundary; the cause is not established. The test waits for a pane to emit and process over one megabyte before a deadline, so a timeout alone does not distinguish fixture timing from parser failure.
- **Recommendation:** Reproduce on the affected CI platform with pane output and daemon diagnostics on failure; identify the cause and fix the fixture or runtime with a regression test.
- **Fix with:** root-cause
- **Effort:** M

### DSLP-27 · P3 · External stop signal parity test flakes in CI

- **Area:** Needs environment · deslop · parity
- **Where:** tests/tmux_lifecycle_rows.rs:63
- **Problem:** CI has intermittently failed external_stop_signal_suspends_tm_and_is_ignored_by_tmux; the cause and failing observation are not established. Signal delivery and screen transitions are asynchronous.
- **Recommendation:** Reproduce on the affected CI platform with signal and terminal-state diagnostics; identify the cause and fix the fixture or runtime with a regression test.
- **Fix with:** root-cause
- **Effort:** M

