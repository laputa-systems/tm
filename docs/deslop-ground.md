# Deslop Ground evidence

Ground measurements use base `6cf0b949c2593c0959268a3d1995b972b05f58b6`.

## Verification entry point

`make test` installs the pinned tmux 3.8 reference, then runs `cargo test --quiet -- --test-threads=4`. The explicit argument overrides `RUST_TEST_THREADS`. The parity-feature suites run separately under `make parity`. Pull requests run `make test` and `make parity` on Ubuntu arm64 and macOS (`ci.yml`) and `make lint` on Ubuntu arm64 (`lint.yml`); no workflow edit was needed. The `gha-test-workflow` check is not installed here (`Needs environment`), so the shape was read by hand. The [reference provenance](../tests/reference/PROVENANCE) names the upstream release and revision. No tracked vendor tree exists.

## Measurements

Base `6cf0b94`, macOS arm64, `CARGO_BUILD_JOBS=4`, every command through the isolation prefix and `cpu-guard --cores 4`, no `RUSTC_WRAPPER`. `perf-gate-ready` refused every check (load 15 to 37 on 18 cores, with other repositories' benchmarks and builds running), so each number is an observation, not a cost.

| Command | Result | Wall time |
| --- | --- | --- |
| `make test`, cold (`target/` removed) | 500 passed, 0 failed, 1 ignored, 33 binaries | 447 s |
| `make test`, warm, after a test-source rebuild | same | 436 s (load 31) |
| `make test`, warm, no source change | same | 445 s (load 37) |
| `make lint`, first clippy run in the target | clean | 233 s |
| `make lint`, repeat | clean | 200 s |

The wall time is dominated by the suites that start the reference tmux; the host load, not the commands, decides the spread. A quiet-host rerun is `Needs environment`.

## Reds at base

The first cold `make test` at base stopped in `attached_client_pty`. Three causes made base red; each is fixed in the Ground commit:

- `tests/attached_client_pty.rs` (4 tests) and `tests/socket_permissions.rs` (2 tests): `daemon: path must be shorter than SUN_LEN`. The fixtures named sockets with long labels, PID and a nanosecond timestamp, which overflowed the 103-byte limit under a checkout in `.work/wt/<name>` (the TMPDIR alone uses 67 bytes). Proof that they pre-exist: a detached worktree of `6cf0b94` fails `cargo test --test attached_client_pty --test socket_permissions` with the same error (4 and 2 failures). The earlier shortening in `0202a2d` left too little margin. The names are now `<n>.sock`, `p<pid>` and `d<pid>-<label>`.
- `server::tests::synchronized_panes_receive_active_input_headlessly` waited a fixed 150 ms for two shells to print. It failed once at base sources under load 14 (`left: "hello\n\n\n\n\n"`) and passed six isolated runs. It now polls both pane captures for their own markers with a ten-second deadline.
- `panes::panes_respawn_pane_flags_match_the_reference` compared `list-panes` immediately after respawning a pane running `true`; tmux had already recorded the exit and tm had not (`true 1` against `true 0`). It failed once at base sources under load 24. It now waits for both servers to report `pane_dead` before the comparison. A base worktree cannot reproduce it in isolation because its path overflows the socket limit.

The two flakes are host-dependent; their base failures are the logged runs, since the isolated base reruns pass or fail on the path limit instead. The Linux CI failure in `commands_from_a_killed_pane_fall_back_like_the_reference` is not reproduced here and stays ticketed.

## Component map

`src/main.rs` declares private modules, rather than separate crates. The following slices map later Types, Tests, and Dead code work. DSLP-3 through DSLP-5 begin with command and protocol; further slices require their own bounded tickets when those steps proceed. Server and topology must be split at function boundaries into units of at most about 20k lines, since `server.rs` alone has 20,457 lines. Each must retain its listed consumer contracts and run the named focused tests through the isolation prefix, then `make test` and `make lint`.

| Slice | Ownership and consumers | Focused evidence |
| --- | --- | --- |
| Command and protocol | `command.rs` parses CLI requests; `protocol.rs` encodes them for `client.rs` and `server.rs`. Preserve flags, errors, version and wire bytes. | `cargo test --bin tm command::tests`; `cargo test --bin tm protocol::tests`; `cargo test --test protocol_version` |
| Server and topology | `server.rs` owns sessions and dispatch; `model.rs` owns layouts. CLI clients and attached clients consume topology and lifecycle behavior. | `cargo test --test tmux_core_compat`; `cargo test --test tmux_targeting`; `cargo test --test tmux_lifecycle` |
| Terminal and rendering | `pty.rs`, `terminal.rs`, `cells.rs`, `capture.rs`, and rendering in `server.rs` serve pane processes and attached clients. Preserve terminal bytes, screen state, and render budgets. | `cargo test --test attached_client_pty`; `cargo test --test terminal_corpus`; `make perf` when the hot path changes |
| Copy and input | `copy_mode.rs`, `config.rs`, and `options.rs` feed server input and display. Preserve bindings, copy selections, buffers, and compiled settings. | `cargo test --test tmux_copy_mode_compat`; `cargo test --features parity --test tmux_bindings`; `cargo test --test tmux_config_compat` |
| Harness and parity | `tests/support` supplies private sockets, PTYs, and reference comparisons to the integration suites. Preserve cleanup and comparison semantics. | `cargo test --test parity_contract`; `cargo test --test parity_scratch`; `make parity` |

The rendering benchmark (`make perf`, `benches/tm_performance.rs`) covers full and incremental frames and copy mode. It is the declared workload for changes on those paths. The Ground commit changes only test code (two integration files, one unit test) and docs, so the shipped binary and its size are unaffected. It makes no performance claim.
