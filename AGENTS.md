# tm

Unix terminal multiplexer. Build with `make build`, test with `make test`, and check with `cargo clippy --all-targets --all-features -- --deny warnings`.

- Interactive settings and bindings are compiled in `src/config.rs`; startup does not read tmux configuration files. See [architecture](docs/architecture.md).
- Integration tests use private `TM_SOCKET` paths. On a host running tmux, run every build, test, and manual `tm`/`tmux` through the isolation prefix in [testing](docs/testing.md#isolation).
- PTY tests synchronize on fixture markers or terminal state, not settle sleeps. See [PTY tests](docs/ptytest.md).
- Attach rendering waits for state revisions and suppresses unchanged frames. See [performance](docs/performance.md).
- The behavior contract against tmux 3.8 is in the [parity matrix](docs/parity.md).

Build and release targets are in [release](docs/release.md).

## Verification

- Commands: run each through the isolation prefix in [testing](docs/testing.md#isolation). `make test` installs tmux 3.8 and runs the default suite (500 tests, 33 binaries, one ignored) with four test threads; `make lint` is clippy with warnings denied; `make parity` runs the differential suites; PR CI runs `make test`, `make parity` and `make lint` (`.github/workflows/ci.yml`, `lint.yml`).
- Costs, Apple M-series, `CARGO_BUILD_JOBS=4`, host busy (load 15-37 on 18 cores), so observations only: cold `make test` 447 s, warm 436-445 s, `make lint` 200-233 s. On 2026-11-20 in a fresh worktree without `cpu-guard`, `make test` exited 2 after 289.41 s when a compiler was killed (SIGKILL); a retry exited 2 after 11.72 s with SIGKILL and SIGTERM. One-minute load was 45.21 at start, 54.81 at first exit, 54.51 at retry start and 57.88 at retry exit. `make lint` exited 2 after 85.40 s with SIGKILL and SIGTERM (load 57.97 → 47.49); its retry passed in 138.66 s (load 46.33 → 49.36). Quiet-host costs are `Needs environment`; evidence in [Ground](docs/deslop-ground.md).
- Test paths must stay short: a socket path is limited to about 103 bytes, and `.work/wt/<name>/.work/tmp` already uses 67.
- CLI flags, commands and exit codes: `tests/tmux_cli_flags.rs` (every `## CLI flags` row), `tests/tmux_core_compat.rs`; the parity matrix records exercised coverage.
- Wire protocol and version preamble: `tests/protocol_version.rs` and the golden and round-trip tests in `src/protocol.rs`.
- Daemon socket, private directory, mode and refusal of foreign files: `tests/socket_permissions.rs`. Sidecar `.lock` and `.log` symlink handling and descriptor exhaustion: none (filed in the backlog).
- Terminal bytes, attached client and PTY lifecycle: `tests/attached_client_pty.rs`, `tests/terminal_corpus.rs`, `tests/terminal_bounds.rs`, `tests/tmux_liveness.rs`, `tests/tmux_input.rs`.
- Compiled settings and bindings: `tests/tmux_config_compat.rs`, `tests/tmux_bindings.rs`. There is no public library API or persistent configuration file; the daemon log and socket are local runtime artifacts.
- Performance workload: `make perf` (`benches/tm_performance.rs`): full and incremental attach rendering, 16 ms and 32 KiB budgets in [performance](docs/performance.md). CI does not run it; run it for changes to rendering or the attach path.
- Vendored trees: none. The tmux 3.8 reference is built from `tests/reference/PROVENANCE` (upstream tarball and SHA-256).
- Components (about 32k lines in `src/`, 22k in `tests/`; the tickets split them): command and protocol (`command.rs`, `protocol.rs`, 4.1k); server and topology (`server.rs` 20.5k, `model.rs`); terminal and rendering (`pty.rs`, `terminal.rs`, `cells.rs`, `capture.rs`); copy and input (`copy_mode.rs`, `config.rs`, `options.rs`); harness and parity (`tests/support`). See [Ground](docs/deslop-ground.md).
