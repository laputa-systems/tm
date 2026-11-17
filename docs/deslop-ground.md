# Deslop Ground evidence

Ground remains open. This record describes base `7fd49943ef6d79af7c70390df80db35b555100bb` and the checks needed before closing DSLP-1.

## Verification entry point

`make test` installs the pinned tmux 3.8 reference, then runs `cargo test --quiet -- --test-threads=4`. The explicit argument overrides `RUST_TEST_THREADS`. The parity-feature suites run separately under `make parity`. CI runs both on Ubuntu arm64 and macOS. The [reference provenance](../tests/reference/PROVENANCE) names the upstream release and revision. No tracked vendor tree was found.

The [PR 37 checks](https://github.com/laputa-systems/tm/actions/runs/43977752926) passed on both platforms. The subsequent [base push CI](https://github.com/laputa-systems/tm/actions/runs/43983198066) failed on Ubuntu in `commands_from_a_killed_pane_fall_back_like_the_reference`: tm's in-pane output did not finish within six seconds. macOS passed. The log does not establish the cause. The test releases a command after killing its originating pane and compares the result with tmux. Do not infer a runtime defect from the timeout alone.

On macOS arm64, the isolated, CPU-capped baseline `make test` failed in `daemon_socket_is_private_and_missing_parent_is_private` with `path must be shorter than SUN_LEN`. Its PID, timestamp and nested socket produced a 107-byte pathname under this worktree's `TMPDIR`. Shortening the generated directory prefix from `tm-perm-` to `p` produces 100 bytes with the same PID and timestamp; uniqueness and permission assertions are unchanged. All five socket-permission tests then passed. The final isolated `make test` passed 498 cases across 33 test binaries, with one existing ignored helper; `make lint` passed. Both used `CARGO_BUILD_JOBS=1`, `RUST_TEST_THREADS=1`, `RUSTC_WRAPPER=kache`, and `cpu-guard --cores 1`; `make test` still specifies four test threads. The [PR 37 repeat qualification](deslop-repeat-flake.md) remains evidence for that unchanged repeat fixture.

Cold and warm test costs and lint cost are unqualified: the quiet-host check reported a hot external process. Repeat the measurements with successful `perf-gate-ready` checks before and immediately after each run. The Linux CI failure above remains unexplained; the local socket-path repair does not establish its cause.

## Component map

`src/main.rs` declares private modules, rather than separate crates. The following slices are proposed units for later Types, Tests, and Dead code work. Each must retain its listed consumer contracts and run the named focused tests through the isolation prefix, then `make test` and `make lint`.

| Slice | Ownership and consumers | Focused evidence |
| --- | --- | --- |
| Command and protocol | `command.rs` parses CLI requests; `protocol.rs` encodes them for `client.rs` and `server.rs`. Preserve flags, errors, version and wire bytes. | `cargo test --bin tm command::tests`; `cargo test --bin tm protocol::tests`; `cargo test --test protocol_version` |
| Server and topology | `server.rs` owns sessions and dispatch; `model.rs` owns layouts. CLI clients and attached clients consume topology and lifecycle behavior. | `cargo test --test tmux_core_compat`; `cargo test --test tmux_targeting`; `cargo test --test tmux_lifecycle` |
| Terminal and rendering | `pty.rs`, `terminal.rs`, `cells.rs`, `capture.rs`, and rendering in `server.rs` serve pane processes and attached clients. Preserve terminal bytes, screen state, and render budgets. | `cargo test --test attached_client_pty`; `cargo test --test terminal_corpus`; `make perf` when the hot path changes |
| Copy and input | `copy_mode.rs`, `config.rs`, and `options.rs` feed server input and display. Preserve bindings, copy selections, buffers, and compiled settings. | `cargo test --test tmux_copy_mode_compat`; `cargo test --features parity --test tmux_bindings`; `cargo test --test tmux_config_compat` |
| Harness and parity | `tests/support` supplies private sockets, PTYs, and reference comparisons to the integration suites. Preserve cleanup and comparison semantics. | `cargo test --test parity_contract`; `cargo test --test parity_scratch`; `make parity` |

The rendering benchmark covers full and incremental frames and copy mode. It is the declared workload for changes on those paths. This docs-only slice changes no executable or dependency, so runtime and binary size are unaffected. It makes no performance measurement claim.
