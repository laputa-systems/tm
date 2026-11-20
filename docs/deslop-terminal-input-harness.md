# Terminal, input and harness audit

The scope is the files named by DSLP-17 through DSLP-25, with `server.rs`
restricted to rendering and input functions. No runtime or test source was
changed in this audit; therefore its consumer behavior and escape-hatch counts
are unchanged. Commands run on macOS arm64 with the private socket isolation
prefix from `docs/testing.md` and `CARGO_BUILD_JOBS=4`.

## Terminal and rendering: tests (DSLP-18)

The finite process-level inventory comprises four attached-client PTY tests,
four terminal-corpus tests and two terminal-bound tests. Unit tests in
`src/terminal.rs`, `src/cells.rs`, and `src/pty.rs` exercise parser state,
width mapping and bounded pane input. The process tier drives a real daemon.

| Contract | Guard and disposition |
| --- | --- |
| Attached screen, pane content, color, mouse and resize | Retain `tests/attached_client_pty.rs`: checks visible screen and client input. |
| VT100 rendering, terminal replies and title | Retain `tests/terminal_corpus.rs`: compares real sessions with the pinned reference. |
| Oversized repeat and control sequences | Retain `tests/terminal_bounds.rs`; the recovery test has an unexplained CI flake, filed as DSLP-26. |
| Chunked terminal output and UTF-8 | Retain `output_state_is_chunk_invariant_for_vt100_fixture` and the malformed-sequence variant in `src/terminal.rs`. |
| Cell widths and coordinate mapping | Retain `wide_combining_and_tab_widths_share_one_model` and `display_columns_map_to_logical_characters` in `src/cells.rs`. |

Coverage and systematic mutation are Needs environment: `cargo-llvm-cov` and
`cargo-mutants` are unavailable. `test-pyramid` is unavailable; the tiers above
are a manual inventory. No coverage or survivor count is asserted.

## Copy and input: tests (DSLP-21)

The finite named process inventory has 32 copy-mode cases, ten parity binding
cases and one compiled-settings case. Unit tests in `src/config.rs` cover
parsing and key bytes; input also uses attached-client process tests.

| Contract | Guard and disposition |
| --- | --- |
| Copy selection, buffers, scrolling and search | Retain `tests/tmux_copy_mode_compat.rs`: checks concrete selections, navigation and searches. |
| Bindings and attached input | Retain `tests/tmux_bindings.rs` and `attached_client_key_bytes_drive_copy_mode_actions_headlessly`. |
| Compiled settings and ignored external config | Retain `compiled_defaults_ignore_tm_config_and_home_tmux_conf`. |
| Unterminated SGR mouse prefix | Known memory growth remains filed as DSLP-9; no correction is claimed. |

Coverage, systematic mutation and test-pyramid are Needs environment for the
same unavailable tools. No survivor or before/after coverage count is claimed.

## Harness and parity: tests (DSLP-24)

The finite inventory contains 22 parity-contract tests, the oracle tests in
`tests/parity_oracle.rs`, and one socket-path scratch test, alongside the
differential suites in `make parity`. The latter
spawn the candidate and pinned reference and compare observations.

| Contract | Guard and disposition |
| --- | --- |
| Matrix citations and command, option and binding coverage | Retain `tests/parity_contract.rs`, including malformed-row negative cases. |
| Short private Unix socket path | Retain `descriptive_scenario_names_leave_room_for_private_unix_sockets`, which binds a real socket. |
| Process outcome, terminal state and differential comparison | Retain `tests/parity_oracle.rs` negative cases and the `make parity` suites. |
| External stop and resume | Retain `external_stop_signal_suspends_tm_and_is_ignored_by_tmux`; its unexplained CI flake is DSLP-27. |
| Sidecar symlinks | Known runtime issue remains filed as DSLP-10. |

Coverage, systematic mutation and test-pyramid are Needs environment for the
same unavailable tools. No survivor or before/after coverage count is claimed.
