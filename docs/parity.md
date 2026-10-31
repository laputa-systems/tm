# Parity matrix

The contract is pinned to tmux 3.8, built by
[`tests/support/install-tmux-ref.sh`](../tests/support/install-tmux-ref.sh) from
the verified release tarball recorded in
[`tests/reference/PROVENANCE`](../tests/reference/PROVENANCE). A row may turn
`pass` only after a scenario runs against that reference and the linked test
proves both the selected behavior and the prohibited behavior when the option
is off.

Status values are `pass`, `fail`, and `blocked`. `fail` has a reproducing test;
`blocked` names the missing environment or prerequisite. The test column names
the proving slice and the test that must carry the evidence.

## Options

| Contract item | Selected semantics | Scenario | Implementation | Test | Status | Intentional difference |
| --- | --- | --- | --- | --- | --- | --- |
| `default-terminal` | Pane `TERM=tmux-256color` | R2 | `src/server.rs` pane spawn environment | R2 `tests/terminal_corpus.rs::pane_terminal_and_title_defaults` | fail | none |
| `terminal-features` | `tmux-256color:Sync:clipboard` selects sync and clipboard | C1 | `src/terminal.rs` capability profile | C1 `tests/tmux_terminal_modes.rs::terminal_features_select_capabilities` | fail | none |
| `set-clipboard` | `external`: tm copies use OSC 52, application OSC 52 ignored | R5 | `src/server.rs` clipboard path | R5 `tests/tmux_notifications.rs::external_clipboard_rejects_application_osc52` | fail | none |
| `extended-keys` | `on`: pane-requested CSI-u input | I3 | `src/server.rs` input decoder | I3 `tests/tmux_modes.rs::pane_extended_key_encoding` | fail | none |
| `extended-keys-format` | `csi-u` selected for extended panes | I3 | `src/server.rs` pane encoder | I3 `tests/tmux_modes.rs::pane_extended_key_encoding` | fail | none |
| `monitor-bell` | `on`: record a bell for the window list | R5 | `src/server.rs` bell state | R5 `tests/tmux_notifications.rs::audible_bell_and_monitor_state` | fail | none |
| `bell-action` | `any`: alert every attached client of the session | R5 | `src/server.rs` alert routing | R5 `tests/tmux_notifications.rs::audible_bell_and_monitor_state` | fail | none |
| `visual-bell` | `off`: audible BEL only | R5 | `src/server.rs` alert rendering | R5 `tests/tmux_notifications.rs::visual_bell_suppresses_audible_bell` | fail | none |
| `escape-time` | `0`: no partial prefix or escape held | I1 | `src/server.rs` key state machine | I1 `tests/tmux_input.rs::escape_time_zero_forwards_lone_escape` | fail | none |
| `focus-events` | `on`: forward focus only to a requesting pane | I3 | `src/server.rs` pane modes | I3 `tests/tmux_modes.rs::focus_forwarding_follows_pane_request` | fail | none |
| `prefix` | `C-a` | T1 | `src/config.rs` default prefix | T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | pass | none |
| `mouse` | `on`: route pointer events with pane coordinates | I3 | `src/server.rs` mouse routing | I3 `tests/tmux_modes.rs::mouse_routing_and_wheel_copy_mode` | fail | none |
| `history-limit` | `10000` retained lines | R1 | `src/terminal.rs` retained output | R1 `tests/tmux_history.rs::checkpoint_respects_history_limit` | fail | none |
| `mode-keys` | `emacs` copy-mode keys | R3 | `src/copy_mode.rs` key map | R3 `tests/tmux_copy_cells.rs::emacs_copy_actions` | fail | none |
| `base-index` | `1` for session window numbering | W3 | `src/model.rs` session options | W3 `tests/tmux_windows.rs::base_index_renumbering_and_window_lifecycle_match_reference` | pass | none |
| `renumber-windows` | `on` closes index gaps | W3 | `src/server.rs` window removal | W3 `tests/tmux_windows.rs::base_index_renumbering_and_window_lifecycle_match_reference` | pass | none |
| `automatic-rename` | `off` preserves the selected name | W3 | `src/server.rs` window naming | W3 `tests/tmux_windows.rs::base_index_renumbering_and_window_lifecycle_match_reference` | pass | none |
| `status-position` | `bottom` status row | R4 | `src/server.rs` status layout | R4 `tests/tmux_status.rs::status_position_and_width` | fail | none |
| `status-bg` | `black` background with reset between spans | R4 | `src/server.rs` status spans | R4 `tests/tmux_status.rs::status_colors_and_reset` | fail | none |
| `status-fg` | `white` foreground with reset between spans | R4 | `src/server.rs` status spans | R4 `tests/tmux_status.rs::status_colors_and_reset` | fail | none |
| `window-status-format` | inactive `#I:#W` with zoom marker | R4 | `src/server.rs` window list | R4 `tests/tmux_status.rs::window_list_truncation_keeps_active` | fail | none |
| `window-status-current-format` | active `#I:#W` in green with zoom marker | R4 | `src/server.rs` window list | R4 `tests/tmux_status.rs::window_list_truncation_keeps_active` | fail | none |
| `status-left` | session name with prefix highlight | R4 | `src/server.rs` status left | R4 `tests/tmux_status.rs::status_left_prefix_highlight` | fail | none |
| `status-left-length` | `32` display columns | R4 | `src/server.rs` status truncation | R4 `tests/tmux_status.rs::status_left_prefix_highlight` | fail | none |
| `status-right` | empty | R4 | `src/server.rs` status right | R4 `tests/tmux_status.rs::status_right_and_prompt_bounds` | fail | none |
| `monitor-activity` | `off` keeps activity out of the window list | W3 | `src/server.rs` window flags | W3 `tests/tmux_windows.rs::base_index_renumbering_and_window_lifecycle_match_reference` | pass | none |
| `set-titles` | `off` emits no outer title escape | R2 | `src/server.rs` client output | R2 `tests/terminal_corpus.rs::pane_terminal_and_title_defaults` | fail | none |

## Bindings

| Contract item | Selected semantics | Scenario | Implementation | Test | Status | Intentional difference |
| --- | --- | --- | --- | --- | --- | --- |
| `n` | prompt for a name, then new window | T1, W3 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone`; W3 `tests/tmux_windows.rs::prompt_values_are_inserted_verbatim` | pass | none |
| `p` | display pane numbers | T1 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::every_effective_binding_runs_in_client_context` | fail | none |
| `Enter` | display the compiled-profile message | T1 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::every_effective_binding_runs_in_client_context` | fail | none |
| `C-s` | send two literal prefix bytes | T1 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::every_effective_binding_runs_in_client_context` | fail | none |
| `/` | open a command prompt | T1 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::slash_prompt_resolves_omitted_relative_and_stable_targets` | pass | none |
| `r` | prompt to rename the window | T1, W3 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone`; W3 `tests/tmux_windows.rs::prompt_values_are_inserted_verbatim` | pass | none |
| `C-Left` | previous window, repeating | T1 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::two_clients_keep_binding_context_independent` | pass | none |
| `C-Right` | next window, repeating | T1 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::two_clients_keep_binding_context_independent` | pass | none |
| `Left` | swap and select the previous window, repeating | T1 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::client_bindings_use_the_attached_session_and_window` | pass | none |
| `Right` | swap and select the next window, repeating | T1 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::client_bindings_use_the_attached_session_and_window` | pass | none |
| `\` | split horizontal in the current path, even-horizontal | W1 | `src/config.rs` compiled binding | W1 `tests/tmux_layouts.rs::even_layouts_match_reference_for_two_to_five_panes`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | pass | none |
| `-` | split vertical in the current path, even-vertical | W1 | `src/config.rs` compiled binding | W1 `tests/tmux_layouts.rs::even_layouts_match_reference_for_two_to_five_panes`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | pass | none |
| `z` | toggle pane zoom | W1 | `src/server.rs` zoom state | W1 `tests/tmux_layouts.rs::zoom_and_unzoom_match_reference_geometry` | pass | none |
| `k` | kill the current pane, then display a message | T2 | `src/config.rs` compiled binding | T2 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | pass | none |
| `m` | prompt for a join target | T1 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::break_and_join_bindings_use_client_context` | pass | none |
| `C-n` | break pane to the next free index | T1 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::break_and_join_bindings_use_client_context` | pass | none |
| `%` | split horizontal | W1 | `src/config.rs` compiled binding | W1 `tests/tmux_layouts.rs::even_layouts_match_reference_for_two_to_five_panes`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | pass | none |
| `"` | split vertical | W1 | `src/config.rs` compiled binding | W1 `tests/tmux_layouts.rs::even_layouts_match_reference_for_two_to_five_panes`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | pass | none |
| `h` | select pane left | W1 | `src/config.rs` compiled binding | W1 `tests/tmux_layouts.rs::pane_navigation_and_last_targets_match_reference`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | pass | none |
| `j` | select pane down | W1 | `src/config.rs` compiled binding | W1 `tests/tmux_layouts.rs::pane_navigation_and_last_targets_match_reference`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | pass | none |
| `l` | select pane right | W1 | `src/config.rs` compiled binding | W1 `tests/tmux_layouts.rs::pane_navigation_and_last_targets_match_reference`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | pass | none |
| `x` | kill pane | T2 | `src/config.rs` compiled binding | T2 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | pass | none |
| `&` | kill window | T2 | `src/config.rs` compiled binding | T2 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | pass | none |
| `c` | new window | W3 | `src/config.rs` compiled binding | W3 `tests/tmux_windows.rs::base_index_renumbering_and_window_lifecycle_match_reference`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | pass | none |
| `b` | send the prefix to the pane | I1 | `src/config.rs` compiled binding | I1 `tests/tmux_input.rs::prefix_and_repeat_state` | fail | none |
| `[` | enter copy mode | R2, R3 | `src/config.rs` compiled binding | R3 `tests/tmux_copy_cells.rs::copy_mode_entry_and_selection` | fail | none |
| `d` | detach the client | L2 | `src/config.rs` compiled binding | L2 `tests/tmux_lifecycle.rs::idle_disconnect_and_single_attach` | fail | none |

## Interactive modes

| Contract item | Selected semantics | Scenario | Implementation | Test | Status | Intentional difference |
| --- | --- | --- | --- | --- | --- | --- |
| command prompt | tokenized template, `%%` replaced with raw input | T1 | `src/server.rs` prompt state | T1 `tests/tmux_targeting.rs::slash_prompt_resolves_omitted_relative_and_stable_targets` | pass | none |
| display-panes | pane number overlay for the configured interval | T1 | `src/server.rs` overlay | T1 `tests/tmux_targeting.rs::display_panes_and_choosers` | fail | none |
| choose-tree | choose a session or window from an attached client | T1 | `src/server.rs` tree mode | T1 `tests/tmux_targeting.rs::display_panes_and_choosers` | fail | none |
| choose-window | choose a window from an attached client | T1 | `src/server.rs` tree mode | T1 `tests/tmux_targeting.rs::display_panes_and_choosers` | fail | none |
| choose-client | choose an attached client | T1 | `src/server.rs` client mode | T1 `tests/tmux_targeting.rs::display_panes_and_choosers` | fail | none |
| choose-buffer | choose a paste buffer | T1 | `src/server.rs` buffer mode | T1 `tests/tmux_targeting.rs::display_panes_and_choosers` | fail | none |
| copy mode | emacs navigation, selection, and copy actions | R3 | `src/copy_mode.rs` | R3 `tests/tmux_copy_cells.rs::emacs_copy_actions` | fail | none |

## CLI and lifecycle

| Contract item | Selected semantics | Scenario | Implementation | Test | Status | Intentional difference |
| --- | --- | --- | --- | --- | --- | --- |
| formats | supported format names expand in client context | T1, W2 | `src/command.rs` format expansion | T1 `tests/tmux_targeting.rs::formats_use_client_context`; W2 `tests/tmux_cwd.rs::foreground_shell_cwd_matches_reference` | fail | tm prefers an OSC 7 `pane_current_path` report; tmux 3.8 uses only the foreground process cwd (recorded in `tests/tmux_cwd.rs`) |
| targeting | omitted and relative targets resolve in client context | T1 | `src/server.rs` target resolution | T1 `tests/tmux_targeting.rs::client_bindings_use_the_attached_session_and_window` | pass | none |
| buffers | set/show/paste/delete with the compiled buffer limit | R5 | `src/server.rs` buffers | R5 `tests/tmux_notifications.rs::buffer_and_clipboard_side_effects` | fail | none |
| capture | capture visible and history ranges | R1 | `src/server.rs` capture | R1 `tests/tmux_history.rs::capture_history_and_visible` | fail | none |
| copy mode | selection, copy, search, and cursor movement | R3 | `src/copy_mode.rs` | R3 `tests/tmux_copy_cells.rs::selection_copy_and_search` | fail | none |
| session lifecycle | create, rename, kill, attach, and detach | L2 | `src/server.rs` sessions | L2 `tests/tmux_lifecycle.rs::session_lifecycle_and_detach` | fail | none |
| window lifecycle | create, rename, select, swap, and kill | W3 | `src/server.rs` windows | W3 `tests/tmux_windows.rs::base_index_renumbering_and_window_lifecycle_match_reference` | pass | none |
| pane lifecycle | split, break, join, move, swap, and kill | T2 | `src/server.rs` panes | T2 `tests/tmux_atomicity.rs::invalid_join_and_swap_targets_error_without_mutation` | pass | none |
| attached terminal | alternate screen, mode ownership, and restoration | C1 | `src/client.rs` terminal | C1 `tests/tmux_terminal_modes.rs::mode_ownership_and_restoration` | fail | none |
| input | prefix, repeat, escape, paste, and extended keys | I1 | `src/server.rs` input state machine | I1 `tests/tmux_input.rs::input_state_machine` | fail | none |
| daemon lifecycle | startup lock, private socket, and diagnostics | L2 | `src/server.rs` daemon startup | L2 `tests/tmux_lifecycle.rs::startup_lock_and_diagnostics` | fail | none |
| signals | restore termios and screen on handled signals | L3 | `src/client.rs` signal handling | L3 `tests/tmux_signals.rs::signal_restoration` | fail | none |

## Baseline

Recorded 2026-10-30 from commit `05f4737` before the first product slice.

- `make test`: green, 20 s.
- `make lint`: green, 4 s.
- `make test-ci`: green, 29 s.
- `cargo test --test attached_client_pty`: green, 2 s.
- `make perf`: 28.7 µs (80×24, 1 pane), 32.3 µs (80×24, 2), 76.2 µs (120×40, 4), 327.1 µs (delta, 120×40, 4).
- Reference probe: `tmux 3.8` from the verified tarball, private `-S` socket, ambient multiplexer variables cleared.
