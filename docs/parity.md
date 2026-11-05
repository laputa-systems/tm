# Parity matrix

The contract is pinned to tmux 3.8, built by
[`tests/support/install-tmux-ref.sh`](../tests/support/install-tmux-ref.sh) from
the verified release tarball recorded in
[`tests/reference/PROVENANCE`](../tests/reference/PROVENANCE). A row may turn
`pass` only after a scenario runs against that reference and the linked test
proves both the selected behavior and the prohibited behavior when the option
is off.

Status values are `pass`, `fail`, and `blocked`. `fail` means the behavior is
not shown to match tmux 3.8: a cited test reproduces the difference, or no
cited test compares it with the reference. `blocked` names the missing
environment or prerequisite. The test column names the proving slice and the
tests that carry the evidence.

Evidence values say how the cited tests establish the row:

- `differential`: a cited test constructs `Mux::reference`, directly or through a
  helper, and runs the same scenario against tm and tmux 3.8.
- `recorded`: a cited test compares tm with a reference observation stored under
  `tests/reference/`.
- `internal`: the cited tests exercise tm only.

A row may be `pass` only with `differential` or `recorded` evidence. A
`differential` row needs at least one cited test that reaches the reference
and compares it with tm for the row's own behavior; a binding row is
`differential` only when the test presses the key in tm and compares the
outcome with the reference. `tests/parity_contract.rs` checks every citation: the file and
test function exist, `pass` is never `internal`, `differential` reaches
`Mux::reference`, and `recorded` reads a file under `tests/reference/`.

## Options

| Contract item | Selected semantics | Scenario | Implementation | Test | Evidence | Status | Intentional difference |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `default-terminal` | Pane `TERM=tmux-256color` | R2 | `src/server.rs` pane spawn environment | R2 `tests/terminal_corpus.rs::pane_terminal_and_set_titles_match_the_reference` | differential | pass | none |
| `terminal-features` | `tmux-256color:Sync:clipboard` selects sync and clipboard | C1 | `src/terminal.rs` capability profile | C1 `tests/tmux_terminal_modes.rs::terminal_features_sync_and_clipboard_select_for_the_term_like_the_reference` | differential | pass | none |
| `set-clipboard` | `external`: tm copies use OSC 52, application OSC 52 ignored | R5 | `src/server.rs` clipboard path | R5 `tests/tmux_notifications.rs::application_osc52_is_ignored_in_external_mode`; R5 `tests/tmux_notifications.rs::tm_copy_emits_osc52_once_per_capable_client` | differential | pass | none |
| `extended-keys` | `on`: pane-requested CSI-u input | I3 | `src/server.rs` input decoder | I3 `tests/tmux_modes.rs::extended_pane_receives_all_modified_printable_keys`; I3 `tests/tmux_modes.rs::extended_pane_receives_specials_and_special_modifiers` | internal | fail | none |
| `extended-keys-format` | `csi-u` selected for extended panes | I3 | `src/server.rs` pane encoder | I3 `tests/tmux_modes.rs::extended_pane_receives_all_modified_printable_keys`; I3 `tests/tmux_modes.rs::legacy_pane_receives_the_legacy_encoding` | internal | fail | none |
| `monitor-bell` | `on`: record a bell for the window list | R5 | `src/server.rs` bell state | R5 `tests/tmux_notifications.rs::audible_bell_reaches_each_attached_client_once` | differential | pass | none |
| `bell-action` | `any`: alert every attached client of the session | R5 | `src/server.rs` alert routing | R5 `tests/tmux_notifications.rs::audible_bell_reaches_each_attached_client_once` | differential | pass | none |
| `visual-bell` | `off`: audible BEL only | R5 | `src/server.rs` alert rendering | R5 `tests/tmux_notifications.rs::visual_bell_suppresses_the_outer_bell` | differential | pass | none |
| `escape-time` | `0`: no partial prefix or escape held | I1 | `src/server.rs` key state machine | I1 `tests/tmux_input.rs::lone_escape_after_prefix_is_forwarded_once` | internal | fail | none |
| `focus-events` | `on`: forward focus only to a requesting pane | I3 | `src/server.rs` pane modes | I3 `tests/tmux_modes.rs::focus_events_reach_only_a_requesting_pane` | internal | fail | none |
| `prefix` | `C-a` | T1 | `src/config.rs` default prefix | T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone`; I1 `tests/tmux_input.rs::literal_prefix_bindings_match_reference_bytes` | differential | pass | none |
| `mouse` | `on`: route pointer events with pane coordinates | I3 | `src/server.rs` mouse routing | I3 `tests/tmux_modes.rs::mouse_routes_only_to_the_pane_that_requested_it` | internal | fail | none |
| `history-limit` | `10000` retained lines | R1 | `src/terminal.rs` retained output | R1 `tests/tmux_history.rs::checkpoint_preserves_recent_history_and_later_output`; R1 `tests/tmux_history.rs::history_limit_capture_matches_the_reference` | differential | pass | none |
| `mode-keys` | `emacs` copy-mode keys | R3 | `src/copy_mode.rs` key map | R3 `tests/tmux_copy_cells.rs::emacs_copy_actions_move_search_and_copy_with_the_cell_model` | internal | fail | none |
| `base-index` | `1` for session window numbering | W3 | `src/model.rs` session options | W3 `tests/tmux_windows.rs::base_index_renumbering_and_window_lifecycle_match_reference` | differential | pass | none |
| `renumber-windows` | `on` closes index gaps | W3 | `src/server.rs` window removal | W3 `tests/tmux_windows.rs::base_index_renumbering_and_window_lifecycle_match_reference` | differential | pass | none |
| `automatic-rename` | `off` preserves the selected name | W3 | `src/server.rs` window naming | W3 `tests/tmux_windows.rs::base_index_renumbering_and_window_lifecycle_match_reference` | differential | pass | none |
| `status-position` | `bottom` status row | R4 | `src/server.rs` status layout | R4 `tests/tmux_status.rs::status_position_and_colors_bound_the_bottom_row` | internal | fail | none |
| `status-bg` | `black` background with reset between spans | R4 | `src/server.rs` status spans | R4 `tests/tmux_status.rs::status_position_and_colors_bound_the_bottom_row` | internal | fail | none |
| `status-fg` | `white` foreground with reset between spans | R4 | `src/server.rs` status spans | R4 `tests/tmux_status.rs::status_position_and_colors_bound_the_bottom_row` | internal | fail | none |
| `window-status-format` | inactive `#I:#W` with zoom marker | R4 | `src/server.rs` window list | R4 `tests/tmux_status.rs::window_list_truncation_keeps_first_middle_and_last_active_visible` | internal | fail | none |
| `window-status-current-format` | active `#I:#W` in green with zoom marker | R4 | `src/server.rs` window list | R4 `tests/tmux_status.rs::window_list_truncation_keeps_first_middle_and_last_active_visible` | internal | fail | none |
| `status-left` | session name with prefix highlight | R4 | `src/server.rs` status left | R4 `tests/tmux_status.rs::status_left_prefix_highlight_and_length_are_bounded` | internal | fail | none |
| `status-left-length` | `32` display columns | R4 | `src/server.rs` status truncation | R4 `tests/tmux_status.rs::status_left_prefix_highlight_and_length_are_bounded` | internal | fail | none |
| `status-right` | empty | R4 | `src/server.rs` status right | R4 `tests/tmux_status.rs::long_messages_prompts_and_unicode_names_never_wrap_the_status_row` | internal | fail | none |
| `monitor-activity` | `off` keeps activity out of the window list | W3 | `src/server.rs` window flags | W3 `tests/tmux_windows.rs::base_index_renumbering_and_window_lifecycle_match_reference` | differential | pass | none |
| `set-titles` | `off` emits no outer title escape | R2 | `src/server.rs` client output | R2 `tests/terminal_corpus.rs::pane_terminal_and_set_titles_match_the_reference` | differential | pass | none |

## Bindings

| Contract item | Selected semantics | Scenario | Implementation | Test | Evidence | Status | Intentional difference |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `n` | prompt for a name, then new window | T1, W3 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone`; W3 `tests/tmux_windows.rs::prompt_values_are_inserted_verbatim` | differential | pass | none |
| `p` | display pane numbers | T1 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | internal | fail | none |
| `Enter` | display the compiled-profile message | T1 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | internal | fail | none |
| `C-s` | send two literal prefix bytes | T1 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone`; I1 `tests/tmux_input.rs::literal_prefix_bindings_match_reference_bytes` | differential | pass | none |
| `/` | open a command prompt | T1 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::slash_prompt_resolves_omitted_relative_and_stable_targets` | internal | fail | none |
| `r` | prompt to rename the window | T1, W3 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone`; W3 `tests/tmux_windows.rs::prompt_values_are_inserted_verbatim` | differential | pass | none |
| `C-Left` | previous window, repeating | T1 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::two_clients_keep_binding_context_independent` | internal | fail | none |
| `C-Right` | next window, repeating | T1 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::two_clients_keep_binding_context_independent` | internal | fail | none |
| `Left` | swap and select the previous window, repeating | T1 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::client_bindings_use_the_attached_session_and_window` | internal | fail | none |
| `Right` | swap and select the next window, repeating | T1 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::client_bindings_use_the_attached_session_and_window` | internal | fail | none |
| `\` | split horizontal in the current path, even-horizontal | W1 | `src/config.rs` compiled binding | W1 `tests/tmux_layouts.rs::even_layouts_match_reference_for_two_to_five_panes`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | internal | fail | none |
| `-` | split vertical in the current path, even-vertical | W1 | `src/config.rs` compiled binding | W1 `tests/tmux_layouts.rs::even_layouts_match_reference_for_two_to_five_panes`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | internal | fail | none |
| `z` | toggle pane zoom | W1 | `src/server.rs` zoom state | W1 `tests/tmux_layouts.rs::zoom_and_unzoom_match_reference_geometry` | internal | fail | none |
| `k` | kill the current pane, then display a message | T2 | `src/config.rs` compiled binding | T2 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | internal | fail | none |
| `m` | prompt for a join target | T1 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::break_and_join_bindings_use_client_context` | internal | fail | none |
| `C-n` | break pane to the next free index | T1 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::break_and_join_bindings_use_client_context` | internal | fail | none |
| `%` | split horizontal | W1 | `src/config.rs` compiled binding | W1 `tests/tmux_layouts.rs::even_layouts_match_reference_for_two_to_five_panes`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | internal | fail | none |
| `"` | split vertical | W1 | `src/config.rs` compiled binding | W1 `tests/tmux_layouts.rs::even_layouts_match_reference_for_two_to_five_panes`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | internal | fail | none |
| `h` | select pane left | W1 | `src/config.rs` compiled binding | W1 `tests/tmux_layouts.rs::pane_navigation_and_last_targets_match_reference`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | internal | fail | none |
| `j` | select pane down | W1 | `src/config.rs` compiled binding | W1 `tests/tmux_layouts.rs::pane_navigation_and_last_targets_match_reference`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | internal | fail | none |
| `l` | select pane right | W1 | `src/config.rs` compiled binding | W1 `tests/tmux_layouts.rs::pane_navigation_and_last_targets_match_reference`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | internal | fail | none |
| `x` | kill pane | T2 | `src/config.rs` compiled binding | T2 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | internal | fail | none |
| `&` | kill window | T2 | `src/config.rs` compiled binding | T2 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | internal | fail | none |
| `c` | new window | W3 | `src/config.rs` compiled binding | W3 `tests/tmux_windows.rs::base_index_renumbering_and_window_lifecycle_match_reference`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | internal | fail | none |
| `b` | send the prefix to the pane | I1 | `src/config.rs` compiled binding | I1 `tests/tmux_input.rs::literal_prefix_bindings_match_reference_bytes` | differential | pass | none |
| `[` | enter copy mode | R2, R3 | `src/config.rs` compiled binding | R3 `tests/tmux_copy_cells.rs::emacs_copy_actions_move_search_and_copy_with_the_cell_model` | internal | fail | none |
| `d` | detach the client | L2 | `src/config.rs` compiled binding | L2 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone`; L2 `tests/tmux_lifecycle.rs::idle_disconnect_releases_the_client_registration` | internal | fail | none |

## Interactive modes

| Contract item | Selected semantics | Scenario | Implementation | Test | Evidence | Status | Intentional difference |
| --- | --- | --- | --- | --- | --- | --- | --- |
| command prompt | tokenized template, `%%` replaced with raw input | T1 | `src/server.rs` prompt state | T1 `tests/tmux_targeting.rs::slash_prompt_resolves_omitted_relative_and_stable_targets` | internal | fail | none |
| display-panes | pane number overlay for the configured interval | T1 | `src/server.rs` overlay | T1 `src/server.rs::tests::display_panes_mode_renders_labels_restores_zoom_and_selects_headlessly` | internal | fail | none |
| choose-tree | choose a session or window from an attached client | T1 | `src/server.rs` tree mode | T1 `src/server.rs::tests::choose_tree_filters_renders_and_switches_clients_headlessly` | internal | fail | none |
| choose-window | choose a window from an attached client | T1 | `src/server.rs` tree mode | T1 `src/server.rs::tests::choose_tree_filters_renders_and_switches_clients_headlessly` | internal | fail | none |
| choose-client | choose an attached client | T1 | `src/server.rs` client mode | T1 `src/server.rs::tests::choose_buffer_and_client_modes_match_headless_selection_contract` | internal | fail | none |
| choose-buffer | choose a paste buffer | T1 | `src/server.rs` buffer mode | T1 `src/server.rs::tests::choose_buffer_and_client_modes_match_headless_selection_contract` | internal | fail | none |
| copy mode | emacs navigation, selection, and copy actions | R3 | `src/copy_mode.rs` | R3 `tests/tmux_copy_cells.rs::emacs_copy_actions_move_search_and_copy_with_the_cell_model` | internal | fail | none |

## CLI and lifecycle

| Contract item | Selected semantics | Scenario | Implementation | Test | Evidence | Status | Intentional difference |
| --- | --- | --- | --- | --- | --- | --- | --- |
| formats | supported format names expand in client context | T1, W2 | `src/command.rs` format expansion | W2 `tests/tmux_cwd.rs::foreground_shell_cwd_matches_reference`; W2 `tests/tmux_cwd.rs::osc7_path_takes_precedence_over_the_process_cwd` | differential | pass | tm prefers an OSC 7 `pane_current_path` report; tmux 3.8 uses only the foreground process cwd (recorded in `tests/tmux_cwd.rs`) |
| targeting | omitted and relative targets resolve in client context | T1 | `src/server.rs` target resolution | T1 `tests/tmux_targeting.rs::client_bindings_use_the_attached_session_and_window` | internal | fail | none |
| buffers | set/show/paste/delete with the compiled buffer limit | R5 | `src/server.rs` buffers | R5 `tests/tmux_buffers_compat.rs::buffers_match_the_headless_tmux_command_contract` | internal | fail | none |
| capture | capture visible and history ranges | R1 | `src/server.rs` capture | R1 `tests/tmux_history.rs::checkpoint_preserves_recent_history_and_later_output`; R1 `tests/tmux_history.rs::history_limit_capture_matches_the_reference` | differential | pass | none |
| capture options | `capture-pane -J`, `-e`, and `-N` output matches tmux 3.8 | R1 | `src/capture.rs` | R1 `tests/tmux_history.rs::capture_keeps_trailing_blank_rows_like_the_reference`, `capture_options_join_wrapped_rows_and_keep_written_spaces_like_the_reference`, `capture_options_emit_attribute_transitions_like_the_reference`, `capture_options_treat_cleared_cells_like_the_reference` | differential | pass | none |
| capture of erased cells | `capture-pane -J` and `-N` keep cells that were written and then erased, as tmux's line storage does | R1 | `src/capture.rs` | R1 `tests/tmux_history.rs::capture_keeps_cells_erased_after_being_written_like_the_reference` | differential | fail | tmux remembers how far each line was ever written (`cellused`, `cellsize` in `grid.c`); vt100 keeps no such history, so tm rebuilds the extent from the cells that still hold content, and `-J` and `-N` omit the erased cells |
| `new-session -E` | `-E` only skips `update-environment`, which tm lacks, so the pane runs the given command or the default shell; `new-session` prints nothing unless `-P` or `-F` is given | C2 | `src/command.rs` `parse_new_session`; `src/client.rs` | C2 `tests/tmux_core_compat.rs::new_session_e_keeps_the_command_and_the_default_shell_like_the_reference` | differential | pass | none |
| `pipe-pane` | an exited helper is reaped within 1 s; `#{pane_pipe}` stays `1` until pane output fails to reach the helper, then `0` | L2 | `src/server.rs` `pipe_pane`, `PipeHelper` | L2 `tests/tmux_lifecycle.rs::exited_pipe_helper_is_reaped_and_pane_pipe_follows_the_reference` | differential | pass | none |
| `send-prefix` | CLI and binding send the configured `prefix` to the target pane (and its synchronized siblings); `-2` is rejected because tm has no `prefix2` | C1 | `src/server.rs` `send_prefix` | C1 `tests/tmux_input.rs::cli_send_prefix_writes_the_configured_prefix_like_the_reference`; C1 `tests/tmux_input.rs::cli_send_prefix_rejects_the_secondary_prefix_it_cannot_send` | differential | pass | tmux accepts `-2` and sends nothing while `prefix2` is `None`; tm rejects it |
| copy mode | selection, copy, search, and cursor movement | R3 | `src/copy_mode.rs` | R3 `tests/tmux_copy_cells.rs::wide_combining_and_styled_cells_copy_the_logical_text`; R3 `tests/tmux_copy_cells.rs::hyperlink_cells_copy_text_and_preserve_the_link_in_capture` | differential | pass | none |
| session lifecycle | create, rename, kill, attach, and detach | L2 | `src/server.rs` sessions | L2 `tests/tmux_lifecycle.rs::idle_disconnect_releases_the_client_registration` | internal | fail | none |
| window lifecycle | create, rename, select, swap, and kill | W3 | `src/server.rs` windows | W3 `tests/tmux_windows.rs::base_index_renumbering_and_window_lifecycle_match_reference` | differential | pass | none |
| pane lifecycle | split, break, join, move, swap, and kill | T2 | `src/server.rs` panes | T2 `tests/tmux_atomicity.rs::invalid_join_and_swap_targets_error_without_mutation` | internal | fail | none |
| attached terminal | alternate screen, mode ownership, and restoration | C1 | `src/client.rs` terminal | C1 `tests/tmux_terminal_modes.rs::attach_owns_modes_once_and_restores_them_on_detach`; C1 `tests/tmux_terminal_modes.rs::attach_sets_and_detach_resets_each_terminal_mode_like_the_reference` | differential | pass | none |
| input | prefix, repeat, escape, paste, and extended keys | I1 | `src/server.rs` input state machine | I1 `tests/tmux_input.rs::repeat_deadline_and_switching_follow_the_500ms_contract`; I2 `tests/tmux_paste.rs::fragmented_end_after_payload_finishes_the_paste` | internal | fail | none |
| daemon lifecycle | startup lock, private socket, and diagnostics | L2 | `src/server.rs` daemon startup | L2 `tests/tmux_lifecycle.rs::concurrent_startup_converges_and_replaces_a_stale_socket` | internal | fail | none |
| signals | restore termios and screen on handled signals | L3 | `src/client.rs` signal handling | L3 `tests/tmux_signals.rs::terminate_signal_restores_termios_and_screen` | internal | fail | CONT redraw is implemented but not yet independently replayed in the PTY test |

## CLI flags

Every command and flag that tm's parser accepts is `match`, `rejected`, or
`tm-only`. A `rejected` row exits nonzero with `tm: unsupported flag <flag> for
<command>`; tmux 3.8 accepts or defines the flag. A `tm-only` row is an
intentional difference. `tests/parity_contract.rs` validates the table.

| Command | Flag | Classification | tmux 3.8 meaning | Test | Evidence |
| --- | --- | --- | --- | --- | --- |
| `new-session` | `-A` | match | attach to the named session when it exists, otherwise create it; tm requires `-s` with it, and `-d` does not stop the attach | C3 `tests/tmux_cli_flags.rs::sessions_new_session_flags_match_the_reference`; C3 `tests/tmux_cli_flags.rs::sessions_new_session_attach_forms_match_the_reference` | differential |
| `new-session` | `-A (without `-s`)` | rejected | tmux attaches to the current session; tm has no equivalent resolution | C3 `tests/tmux_cli_flags.rs::sessions_new_session_rejects_flags_it_does_not_implement` | internal |
| `new-session` | `-c` | match | start directory of the first window | C3 `tests/tmux_cli_flags.rs::sessions_new_session_flags_match_the_reference` | differential |
| `new-session` | `-d` | match | create the session without attaching; without it the command needs a terminal and fails before creating anything otherwise | C3 `tests/tmux_cli_flags.rs::sessions_new_session_flags_match_the_reference`; C3 `tests/tmux_cli_flags.rs::sessions_new_session_attach_forms_match_the_reference` | differential |
| `new-session` | `-D` | rejected | with `-A`, detach other clients of the attached session | C3 `tests/tmux_cli_flags.rs::sessions_new_session_rejects_flags_it_does_not_implement` | internal |
| `new-session` | `-e` | rejected | set an environment variable for the session | C3 `tests/tmux_cli_flags.rs::sessions_new_session_rejects_flags_it_does_not_implement` | internal |
| `new-session` | `-E` | match | skip `update-environment`, which tm lacks, so it changes nothing | C3 `tests/tmux_cli_flags.rs::sessions_new_session_flags_match_the_reference` | differential |
| `new-session` | `-F` | match | format printed with `-P`; without `-P` nothing is printed | C3 `tests/tmux_cli_flags.rs::sessions_new_session_flags_match_the_reference` | differential |
| `new-session` | `-f` | rejected | client flags for the attached client | C3 `tests/tmux_cli_flags.rs::sessions_new_session_rejects_flags_it_does_not_implement` | internal |
| `new-session` | `-n` | match | name of the first window | C3 `tests/tmux_cli_flags.rs::sessions_new_session_flags_match_the_reference` | differential |
| `new-session` | `-P` | match | print information about the new session; the default format is `#{session_name}:` | C3 `tests/tmux_cli_flags.rs::sessions_new_session_flags_match_the_reference` | differential |
| `new-session` | `-s` | match | session name, including the attached `-sNAME` and combined `-ds NAME` forms | C3 `tests/tmux_cli_flags.rs::sessions_new_session_flags_match_the_reference` | differential |
| `new-session` | `-t` | match | group the new session with an existing session or its group | C3 `tests/tmux_cli_flags.rs::sessions_new_session_flags_match_the_reference` | differential |
| `new-session` | `-t (unknown name)` | tm-only | tmux creates a named session group; tm needs an existing session and fails with `can't find session` | C3 `tests/tmux_cli_flags.rs::sessions_new_session_group_target_must_name_a_session` | internal |
| `new-session` | `-X` | rejected | with `-A`, detach other clients and send them SIGHUP | C3 `tests/tmux_cli_flags.rs::sessions_new_session_rejects_flags_it_does_not_implement` | internal |
| `new-session` | `-x` | match | width of a detached session | C3 `tests/tmux_cli_flags.rs::sessions_new_session_flags_match_the_reference` | differential |
| `new-session` | `-y` | match | height of a detached session | C3 `tests/tmux_cli_flags.rs::sessions_new_session_flags_match_the_reference` | differential |
| `attach-session` | `-c` | rejected | working directory for the session | C3 `tests/tmux_cli_flags.rs::sessions_attach_session_rejects_flags_it_does_not_implement` | internal |
| `attach-session` | `-d` | rejected | detach other clients of the session | C3 `tests/tmux_cli_flags.rs::sessions_attach_session_rejects_flags_it_does_not_implement` | internal |
| `attach-session` | `-E` | match | skip `update-environment`, which tm lacks, so it changes nothing | C3 `tests/tmux_cli_flags.rs::sessions_attach_session_flags_match_the_reference` | differential |
| `attach-session` | `-f` | rejected | client flags | C3 `tests/tmux_cli_flags.rs::sessions_attach_session_rejects_flags_it_does_not_implement` | internal |
| `attach-session` | `-r` | rejected | read-only client | C3 `tests/tmux_cli_flags.rs::sessions_attach_session_rejects_flags_it_does_not_implement` | internal |
| `attach-session` | `-t` | match | session to attach to; a missing session fails with `can't find session`, and no terminal fails with `open terminal failed: not a terminal` after the lookup | C3 `tests/tmux_cli_flags.rs::sessions_attach_session_flags_match_the_reference` | differential |
| `attach-session` | `-x` | rejected | with `-d`, send SIGHUP to the other clients (tm formerly read it as a width) | C3 `tests/tmux_cli_flags.rs::sessions_attach_session_rejects_flags_it_does_not_implement` | internal |
| `attach-session` | `a` | tm-only | alias that tmux 3.8 lacks (ambiguous prefix there) | C3 `tests/tmux_cli_flags.rs::sessions_tm_only_aliases_run_the_canonical_command` | internal |
| `detach-client` | `-a` | match | detach every client except the target client | C3 `tests/tmux_cli_flags.rs::sessions_detach_client_flags_match_the_reference` | differential |
| `detach-client` | `-E` | rejected | run a shell command in the client instead of detaching | C3 `tests/tmux_cli_flags.rs::sessions_detach_client_rejects_flags_it_does_not_implement` | internal |
| `detach-client` | `-P` | rejected | send SIGHUP to the client's parent | C3 `tests/tmux_cli_flags.rs::sessions_detach_client_rejects_flags_it_does_not_implement` | internal |
| `detach-client` | `-s` | rejected | detach every client of a session | C3 `tests/tmux_cli_flags.rs::sessions_detach_client_rejects_flags_it_does_not_implement` | internal |
| `detach-client` | `-t` | match | target client; the default is the most recently active client | C3 `tests/tmux_cli_flags.rs::sessions_detach_client_flags_match_the_reference` | differential |
| `has-session` | `-t` | match | exit 0 when the session exists; the default is the current session | C3 `tests/tmux_cli_flags.rs::sessions_has_session_flags_match_the_reference` | differential |
| `has-session` | `(any other flag)` | rejected | tmux 3.8 defines only `-t` | C3 `tests/tmux_cli_flags.rs::sessions_has_session_rejects_flags_it_does_not_implement` | internal |
| `kill-session` | `-a` | match | kill every session except the target | C3 `tests/tmux_cli_flags.rs::sessions_kill_session_flags_match_the_reference` | differential |
| `kill-session` | `-C` | rejected | clear alert flags instead of killing | C3 `tests/tmux_cli_flags.rs::sessions_kill_session_rejects_flags_it_does_not_implement` | internal |
| `kill-session` | `-f` | rejected | filter for `-a` (tm formerly read it as force) | C3 `tests/tmux_cli_flags.rs::sessions_kill_session_rejects_flags_it_does_not_implement` | internal |
| `kill-session` | `-g` | rejected | kill every session in the target's group | C3 `tests/tmux_cli_flags.rs::sessions_kill_session_rejects_flags_it_does_not_implement` | internal |
| `kill-session` | `-t` | match | session to kill; a missing session fails with `can't find session` | C3 `tests/tmux_cli_flags.rs::sessions_kill_session_flags_match_the_reference` | differential |
| `kill-session` | `kill` | tm-only | alias that tmux 3.8 lacks (ambiguous prefix there) | C3 `tests/tmux_cli_flags.rs::sessions_tm_only_aliases_run_the_canonical_command` | internal |
| `kill-server` | `(any flag)` | rejected | tmux 3.8 defines no flags | C3 `tests/tmux_cli_flags.rs::sessions_kill_server_accepts_no_flags` | internal |
| `list-clients` | `-F` | match | output format; `client_name` is `client<N>` in tm and the tty path in tmux, so formats compare through the shared variables | C3 `tests/tmux_cli_flags.rs::sessions_list_clients_flags_match_the_reference` | differential |
| `list-clients` | `-F (default format)` | tm-only | tm prints `client<N>: <session>`; tmux prints the tty, session, size, and terminal | C3 `tests/tmux_cli_flags.rs::sessions_list_clients_names_and_default_format_are_tm_only` | internal |
| `list-clients` | `-f` | rejected | format filter | C3 `tests/tmux_cli_flags.rs::sessions_list_clients_rejects_flags_it_does_not_implement` | internal |
| `list-clients` | `-O` | rejected | sort order | C3 `tests/tmux_cli_flags.rs::sessions_list_clients_rejects_flags_it_does_not_implement` | internal |
| `list-clients` | `-r` | rejected | reverse the sort order | C3 `tests/tmux_cli_flags.rs::sessions_list_clients_rejects_flags_it_does_not_implement` | internal |
| `list-clients` | `-t` | rejected | list only the clients of a session | C3 `tests/tmux_cli_flags.rs::sessions_list_clients_rejects_flags_it_does_not_implement` | internal |
| `list-sessions` | `-a` | rejected | not a tmux 3.8 flag (tm formerly ignored it) | C3 `tests/tmux_cli_flags.rs::sessions_list_sessions_rejects_flags_it_does_not_implement` | internal |
| `list-sessions` | `-F` | match | output format, including `session_attached` for attached clients | C3 `tests/tmux_cli_flags.rs::sessions_list_sessions_flags_match_the_reference` | differential |
| `list-sessions` | `-F (default format)` | tm-only | tm prints `<name>: <n> windows`; tmux appends the creation time, group, and `(attached)` | C3 `tests/tmux_cli_flags.rs::sessions_list_sessions_default_format_is_a_tm_only_prefix` | internal |
| `list-sessions` | `-f` | rejected | format filter (tm formerly ignored it) | C3 `tests/tmux_cli_flags.rs::sessions_list_sessions_rejects_flags_it_does_not_implement` | internal |
| `list-sessions` | `-O` | rejected | sort order | C3 `tests/tmux_cli_flags.rs::sessions_list_sessions_rejects_flags_it_does_not_implement` | internal |
| `list-sessions` | `-r` | rejected | reverse the sort order | C3 `tests/tmux_cli_flags.rs::sessions_list_sessions_rejects_flags_it_does_not_implement` | internal |
| `rename-session` | `-t` | match | session to rename; a duplicate name or missing session fails like tmux | C3 `tests/tmux_cli_flags.rs::sessions_rename_session_flags_match_the_reference` | differential |
| `rename-session` | `(any other flag)` | rejected | tmux 3.8 defines only `-t` | C3 `tests/tmux_cli_flags.rs::sessions_rename_session_rejects_flags_it_does_not_implement` | internal |
| `switch-client` | `-c` | match | target client; the default is the most recently active client | C3 `tests/tmux_cli_flags.rs::sessions_switch_client_flags_match_the_reference` | differential |
| `switch-client` | `-E` | rejected | skip `update-environment` | C3 `tests/tmux_cli_flags.rs::sessions_switch_client_rejects_flags_and_targets_it_does_not_implement` | internal |
| `switch-client` | `-F` | rejected | format for the target | C3 `tests/tmux_cli_flags.rs::sessions_switch_client_rejects_flags_and_targets_it_does_not_implement` | internal |
| `switch-client` | `-l` | rejected | switch to the last session | C3 `tests/tmux_cli_flags.rs::sessions_switch_client_rejects_flags_and_targets_it_does_not_implement` | internal |
| `switch-client` | `-n` | rejected | switch to the next session | C3 `tests/tmux_cli_flags.rs::sessions_switch_client_rejects_flags_and_targets_it_does_not_implement` | internal |
| `switch-client` | `-O` | rejected | sort order for `-n` and `-p` | C3 `tests/tmux_cli_flags.rs::sessions_switch_client_rejects_flags_and_targets_it_does_not_implement` | internal |
| `switch-client` | `-p` | rejected | switch to the previous session | C3 `tests/tmux_cli_flags.rs::sessions_switch_client_rejects_flags_and_targets_it_does_not_implement` | internal |
| `switch-client` | `-r` | rejected | toggle the read-only client flag | C3 `tests/tmux_cli_flags.rs::sessions_switch_client_rejects_flags_and_targets_it_does_not_implement` | internal |
| `switch-client` | `-T` | rejected | set the client key table | C3 `tests/tmux_cli_flags.rs::sessions_switch_client_rejects_flags_and_targets_it_does_not_implement` | internal |
| `switch-client` | `-t` | match | target session by name, `=name`, or `$id` | C3 `tests/tmux_cli_flags.rs::sessions_switch_client_flags_match_the_reference` | differential |
| `switch-client` | `-t (window or pane target)` | rejected | `session:window`, `.pane`, `%pane`, and `=` select a window or pane as well | C3 `tests/tmux_cli_flags.rs::sessions_switch_client_rejects_flags_and_targets_it_does_not_implement` | internal |
| `switch-client` | `-Z` | rejected | keep the window zoomed | C3 `tests/tmux_cli_flags.rs::sessions_switch_client_rejects_flags_and_targets_it_does_not_implement` | internal |
| `refresh-client` | `(no flag)` | match | redraw the whole target client | C3 `tests/tmux_cli_flags.rs::sessions_refresh_client_flags_match_the_reference` | differential |
| `refresh-client` | `-A` | rejected | pane output state | C3 `tests/tmux_cli_flags.rs::sessions_refresh_client_rejects_flags_it_does_not_implement` | internal |
| `refresh-client` | `-B` | rejected | control-mode subscription | C3 `tests/tmux_cli_flags.rs::sessions_refresh_client_rejects_flags_it_does_not_implement` | internal |
| `refresh-client` | `-c` | rejected | reset the pan offset | C3 `tests/tmux_cli_flags.rs::sessions_refresh_client_rejects_flags_it_does_not_implement` | internal |
| `refresh-client` | `-C` | rejected | client size | C3 `tests/tmux_cli_flags.rs::sessions_refresh_client_rejects_flags_it_does_not_implement` | internal |
| `refresh-client` | `-D` | rejected | pan down | C3 `tests/tmux_cli_flags.rs::sessions_refresh_client_rejects_flags_it_does_not_implement` | internal |
| `refresh-client` | `-f` | rejected | client flags | C3 `tests/tmux_cli_flags.rs::sessions_refresh_client_rejects_flags_it_does_not_implement` | internal |
| `refresh-client` | `-F` | rejected | client flags (alias of `-f`) | C3 `tests/tmux_cli_flags.rs::sessions_refresh_client_rejects_flags_it_does_not_implement` | internal |
| `refresh-client` | `-l` | rejected | request the clipboard (tm formerly ignored `-S`, `-l`, and `-r`) | C3 `tests/tmux_cli_flags.rs::sessions_refresh_client_rejects_flags_it_does_not_implement` | internal |
| `refresh-client` | `-L` | rejected | pan left | C3 `tests/tmux_cli_flags.rs::sessions_refresh_client_rejects_flags_it_does_not_implement` | internal |
| `refresh-client` | `-r` | rejected | report a pane's terminal colors (tm formerly ignored `-S`, `-l`, and `-r`) | C3 `tests/tmux_cli_flags.rs::sessions_refresh_client_rejects_flags_it_does_not_implement` | internal |
| `refresh-client` | `-R` | rejected | pan right | C3 `tests/tmux_cli_flags.rs::sessions_refresh_client_rejects_flags_it_does_not_implement` | internal |
| `refresh-client` | `-S` | rejected | refresh only the status line (tm formerly ignored `-S`, `-l`, and `-r`) | C3 `tests/tmux_cli_flags.rs::sessions_refresh_client_rejects_flags_it_does_not_implement` | internal |
| `refresh-client` | `-U` | rejected | pan up | C3 `tests/tmux_cli_flags.rs::sessions_refresh_client_rejects_flags_it_does_not_implement` | internal |
| `refresh-client` | `-t` | match | target client; the default is the most recently active client | C3 `tests/tmux_cli_flags.rs::sessions_refresh_client_flags_match_the_reference` | differential |
| `refresh-client` | `refresh` | match | alias of `refresh-client` | C3 `tests/tmux_cli_flags.rs::sessions_tm_only_aliases_run_the_canonical_command` | differential |
| `refresh-client` | `refreshc` | tm-only | alias that tmux 3.8 lacks | C3 `tests/tmux_cli_flags.rs::sessions_tm_only_aliases_run_the_canonical_command` | internal |
| `new-window` | `-a` | match | insert after the target window, shifting only the contiguous run of windows after it | C4 `tests/tmux_cli_flags.rs::windows_new_window_flags_match_the_reference` | differential |
| `new-window` | `-b` | match | insert before the target window; wins over `-a` | C4 `tests/tmux_cli_flags.rs::windows_new_window_flags_match_the_reference` | differential |
| `new-window` | `-c` | match | start directory of the new pane | C4 `tests/tmux_cli_flags.rs::windows_new_window_flags_match_the_reference` | differential |
| `new-window` | `-d` | match | do not make the new window current | C4 `tests/tmux_cli_flags.rs::windows_new_window_flags_match_the_reference` | differential |
| `new-window` | `-e` | rejected | set an environment variable in the new pane | C4 `tests/tmux_cli_flags.rs::windows_new_window_rejects_flags_it_does_not_implement` | internal |
| `new-window` | `-E` | match | empty pane with no process; a command is an error | C4 `tests/tmux_cli_flags.rs::windows_new_window_flags_match_the_reference` | differential |
| `new-window` | `-F` | match | format for `-P`; without `-P` nothing is printed | C4 `tests/tmux_cli_flags.rs::windows_new_window_flags_match_the_reference` | differential |
| `new-window` | `-I` | rejected | tm-only index flag; tmux takes the index from `-t` | C4 `tests/tmux_cli_flags.rs::windows_new_window_rejects_flags_it_does_not_implement` | internal |
| `new-window` | `-k` | match | destroy an existing window at the target index | C4 `tests/tmux_cli_flags.rs::windows_new_window_flags_match_the_reference` | differential |
| `new-window` | `-n` | match | window name | C4 `tests/tmux_cli_flags.rs::windows_new_window_flags_match_the_reference` | differential |
| `new-window` | `-P` | match | print the new window; default format `#{session_name}:#{window_index}.#{pane_index}` | C4 `tests/tmux_cli_flags.rs::windows_new_window_flags_match_the_reference` | differential |
| `new-window` | `-S` | match | select the window at the target index or with the `-n` name instead of creating one | C4 `tests/tmux_cli_flags.rs::windows_new_window_flags_match_the_reference` | differential |
| `new-window` | `-t` | match | target index: number, name, `+n`, `-n`, `!`, `^`, `$`, or empty for the next free index | C4 `tests/tmux_cli_flags.rs::windows_new_window_flags_match_the_reference` | differential |
| `new-window` | `shell-command` | match | one word runs through the shell, several words run directly; the window is named after the command's basename | C4 `tests/tmux_cli_flags.rs::windows_new_window_flags_match_the_reference` | differential |
| `kill-window` | `-a` | match | kill every window except the target, then renumber | C4 `tests/tmux_cli_flags.rs::windows_kill_window_flags_match_the_reference` | differential |
| `kill-window` | `-f` | rejected | filter format for `-a` | C4 `tests/tmux_cli_flags.rs::windows_kill_window_rejects_flags_it_does_not_implement` | internal |
| `kill-window` | `-t` | match | target window; killing the current window selects the last window, else the previous one | C4 `tests/tmux_cli_flags.rs::windows_kill_window_flags_match_the_reference` | differential |
| `select-window` | `-l` | match | select the last window of the target session | C4 `tests/tmux_cli_flags.rs::windows_select_window_flags_match_the_reference` | differential |
| `select-window` | `-n` | match | select the next window of the target session | C4 `tests/tmux_cli_flags.rs::windows_select_window_flags_match_the_reference` | differential |
| `select-window` | `-p` | match | select the previous window of the target session | C4 `tests/tmux_cli_flags.rs::windows_select_window_flags_match_the_reference` | differential |
| `select-window` | `-T` | rejected | switch to the last window when the target is already current | C4 `tests/tmux_cli_flags.rs::windows_select_window_rejects_flags_it_does_not_implement` | internal |
| `select-window` | `-t` | match | target window | C4 `tests/tmux_cli_flags.rs::windows_select_window_flags_match_the_reference` | differential |
| `next-window` | `-a` | rejected | move to the next window with an alert | C4 `tests/tmux_cli_flags.rs::windows_next_previous_and_last_window_reject_flags_they_do_not_implement` | internal |
| `next-window` | `-t` | match | target session | C4 `tests/tmux_cli_flags.rs::windows_next_previous_and_last_window_flags_match_the_reference` | differential |
| `previous-window` | `-a` | rejected | move to the previous window with an alert | C4 `tests/tmux_cli_flags.rs::windows_next_previous_and_last_window_reject_flags_they_do_not_implement` | internal |
| `previous-window` | `-t` | match | target session | C4 `tests/tmux_cli_flags.rs::windows_next_previous_and_last_window_flags_match_the_reference` | differential |
| `last-window` | `-t` | match | target session | C4 `tests/tmux_cli_flags.rs::windows_next_previous_and_last_window_flags_match_the_reference` | differential |
| `link-window` | `(any flag)` | rejected | link a window into another session; tm has no shared windows, and its linked copies diverged from the original | C4 `tests/tmux_cli_flags.rs::windows_link_window_is_rejected` | internal |
| `unlink-window` | `-k` | match | unlink even when the window is linked only here | C4 `tests/tmux_cli_flags.rs::windows_unlink_window_flags_match_the_reference` | differential |
| `unlink-window` | `-t` | match | target window | C4 `tests/tmux_cli_flags.rs::windows_unlink_window_flags_match_the_reference` | differential |
| `move-window` | `-a` | match | move after the target window, shifting only the contiguous run of windows after it | C4 `tests/tmux_cli_flags.rs::windows_move_window_flags_match_the_reference` | differential |
| `move-window` | `-b` | rejected | move before the target window | C4 `tests/tmux_cli_flags.rs::windows_move_window_rejects_flags_it_does_not_implement` | internal |
| `move-window` | `-d` | match | do not select the moved window | C4 `tests/tmux_cli_flags.rs::windows_move_window_flags_match_the_reference` | differential |
| `move-window` | `-k` | match | destroy an existing window at the target index | C4 `tests/tmux_cli_flags.rs::windows_move_window_flags_match_the_reference` | differential |
| `move-window` | `-r` | match | renumber the windows of the target session | C4 `tests/tmux_cli_flags.rs::windows_move_window_flags_match_the_reference` | differential |
| `move-window` | `-s` | match | source window; the default is the current window | C4 `tests/tmux_cli_flags.rs::windows_move_window_flags_match_the_reference` | differential |
| `move-window` | `-t` | match | target index or session | C4 `tests/tmux_cli_flags.rs::windows_move_window_flags_match_the_reference` | differential |
| `swap-window` | `-d` | match | select the swapped windows' indexes in both sessions | C4 `tests/tmux_cli_flags.rs::windows_swap_window_flags_match_the_reference` | differential |
| `swap-window` | `-r` | rejected | accepted and ignored before C4; tmux has no such flag | C4 `tests/tmux_cli_flags.rs::windows_swap_window_rejects_flags_it_does_not_implement` | internal |
| `swap-window` | `-s` | match | source window; the default is the current window | C4 `tests/tmux_cli_flags.rs::windows_swap_window_flags_match_the_reference` | differential |
| `swap-window` | `-t` | match | target window; windows may be in different sessions | C4 `tests/tmux_cli_flags.rs::windows_swap_window_flags_match_the_reference` | differential |
| `rotate-window` | `-D` | match | rotate panes down | C4 `tests/tmux_cli_flags.rs::windows_rotate_window_flags_match_the_reference` | differential |
| `rotate-window` | `-t` | match | target window | C4 `tests/tmux_cli_flags.rs::windows_rotate_window_flags_match_the_reference` | differential |
| `rotate-window` | `-U` | match | rotate panes up | C4 `tests/tmux_cli_flags.rs::windows_rotate_window_flags_match_the_reference` | differential |
| `rotate-window` | `-Z` | rejected | keep the window zoomed | C4 `tests/tmux_cli_flags.rs::windows_rotate_window_rejects_flags_it_does_not_implement` | internal |
| `rename-window` | `-t` | match | target window | C4 `tests/tmux_cli_flags.rs::windows_rename_window_flags_match_the_reference` | differential |
| `rename-window` | `new-name` | match | exactly one name argument | C4 `tests/tmux_cli_flags.rs::windows_rename_window_flags_match_the_reference` | differential |
| `respawn-window` | `-c` | match | start directory | C4 `tests/tmux_cli_flags.rs::windows_respawn_window_flags_match_the_reference` | differential |
| `respawn-window` | `-d` | rejected | accepted and ignored before C4; tmux has no such flag | C4 `tests/tmux_cli_flags.rs::windows_respawn_window_rejects_flags_it_does_not_implement` | internal |
| `respawn-window` | `-e` | rejected | set an environment variable in the pane | C4 `tests/tmux_cli_flags.rs::windows_respawn_window_rejects_flags_it_does_not_implement` | internal |
| `respawn-window` | `-E` | rejected | respawn into an empty pane; tmux then drops the window | C4 `tests/tmux_cli_flags.rs::windows_respawn_window_rejects_flags_it_does_not_implement` | internal |
| `respawn-window` | `-k` | match | respawn a window whose panes still run; the window becomes current | C4 `tests/tmux_cli_flags.rs::windows_respawn_window_flags_match_the_reference` | differential |
| `respawn-window` | `-t` | match | target window | C4 `tests/tmux_cli_flags.rs::windows_respawn_window_flags_match_the_reference` | differential |
| `respawn-window` | `shell-command` | match | command for the respawned pane | C4 `tests/tmux_cli_flags.rs::windows_respawn_window_flags_match_the_reference` | differential |
| `select-layout` | `-E` | rejected | spread the panes evenly | C4 `tests/tmux_cli_flags.rs::windows_select_layout_rejects_flags_it_does_not_implement` | internal |
| `select-layout` | `-n` | rejected | next preset layout | C4 `tests/tmux_cli_flags.rs::windows_select_layout_rejects_flags_it_does_not_implement` | internal |
| `select-layout` | `-o` | rejected | undo the last layout change | C4 `tests/tmux_cli_flags.rs::windows_select_layout_rejects_flags_it_does_not_implement` | internal |
| `select-layout` | `-p` | rejected | previous preset layout | C4 `tests/tmux_cli_flags.rs::windows_select_layout_rejects_flags_it_does_not_implement` | internal |
| `select-layout` | `-t` | match | target pane; the layout applies to its window | C4 `tests/tmux_cli_flags.rs::windows_select_layout_flags_match_the_reference` | differential |
| `select-layout` | `even-horizontal` | match | panes side by side with even widths | C4 `tests/tmux_cli_flags.rs::windows_select_layout_flags_match_the_reference` | differential |
| `select-layout` | `even-vertical` | match | panes stacked with even heights | C4 `tests/tmux_cli_flags.rs::windows_select_layout_flags_match_the_reference` | differential |
| `select-layout` | `(other layouts)` | rejected | main-horizontal, main-vertical, tiled, mirrored, and custom layout strings | C4 `tests/tmux_cli_flags.rs::windows_select_layout_rejects_layouts_it_does_not_implement`; C4 `tests/tmux_cli_flags.rs::windows_select_layout_rejects_flags_it_does_not_implement` | internal |
| `select-layout` | `(no layout)` | rejected | re-apply the last layout | C4 `tests/tmux_cli_flags.rs::windows_select_layout_rejects_layouts_it_does_not_implement`; C4 `tests/tmux_cli_flags.rs::windows_select_layout_rejects_flags_it_does_not_implement` | internal |

## Baseline

Recorded 2026-10-30 from commit `05f4737` before the first product slice.

- `make test`: green, 20 s.
- `make lint`: green, 4 s.
- `make test-ci`: green, 29 s.
- `cargo test --test attached_client_pty`: green, 2 s.
- `make perf`: 28.7 µs (80×24, 1 pane), 32.3 µs (80×24, 2), 76.2 µs (120×40, 4), 327.1 µs (delta, 120×40, 4).
- Reference probe: `tmux 3.8` from the verified tarball, private `-S` socket, ambient multiplexer variables cleared.
