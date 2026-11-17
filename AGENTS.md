# tm

Unix terminal multiplexer. Build with `make build`, test with `make test`, and check with `cargo clippy --all-targets --all-features -- --deny warnings`.

- Interactive settings and bindings are compiled in `src/config.rs`; startup does not read tmux configuration files. See [architecture](docs/architecture.md).
- Integration tests use private `TM_SOCKET` paths. On a host running tmux, run every build, test, and manual `tm`/`tmux` through the isolation prefix in [testing](docs/testing.md#isolation).
- PTY tests synchronize on fixture markers or terminal state, not settle sleeps. See [PTY tests](docs/ptytest.md).
- Attach rendering waits for state revisions and suppresses unchanged frames. See [performance](docs/performance.md).
- The behavior contract against tmux 3.8 is in the [parity matrix](docs/parity.md).

Build and release targets are in [release](docs/release.md).

## Verification

- Use the isolation prefix in [testing](docs/testing.md#isolation) for every build, test, and manual invocation.
- `make test` installs tmux 3.8 and runs the default suite with four test threads; `make lint` checks all targets and features.
- Qualified cold and warm test costs and lint cost are pending; see [Ground](docs/deslop-ground.md).
- CLI commands, flags, and exit behavior: `tests/tmux_cli_flags.rs` and `tests/tmux_core_compat.rs`; the parity matrix records exercised coverage.
- Unix socket and wire format: `tests/socket_permissions.rs`, `tests/protocol_version.rs`, and the goldens in `src/protocol.rs`.
- Terminal and PTY behavior: `tests/attached_client_pty.rs`, `tests/terminal_corpus.rs`, and `tests/tmux_input.rs`.
- There is no public library API or persistent configuration file format. The daemon log and socket are local runtime artifacts.
- Rendering and copy-mode workload: `make perf`; budgets and fixtures are in [performance](docs/performance.md).
- Component boundaries and proposed bounded slices are in [Ground](docs/deslop-ground.md).
