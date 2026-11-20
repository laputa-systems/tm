Prefix: DSLP
Next: 8

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

### DSLP-2 · P2 · Diagnose: ultrareview into this backlog

- **Area:** deslop · diagnose
- **Where:** whole repository
- **Problem:** No review has ranked this repository's risks for the deslop pass.
- **Recommendation:** Run the deslop Diagnose step. Accept: the diff touches only docs, comments, and this file, and every finding names its Fix with.
- **Fix with:** deslop
- **Effort:** M

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

### DSLP-7 · P3 · Linux CI failure in commands_from_a_killed_pane_fall_back_like_the_reference is unexplained

- **Area:** Needs environment · deslop · ground
- **Where:** tests/tmux_core_compat.rs; .github/workflows/ci.yml (ubuntu-26.04-arm)
- **Problem:** The base push CI run 43983198066 failed on Ubuntu in this test (tm's in-pane output did not finish within six seconds); macOS passed and the log does not give the cause. It does not reproduce on macOS arm64 at base 6cf0b94 (the test passed in every full run during Ground). No runtime defect is established.
- **Recommendation:** On a Linux arm64 host, run cargo test --test tmux_core_compat commands_from_a_killed_pane_fall_back_like_the_reference 50 times, capture the pane and daemon log on failure, and decide between a fixture wait and a runtime defect. Accept: root cause named, and a test or fix lands.
- **Fix with:** root-cause
- **Effort:** M

