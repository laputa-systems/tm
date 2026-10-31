# tm

Unix terminal multiplexer. Build with `make build`, test with `make test`, and check with `cargo clippy --all-targets --all-features -- --deny warnings`.

- Interactive settings and bindings are compiled in `src/config.rs`; startup does not read tmux configuration files. See [architecture](docs/architecture.md).
- Integration tests use private `TM_SOCKET` paths. See [testing](docs/testing.md).
- PTY tests synchronize on fixture markers or terminal state, not settle sleeps. See [PTY tests](docs/ptytest.md).
- Attach rendering waits for state revisions and suppresses unchanged frames. See [performance](docs/performance.md).
- The behavior contract against tmux 3.8 is in the [parity matrix](docs/parity.md).

Build and release targets are in [release](docs/release.md).
