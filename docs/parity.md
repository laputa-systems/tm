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
| `extended-keys` | `on`: pane-requested CSI-u input | I3 | `src/server.rs` input decoder | I3 `tests/tmux_modes.rs::extended_pane_receives_all_modified_printable_keys`; I3 `tests/tmux_modes.rs::extended_pane_receives_specials_and_special_modifiers`; H3 `tests/tmux_input.rs::split_extended_keys_deliver_the_unsplit_pane_bytes` | differential | pass | none |
| `extended-keys-format` | `csi-u` selected for extended panes | I3 | `src/server.rs` pane encoder | I3 `tests/tmux_modes.rs::extended_pane_receives_all_modified_printable_keys`; I3 `tests/tmux_modes.rs::legacy_pane_receives_the_legacy_encoding` | internal | fail | none |
| `monitor-bell` | `on`: record a bell for the window list | R5 | `src/server.rs` bell state | R5 `tests/tmux_notifications.rs::audible_bell_reaches_each_attached_client_once` | differential | pass | none |
| `bell-action` | `any`: alert every attached client of the session | R5 | `src/server.rs` alert routing | R5 `tests/tmux_notifications.rs::audible_bell_reaches_each_attached_client_once` | differential | pass | none |
| `visual-bell` | `off`: audible BEL only | R5 | `src/server.rs` alert rendering | R5 `tests/tmux_notifications.rs::visual_bell_suppresses_the_outer_bell` | differential | pass | none |
| `escape-time` | `0`: a partial escape is not held past the escape delay | I1 | `src/server.rs` key state machine | H3 `tests/tmux_input.rs::escape_then_a_delay_longer_than_escape_time_follows_the_reference` | differential | pass | none |
| `focus-events` | `on`: forward focus only to a requesting pane | I3 | `src/server.rs` pane modes | I3 `tests/tmux_modes.rs::focus_events_reach_only_a_requesting_pane`; H3 `tests/tmux_input.rs::split_focus_events_deliver_the_unsplit_pane_bytes`; H3 `tests/tmux_input.rs::focus_in_reaches_a_requesting_pane_when_a_client_attaches` | differential | pass | none |
| `prefix` | `C-a` | T1 | `src/config.rs` default prefix | T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone`; I1 `tests/tmux_input.rs::literal_prefix_bindings_match_reference_bytes` | differential | pass | none |
| `mouse` | `on`: route pointer events with pane coordinates | I3 | `src/server.rs` mouse routing | I3 `tests/tmux_modes.rs::mouse_routes_only_to_the_pane_that_requested_it`; H3 `tests/tmux_input.rs::split_sgr_mouse_delivers_the_unsplit_pane_bytes` | differential | pass | none |
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
| `p` | display pane numbers; tm draws stacked labels in the zoomed pane and tmux draws size and number per pane (TMUX-18) | T1 | `src/config.rs` compiled binding | M1.1a `tests/tmux_bindings.rs::display_panes_binding_is_a_client_overlay_where_the_reference_has_a_pane_mode`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | differential | fail | none |
| `Enter` | display the compiled-profile message | T1 | `src/config.rs` compiled binding | M1.1a `tests/tmux_bindings.rs::message_and_overlay_bindings_match_the_reference`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | differential | pass | none |
| `C-s` | send two literal prefix bytes | T1 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone`; I1 `tests/tmux_input.rs::literal_prefix_bindings_match_reference_bytes` | differential | pass | none |
| `/` | open a command prompt | T1 | `src/config.rs` compiled binding | M1.1a `tests/tmux_bindings.rs::prompt_bindings_match_the_reference`; T1 `tests/tmux_targeting.rs::slash_prompt_resolves_omitted_relative_and_stable_targets` | differential | pass | none |
| `r` | prompt to rename the window | T1, W3 | `src/config.rs` compiled binding | T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone`; W3 `tests/tmux_windows.rs::prompt_values_are_inserted_verbatim` | differential | pass | none |
| `C-Left` | previous window, repeating | T1 | `src/config.rs` compiled binding | M1.1a `tests/tmux_bindings.rs::window_navigation_bindings_repeat_like_the_reference`; T1 `tests/tmux_targeting.rs::two_clients_keep_binding_context_independent` | differential | pass | none |
| `C-Right` | next window, repeating | T1 | `src/config.rs` compiled binding | M1.1a `tests/tmux_bindings.rs::window_navigation_bindings_repeat_like_the_reference`; T1 `tests/tmux_targeting.rs::two_clients_keep_binding_context_independent` | differential | pass | none |
| `Left` | swap and select the previous window, repeating | T1 | `src/config.rs` compiled binding | M1.1a `tests/tmux_bindings.rs::window_navigation_bindings_repeat_like_the_reference`; T1 `tests/tmux_targeting.rs::client_bindings_use_the_attached_session_and_window` | differential | pass | none |
| `Right` | swap and select the next window, repeating | T1 | `src/config.rs` compiled binding | M1.1a `tests/tmux_bindings.rs::window_navigation_bindings_repeat_like_the_reference`; T1 `tests/tmux_targeting.rs::client_bindings_use_the_attached_session_and_window` | differential | pass | none |
| `\` | split horizontal in the current path, even-horizontal | W1 | `src/config.rs` compiled binding | M1.1a `tests/tmux_bindings.rs::directory_splits_keep_the_current_path_and_even_layouts`; W1 `tests/tmux_layouts.rs::even_layouts_match_reference_for_two_to_five_panes`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | differential | pass | none |
| `-` | split vertical in the current path, even-vertical | W1 | `src/config.rs` compiled binding | M1.1a `tests/tmux_bindings.rs::directory_splits_keep_the_current_path_and_even_layouts`; W1 `tests/tmux_layouts.rs::even_layouts_match_reference_for_two_to_five_panes`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | differential | pass | none |
| `z` | toggle pane zoom | W1 | `src/server.rs` zoom state | M1.1a `tests/tmux_bindings.rs::pane_bindings_match_the_reference`; W1 `tests/tmux_layouts.rs::zoom_and_unzoom_match_reference_geometry` | differential | pass | none |
| `k` | kill the current pane, then display a message | T2 | `src/config.rs` compiled binding | M1.1a `tests/tmux_bindings.rs::kill_pane_with_message_matches_the_reference`; T2 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | differential | pass | none |
| `m` | prompt for a join target | T1 | `src/config.rs` compiled binding | M1.1a `tests/tmux_bindings.rs::prompt_bindings_match_the_reference`; T1 `tests/tmux_targeting.rs::break_and_join_bindings_use_client_context` | differential | pass | none |
| `C-n` | break pane to the next free index | T1 | `src/config.rs` compiled binding | M1.1a `tests/tmux_bindings.rs::window_lifecycle_bindings_match_the_reference`; T1 `tests/tmux_targeting.rs::break_and_join_bindings_use_client_context` | differential | pass | none |
| `%` | split horizontal | W1 | `src/config.rs` compiled binding | M1.1a `tests/tmux_bindings.rs::pane_bindings_match_the_reference`; W1 `tests/tmux_layouts.rs::even_layouts_match_reference_for_two_to_five_panes`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | differential | pass | none |
| `"` | split vertical | W1 | `src/config.rs` compiled binding | M1.1a `tests/tmux_bindings.rs::pane_bindings_match_the_reference`; W1 `tests/tmux_layouts.rs::even_layouts_match_reference_for_two_to_five_panes`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | differential | pass | none |
| `h` | select pane left | W1 | `src/config.rs` compiled binding | M1.1a `tests/tmux_bindings.rs::pane_bindings_match_the_reference`; W1 `tests/tmux_layouts.rs::pane_navigation_and_last_targets_match_reference`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | differential | pass | none |
| `j` | select pane down | W1 | `src/config.rs` compiled binding | M1.1a `tests/tmux_bindings.rs::pane_bindings_match_the_reference`; W1 `tests/tmux_layouts.rs::pane_navigation_and_last_targets_match_reference`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | differential | pass | none |
| `l` | select pane right | W1 | `src/config.rs` compiled binding | M1.1a `tests/tmux_bindings.rs::pane_bindings_match_the_reference`; W1 `tests/tmux_layouts.rs::pane_navigation_and_last_targets_match_reference`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | differential | pass | none |
| `x` | kill pane | T2 | `src/config.rs` compiled binding | M1.1a `tests/tmux_bindings.rs::pane_bindings_match_the_reference`; T2 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | differential | pass | none |
| `&` | kill window | T2 | `src/config.rs` compiled binding | M1.1a `tests/tmux_bindings.rs::window_lifecycle_bindings_match_the_reference`; T2 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | differential | pass | none |
| `c` | new window | W3 | `src/config.rs` compiled binding | M1.1a `tests/tmux_bindings.rs::window_lifecycle_bindings_match_the_reference`; W3 `tests/tmux_windows.rs::base_index_renumbering_and_window_lifecycle_match_reference`; T1 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone` | differential | pass | none |
| `b` | send the prefix to the pane | I1 | `src/config.rs` compiled binding | I1 `tests/tmux_input.rs::literal_prefix_bindings_match_reference_bytes` | differential | pass | none |
| `[` | enter copy mode | R2, R3 | `src/config.rs` compiled binding | M1.1a `tests/tmux_bindings.rs::copy_mode_binding_matches_the_reference`; R3 `tests/tmux_copy_cells.rs::emacs_copy_actions_move_search_and_copy_with_the_cell_model` | differential | pass | none |
| `d` | detach the client | L2 | `src/config.rs` compiled binding | M1.1a `tests/tmux_bindings.rs::detach_binding_matches_the_reference`; L2 `tests/tmux_targeting.rs::every_effective_binding_from_a_second_session_leaves_the_first_session_alone`; L2 `tests/tmux_lifecycle.rs::idle_disconnect_releases_the_client_registration` | differential | pass | none |

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
| in-pane targeting | a command run from a pane resolves omitted targets and `-s` sources to the session containing the pane, its current window, and the window's active pane; a pane that no longer exists resolves as outside any pane | T1 | `src/client.rs` `caller_pane`; `src/server.rs` `caller_context` | T1 `tests/tmux_targeting.rs::commands_in_a_pane_resolve_omitted_targets_from_that_pane_like_the_reference`; T1 `tests/tmux_targeting.rs::commands_from_a_killed_pane_fall_back_like_the_reference`; T1 `tests/tmux_histories.rs::attached_histories_match_the_reference` | differential | pass | tm exports `TM_SOCKET` and `TM_PANE` and never reads or sets `TMUX` or `TMUX_PANE`, and sends the pane only when the socket is the one named by `TM_SOCKET`; tmux applies `TMUX_PANE` to any server it reaches |
| buffers | set/show/paste/delete with the compiled buffer limit | R5 | `src/server.rs` buffers | R5 `tests/tmux_buffers_compat.rs::buffers_match_the_headless_tmux_command_contract` | internal | fail | none |
| capture | capture visible and history ranges | R1 | `src/server.rs` capture | R1 `tests/tmux_history.rs::checkpoint_preserves_recent_history_and_later_output`; R1 `tests/tmux_history.rs::history_limit_capture_matches_the_reference` | differential | pass | none |
| capture options | `capture-pane -J`, `-e`, and `-N` output matches tmux 3.8 | R1 | `src/capture.rs` | R1 `tests/tmux_history.rs::capture_keeps_trailing_blank_rows_like_the_reference`, `capture_options_join_wrapped_rows_and_keep_written_spaces_like_the_reference`, `capture_options_emit_attribute_transitions_like_the_reference`, `capture_options_treat_cleared_cells_like_the_reference` | differential | pass | none |
| capture of erased cells | `capture-pane -J` and `-N` keep cells that were written and then erased, as tmux's line storage does | R1 | `src/capture.rs` | R1 `tests/tmux_history.rs::capture_keeps_cells_erased_after_being_written_like_the_reference` | differential | fail | tmux remembers how far each line was ever written (`cellused`, `cellsize` in `grid.c`); vt100 keeps no such history, so tm rebuilds the extent from the cells that still hold content, and `-J` and `-N` omit the erased cells |
| wide character at the margin | a wide character that cannot fit in the last column wraps with the row marked wrapped, so `capture-pane -J` joins it; one that exactly fits stays on its row | H4 | `src/terminal.rs` | H4 `tests/tmux_unicode_history.rs::wide_characters_wrap_trim_copy_and_capture_like_the_reference`; H4 `tests/tmux_unicode_history.rs::wide_characters_survive_narrower_and_wider_resizes_like_the_reference` | differential | fail | `vt100` marks a row wrapped only when its last cell holds content (TMUX-41) |
| resize reflow | a narrower or wider resize reflows wrapped lines and history | H4 | `src/model.rs` | H4 `tests/tmux_unicode_history.rs::wide_characters_survive_narrower_and_wider_resizes_like_the_reference` | differential | fail | tm truncates rows and never recovers them (TMUX-42) |
| history trim granularity | reaching `history-limit` drops a tenth of the limit at once | H4 | `src/model.rs` | H4 `tests/tmux_unicode_history.rs::wide_characters_wrap_trim_copy_and_capture_like_the_reference` | differential | fail | `vt100` drops one line at a time (TMUX-43) |
| copy mode wrapped rows | `start-of-line` and `end-of-line` span a wrapped line, and a selection joins wrapped rows without a newline | H4 | `src/copy_mode.rs` | H4 `tests/tmux_unicode_history.rs::copy_mode_lines_and_selections_follow_wrapped_rows_like_the_reference`; H4 `tests/tmux_unicode_history.rs::wide_characters_wrap_trim_copy_and_capture_like_the_reference` | differential | pass | none |
| capture in copy mode | `capture-pane` reads the pane's own screen, not the copy-mode viewport | H4 | `src/server.rs` capture | H4 `tests/tmux_unicode_history.rs::capture_pane_ignores_the_copy_mode_viewport_like_the_reference` | differential | pass | none |
| `new-session -E` | `-E` only skips `update-environment`, which tm lacks, so the pane runs the given command or the default shell; `new-session` prints nothing unless `-P` or `-F` is given | C2 | `src/command.rs` `parse_new_session`; `src/client.rs` | C2 `tests/tmux_core_compat.rs::new_session_e_keeps_the_command_and_the_default_shell_like_the_reference` | differential | pass | none |
| `pipe-pane` | an exited helper is reaped within 1 s; `#{pane_pipe}` stays `1` until pane output fails to reach the helper, then `0` | L2 | `src/server.rs` `pipe_pane`, `PipeHelper` | L2 `tests/tmux_lifecycle.rs::exited_pipe_helper_is_reaped_and_pane_pipe_follows_the_reference` | differential | pass | none |
| `send-prefix` | CLI and binding send the configured `prefix` to the target pane (and its synchronized siblings); `-2` is rejected because tm has no `prefix2` | C1 | `src/server.rs` `send_prefix` | C1 `tests/tmux_input.rs::cli_send_prefix_writes_the_configured_prefix_like_the_reference`; C1 `tests/tmux_input.rs::cli_send_prefix_rejects_the_secondary_prefix_it_cannot_send` | differential | pass | tmux accepts `-2` and sends nothing while `prefix2` is `None`; tm rejects it |
| copy mode | selection, copy, search, and cursor movement | R3 | `src/copy_mode.rs` | R3 `tests/tmux_copy_cells.rs::wide_combining_and_styled_cells_copy_the_logical_text`; R3 `tests/tmux_copy_cells.rs::hyperlink_cells_copy_text_and_preserve_the_link_in_capture` | differential | pass | none |
| session lifecycle | create, rename, kill, attach, and detach | L2 | `src/server.rs` sessions | L2 `tests/tmux_lifecycle.rs::idle_disconnect_releases_the_client_registration` | internal | fail | none |
| window lifecycle | create, rename, select, swap, and kill | W3 | `src/server.rs` windows | W3 `tests/tmux_windows.rs::base_index_renumbering_and_window_lifecycle_match_reference` | differential | pass | none |
| pane lifecycle | split, break, join, move, swap, and kill | T2 | `src/server.rs` panes | T2 `tests/tmux_atomicity.rs::invalid_join_and_swap_targets_error_without_mutation` | internal | fail | none |
| attached terminal | alternate screen, mode ownership, and restoration | C1 | `src/client.rs` terminal | C1 `tests/tmux_terminal_modes.rs::attach_owns_modes_once_and_restores_them_on_detach`; C1 `tests/tmux_terminal_modes.rs::attach_sets_and_detach_resets_each_terminal_mode_like_the_reference` | differential | pass | none |
| input | prefix, repeat, escape, paste, and extended keys | I1 | `src/server.rs` input state machine | I1 `tests/tmux_input.rs::repeat_deadline_and_switching_follow_the_500ms_contract`; I2 `tests/tmux_paste.rs::fragmented_end_after_payload_finishes_the_paste`; H3 `tests/tmux_input.rs::split_prefix_bindings_deliver_the_unsplit_pane_bytes`; H3 `tests/tmux_input.rs::split_prefix_window_switch_follows_the_reference`; T1 `tests/tmux_input.rs::keys_unbound_in_the_prefix_table_are_dropped_like_the_reference`; H3 `tests/tmux_paste.rs::split_bracketed_paste_delivers_the_unsplit_pane_bytes` | differential | pass | none |
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
| `list-windows` | `-a` | match | list the windows of every session, ordered by session name then index, with the session-qualified default format | C6a `tests/tmux_cli_flags.rs::windows_list_windows_flags_match_the_reference` | differential |
| `list-windows` | `-F` | match | output format, evaluated for each window with its active pane; an empty format prints an empty line per window | C6a `tests/tmux_cli_flags.rs::windows_list_windows_flags_match_the_reference` | differential |
| `list-windows` | `-f` | match | format filter; every value except an empty string and `0` is true | C6a `tests/tmux_cli_flags.rs::windows_list_windows_flags_match_the_reference` | differential |
| `list-windows` | `-t` | match | target session; an unknown session fails with `can't find session` | C6a `tests/tmux_cli_flags.rs::windows_list_windows_flags_match_the_reference` | differential |
| `list-windows` | `(default format)` | tm-only | tm prints tmux's default line with an empty `#{window_layout}`, because tm has no layout dump in tmux 3.8's format | C6a `tests/tmux_cli_flags.rs::windows_list_windows_default_format_matches_except_the_layout` | differential |
| `list-windows` | `-O` | rejected | sort order | C6a `tests/tmux_cli_flags.rs::windows_list_windows_rejects_flags_it_does_not_implement` | internal |
| `list-windows` | `-r` | rejected | reverse the sort order | C6a `tests/tmux_cli_flags.rs::windows_list_windows_rejects_flags_it_does_not_implement` | internal |
| `list-windows` | `(any other flag)` | rejected | tmux 3.8 defines only `-a`, `-F`, `-f`, `-O`, `-r`, and `-t` | C6a `tests/tmux_cli_flags.rs::windows_list_windows_rejects_flags_it_does_not_implement` | internal |
| `select-window` | `-t (offset targets)` | match | `+N` and `-N` need a count of 1 or more; `--`, `-x`, and `+0` are looked up as window names and fail with `can't find window` | C6a `tests/tmux_cli_flags.rs::windows_offset_targets_that_are_not_counts_fail_like_the_reference` | differential |
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
| `list-panes` | `-a` | match | every pane of every session, sessions ordered by name | C5 `tests/tmux_cli_flags.rs::panes_list_panes_flags_match_the_reference` | differential |
| `list-panes` | `-F` | match | format for each pane | C5 `tests/tmux_cli_flags.rs::panes_list_panes_flags_match_the_reference` | differential |
| `list-panes` | `-f` | match | print only the panes for which the expanded filter is true (non-empty and not `0`) | C5 `tests/tmux_cli_flags.rs::panes_list_panes_flags_match_the_reference` | differential |
| `list-panes` | `-s` | match | every pane of the target window's session | C5 `tests/tmux_cli_flags.rs::panes_list_panes_flags_match_the_reference` | differential |
| `list-panes` | `-t` | match | target window | C5 `tests/tmux_cli_flags.rs::panes_list_panes_flags_match_the_reference` | differential |
| `list-panes` | `-F (default format)` | tm-only | tm prints `%<id>: <index> <marker>`, with a `<window>.<index>: ` prefix for `-s` and `<session>:<window>.<index>: ` for `-a`; tmux prints geometry and history sizes | C5 `tests/tmux_cli_flags.rs::panes_list_panes_default_format_is_tm_only` | internal |
| `list-panes` | `-O` | rejected | sort order | C5 `tests/tmux_cli_flags.rs::panes_list_panes_rejects_flags_it_does_not_implement` | internal |
| `list-panes` | `-r` | rejected | reverse the sort order | C5 `tests/tmux_cli_flags.rs::panes_list_panes_rejects_flags_it_does_not_implement` | internal |
| `split-window` | `-b` | match | place the new pane before the target; the new pane is the larger half on odd sizes | C5 `tests/tmux_cli_flags.rs::panes_split_window_size_and_placement_flags_match_the_reference` | differential |
| `split-window` | `-c` | match | start directory of the new pane | C5 `tests/tmux_cli_flags.rs::panes_split_window_command_and_format_flags_match_the_reference` | differential |
| `split-window` | `-d` | match | do not make the new pane active | C5 `tests/tmux_cli_flags.rs::panes_split_window_size_and_placement_flags_match_the_reference` | differential |
| `split-window` | `-E` | match | empty pane with no process; a command is an error | C5 `tests/tmux_cli_flags.rs::panes_split_window_command_and_format_flags_match_the_reference` | differential |
| `split-window` | `-F` | match | format for `-P`; without `-P` nothing is printed | C5 `tests/tmux_cli_flags.rs::panes_split_window_command_and_format_flags_match_the_reference` | differential |
| `split-window` | `-h` | match | split side by side | C5 `tests/tmux_cli_flags.rs::panes_split_window_size_and_placement_flags_match_the_reference` | differential |
| `split-window` | `-l` | match | size in cells or percent, clamped to leave one cell for each pane; invalid sizes fail with `invalid tiled geometry` | C5 `tests/tmux_cli_flags.rs::panes_split_window_size_and_placement_flags_match_the_reference` | differential |
| `split-window` | `-p` | match | size as a percentage; `-l` wins when both are given | C5 `tests/tmux_cli_flags.rs::panes_split_window_size_and_placement_flags_match_the_reference` | differential |
| `split-window` | `-P` | match | print the new pane; default format `#{session_name}:#{window_index}.#{pane_index}` | C5 `tests/tmux_cli_flags.rs::panes_split_window_command_and_format_flags_match_the_reference` | differential |
| `split-window` | `-t` | match | target pane | C5 `tests/tmux_cli_flags.rs::panes_split_window_size_and_placement_flags_match_the_reference`; C5 `tests/tmux_cli_flags.rs::panes_split_window_command_and_format_flags_match_the_reference` | differential |
| `split-window` | `-v` | match | split top and bottom (the default) | C5 `tests/tmux_cli_flags.rs::panes_split_window_size_and_placement_flags_match_the_reference` | differential |
| `split-window` | `-Z` | match | zoom the new pane | C5 `tests/tmux_cli_flags.rs::panes_split_window_size_and_placement_flags_match_the_reference` | differential |
| `split-window` | `shell-command` | match | one word runs through the shell, several words run directly; `pane_start_command` quotes each word like tmux | C5 `tests/tmux_cli_flags.rs::panes_split_window_command_and_format_flags_match_the_reference` | differential |
| `split-window` | `-Z (with -d)` | rejected | zoom while keeping the old pane active; tmux sizes the hidden new pane to the whole window | C5 `tests/tmux_cli_flags.rs::panes_split_window_rejects_flags_it_does_not_implement` | internal |
| `split-window` | `-B` | rejected | border lines of the new pane | C5 `tests/tmux_cli_flags.rs::panes_split_window_rejects_flags_it_does_not_implement` | internal |
| `split-window` | `-e` | rejected | set an environment variable in the new pane | C5 `tests/tmux_cli_flags.rs::panes_split_window_rejects_flags_it_does_not_implement` | internal |
| `split-window` | `-f` | rejected | full-width or full-height split of the window root; tm's old implementation overlapped the other panes | C5 `tests/tmux_cli_flags.rs::panes_split_window_rejects_flags_it_does_not_implement` | internal |
| `split-window` | `-I` | rejected | forward standard input to the new pane | C5 `tests/tmux_cli_flags.rs::panes_split_window_rejects_flags_it_does_not_implement` | internal |
| `split-window` | `-k` | rejected | keep the pane after its command exits | C5 `tests/tmux_cli_flags.rs::panes_split_window_rejects_flags_it_does_not_implement` | internal |
| `split-window` | `-m` | rejected | message shown when the pane stays open | C5 `tests/tmux_cli_flags.rs::panes_split_window_rejects_flags_it_does_not_implement` | internal |
| `split-window` | `-R` | rejected | inactive border style | C5 `tests/tmux_cli_flags.rs::panes_split_window_rejects_flags_it_does_not_implement` | internal |
| `split-window` | `-s` | rejected | pane style | C5 `tests/tmux_cli_flags.rs::panes_split_window_rejects_flags_it_does_not_implement` | internal |
| `split-window` | `-S` | rejected | active border style | C5 `tests/tmux_cli_flags.rs::panes_split_window_rejects_flags_it_does_not_implement` | internal |
| `split-window` | `-T` | rejected | pane title | C5 `tests/tmux_cli_flags.rs::panes_split_window_rejects_flags_it_does_not_implement` | internal |
| `split-window` | `-W` | rejected | wait until the pane's command exits | C5 `tests/tmux_cli_flags.rs::panes_split_window_rejects_flags_it_does_not_implement` | internal |
| `new-pane` | `(any flag)` | rejected | create a floating pane; tm has no floating panes and used to run split-window instead | C5 `tests/tmux_cli_flags.rs::panes_new_pane_is_rejected` | internal |
| `select-pane` | `-D` | match | pane below, wrapping to the top; ties go to the most recently active pane | C5 `tests/tmux_cli_flags.rs::panes_select_pane_navigation_flags_match_the_reference` | differential |
| `select-pane` | `-d` | match | disable input to the pane | C5 `tests/tmux_cli_flags.rs::panes_select_pane_state_flags_match_the_reference` | differential |
| `select-pane` | `-e` | match | enable input to the pane | C5 `tests/tmux_cli_flags.rs::panes_select_pane_state_flags_match_the_reference` | differential |
| `select-pane` | `-L` | match | pane to the left, wrapping to the right | C5 `tests/tmux_cli_flags.rs::panes_select_pane_navigation_flags_match_the_reference` | differential |
| `select-pane` | `-l` | match | last pane; with `-d` or `-e` it changes the last pane's input without selecting it | C5 `tests/tmux_cli_flags.rs::panes_select_pane_navigation_flags_match_the_reference`; C5 `tests/tmux_cli_flags.rs::panes_select_pane_state_flags_match_the_reference` | differential |
| `select-pane` | `-M` | match | clear the marked pane | C5 `tests/tmux_cli_flags.rs::panes_select_pane_state_flags_match_the_reference` | differential |
| `select-pane` | `-m` | match | toggle the mark on the target pane without selecting it | C5 `tests/tmux_cli_flags.rs::panes_select_pane_state_flags_match_the_reference` | differential |
| `select-pane` | `-R` | match | pane to the right, wrapping to the left | C5 `tests/tmux_cli_flags.rs::panes_select_pane_navigation_flags_match_the_reference` | differential |
| `select-pane` | `-t` | match | target pane | C5 `tests/tmux_cli_flags.rs::panes_select_pane_navigation_flags_match_the_reference` | differential |
| `select-pane` | `-T` | match | pane title, expanded as a format; the pane is not selected | C5 `tests/tmux_cli_flags.rs::panes_select_pane_state_flags_match_the_reference` | differential |
| `select-pane` | `-U` | match | pane above, wrapping to the bottom | C5 `tests/tmux_cli_flags.rs::panes_select_pane_navigation_flags_match_the_reference` | differential |
| `select-pane` | `-g` | rejected | print the pane style | C5 `tests/tmux_cli_flags.rs::panes_select_pane_rejects_flags_it_does_not_implement` | internal |
| `select-pane` | `-P` | rejected | set the pane style | C5 `tests/tmux_cli_flags.rs::panes_select_pane_rejects_flags_it_does_not_implement` | internal |
| `select-pane` | `-Z` | rejected | keep the window zoomed | C5 `tests/tmux_cli_flags.rs::panes_select_pane_rejects_flags_it_does_not_implement` | internal |
| `select-pane` | `-n` | rejected | tm-only next-pane flag; tmux 3.8 has no such flag | C5 `tests/tmux_cli_flags.rs::panes_select_pane_rejects_flags_it_does_not_implement` | internal |
| `select-pane` | `-p` | rejected | tm-only previous-pane flag; tmux 3.8 has no such flag | C5 `tests/tmux_cli_flags.rs::panes_select_pane_rejects_flags_it_does_not_implement` | internal |
| `kill-pane` | `-a` | match | kill every pane of the window except the target | C5 `tests/tmux_cli_flags.rs::panes_kill_pane_flags_match_the_reference` | differential |
| `kill-pane` | `-f` | match | with `-a`, kill only the panes for which the filter is true; without `-a` it is an error | C5 `tests/tmux_cli_flags.rs::panes_kill_pane_flags_match_the_reference` | differential |
| `kill-pane` | `-t` | match | target pane; the last pane takes its window with it, and the last window its session | C5 `tests/tmux_cli_flags.rs::panes_kill_pane_flags_match_the_reference` | differential |
| `kill-pane` | `-K` | rejected | tm-only flag; tmux 3.8 has no such flag | C5 `tests/tmux_cli_flags.rs::panes_kill_pane_rejects_flags_it_does_not_implement` | internal |
| `kill-pane` | `-F` | rejected | tm-only flag; tmux 3.8 has no such flag | C5 `tests/tmux_cli_flags.rs::panes_kill_pane_rejects_flags_it_does_not_implement` | internal |
| `break-pane` | `-d` | match | do not select the new window | C5 `tests/tmux_cli_flags.rs::panes_break_pane_flags_match_the_reference` | differential |
| `break-pane` | `-F` | match | format for `-P` | C5 `tests/tmux_cli_flags.rs::panes_break_pane_flags_match_the_reference` | differential |
| `break-pane` | `-n` | match | window name; by default the window is named after the pane's command | C5 `tests/tmux_cli_flags.rs::panes_break_pane_flags_match_the_reference` | differential |
| `break-pane` | `-P` | match | print the new window's pane; default format `#{session_name}:#{window_index}.#{pane_index}` | C5 `tests/tmux_cli_flags.rs::panes_break_pane_flags_match_the_reference` | differential |
| `break-pane` | `-s` | match | source pane; the default is the marked pane or the current pane | C5 `tests/tmux_cli_flags.rs::panes_break_pane_flags_match_the_reference` | differential |
| `break-pane` | `-t` | match | destination window index or session; the new window may be in another session, and a window's only pane moves the window | C5 `tests/tmux_cli_flags.rs::panes_break_pane_flags_match_the_reference` | differential |
| `break-pane` | `-a` | rejected | insert after the target window | C5 `tests/tmux_cli_flags.rs::panes_break_pane_rejects_flags_it_does_not_implement` | internal |
| `break-pane` | `-b` | rejected | insert before the target window | C5 `tests/tmux_cli_flags.rs::panes_break_pane_rejects_flags_it_does_not_implement` | internal |
| `break-pane` | `-W` | rejected | turn the pane into a floating pane | C5 `tests/tmux_cli_flags.rs::panes_break_pane_rejects_flags_it_does_not_implement` | internal |
| `break-pane` | `-x` | rejected | floating pane width | C5 `tests/tmux_cli_flags.rs::panes_break_pane_rejects_flags_it_does_not_implement` | internal |
| `break-pane` | `-X` | rejected | floating pane x position | C5 `tests/tmux_cli_flags.rs::panes_break_pane_rejects_flags_it_does_not_implement` | internal |
| `break-pane` | `-y` | rejected | floating pane height | C5 `tests/tmux_cli_flags.rs::panes_break_pane_rejects_flags_it_does_not_implement` | internal |
| `break-pane` | `-Y` | rejected | floating pane y position | C5 `tests/tmux_cli_flags.rs::panes_break_pane_rejects_flags_it_does_not_implement` | internal |
| `join-pane` | `-b` | match | place the pane before the target | C5 `tests/tmux_cli_flags.rs::panes_join_pane_flags_match_the_reference` | differential |
| `join-pane` | `-d` | match | do not select the joined pane | C5 `tests/tmux_cli_flags.rs::panes_join_pane_flags_match_the_reference` | differential |
| `join-pane` | `-h` | match | split side by side | C5 `tests/tmux_cli_flags.rs::panes_join_pane_flags_match_the_reference` | differential |
| `join-pane` | `-l` | match | size in cells or percent; invalid sizes fail with `size or position invalid tiled geometry` | C5 `tests/tmux_cli_flags.rs::panes_join_pane_flags_match_the_reference` | differential |
| `join-pane` | `-p` | match | size as a percentage | C5 `tests/tmux_cli_flags.rs::panes_join_pane_flags_match_the_reference` | differential |
| `join-pane` | `-s` | match | source pane, possibly in another session; the default is the marked pane | C5 `tests/tmux_cli_flags.rs::panes_join_pane_flags_match_the_reference` | differential |
| `join-pane` | `-t` | match | target pane | C5 `tests/tmux_cli_flags.rs::panes_join_pane_flags_match_the_reference` | differential |
| `join-pane` | `-v` | match | split top and bottom (the default) | C5 `tests/tmux_cli_flags.rs::panes_join_pane_flags_match_the_reference` | differential |
| `join-pane` | `-f` | rejected | full-width or full-height split of the window root | C5 `tests/tmux_cli_flags.rs::panes_join_pane_rejects_flags_it_does_not_implement` | internal |
| `join-pane` | `-s (same window as -t)` | tm-only | tmux re-lays out the window; tm fails with `source and target panes must be in different windows` | C5 `tests/tmux_cli_flags.rs::panes_join_pane_within_one_window_is_tm_only` | internal |
| `move-pane` | `-b` | match | same as `join-pane -b` | C5 `tests/tmux_cli_flags.rs::panes_move_pane_flags_match_the_reference` | differential |
| `move-pane` | `-d` | match | same as `join-pane -d` | C5 `tests/tmux_cli_flags.rs::panes_move_pane_flags_match_the_reference` | differential |
| `move-pane` | `-h` | match | same as `join-pane -h` | C5 `tests/tmux_cli_flags.rs::panes_move_pane_flags_match_the_reference` | differential |
| `move-pane` | `-l` | match | same as `join-pane -l` | C5 `tests/tmux_cli_flags.rs::panes_move_pane_flags_match_the_reference` | differential |
| `move-pane` | `-s` | match | same as `join-pane -s` | C5 `tests/tmux_cli_flags.rs::panes_move_pane_flags_match_the_reference` | differential |
| `move-pane` | `-t` | match | same as `join-pane -t` | C5 `tests/tmux_cli_flags.rs::panes_move_pane_flags_match_the_reference` | differential |
| `move-pane` | `-v` | match | same as `join-pane -v` | C5 `tests/tmux_cli_flags.rs::panes_move_pane_flags_match_the_reference` | differential |
| `move-pane` | `-f` | rejected | full-width or full-height split of the window root | C5 `tests/tmux_cli_flags.rs::panes_move_pane_rejects_flags_it_does_not_implement` | internal |
| `move-pane` | `-p` | rejected | not a tmux move-pane flag | C5 `tests/tmux_cli_flags.rs::panes_move_pane_rejects_flags_it_does_not_implement` | internal |
| `move-pane` | `-M` | rejected | move a floating pane with the mouse | C5 `tests/tmux_cli_flags.rs::panes_move_pane_rejects_flags_it_does_not_implement` | internal |
| `move-pane` | `-D` | rejected | move a floating pane down | C5 `tests/tmux_cli_flags.rs::panes_move_pane_rejects_flags_it_does_not_implement` | internal |
| `move-pane` | `-L` | rejected | move a floating pane left | C5 `tests/tmux_cli_flags.rs::panes_move_pane_rejects_flags_it_does_not_implement` | internal |
| `move-pane` | `-P` | rejected | place a floating pane | C5 `tests/tmux_cli_flags.rs::panes_move_pane_rejects_flags_it_does_not_implement` | internal |
| `move-pane` | `-R` | rejected | move a floating pane right | C5 `tests/tmux_cli_flags.rs::panes_move_pane_rejects_flags_it_does_not_implement` | internal |
| `move-pane` | `-U` | rejected | move a floating pane up | C5 `tests/tmux_cli_flags.rs::panes_move_pane_rejects_flags_it_does_not_implement` | internal |
| `move-pane` | `-X` | rejected | floating pane x position | C5 `tests/tmux_cli_flags.rs::panes_move_pane_rejects_flags_it_does_not_implement` | internal |
| `move-pane` | `-Y` | rejected | floating pane y position | C5 `tests/tmux_cli_flags.rs::panes_move_pane_rejects_flags_it_does_not_implement` | internal |
| `move-pane` | `-z` | rejected | floating pane z-index | C5 `tests/tmux_cli_flags.rs::panes_move_pane_rejects_flags_it_does_not_implement` | internal |
| `swap-pane` | `-D` | match | swap with the next pane, wrapping; wins over `-U` and ignores `-s` | C5 `tests/tmux_cli_flags.rs::panes_swap_pane_flags_match_the_reference` | differential |
| `swap-pane` | `-d` | match | keep the active pane's slot instead of following the swap | C5 `tests/tmux_cli_flags.rs::panes_swap_pane_flags_match_the_reference` | differential |
| `swap-pane` | `-s` | match | source pane; the default is the marked pane or the current pane, and swapping a pane with itself does nothing | C5 `tests/tmux_cli_flags.rs::panes_swap_pane_flags_match_the_reference` | differential |
| `swap-pane` | `-t` | match | target pane; the panes may be in different windows or sessions | C5 `tests/tmux_cli_flags.rs::panes_swap_pane_flags_match_the_reference` | differential |
| `swap-pane` | `-U` | match | swap with the previous pane, wrapping | C5 `tests/tmux_cli_flags.rs::panes_swap_pane_flags_match_the_reference` | differential |
| `swap-pane` | `-Z` | rejected | keep the window zoomed | C5 `tests/tmux_cli_flags.rs::panes_swap_pane_rejects_flags_it_does_not_implement` | internal |
| `resize-pane` | `-D` | match | move the border below the pane down by the count (default 1); the count is the value or the positional argument; limited to the nearest enclosing split, so three or more panes along one axis can differ (TMUX-15) | C5b `tests/tmux_cli_flags.rs::panes_resize_pane_flags_match_the_reference`; C5b `tests/tmux_cli_flags.rs::panes_resize_pane_edges_and_zoom_match_the_reference` | differential |
| `resize-pane` | `-L` | match | move the border at the pane left by the count; limited to the nearest enclosing split, so three or more panes along one axis can differ (TMUX-15) | C5b `tests/tmux_cli_flags.rs::panes_resize_pane_flags_match_the_reference`; C5b `tests/tmux_cli_flags.rs::panes_resize_pane_edges_and_zoom_match_the_reference` | differential |
| `resize-pane` | `-R` | match | move the border at the pane right by the count; limited to the nearest enclosing split, so three or more panes along one axis can differ (TMUX-15) | C5b `tests/tmux_cli_flags.rs::panes_resize_pane_flags_match_the_reference`; C5b `tests/tmux_cli_flags.rs::panes_resize_pane_edges_and_zoom_match_the_reference` | differential |
| `resize-pane` | `-U` | match | move the border above the pane up by the count; limited to the nearest enclosing split, so three or more panes along one axis can differ (TMUX-15) | C5b `tests/tmux_cli_flags.rs::panes_resize_pane_flags_match_the_reference`; C5b `tests/tmux_cli_flags.rs::panes_resize_pane_edges_and_zoom_match_the_reference` | differential |
| `resize-pane` | `-t` | match | target pane | C5b `tests/tmux_cli_flags.rs::panes_resize_pane_flags_match_the_reference`; C5b `tests/tmux_cli_flags.rs::panes_resize_pane_edges_and_zoom_match_the_reference` | differential |
| `resize-pane` | `-x` | match | width in columns or percent of the window, clamped to the room available; errors read `width invalid`, `too small`, `too large`, `empty` | C5b `tests/tmux_cli_flags.rs::panes_resize_pane_flags_match_the_reference`; C5b `tests/tmux_cli_flags.rs::panes_resize_pane_edges_and_zoom_match_the_reference` | differential |
| `resize-pane` | `-y` | match | height in rows or percent of the window, with the same errors as `-x` | C5b `tests/tmux_cli_flags.rs::panes_resize_pane_flags_match_the_reference`; C5b `tests/tmux_cli_flags.rs::panes_resize_pane_edges_and_zoom_match_the_reference` | differential |
| `resize-pane` | `-Z` | match | toggle zoom and make the target pane active when zooming; resizing a zoomed window unzooms it first | C5b `tests/tmux_cli_flags.rs::panes_resize_pane_flags_match_the_reference`; C5b `tests/tmux_cli_flags.rs::panes_resize_pane_edges_and_zoom_match_the_reference` | differential |
| `resize-pane` | `-M` | rejected | mouse resize | C5b `tests/tmux_cli_flags.rs::panes_resize_pane_rejects_flags_it_does_not_implement` | internal |
| `resize-pane` | `-T` | rejected | trim the pane history below the cursor | C5b `tests/tmux_cli_flags.rs::panes_resize_pane_rejects_flags_it_does_not_implement` | internal |
| `respawn-pane` | `-c` | match | start directory of the respawned process | C5b `tests/tmux_cli_flags.rs::panes_respawn_pane_flags_match_the_reference` | differential |
| `respawn-pane` | `-k` | match | kill a running process first; without it a live pane fails | C5b `tests/tmux_cli_flags.rs::panes_respawn_pane_flags_match_the_reference` | differential |
| `respawn-pane` | `-t` | match | target pane, including an exited pane kept by remain-on-exit | C5b `tests/tmux_cli_flags.rs::panes_respawn_pane_flags_match_the_reference` | differential |
| `respawn-pane` | `shell-command` | match | replaces the pane command; one word runs through the shell | C5b `tests/tmux_cli_flags.rs::panes_respawn_pane_flags_match_the_reference` | differential |
| `respawn-pane` | `-d` | rejected | not a tmux 3.8 flag (tm formerly ignored it) | C5b `tests/tmux_cli_flags.rs::panes_respawn_pane_rejects_flags_it_does_not_implement` | internal |
| `respawn-pane` | `-e` | rejected | set an environment variable in the pane | C5b `tests/tmux_cli_flags.rs::panes_respawn_pane_rejects_flags_it_does_not_implement` | internal |
| `respawn-pane` | `-E` | rejected | respawn without a process; tmux removes the pane in that case and tm kept an empty one | C5b `tests/tmux_cli_flags.rs::panes_respawn_pane_rejects_flags_it_does_not_implement` | internal |
| `display-panes` | `-t` | tm-only | tmux opens the panes mode on the target pane even with no client attached; tm opens it only on an attached client and fails with `chooser target has no attached client` | C5b `tests/tmux_cli_flags.rs::panes_display_panes_needs_an_attached_client_in_tm` | internal |
| `display-panes` | `template` | tm-only | same attached-client requirement; the template is split into a command at the selection | C5b `tests/tmux_cli_flags.rs::panes_display_panes_needs_an_attached_client_in_tm` | internal |
| `display-panes` | `-d` | rejected | overlay duration | C5b `tests/tmux_cli_flags.rs::panes_display_panes_rejects_flags_it_does_not_implement` | internal |
| `display-panes` | `-k` | rejected | kill the overlay on exit | C5b `tests/tmux_cli_flags.rs::panes_display_panes_rejects_flags_it_does_not_implement` | internal |
| `display-panes` | `-N` | rejected | skip the overlay and run the template | C5b `tests/tmux_cli_flags.rs::panes_display_panes_rejects_flags_it_does_not_implement` | internal |
| `display-panes` | `-s` | rejected | source window | C5b `tests/tmux_cli_flags.rs::panes_display_panes_rejects_flags_it_does_not_implement` | internal |
| `display-panes` | `-Z` | rejected | keep the window zoomed | C5b `tests/tmux_cli_flags.rs::panes_display_panes_rejects_flags_it_does_not_implement` | internal |
| `pipe-pane` | `-O` | match | pipe pane output to the command (the default) | C5b `tests/tmux_cli_flags.rs::panes_pipe_pane_flags_match_the_reference`; C5b `tests/tmux_cli_flags.rs::panes_pipe_pane_toggle_matches_the_reference` | differential |
| `pipe-pane` | `-o` | match | open the pipe only when none was open; an open pipe is closed and not reopened | C5b `tests/tmux_cli_flags.rs::panes_pipe_pane_flags_match_the_reference`; C5b `tests/tmux_cli_flags.rs::panes_pipe_pane_toggle_matches_the_reference` | differential |
| `pipe-pane` | `-t` | match | target pane; an exited pane fails with `target pane has exited` | C5b `tests/tmux_cli_flags.rs::panes_pipe_pane_flags_match_the_reference`; C5b `tests/tmux_cli_flags.rs::panes_pipe_pane_toggle_matches_the_reference` | differential |
| `pipe-pane` | `shell-command` | match | replaces any open pipe; an empty or missing command only closes it; `#{}` formats are expanded (strftime is not) | C5b `tests/tmux_cli_flags.rs::panes_pipe_pane_flags_match_the_reference`; C5b `tests/tmux_cli_flags.rs::panes_pipe_pane_toggle_matches_the_reference`; C5b `tests/tmux_cli_flags.rs::panes_pipe_pane_expands_formats_in_the_command` | differential |
| `pipe-pane` | `-I` | rejected | feed the command output into the pane | C5b `tests/tmux_cli_flags.rs::panes_pipe_pane_rejects_flags_it_does_not_implement` | internal |
| `capture-pane` | `-b` | match | name of the buffer written without `-p`; an existing name is replaced and an empty name fails with `empty buffer name` | C5b `tests/tmux_cli_flags.rs::panes_capture_pane_flags_match_the_reference` | differential |
| `capture-pane` | `-e` | match | include escape sequences for text attributes | C5b `tests/tmux_cli_flags.rs::panes_capture_pane_flags_match_the_reference` | differential |
| `capture-pane` | `-E` | match | last line; `-` is the last history line, an invalid value is ignored, and the range is swapped when reversed | C5b `tests/tmux_cli_flags.rs::panes_capture_pane_flags_match_the_reference` | differential |
| `capture-pane` | `-J` | match | join wrapped lines and keep trailing spaces | C5b `tests/tmux_cli_flags.rs::panes_capture_pane_flags_match_the_reference` | differential |
| `capture-pane` | `-N` | match | keep trailing spaces | C5b `tests/tmux_cli_flags.rs::panes_capture_pane_flags_match_the_reference` | differential |
| `capture-pane` | `-p` | match | print to stdout instead of writing a paste buffer | C5b `tests/tmux_cli_flags.rs::panes_capture_pane_flags_match_the_reference` | differential |
| `capture-pane` | `-S` | match | first line; `-` is the first history line, an invalid value is ignored | C5b `tests/tmux_cli_flags.rs::panes_capture_pane_flags_match_the_reference` | differential |
| `capture-pane` | `-t` | match | target pane | C5b `tests/tmux_cli_flags.rs::panes_capture_pane_flags_match_the_reference` | differential |
| `capture-pane` | `(no -p)` | match | writes an automatic paste buffer holding the capture, one newline per line | C5b `tests/tmux_cli_flags.rs::panes_capture_pane_flags_match_the_reference` | differential |
| `capture-pane` | `-a` | rejected | capture the alternate screen | C5b `tests/tmux_cli_flags.rs::panes_capture_pane_rejects_flags_it_does_not_implement` | internal |
| `capture-pane` | `-C` | rejected | escape non-printable characters | C5b `tests/tmux_cli_flags.rs::panes_capture_pane_rejects_flags_it_does_not_implement` | internal |
| `capture-pane` | `-F` | rejected | prefix lines with their flags | C5b `tests/tmux_cli_flags.rs::panes_capture_pane_rejects_flags_it_does_not_implement` | internal |
| `capture-pane` | `-H` | rejected | capture hyperlinks | C5b `tests/tmux_cli_flags.rs::panes_capture_pane_rejects_flags_it_does_not_implement` | internal |
| `capture-pane` | `-I` | rejected | prefix lines with their time | C5b `tests/tmux_cli_flags.rs::panes_capture_pane_rejects_flags_it_does_not_implement` | internal |
| `capture-pane` | `-L` | rejected | number lines | C5b `tests/tmux_cli_flags.rs::panes_capture_pane_rejects_flags_it_does_not_implement` | internal |
| `capture-pane` | `-M` | rejected | capture the mode screen | C5b `tests/tmux_cli_flags.rs::panes_capture_pane_rejects_flags_it_does_not_implement` | internal |
| `capture-pane` | `-P` | rejected | capture pending input | C5b `tests/tmux_cli_flags.rs::panes_capture_pane_rejects_flags_it_does_not_implement` | internal |
| `capture-pane` | `-q` | rejected | suppress the no-alternate-screen error | C5b `tests/tmux_cli_flags.rs::panes_capture_pane_rejects_flags_it_does_not_implement` | internal |
| `capture-pane` | `-R` | rejected | capture the raw grid | C5b `tests/tmux_cli_flags.rs::panes_capture_pane_rejects_flags_it_does_not_implement` | internal |
| `capture-pane` | `-T` | rejected | stop at the last used cell, which tm cannot know after an erase | C5b `tests/tmux_cli_flags.rs::panes_capture_pane_rejects_flags_it_does_not_implement` | internal |
| `clear-history` | `-t` | match | target pane; the capture keeps only the visible screen afterwards | C5b `tests/tmux_cli_flags.rs::panes_clear_history_flags_match_the_reference` | differential |
| `clear-history` | `-H` | rejected | also reset hyperlinks | C5b `tests/tmux_cli_flags.rs::panes_clear_history_rejects_flags_it_does_not_implement` | internal |
| `set-buffer` | `(data)` | match | store the argument as a new automatic buffer; empty data stores nothing and no data is an error; buffer names with control characters are invalid and backslashes are doubled | C6a `tests/tmux_cli_flags.rs::buffers_set_buffer_flags_match_the_reference`; C6a `tests/tmux_cli_flags.rs::buffers_set_buffer_without_buffers_matches_the_reference` | differential |
| `set-buffer` | `-a` | match | append to the buffer named by `-b`; without `-b` or an existing buffer it creates one | C6a `tests/tmux_cli_flags.rs::buffers_set_buffer_flags_match_the_reference` | differential |
| `set-buffer` | `-b` | match | buffer name; replaces a buffer of that name | C6a `tests/tmux_cli_flags.rs::buffers_set_buffer_flags_match_the_reference` | differential |
| `set-buffer` | `-n` | match | rename the `-b` buffer, or the newest automatic buffer, and make it a named buffer | C6a `tests/tmux_cli_flags.rs::buffers_set_buffer_flags_match_the_reference`; C6a `tests/tmux_cli_flags.rs::buffers_set_buffer_without_buffers_matches_the_reference` | differential |
| `set-buffer` | `-t` | rejected | target client for `-w` | C6a `tests/tmux_cli_flags.rs::buffers_set_buffer_rejects_flags_it_does_not_implement` | internal |
| `set-buffer` | `-w` | rejected | also set the client clipboard | C6a `tests/tmux_cli_flags.rs::buffers_set_buffer_rejects_flags_it_does_not_implement` | internal |
| `set-buffer` | `(any other flag)` | rejected | tmux 3.8 defines only `-a`, `-b`, `-n`, `-t`, and `-w` | C6a `tests/tmux_cli_flags.rs::buffers_commands_reject_flags_tmux_does_not_define` | internal |
| `show-buffer` | `-b` | match | buffer name; the default is the newest automatic buffer; the data is written without a trailing newline | C6a `tests/tmux_cli_flags.rs::buffers_show_buffer_flags_match_the_reference` | differential |
| `show-buffer` | `(non-UTF-8 data)` | tm-only | tm replaces bytes that are not UTF-8 with U+FFFD; tmux writes the bytes unchanged | C6a `tests/tmux_cli_flags.rs::buffers_show_buffer_replaces_non_utf8_bytes_in_tm` | internal |
| `show-buffer` | `(any other flag)` | rejected | tmux 3.8 defines only `-b` | C6a `tests/tmux_cli_flags.rs::buffers_commands_reject_flags_tmux_does_not_define` | internal |
| `list-buffers` | `-F` | match | output format; the default is `#{buffer_name}: #{buffer_size} bytes: "#{buffer_sample}"` with control bytes shown as escapes and samples cut at 200 bytes | C6a `tests/tmux_cli_flags.rs::buffers_list_buffers_flags_match_the_reference` | differential |
| `list-buffers` | `-f` | match | format filter; every value except an empty string and `0` is true | C6a `tests/tmux_cli_flags.rs::buffers_list_buffers_flags_match_the_reference` | differential |
| `list-buffers` | `-O` | rejected | sort order | C6a `tests/tmux_cli_flags.rs::buffers_list_buffers_rejects_flags_it_does_not_implement` | internal |
| `list-buffers` | `-r` | rejected | reverse the sort order | C6a `tests/tmux_cli_flags.rs::buffers_list_buffers_rejects_flags_it_does_not_implement` | internal |
| `list-buffers` | `-q` | rejected | not a tmux 3.8 flag (tm formerly ignored it) | C6a `tests/tmux_cli_flags.rs::buffers_list_buffers_rejects_flags_it_does_not_implement` | internal |
| `list-buffers` | `(any other flag)` | rejected | tmux 3.8 defines only `-F`, `-f`, `-O`, and `-r` | C6a `tests/tmux_cli_flags.rs::buffers_commands_reject_flags_tmux_does_not_define` | internal |
| `delete-buffer` | `-b` | match | buffer name; the default is the newest automatic buffer | C6a `tests/tmux_cli_flags.rs::buffers_delete_buffer_flags_match_the_reference` | differential |
| `delete-buffer` | `(any other flag)` | rejected | tmux 3.8 defines only `-b` | C6a `tests/tmux_cli_flags.rs::buffers_commands_reject_flags_tmux_does_not_define` | internal |
| `load-buffer` | `-b` | match | buffer name; an empty file stores nothing | C6a `tests/tmux_cli_flags.rs::buffers_load_buffer_flags_match_the_reference` | differential |
| `load-buffer` | `-t` | rejected | target client for `-w` | C6a `tests/tmux_cli_flags.rs::buffers_load_buffer_rejects_flags_it_does_not_implement` | internal |
| `load-buffer` | `-w` | rejected | also set the client clipboard | C6a `tests/tmux_cli_flags.rs::buffers_load_buffer_rejects_flags_it_does_not_implement` | internal |
| `load-buffer` | `(path)` | tm-only | tmux expands formats in the path; tm uses the path as written | C6a `tests/tmux_cli_flags.rs::buffers_load_and_save_paths_are_not_format_expanded_in_tm` | internal |
| `load-buffer` | `(any other flag)` | rejected | tmux 3.8 defines only `-b`, `-t`, and `-w` | C6a `tests/tmux_cli_flags.rs::buffers_commands_reject_flags_tmux_does_not_define` | internal |
| `save-buffer` | `-a` | match | append to the file | C6a `tests/tmux_cli_flags.rs::buffers_save_buffer_flags_match_the_reference` | differential |
| `save-buffer` | `-b` | match | buffer name; the default is the newest automatic buffer | C6a `tests/tmux_cli_flags.rs::buffers_save_buffer_flags_match_the_reference` | differential |
| `save-buffer` | `(path)` | tm-only | tmux expands formats in the path; tm uses the path as written | C6a `tests/tmux_cli_flags.rs::buffers_load_and_save_paths_are_not_format_expanded_in_tm` | internal |
| `save-buffer` | `(any other flag)` | rejected | tmux 3.8 defines only `-a` and `-b` | C6a `tests/tmux_cli_flags.rs::buffers_save_buffer_rejects_flags_it_does_not_implement` | internal |
| `paste-buffer` | `-b` | match | buffer name; a missing buffer is an error and no buffer at all pastes nothing | C6a `tests/tmux_cli_flags.rs::buffers_paste_buffer_flags_match_the_reference`; C6a `tests/tmux_cli_flags.rs::buffers_paste_buffer_without_buffers_matches_the_reference` | differential |
| `paste-buffer` | `-d` | match | delete the buffer after pasting | C6a `tests/tmux_cli_flags.rs::buffers_paste_buffer_flags_match_the_reference` | differential |
| `paste-buffer` | `-p` | match | bracketed paste when the pane enabled it | C6a `tests/tmux_cli_flags.rs::buffers_paste_buffer_flags_match_the_reference` | differential |
| `paste-buffer` | `-r` | match | separate lines with a line feed instead of a carriage return | C6a `tests/tmux_cli_flags.rs::buffers_paste_buffer_flags_match_the_reference` | differential |
| `paste-buffer` | `-s` | match | line separator; wins over `-r` | C6a `tests/tmux_cli_flags.rs::buffers_paste_buffer_flags_match_the_reference` | differential |
| `paste-buffer` | `-t` | match | target pane | C6a `tests/tmux_cli_flags.rs::buffers_paste_buffer_flags_match_the_reference` | differential |
| `paste-buffer` | `(control bytes)` | match | control characters other than bell, backspace, tab, and carriage return are pasted as `^X` | C6a `tests/tmux_cli_flags.rs::buffers_paste_buffer_escapes_control_bytes_like_the_reference` | differential |
| `paste-buffer` | `-S` | rejected | paste without escaping control characters | C6a `tests/tmux_cli_flags.rs::buffers_paste_buffer_rejects_flags_it_does_not_implement` | internal |
| `paste-buffer` | `(any other flag)` | rejected | tmux 3.8 defines only `-d`, `-b`, `-p`, `-r`, `-S`, `-s`, and `-t` | C6a `tests/tmux_cli_flags.rs::buffers_commands_reject_flags_tmux_does_not_define` | internal |
| `send-keys` | `-l` | match | send the arguments as text, concatenated without separators, instead of looking up key names | C6a `tests/tmux_cli_flags.rs::keys_send_keys_key_names_and_literals_match_the_reference` | differential |
| `send-keys` | `-N` | match | repeat the keys; a count below 1, above 4294967295, or not a number is an error | C6a `tests/tmux_cli_flags.rs::keys_send_keys_repeat_count_matches_the_reference` | differential |
| `send-keys` | `-R` | match | reset the pane terminal state before sending | C6a `tests/tmux_cli_flags.rs::keys_send_keys_reset_matches_the_reference` | differential |
| `send-keys` | `-t` | match | target pane | C6a `tests/tmux_cli_flags.rs::keys_send_keys_key_names_and_literals_match_the_reference` | differential |
| `send-keys` | `-X` | match | run a copy-mode command; outside a mode it fails with `not in a mode`; `-C` and `-P` after the command are copy-mode command flags | C6a `tests/tmux_cli_flags.rs::keys_send_keys_copy_mode_command_matches_the_reference`; C6a `tests/tmux_cli_flags.rs::keys_send_keys_copy_selection_flags_match_the_reference` | differential |
| `send-keys` | `-X (unknown command)` | tm-only | tm fails with `unknown copy-mode command`; tmux ignores a name it does not know | C6a `tests/tmux_cli_flags.rs::keys_send_keys_unknown_copy_mode_command_fails_in_tm` | internal |
| `send-keys` | `-t (after the -X command)` | tm-only | tm reads `-t` after the copy-mode command as the target pane; tmux passes it to the command | C6a `tests/tmux_cli_flags.rs::keys_send_keys_copy_mode_command_matches_the_reference` | internal |
| `send-keys` | `(key names)` | tm-only | tm sends the names Enter, Tab, Space, Escape, the arrow keys, BTab, and `C-` and `M-` letters like tmux and types other names as text; tmux also knows BSpace, DC, IC, Home, End, the page keys, function keys, keypad keys, and modified named keys | C6a `tests/tmux_cli_flags.rs::keys_send_keys_key_names_and_literals_match_the_reference` | internal |
| `send-keys` | `-c` | rejected | target client | C6a `tests/tmux_cli_flags.rs::keys_send_keys_rejects_flags_it_does_not_implement` | internal |
| `send-keys` | `-F` | rejected | expand formats in the arguments | C6a `tests/tmux_cli_flags.rs::keys_send_keys_rejects_flags_it_does_not_implement` | internal |
| `send-keys` | `-H` | rejected | send arguments as hexadecimal bytes | C6a `tests/tmux_cli_flags.rs::keys_send_keys_rejects_flags_it_does_not_implement` | internal |
| `send-keys` | `-K` | rejected | send keys to the client | C6a `tests/tmux_cli_flags.rs::keys_send_keys_rejects_flags_it_does_not_implement` | internal |
| `send-keys` | `-M` | rejected | send the mouse event | C6a `tests/tmux_cli_flags.rs::keys_send_keys_rejects_flags_it_does_not_implement` | internal |
| `send-keys` | `-C` | rejected | not a tmux 3.8 send-keys flag (tm formerly accepted it after `-X`; it is now a copy-mode command flag) | C6a `tests/tmux_cli_flags.rs::keys_send_keys_rejects_flags_it_does_not_implement` | internal |
| `send-keys` | `-P` | rejected | not a tmux 3.8 send-keys flag (tm formerly accepted it after `-X`; it is now a copy-mode command flag) | C6a `tests/tmux_cli_flags.rs::keys_send_keys_rejects_flags_it_does_not_implement` | internal |
| `send-prefix` | `-t` | match | target pane | C6a `tests/tmux_cli_flags.rs::keys_send_prefix_flags_match_the_reference` | differential |
| `send-prefix` | `-2` | rejected | send the secondary prefix; tm has no `prefix2` | C6a `tests/tmux_cli_flags.rs::keys_send_prefix_rejects_the_secondary_prefix` | internal |
| `display-message` | `-p` | match | print the message on standard output (row `(without -p)` covers the other case); tmux without it shows the message on an attached client and prints nothing otherwise | C6a `tests/tmux_cli_flags.rs::display_display_message_flags_match_the_reference` | differential |
| `display-message` | `(message)` | match | format expanded after `strftime`; with no message and no `-F` the default template `[#{session_name}] #{window_index}:#{window_name}, current pane #{pane_index} - (%H:%M %d-%b-%y)` | C6a `tests/tmux_cli_flags.rs::display_display_message_flags_match_the_reference`; C6a `tests/tmux_cli_flags.rs::display_display_message_default_template_matches_the_reference` | differential |
| `display-message` | `-F` | match | format to expand; combined with a message it is an error | C6a `tests/tmux_cli_flags.rs::display_display_message_flags_match_the_reference` | differential |
| `display-message` | `-l` | match | print the message without expanding formats | C6a `tests/tmux_cli_flags.rs::display_display_message_flags_match_the_reference` | differential |
| `display-message` | `-t` | match | target pane | C6a `tests/tmux_cli_flags.rs::display_display_message_flags_match_the_reference` | differential |
| `display-message` | `-t (unresolved target)` | tm-only | tmux falls back to the current session, window, or pane (or to empty values for an unknown session or pane id); tm fails with `can't find ...` | C6a `tests/tmux_cli_flags.rs::display_display_message_unresolved_targets_fail_in_tm` | internal |
| `display-message` | `(without -p)` | tm-only | tmux shows the message on the status line of an attached client; tm has no status messages and prints nothing | C6a `tests/tmux_cli_flags.rs::display_display_message_without_p_prints_nothing_in_tm` | internal |
| `display-message` | `-a` | rejected | list every format variable | C6a `tests/tmux_cli_flags.rs::display_display_message_rejects_flags_it_does_not_implement` | internal |
| `display-message` | `-C` | rejected | status message without freezing the screen | C6a `tests/tmux_cli_flags.rs::display_display_message_rejects_flags_it_does_not_implement` | internal |
| `display-message` | `-c` | rejected | target client | C6a `tests/tmux_cli_flags.rs::display_display_message_rejects_flags_it_does_not_implement` | internal |
| `display-message` | `-d` | rejected | status message delay | C6a `tests/tmux_cli_flags.rs::display_display_message_rejects_flags_it_does_not_implement` | internal |
| `display-message` | `-I` | rejected | forward pane input | C6a `tests/tmux_cli_flags.rs::display_display_message_rejects_flags_it_does_not_implement` | internal |
| `display-message` | `-j` | rejected | print JSON | C6a `tests/tmux_cli_flags.rs::display_display_message_rejects_flags_it_does_not_implement` | internal |
| `display-message` | `-N` | rejected | ignore key presses on the status message | C6a `tests/tmux_cli_flags.rs::display_display_message_rejects_flags_it_does_not_implement` | internal |
| `display-message` | `-v` | rejected | print the format expansion trace (tm formerly ignored it) | C6a `tests/tmux_cli_flags.rs::display_display_message_rejects_flags_it_does_not_implement` | internal |
| `run-shell` | `(command)` | match | run through `/bin/sh -c` in the client directory after format expansion; output is printed a line at a time, a nonzero status adds `'cmd' returned N` and becomes the exit status, a signal adds `terminated by signal N`; stderr is dropped | C6a `tests/tmux_cli_flags.rs::display_run_shell_flags_match_the_reference` | differential |
| `run-shell` | `-b` | tm-only | return immediately and run the command in the background, as tmux does; tmux shows the command's output in a view-mode pane, tm drops it | C6a `tests/tmux_cli_flags.rs::display_run_shell_flags_match_the_reference`; C6a `tests/tmux_cli_flags.rs::display_run_shell_delay_waits_in_both` | differential |
| `run-shell` | `-d` | match | wait this many seconds before running; a value that is not a number is an error | C6a `tests/tmux_cli_flags.rs::display_run_shell_flags_match_the_reference`; C6a `tests/tmux_cli_flags.rs::display_run_shell_delay_waits_in_both` | differential |
| `run-shell` | `-s` | match | accepted and ignored | C6a `tests/tmux_cli_flags.rs::display_run_shell_flags_match_the_reference` | differential |
| `run-shell` | `-t` | tm-only | pane for format expansion and exit status match tmux; tmux shows the output in a view-mode pane, tm drops it | C6a `tests/tmux_cli_flags.rs::display_run_shell_flags_match_the_reference` | differential |
| `run-shell` | `(extra arguments)` | tm-only | tm joins every argument with spaces; tmux runs only the first and exposes the rest as `#{1}`, `#{2}` | C6a `tests/tmux_cli_flags.rs::display_run_shell_joins_arguments_and_fails_unresolved_targets_in_tm` | internal |
| `run-shell` | `-t (unresolved target)` | tm-only | tmux prints to standard output when the target is unresolved; tm fails with `can't find ...` | C6a `tests/tmux_cli_flags.rs::display_run_shell_joins_arguments_and_fails_unresolved_targets_in_tm` | internal |
| `run-shell` | `-C` | rejected | run the argument as a tmux command | C6a `tests/tmux_cli_flags.rs::display_run_shell_rejects_flags_it_does_not_implement` | internal |
| `run-shell` | `-E` | rejected | show standard error | C6a `tests/tmux_cli_flags.rs::display_run_shell_rejects_flags_it_does_not_implement` | internal |
| `run-shell` | `-c` | rejected | start directory | C6a `tests/tmux_cli_flags.rs::display_run_shell_rejects_flags_it_does_not_implement` | internal |
| `run-shell` | `(any other flag)` | rejected | tmux 3.8 defines only `-b`, `-d`, `-C`, `-t`, `-E`, `-s`, and `-c` | C6a `tests/tmux_cli_flags.rs::display_run_shell_rejects_flags_it_does_not_implement` | internal |
| `set-option` | `-g` | match | use the global table of the option's level (session or window); server options ignore it | C6b `tests/tmux_cli_flags.rs::options_set_option_flags_match_the_reference`; C6b `tests/tmux_cli_flags.rs::options_session_window_and_pane_tables_are_separate` | differential |
| `set-option` | `-p` | match | set a pane option; only options with window and pane scope take it, others use their own table | C6b `tests/tmux_cli_flags.rs::options_set_option_flags_match_the_reference`; C6b `tests/tmux_cli_flags.rs::options_session_window_and_pane_tables_are_separate` | differential |
| `set-option` | `-q` | match | suppress unknown-option, ambiguous-option, and missing-target errors; value errors still print | C6b `tests/tmux_cli_flags.rs::options_set_option_flags_match_the_reference` | differential |
| `set-option` | `-s` | match | selects the server table only for `@` user options; named options use the table of their level | C6b `tests/tmux_cli_flags.rs::options_set_option_flags_match_the_reference`; C6b `tests/tmux_cli_flags.rs::options_session_window_and_pane_tables_are_separate` | differential |
| `set-option` | `-t` | match | target pane, window, or session by option level; an unresolved target fails with `no such session`, `no such window`, or `no such pane` | C6b `tests/tmux_cli_flags.rs::options_set_option_flags_match_the_reference`; C6b `tests/tmux_cli_flags.rs::options_session_window_and_pane_tables_are_separate` | differential |
| `set-option` | `-u` | match | remove the option from the target table; a user option may be removed globally | C6b `tests/tmux_cli_flags.rs::options_set_option_flags_match_the_reference`; C6b `tests/tmux_cli_flags.rs::options_session_window_and_pane_tables_are_separate` | differential |
| `set-option` | `-w` | match | selects the window table only for `@` user options; named options use the table of their level | C6b `tests/tmux_cli_flags.rs::options_set_option_flags_match_the_reference`; C6b `tests/tmux_cli_flags.rs::options_session_window_and_pane_tables_are_separate` | differential |
| `set-option` | `(option name)` | match | an exact name, a unique prefix, or a renamed colour spelling; an unknown or ambiguous name fails with `invalid option` or `ambiguous option` | C6b `tests/tmux_cli_flags.rs::options_set_option_flags_match_the_reference` | differential |
| `set-option` | `(value)` | match | flags accept `1`/`0`/`on`/`off`/`yes`/`no` in any case and toggle without a value; numbers and choices fail with tmux's `value is invalid`, `value is too small`, `value is too large`, `unknown value`, `bad value`, and `empty value`; style options fail with `invalid style`, colour options with `invalid colour` (`bad colour` for `status-bg` and `status-fg`, which store the normalized name), and values containing `#{` skip validation | C6b `tests/tmux_cli_flags.rs::options_values_are_validated_and_normalized_like_the_reference`; C6b.1 `tests/tmux_cli_flags.rs::options_style_and_colour_values_are_validated_like_the_reference` | differential |
| `set-option` | `(unsupported option)` | tm-only | tm stores and acts on the options in its compiled profile; for a tmux 3.8 option outside that set, `show-options` fails with `unsupported option` where tmux prints the default | C6b `tests/tmux_cli_flags.rs::options_names_tm_does_not_implement_are_errors_in_tm` | differential |
| `set-option` | `(array element name[n])` | tm-only | tmux addresses one element of an array option; tm fails with `unsupported array element` | C6b `tests/tmux_cli_flags.rs::options_names_tm_does_not_implement_are_errors_in_tm` | differential |
| `set-option` | `-u (global option)` | tm-only | tmux resets a global option to its table default; tm has no defaults outside its profile, so it fails with `unsupported reset of global option` | C6b `tests/tmux_cli_flags.rs::options_names_tm_does_not_implement_are_errors_in_tm` | differential |
| `set-option` | `-a` | rejected | append to a string or array option | C6b `tests/tmux_cli_flags.rs::options_rejects_flags_it_does_not_implement` | internal |
| `set-option` | `-o` | rejected | fail when the option is already set | C6b `tests/tmux_cli_flags.rs::options_rejects_flags_it_does_not_implement` | internal |
| `set-option` | `-F` | rejected | expand formats in the value | C6b `tests/tmux_cli_flags.rs::options_rejects_flags_it_does_not_implement` | internal |
| `set-option` | `-U` | rejected | also unset the option on every pane of the window | C6b `tests/tmux_cli_flags.rs::options_rejects_flags_it_does_not_implement` | internal |
| `set-option` | `(any other flag)` | rejected | tmux 3.8 defines only `-aFgopqstuUw` | C6b `tests/tmux_cli_flags.rs::options_rejects_flags_it_does_not_implement` | internal |
| `set-window-option` | `-g` | match | use the global window table | C6b `tests/tmux_cli_flags.rs::options_set_window_option_flags_match_the_reference` | differential |
| `set-window-option` | `-q` | match | suppress unknown-option and missing-target errors | C6b `tests/tmux_cli_flags.rs::options_set_window_option_flags_match_the_reference` | differential |
| `set-window-option` | `-t` | match | target window or pane; an unresolved target fails with `no such window` | C6b `tests/tmux_cli_flags.rs::options_set_window_option_flags_match_the_reference` | differential |
| `set-window-option` | `-u` | match | remove the option from the target window | C6b `tests/tmux_cli_flags.rs::options_set_window_option_flags_match_the_reference` | differential |
| `set-window-option` | `(option name and value)` | match | same names, tables, and value rules as `set-option` | C6b `tests/tmux_cli_flags.rs::options_set_window_option_flags_match_the_reference`; C6b `tests/tmux_cli_flags.rs::options_values_are_validated_and_normalized_like_the_reference` | differential |
| `set-window-option` | `-a` | rejected | append to a string or array option | C6b `tests/tmux_cli_flags.rs::options_rejects_flags_it_does_not_implement` | internal |
| `set-window-option` | `-o` | rejected | fail when the option is already set | C6b `tests/tmux_cli_flags.rs::options_rejects_flags_it_does_not_implement` | internal |
| `set-window-option` | `-F` | rejected | expand formats in the value | C6b `tests/tmux_cli_flags.rs::options_rejects_flags_it_does_not_implement` | internal |
| `set-window-option` | `(any other flag)` | rejected | tmux 3.8 defines only `-aFgoqtu` | C6b `tests/tmux_cli_flags.rs::options_rejects_flags_it_does_not_implement` | internal |
| `show-options` | `-A` | match | include inherited values from the parent table, marked with a `*` after the name | C6b `tests/tmux_cli_flags.rs::options_show_options_flags_match_the_reference`; C6b `tests/tmux_cli_flags.rs::options_session_window_and_pane_tables_are_separate` | differential |
| `show-options` | `-g` | match | show the global table of the option's level | C6b `tests/tmux_cli_flags.rs::options_show_options_flags_match_the_reference`; C6b `tests/tmux_cli_flags.rs::options_session_window_and_pane_tables_are_separate` | differential |
| `show-options` | `-p` | match | show a pane table; only options with window and pane scope take it | C6b `tests/tmux_cli_flags.rs::options_show_options_flags_match_the_reference`; C6b `tests/tmux_cli_flags.rs::options_session_window_and_pane_tables_are_separate` | differential |
| `show-options` | `-q` | match | suppress unknown-option, ambiguous-option, and missing-target errors | C6b `tests/tmux_cli_flags.rs::options_show_options_flags_match_the_reference` | differential |
| `show-options` | `-s` | match | show the server table; a named option uses the table of its level | C6b `tests/tmux_cli_flags.rs::options_show_options_flags_match_the_reference`; C6b `tests/tmux_cli_flags.rs::options_session_window_and_pane_tables_are_separate` | differential |
| `show-options` | `-t` | match | target pane, window, or session by option level; an unresolved target fails with `no such session`, `no such window`, or `no such pane` | C6b `tests/tmux_cli_flags.rs::options_show_options_flags_match_the_reference`; C6b `tests/tmux_cli_flags.rs::options_session_window_and_pane_tables_are_separate` | differential |
| `show-options` | `-v` | match | print only the value; an empty value prints an empty line | C6b `tests/tmux_cli_flags.rs::options_show_options_flags_match_the_reference`; C6b `tests/tmux_cli_flags.rs::options_values_are_validated_and_normalized_like_the_reference` | differential |
| `show-options` | `-w` | match | show the window table | C6b `tests/tmux_cli_flags.rs::options_show_options_flags_match_the_reference`; C6b `tests/tmux_cli_flags.rs::options_session_window_and_pane_tables_are_separate` | differential |
| `show-options` | `(option name)` | match | a name prints the line `name value` quoted like tmux's argument escaping, only when the option is set in the selected table | C6b `tests/tmux_cli_flags.rs::options_show_options_flags_match_the_reference`; C6b `tests/tmux_cli_flags.rs::options_show_options_quotes_values_like_the_reference`; C6b `tests/tmux_cli_flags.rs::options_compiled_values_match_the_reference` | differential |
| `show-options` | `(list without a name)` | tm-only | tm lists only the options it stores, so the listing is a subset of tmux's and has no array elements for options tm lacks | C6b `tests/tmux_cli_flags.rs::options_listings_hold_only_values_the_reference_also_lists` | differential |
| `show-options` | `-F` | rejected | format for each option line | C6b `tests/tmux_cli_flags.rs::options_rejects_flags_it_does_not_implement` | internal |
| `show-options` | `-H` | rejected | include hooks | C6b `tests/tmux_cli_flags.rs::options_rejects_flags_it_does_not_implement` | internal |
| `show-options` | `(any other flag)` | rejected | tmux 3.8 defines only `-AgFHpqstvw` | C6b `tests/tmux_cli_flags.rs::options_rejects_flags_it_does_not_implement` | internal |
| `show-window-options` | `-g` | match | show the global window table | C6b `tests/tmux_cli_flags.rs::options_set_window_option_flags_match_the_reference` | differential |
| `show-window-options` | `-t` | match | target window or pane; an unresolved target fails with `no such window` | C6b `tests/tmux_cli_flags.rs::options_set_window_option_flags_match_the_reference` | differential |
| `show-window-options` | `-v` | match | print only the value | C6b `tests/tmux_cli_flags.rs::options_set_window_option_flags_match_the_reference` | differential |
| `show-window-options` | `(list without a name)` | tm-only | tm lists only the options it stores | C6b `tests/tmux_cli_flags.rs::options_listings_hold_only_values_the_reference_also_lists` | differential |
| `show-window-options` | `-F` | rejected | format for each option line | C6b `tests/tmux_cli_flags.rs::options_rejects_flags_it_does_not_implement` | internal |
| `show-window-options` | `(any other flag)` | rejected | tmux 3.8 defines only `-Fgtv` | C6b `tests/tmux_cli_flags.rs::options_rejects_flags_it_does_not_implement` | internal |
| `set-environment` | `-g` | match | set in the global environment instead of the session environment | C6b `tests/tmux_cli_flags.rs::environment_set_and_show_environment_flags_match_the_reference`; C6b `tests/tmux_cli_flags.rs::environment_spawned_panes_see_global_and_session_variables_like_the_reference` | differential |
| `set-environment` | `-r` | match | mark the variable cleared: it is listed as `-NAME` and panes started afterwards do not receive it | C6b `tests/tmux_cli_flags.rs::environment_set_and_show_environment_flags_match_the_reference`; C6b `tests/tmux_cli_flags.rs::environment_session_listing_matches_the_reference_except_update_environment`; C6b `tests/tmux_cli_flags.rs::environment_spawned_panes_see_global_and_session_variables_like_the_reference` | differential |
| `set-environment` | `-t` | match | target session; an unresolved session fails with `no such session` | C6b `tests/tmux_cli_flags.rs::environment_set_and_show_environment_flags_match_the_reference`; C6b `tests/tmux_cli_flags.rs::environment_session_listing_matches_the_reference_except_update_environment` | differential |
| `set-environment` | `-u` | match | remove the variable; panes started afterwards inherit nothing for it | C6b `tests/tmux_cli_flags.rs::environment_set_and_show_environment_flags_match_the_reference`; C6b `tests/tmux_cli_flags.rs::environment_spawned_panes_see_global_and_session_variables_like_the_reference` | differential |
| `set-environment` | `(variable and value)` | match | an empty name, a name containing `=`, a missing value, or a value with `-u` or `-r` fails with tmux's text; panes see the global environment overlaid by the session environment | C6b `tests/tmux_cli_flags.rs::environment_set_and_show_environment_flags_match_the_reference`; C6b `tests/tmux_cli_flags.rs::environment_spawned_panes_see_global_and_session_variables_like_the_reference` | differential |
| `set-environment` | `-F` | rejected | expand formats in the value | C6b `tests/tmux_cli_flags.rs::environment_rejects_flags_it_does_not_implement` | internal |
| `set-environment` | `-h` | rejected | hidden variable | C6b `tests/tmux_cli_flags.rs::environment_rejects_flags_it_does_not_implement` | internal |
| `set-environment` | `(any other flag)` | rejected | tmux 3.8 defines only `-Fghrtu` | C6b `tests/tmux_cli_flags.rs::environment_rejects_flags_it_does_not_implement` | internal |
| `show-environment` | `-g` | match | show the global environment | C6b `tests/tmux_cli_flags.rs::environment_set_and_show_environment_flags_match_the_reference`; C6b `tests/tmux_cli_flags.rs::environment_session_listing_matches_the_reference_except_update_environment` | differential |
| `show-environment` | `-s` | match | print `NAME="value"; export NAME;` and `unset NAME;` lines with `$`, `` ` ``, `"`, and `\` escaped | C6b `tests/tmux_cli_flags.rs::environment_set_and_show_environment_flags_match_the_reference`; C6b `tests/tmux_cli_flags.rs::environment_session_listing_matches_the_reference_except_update_environment` | differential |
| `show-environment` | `-t` | match | target session; an unresolved session fails with `no such session` | C6b `tests/tmux_cli_flags.rs::environment_set_and_show_environment_flags_match_the_reference`; C6b `tests/tmux_cli_flags.rs::environment_session_listing_matches_the_reference_except_update_environment` | differential |
| `show-environment` | `(variable)` | match | an unset variable fails with `unknown variable` | C6b `tests/tmux_cli_flags.rs::environment_set_and_show_environment_flags_match_the_reference` | differential |
| `show-environment` | `(session listing)` | tm-only | tmux seeds each new session with `update-environment` entries such as `-DISPLAY`; tm has no `update-environment`, so those lines and variables are absent | C6b `tests/tmux_cli_flags.rs::environment_session_listing_matches_the_reference_except_update_environment` | differential |
| `show-environment` | `-h` | rejected | hidden variables | C6b `tests/tmux_cli_flags.rs::environment_rejects_flags_it_does_not_implement` | internal |
| `show-environment` | `(any other flag)` | rejected | tmux 3.8 defines only `-ghst` | C6b `tests/tmux_cli_flags.rs::environment_rejects_flags_it_does_not_implement` | internal |
| `copy-mode` | `-t` | match | target pane | C6c `tests/tmux_cli_flags.rs::modes_copy_mode_flags_match_the_reference` | differential |
| `copy-mode` | `-q` | match | leave every mode of the pane | C6c `tests/tmux_cli_flags.rs::modes_copy_mode_flags_match_the_reference` | differential |
| `copy-mode` | `-u` | match | enter and scroll up one page, which is the pane height minus 2 | C6c `tests/tmux_cli_flags.rs::modes_copy_mode_flags_match_the_reference`; C6c `tests/tmux_cli_flags.rs::modes_copy_mode_scroll_exit_and_paging_match_the_reference` | differential |
| `copy-mode` | `-d` | match | scroll down one page; at the bottom with `-e` the mode ends | C6c `tests/tmux_cli_flags.rs::modes_copy_mode_flags_match_the_reference`; C6c `tests/tmux_cli_flags.rs::modes_copy_mode_scroll_exit_and_paging_match_the_reference` | differential |
| `copy-mode` | `-e` | match | leave the mode when scrolling down reaches the bottom | C6c `tests/tmux_cli_flags.rs::modes_copy_mode_flags_match_the_reference`; C6c `tests/tmux_cli_flags.rs::modes_copy_mode_scroll_exit_and_paging_match_the_reference` | differential |
| `copy-mode` | `-H` | match | hide the position indicator; observed as the `[0/n]` text on the pane's top row | C6c `tests/tmux_cli_flags.rs::modes_copy_mode_hide_position_matches_the_reference` | differential |
| `copy-mode` | `-s` | match | show the source pane's content in the target pane | C6c `tests/tmux_cli_flags.rs::modes_copy_mode_flags_match_the_reference` | differential |
| `copy-mode` | `-k` | rejected | accepted by tmux 3.8 and ignored; tm's kill-pane-on-exit meaning differs | C6c `tests/tmux_cli_flags.rs::modes_copy_mode_rejects_flags_it_does_not_implement` | internal |
| `copy-mode` | `-M` | rejected | start the mode from a mouse event; no compiled binding uses it | C6c `tests/tmux_cli_flags.rs::modes_copy_mode_rejects_flags_it_does_not_implement` | internal |
| `copy-mode` | `-S` | rejected | scroll to the mouse position; no compiled binding uses it | C6c `tests/tmux_cli_flags.rs::modes_copy_mode_rejects_flags_it_does_not_implement` | internal |
| `copy-mode-and-page` | `(command)` | tm-only | tm adds this command; it is `copy-mode -u` with the same flags and rejections, and tmux 3.8 reports an unknown command | C6c `tests/tmux_cli_flags.rs::modes_tm_only_commands_and_aliases_are_unknown_to_the_reference` | internal |
| `choose-tree` | `(no flags)` | tm-only | tmux opens the tree mode on the target pane and works without a client; tm opens its own list on the attached client and fails with `requires an attached client` otherwise | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_open_a_client_mode_in_tm` | internal |
| `choose-tree` | `-t` | tm-only | tmux targets the pane that gets the mode; tm uses the target to find the client attached to that session | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_open_a_client_mode_in_tm` | internal |
| `choose-tree` | `template` | rejected | command run at the selection | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-tree` | `choose` | tm-only | alias that tmux 3.8 lacks (ambiguous or unknown there) | C6c `tests/tmux_cli_flags.rs::modes_tm_only_commands_and_aliases_are_unknown_to_the_reference` | internal |
| `choose-tree` | `-F` | rejected | format of each line | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-tree` | `-f` | rejected | filter expression | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-tree` | `-G` | rejected | include all sessions or all windows | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-tree` | `-h` | rejected | hide the source pane | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-tree` | `-K` | rejected | key format for the shortcut column | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-tree` | `-k` | rejected | kill the pane on exit | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-tree` | `-N` | rejected | start without the preview | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-tree` | `-O` | rejected | sort order | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-tree` | `-r` | rejected | reverse the sort order | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-tree` | `-s` | rejected | start collapsed to sessions | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-tree` | `-w` | rejected | start collapsed to windows | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-tree` | `-y` | rejected | run the template without confirmation | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-tree` | `-Z` | rejected | zoom the preview | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-buffer` | `(no flags)` | tm-only | tmux opens the buffer mode on the target pane and works without a client; tm opens its own list on the attached client and fails with `requires an attached client` otherwise | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_open_a_client_mode_in_tm` | internal |
| `choose-buffer` | `-t` | tm-only | tmux targets the pane that gets the mode; tm uses the target to find the client attached to that session | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_open_a_client_mode_in_tm` | internal |
| `choose-buffer` | `template` | rejected | command run at the selection | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-buffer` | `chooseb` | tm-only | alias that tmux 3.8 lacks (ambiguous or unknown there) | C6c `tests/tmux_cli_flags.rs::modes_tm_only_commands_and_aliases_are_unknown_to_the_reference` | internal |
| `choose-buffer` | `-F` | rejected | format of each line | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-buffer` | `-f` | rejected | filter expression | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-buffer` | `-K` | rejected | key format for the shortcut column | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-buffer` | `-k` | rejected | kill the pane on exit | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-buffer` | `-N` | rejected | start without the preview | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-buffer` | `-O` | rejected | sort order | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-buffer` | `-r` | rejected | reverse the sort order | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-buffer` | `-y` | rejected | run the template without confirmation | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-buffer` | `-Z` | rejected | zoom the preview | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-client` | `(no flags)` | tm-only | tmux opens the client mode on the target pane and works without a client; tm opens its own list on the attached client and fails with `requires an attached client` otherwise | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_open_a_client_mode_in_tm` | internal |
| `choose-client` | `-t` | tm-only | tmux targets the pane that gets the mode; tm uses the target to find the client attached to that session | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_open_a_client_mode_in_tm` | internal |
| `choose-client` | `template` | rejected | command run at the selection | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-client` | `choosec` | tm-only | alias that tmux 3.8 lacks (ambiguous or unknown there) | C6c `tests/tmux_cli_flags.rs::modes_tm_only_commands_and_aliases_are_unknown_to_the_reference` | internal |
| `choose-client` | `-F` | rejected | format of each line | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-client` | `-f` | rejected | filter expression | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-client` | `-h` | rejected | hide the source pane | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-client` | `-i` | rejected | include only the attached client's session | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-client` | `-K` | rejected | key format for the shortcut column | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-client` | `-k` | rejected | kill the pane on exit | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-client` | `-N` | rejected | start without the preview | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-client` | `-O` | rejected | sort order | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-client` | `-r` | rejected | reverse the sort order | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-client` | `-y` | rejected | run the template without confirmation | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-client` | `-Z` | rejected | zoom the preview | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-buffer` | `-G` | rejected | not a tmux 3.8 choose-buffer flag | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `choose-client` | `-G` | rejected | not a tmux 3.8 choose-client flag | C6c `tests/tmux_cli_flags.rs::modes_choose_commands_reject_flags_it_does_not_implement` | internal |
| `help` | `(command)` | tm-only | prints tm's usage and exits nonzero; tmux 3.8 reports an unknown command | C6c `tests/tmux_cli_flags.rs::modes_tm_only_commands_and_aliases_are_unknown_to_the_reference` | internal |
| `help` | `-h` | tm-only | prints tm's usage and exits nonzero; tmux prints its own usage | C6c `tests/tmux_cli_flags.rs::modes_tm_only_commands_and_aliases_are_unknown_to_the_reference` | internal |
| `help` | `--help` | tm-only | prints tm's usage and exits nonzero; tmux rejects the option | C6c `tests/tmux_cli_flags.rs::modes_tm_only_commands_and_aliases_are_unknown_to_the_reference` | internal |

## Baseline

Recorded 2026-10-30 from commit `05f4737` before the first product slice.

- `make test`: green, 20 s.
- `make lint`: green, 4 s.
- `make test-ci`: green, 29 s.
- `cargo test --test attached_client_pty`: green, 2 s.
- `make perf`: 28.7 µs (80×24, 1 pane), 32.3 µs (80×24, 2), 76.2 µs (120×40, 4), 327.1 µs (delta, 120×40, 4).
- Reference probe: `tmux 3.8` from the verified tarball, private `-S` socket, ambient multiplexer variables cleared.
