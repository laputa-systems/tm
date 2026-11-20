# Server audit

Scope: `src/server.rs` through `window_index_for_creation` (10,068 lines),
then `window_index` through the end (10,474 lines), at function boundaries;
`src/model.rs` (1,206 lines) belongs to both slices. Request dispatch
`execute_request_inner` belongs to the second slice. The binary registers both
modules in `src/main.rs`; the performance benchmark also imports them.

## Lifecycle slice: types (DSLP-11)

`cargo clippy --bin tm` with all nine deslop candidate warnings completed
successfully on macOS arm64. Counts below are diagnostic instances in this
slice plus `model.rs`, excluding test-only code. The latter is counted again
for the second slice.

| Candidate | Count | Disposition |
| --- | ---: | --- |
| `unsafe_code` | 19 | Reject: libc calls for sockets, process and signal handling; no safe replacement witness established. |
| `wildcard_enum_match_arm` | 9 | Reject: no defect witness; forwarding remaining requests is intentional at `server.rs:698`. |
| `match_wildcard_for_single_variants` | 0 | Reject: no witness. |
| `cast_possible_truncation` | 8 | Reject: no demonstrated defect or deleted trust; for example `model.rs:209` clamps to `u16` first. |
| `cast_sign_loss` | 4 | Reject: no demonstrated defect or deleted trust; the same clamp demonstrates a false positive. |
| `unwrap_used` | 23 | Reject: no demonstrated defect or deletion; dimensions are clamped before `NonZeroU16` construction at `server.rs:940`. |
| `expect_used` | 20 | Reject: no demonstrated defect or deletion; includes benchmark setup and compiled bindings. |
| `redundant_clone` | 4 | Reject: no typing witness or deleted escape hatch. |
| `needless_pass_by_value` | 33 | Reject: no typing witness or deleted escape hatch. |

No candidate was adopted. The escape-hatch search specified in the deslop
tools reference found 49 matching lines in the first server slice and 9 in
model, including tests and textual `unsafe` matches. Neither code nor the
count changed. The scan is evidence of candidates, not proof that each cast
or assertion is safe. No public contract changed.

## Lifecycle slice: tests (DSLP-12)

The process tier uses private sockets and a real daemon. The unit tier includes
six model tests for geometry and pane state (`src/model.rs:1083`). The scoped
process suites cover the following contracts:

| Contract | Guard | Disposition |
| --- | --- | --- |
| Concurrent startup, stale socket, socket privacy | `concurrent_startup_converges_and_replaces_a_stale_socket`, `socket_and_diagnostics_keep_private_permissions`, `tests/socket_permissions.rs` | Retain: assert both sessions and actual file modes/refusals. Sidecar symlink and descriptor-exhaustion gaps are already recorded in the project backlog. |
| Shutdown, restart, last session and in-flight requests | `kill_server_restart_matches_the_reference`, `removing_the_last_session_ends_the_server_like_the_reference`, `request_sent_after_kill_server_replied_gets_an_explicit_error` | Retain: check completion and explicit errors, including reference parity. |
| Attach/disconnect and child cleanup | `idle_disconnect_releases_the_client_registration`, `attach_racing_kill_server_gets_an_error_or_ends_cleanly`, `exited_pipe_helper_is_reaped_and_pane_pipe_follows_the_reference` | Retain: observe client registration, terminal exit and process reaping. |
| Session creation, identity, duplicate and missing targets | `session_ops_port_the_tmux_session_lifecycle` | Retain: checks printed ID, duplicate refusal, rename and removal. |
| Layout and pane topology | `split_window_honors_cell_and_percent_sizes_headlessly`, `rotate_and_swap_windows_preserve_headless_layout_identity`, model layout tests | Retain: check geometry and observable pane identity. |
| Command failure and exit mapping | `commands_without_a_server_fail_like_the_reference`, `tests/tmux_cli_flags.rs`, `session_ops_port_the_tmux_session_lifecycle` | Retain reference output/status comparisons and success/error controls. Dispatch itself is in the second slice. |

The `test-pyramid`, `cargo-llvm-cov` and `cargo-mutants` commands are not
installed. Automated coverage, before/after mutation counts and systematic
survivor dispositions are **Needs environment**, not a clean result. No
suite or test was removed. Existing test costs and full-suite baseline are
recorded in `docs/deslop-ground.md`; since no tests or runtime code changed,
there is no added suite cost to compare. A measured warm before/after timing
on a quiet host remains unverified.

A hand-injected fault changed the response mapping of `Err` to success.
`cargo test --test tmux_core_compat session_ops_port_the_tmux_session_lifecycle
-- --exact` failed (101): a duplicate `new-session` unexpectedly succeeded.
The original source was restored and its SHA-256 matched. The unchanged
focused suites passed 38 core, 10 lifecycle and 8 targeting tests; `make test`
passed 504 and ignored one across 33 result summaries, and `make lint` passed.
