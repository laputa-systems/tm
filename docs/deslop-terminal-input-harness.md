# Terminal, input and harness audit

The scope is the files named by DSLP-17 through DSLP-25, with `src/server.rs`
restricted to rendering and input functions. No runtime or test source was
changed in this audit; therefore its consumer behavior and escape-hatch counts
are unchanged. Commands run on macOS arm64 with the private socket isolation
prefix from `docs/testing.md` and `CARGO_BUILD_JOBS=4`.

## Terminal and rendering: types (DSLP-17)

Scope: `src/pty.rs` (477 lines), `src/terminal.rs` (928 lines), `src/cells.rs`
(65 lines), `src/capture.rs` (221 lines) — 1,691 lines total — plus rendering
functions in `src/server.rs` (`RenderBenchmark` at `1138..1260`, `render_pane`,
`render_session`, `render_session_with_clear`, `parser_for_pane`,
`copy_source_parser`, `replayed_copy_source_parser`, `render_copy_mode_position`,
`copy_mode_line_number_width_from_mode`, `strip_copy_mode_alignment`,
`render_pane_scrollbar`, `strip_status_styles`, `render_layout_separators`,
`append_color`, `append_cell_style`, `render_screen_delta`,
`render_status_prompt_cursor`, `render_pane_prompt_cursor`, `render_status_line`,
`append_status_base`, `render_status_span`, `render_status_styles`,
`capture_absolute_offset`, and `render_*_overlay`).

`cargo clippy --bin tm` with all nine deslop candidate warnings completed
successfully on macOS arm64. Counts below are non-test binary diagnostics in
the four modules plus the named rendering functions in `src/server.rs` (note:
`attach_client` at `server.rs:940, 1018` carries four additional `NonZeroU16`
`unwrap`s already counted under DSLP-11):

| Candidate | Count | Disposition |
| --- | ---: | --- |
| `unsafe_code` | 14 | Reject: 13 `unsafe` blocks and 1 `unsafe extern` block in `src/pty.rs` for `openpty`, `fork`, `from_raw_fd`, `tcgetpgrp`, `waitpid`, `kill`, `ioctl`, macOS `_NSGetEnviron` cleanup, `fcntl`, `close`, and `proc_pidinfo`; no safe replacement witness established. |
| `wildcard_enum_match_arm` | 0 | Reject: no witness. |
| `match_wildcard_for_single_variants` | 0 | Reject: no witness. |
| `cast_possible_truncation` | 1 | Reject: `src/pty.rs:394` casts fixed `size_of::<libc::proc_vnodepathinfo>()` (~2.3 KiB) to `c_int`; cannot truncate. |
| `cast_sign_loss` | 1 | Reject: `src/server.rs:11901` (`capture_absolute_offset`) casts `offset as usize` only in the `else` branch of `if offset < 0`. |
| `unwrap_used` | 8 | Reject: 2 in `RenderBenchmark::new` (`server.rs:1204`) and 6 in `parser_for_pane`, `copy_source_parser`, and `replayed_copy_source_parser` (`server.rs:10355, 10632, 10648`), all constructing `NonZeroU16` after `.max(1)` or fixture constants. |
| `expect_used` | 11 | Reject: 2 in `src/pty.rs:329, 333` for static C strings (`"TERM"`, `"screen-256color"`); 7 in `RenderBenchmark` (`server.rs:1155..1240`); 2 in `strip_status_styles` (`server.rs:11232`) and `render_status_styles` (`server.rs:11815`) inside `while index < value.len()`. |
| `redundant_clone` | 0 | Reject: no witness. |
| `needless_pass_by_value` | 2 | Reject: `src/capture.rs:183` (`write_color_transition` takes `old: Vec<u16>` and `new: Vec<u16>` by value); private helper with no defect witness or deleted escape hatch. |

No candidate was adopted. The deslop escape-hatch search matched 18 lines in
`src/pty.rs` (including 2 in `#[cfg(test)]`), 16 lines in `src/terminal.rs`
(all in `#[cfg(test)]`), 0 in `src/cells.rs`, 0 in `src/capture.rs`, and 15
non-test lines in the `src/server.rs` rendering functions (10 in
`RenderBenchmark` including 2 `#[allow(dead_code)]`, 3 in the replay parser
helpers, and 2 in status style helpers). Neither code nor counts changed, and
no public contract changed.

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

Coverage and systematic mutation are **Needs environment**: `cargo-llvm-cov`
and `cargo-mutants` are unavailable. `test-pyramid` is unavailable; the tiers
above are a manual inventory. No coverage or survivor count is asserted.

A hand-injected fault in `src/terminal.rs:247` suppressed DEC REP expansion
(`for _ in 0..0`). `cargo test --test terminal_bounds
huge_dec_repeat_is_clamped_and_does_not_stall -- --exact` failed (101) at
`tests/terminal_bounds.rs:88`. The original source was restored, its SHA-256
matched, and `cargo test --test terminal_bounds` passed (2 passed) alongside
`cargo test --test attached_client_pty` (4 passed).

## Terminal and rendering: dead code (DSLP-19)

`cargo clippy --bin tm -- -W dead_code -W unreachable_pub` exited 0 on macOS
arm64 with default binary features. It emitted zero `dead_code` or
`unreachable_pub` warnings in `src/pty.rs`, `src/terminal.rs`, `src/cells.rs`,
or `src/capture.rs`, and no warnings in `src/server.rs` rendering functions
outside the five `unreachable_pub` benchmark-fixture items (`RenderBenchmark`,
`new`, `enter_copy_mode_with_history`, `render_frame`, and `render_delta_frame`
at `src/server.rs:1138`), already allowlisted under DSLP-13 and imported by
`benches/tm_performance.rs`.

Entry points and registrations checked: `src/main.rs` and
`benches/tm_performance.rs` register `capture`, `cells`, `pty`, `terminal`, and
`server`. Other platforms were not exercised. `cargo-machete`, `cargo-udeps`,
and `debloat` are unavailable: dependency and size-tool findings are **Needs
environment**, not a package-wide clean result. No code or dependency was
removed, and no public-contract walk-back was proposed.

## Copy and input: types (DSLP-20)

Scope: `src/copy_mode.rs` (2,707 lines), `src/config.rs` (511 lines),
`src/options.rs` (505 lines) — 3,723 lines total — plus input functions in
`src/server.rs` (`binding_table`, `apply_compiled_interactive_config`,
`handle_client_input*`, `write_active*`, `take_expired_paste`,
`process_paste_input`, `escape_time_ms`, `flush_expired_*`, `arm_repeat`,
`feed_*`, `apply_mouse_event`, `copy_mode_scrollbar_*`,
`mouse_context_for_pane`, `execute_bound_commands*`, `expand_mouse_format`,
`execute_config_line`, `install_binding`, `apply_config_option`, `send_prefix`,
`write_pane`, `enter_copy_mode`, `execute_copy_action`,
`parse_sgr_mouse`..`apply_mouse_csi`, `parse_copy_*`, `run_copy_pipe`, and
`copy_input_actions`).

The same strict `cargo clippy --bin tm` scan produced these non-test binary
counts (shared `copy_source_parser` / `replayed_copy_source_parser` /
`parser_for_pane` `unwrap`s are already counted under DSLP-17 above):

| Candidate | Count | Disposition |
| --- | ---: | --- |
| `unsafe_code` | 0 | Reject: no witness. |
| `wildcard_enum_match_arm` | 7 | Reject: 6 in `src/copy_mode.rs:1055, 1409, 1454, 1458, 1526, 2296` (inner matches after exhaustive outer dispatch or explicit prompt-kind filters); 1 in `src/server.rs:5213` (`execute_bound_commands` delegates remaining `Request` arms to `execute_request`). |
| `match_wildcard_for_single_variants` | 0 | Reject: no witness. |
| `cast_possible_truncation` | 2 | Reject: `src/copy_mode.rs:2413` (`wrapped_rows` casts `row < page_rows`, bounded by `u16::MAX`); `src/server.rs:3637` (`apply_mouse_event` clamps to `0..=u16::MAX` first). |
| `cast_sign_loss` | 1 | Reject: `src/server.rs:3637`, the same clamped expression. |
| `unwrap_used` | 1 | Reject: `src/server.rs:3227` (`flush_expired_input`), `self.clients.get_mut(&client_id).unwrap()` immediately after `self.clients.get(&client_id)?` verified presence. |
| `expect_used` | 3 | Reject: `src/copy_mode.rs:2485` (`raw_terminal_lines` on non-empty `vec![String::new()]`); `src/server.rs:156` (`binding_table` compiled keys validated by `config.rs` tests); `src/server.rs:1972` (`apply_compiled_interactive_config` compiled options validated by `config.rs` tests). |
| `redundant_clone` | 1 | Reject: `src/server.rs:2873` (`candidate.clone()` in `handle_client_input_bytes`); no typing witness or deleted escape hatch. |
| `needless_pass_by_value` | 3 | Reject: `src/copy_mode.rs:444, 2265` and `src/server.rs:3588` (`apply_mouse_event` `MouseEvent`); no typing witness or deleted escape hatch. |

No candidate was adopted. The escape-hatch search matched 1 line in
`src/copy_mode.rs` (`2487`), 1 line in `src/config.rs` (`421`,
`#[allow(dead_code)]` on `#[cfg(test)] mod reference_profile`), 0 in
`src/options.rs`, and 3 non-test lines in `src/server.rs` input functions
(`157`, `1973`, `3227`). Neither code nor counts changed, and no public contract
changed.

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

Coverage, systematic mutation and test-pyramid are **Needs environment** for
the same unavailable tools. No survivor or before/after coverage count is
claimed.

A hand-injected fault in `src/copy_mode.rs:587` stopped `EndOfLine` one column
short (`saturating_sub(1)`). `cargo test --test tmux_copy_mode_compat
copy_mode_source_pane_drives_selection_in_a_different_target_pane_headlessly --
--exact` failed (101) with `"SOURCE LIN"` instead of `"SOURCE LINE"`. The
original source was restored, its SHA-256 matched, and `cargo test --test
tmux_copy_mode_compat` passed (32 passed). Note: inside this 66-byte
`.work/wt/terminal-copy-harness` checkout, `tests/tmux_config_compat.rs` fails
on macOS `SUN_LEN` because `.work/tmp/tm-config-<pid>-0/server.sock` is 104
bytes (> 103).

## Copy and input: dead code (DSLP-22)

`cargo clippy --bin tm -- -W dead_code -W unreachable_pub` emitted zero
`dead_code` and zero `unreachable_pub` warnings in `src/copy_mode.rs`,
`src/config.rs`, `src/options.rs`, or `src/server.rs` input functions.
`src/config.rs:421` carries `#[allow(dead_code)]` only on its `#[cfg(test)]`
inclusion of `tests/reference/profile.rs`. `src/main.rs` and
`benches/tm_performance.rs` register `copy_mode`, `config`, `options`, and
`server`. Other platforms were not exercised, and `cargo-machete`,
`cargo-udeps`, and `debloat` remain **Needs environment**. No code or
dependency was removed, and no public-contract walk-back was proposed.

## Harness and parity: types (DSLP-23)

Scope: `tests/support/*.rs` (9 Rust files, 2,227 lines, including the
`tm-pty-fixture` binary) and their integration-suite callers under `tests/*.rs`
(32 files).

`cargo clippy --tests --all-features` with all nine deslop candidate warnings
completed on macOS arm64. Counts below deduplicate identical source locations
across targets that share `tests/support/*.rs` via `#[path = ...]` (support /
caller / combined):

| Candidate | Support | Callers | Combined | Disposition |
| --- | ---: | ---: | ---: | --- |
| `unsafe_code` | 18 | 10 | 28 | Reject: 9 in `pty_fixture.rs` and 9 in `raw_pty.rs` for `openpty`/`termios`/`ioctl`/`pre_exec`/`kill`, plus 10 in signal/liveness/socket integration tests; test-only OS boundaries. |
| `wildcard_enum_match_arm` | 0 | 4 | 4 | Reject: `tmux_histories.rs` (3) and `tmux_lifecycle.rs` (1); no defect witness. |
| `match_wildcard_for_single_variants` | 0 | 0 | 0 | Reject: no witness. |
| `cast_possible_truncation` | 5 | 3 | 8 | Reject: `raw_pty.rs` (4 termios `tcflag_t` masks to `u32`), `history.rs` (1 `usize`/`u64` index), and `tmux_histories.rs` (3); no defect witness. |
| `cast_sign_loss` | 0 | 0 | 0 | Reject: no witness. |
| `unwrap_used` | 4 | 23 | 27 | Reject: `raw_pty.rs` (4 `NonZeroU16` constructions in `spawn`/`resize`) and test assertions; no witness. |
| `expect_used` | 27 | 311 | 338 | Reject: deliberate panic-on-failure test harness assertions. |
| `redundant_clone` | 0 | 3 | 3 | Reject: `parity_oracle.rs` (2) and `tmux_core_compat.rs` (1); test-only. |
| `needless_pass_by_value` | 0 | 1 | 1 | Reject: `tmux_cli_flags.rs` (1); test-only helper. |

No candidate was adopted. The escape-hatch search matched 47 lines in
`tests/support/*.rs` (`attached_pair.rs`: 2, `history.rs`: 1, `parity.rs`: 8,
`pty_fixture.rs`: 9, `pty_harness.rs`: 7, `raw_pty.rs`: 20) and 402 lines in
`tests/*.rs` (449 combined). Neither code nor counts changed, and no public
contract changed.

## Harness and parity: tests (DSLP-24)

The finite inventory contains 22 parity-contract tests, the oracle tests in
`tests/parity_oracle.rs`, and one socket-path scratch test, alongside the
differential suites in `make parity`. The latter spawn the candidate and pinned
reference and compare observations.

| Contract | Guard and disposition |
| --- | --- |
| Matrix citations and command, option and binding coverage | Retain `tests/parity_contract.rs`, including malformed-row negative cases. |
| Short private Unix socket path | Retain `descriptive_scenario_names_leave_room_for_private_unix_sockets`, which binds a real socket. |
| Process outcome, terminal state and differential comparison | Retain `tests/parity_oracle.rs` negative cases and the `make parity` suites. |
| External stop and resume | Retain `external_stop_signal_suspends_tm_and_is_ignored_by_tmux`; its unexplained CI flake is DSLP-27. |
| Sidecar symlinks | Known runtime issue remains filed as DSLP-10. |

Coverage, systematic mutation and test-pyramid are **Needs environment** for
the same unavailable tools. No survivor or before/after coverage count is
claimed.

A hand-injected fault in `tests/support/tm_bin.rs:6` pointed the default
binary path at `/nonexistent/tm`. `cargo test --test attached_client_pty
attached_client_uses_real_pty_and_semantic_screen_barriers -- --exact` failed
(101) with `No such file or directory`. The original source was restored, its
SHA-256 matched, and the restored test, `cargo test --test parity_contract`
(16 passed), and `cargo test --test core_smoke` (3 passed) all passed. Note:
inside this 66-byte `.work/wt/terminal-copy-harness` checkout, suites using
`tests/support/parity.rs` (`Scratch`) and
`socket_permissions::default_socket_lives_in_a_private_per_user_directory` fail
with macOS `SUN_LEN` because `$PWD/.work/tmp` (76 bytes) plus their nested
socket subpaths (29–30 bytes) exceeds the 103-byte limit.

## Harness and parity: dead code (DSLP-25)

`cargo clippy --tests --all-features -- -W dead_code -W unreachable_pub` and
`cargo clippy --bin tm-pty-fixture -- -W dead_code -W unreachable_pub` exited 0
and emitted zero `dead_code` warnings across `tests/support/*.rs`,
`tests/*.rs`, and `tm-pty-fixture`. All `unreachable_pub` warnings in
`tests/support/*.rs` arise because each integration test crate includes shared
helper files via private `#[path = "support/..."] mod` modules (several of
which carry `#[allow(dead_code)]` for helpers used by other test crates). All
public items in `tests/support/*.rs` have callers in `tests/` or within their
own support module, `Cargo.toml` declares `tm-pty-fixture` and the parity
targets, and `tests/parity_contract.rs` validates the harness structure.

Other platforms were not exercised, and `cargo-machete`, `cargo-udeps`, and
`debloat` remain **Needs environment**. No code or dependency was removed, and
no public-contract walk-back was proposed.
