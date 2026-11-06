Prefix: TMUX
Next: 50

### TMUX-11 · P3 · Session format variables are missing or wrong

- **Area:** Formats
- **Where:** `src/server.rs` session format values
- **Problem:** `#{session_group}`, `#{session_created}`, `#{t:session_created}`, and `#{session_width}` expand to nothing in tm, and `#{session_group_size}` is `1` for ungrouped sessions; tmux 3.8 expands them.
- **Recommendation:** Add the values with a `compare_commands` probe of `display-message -p` for each.
- **Fix with:** reference-parity
- **Effort:** S

TMUX-1, TMUX-2, TMUX-3, TMUX-5, TMUX-6, TMUX-7, TMUX-8, TMUX-9, TMUX-10, TMUX-16, TMUX-38, TMUX-39, and TMUX-40 are closed
with tests and commits recorded in git history. Add a new entry here only for a reproduced defect that the current
suites do not cover.

### TMUX-12 · P3 · Default pane_title is empty instead of the hostname

- **Area:** Formats
- **Where:** `src/server.rs` pane title initialisation
- **Problem:** tmux 3.8 sets a new pane's title to the host name; tm leaves `#{pane_title}` empty until the program sets one.
- **Recommendation:** Initialise the title from `gethostname` and add a `display-message -p '#{pane_title}'` probe against the reference.
- **Fix with:** reference-parity

### TMUX-13 · P3 · pane_current_command reports the start command

- **Area:** Formats
- **Where:** `src/server.rs` pane format values
- **Problem:** tm reports the pane's start command; tmux 3.8 reports the pane's foreground process name, so it changes when the shell runs a program.
- **Recommendation:** Read the foreground process group of the pane's PTY (`tcgetpgrp`) and its `/proc/<pid>/comm`; probe with a shell that execs `sleep`.
- **Fix with:** reference-parity

### TMUX-14 · P4 · Last pane is a single slot, not tmux's history

- **Area:** Panes
- **Where:** `src/server.rs` `last_pane`
- **Problem:** After cross-window pane moves, tm keeps one previous pane per window; tmux 3.8 keeps a stack, so `select-pane -l` can pick a different pane after a moved or killed pane.
- **Recommendation:** Replace the slot with a per-window recency list and add a differential history in H2.
- **Fix with:** reference-parity

### TMUX-15 · P3 · resize-pane differs with three or more panes along one axis

- **Area:** Layout
- **Where:** `src/model.rs` `Layout::resize`
- **Problem:** tm lays panes out as a binary tree and moves only the border of the nearest enclosing split. tmux keeps one list of siblings and passes the remaining change on to the next siblings, so `resize-pane -R`, `-L`, `-x` in a row of three panes ends with different widths. The same tree gives a removed middle pane's space to the previous sibling where tmux gives it to the next (`kill-pane`, `break-pane`, `join-pane -s` in a window of three panes). `tests/tmux_histories.rs` `Exclusion::PaneRemovalFromWindowWithThreeOrMorePanes` stops generating them.
- **Recommendation:** Resize over the flattened sibling run of the same axis, with a differential probe for rows of three and four panes.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-17 · P3 · history_size and history_limit formats expand to nothing

- **Area:** Formats
- **Where:** `src/server.rs` pane format values
- **Problem:** `#{history_size}` and `#{history_limit}` are empty in tm; tmux 3.8 expands them to the lines of history and the option value.
- **Recommendation:** Add the values with a `display-message -p` probe after output and after `clear-history`.
- **Fix with:** reference-parity
- **Effort:** S

### TMUX-18 · P3 · display-panes is a client overlay in tm and a pane mode in tmux

- **Area:** Modes
- **Where:** `src/server.rs` `enter_panes_mode`
- **Problem:** tmux 3.8 puts the target pane in panes mode even with no client attached, and the mode zooms the pane. tm needs an attached client. `Request::DisplayPanes` still carries `source`, `no_zoom`, `no_mode`, and `kill_on_exit`, which the CLI can no longer set.
- **Recommendation:** Drop the unused fields and rebuild the mode on the pane, then compare it with a raw-PTY attach.
- **Fix with:** reference-parity
- **Effort:** L

### TMUX-19 · P4 · pipe-pane -I and strftime expansion are missing

- **Area:** Commands
- **Where:** `src/command.rs` `parse_pipe_pane`, `src/server.rs` `pipe_pane`
- **Problem:** `-I` (command output into the pane) is rejected, and the command is expanded with `#{}` formats but not with strftime escapes as tmux's `format_expand_time` does.
- **Recommendation:** Read the helper's stdout into the pane and expand time escapes, with differential probes.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-20 · P3 · The format comparison operators compare numbers, tmux compares strings

- **Area:** Formats
- **Where:** `src/server.rs` `compare_format_values`
- **Problem:** `#{<:a,b}`, `#{>:a,b}`, `#{<=:a,b}`, and `#{>=:a,b}` compare numerically when both sides parse as numbers; tmux 3.8 uses `strcmp`, so `#{>:5,10}` is true there. `list-buffers -f '#{>:#{buffer_size},10}'` lists a 5-byte buffer in tmux and not in tm.
- **Recommendation:** Compare as strings and check `#{==:}`, `#{!=:}`, and the four orderings with a differential `display-message -p` probe.
- **Fix with:** reference-parity
- **Effort:** S

### TMUX-21 · P4 · Choose modes treat the filter result "false" as false

- **Area:** Buffers
- **Where:** `src/server.rs` `build_buffer_entries` and the two other filter checks
- **Problem:** The choose-buffer, choose-tree, and choose-client filters treat an expanded filter of `false` as false; tmux 3.8 treats only an empty string and `0` as false. `list-buffers -f` now follows tmux.
- **Recommendation:** Share one `format_true` helper between the filters and add a differential probe with a filter that expands to `false`.
- **Fix with:** reference-parity
- **Effort:** S

### TMUX-22 · P3 · send-keys knows few key names and no modifier combinations

- **Area:** Keys
- **Where:** `src/command.rs` `key_bytes`
- **Problem:** tm types `BSpace`, `DC`, `IC`, `Home`, `End`, `PageUp`, `PageDown`, `PgUp`, `PgDn`, `NPage`, `PPage`, `Insert`, `F1` to `F12`, `KP0`, `KP/`, `KPEnter`, `S-Tab`, `M-Enter`, `M-Space`, `C-Up`, `M-Up`, `S-Up`, `C-S-a`, `M-C-a`, `C-1`, `C-?`, `^a`, and any key name whose case differs (`enter`) as text, and sends `Home` and `End` as `ESC [ H` and `ESC [ F`. tmux 3.8 sends the key (`Home` is `ESC [ 1~`, `F1` is `ESC O P`, `C-Up` is `ESC [ 1;5 A`).
- **Recommendation:** Port tmux's key name table and the `input_key` encoding (cursor and keypad modes, modifier parameters), then probe every name against the reference.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-23 · P4 · The t/f/ time modifier differs from tmux

- **Area:** Formats
- **Where:** `src/server.rs` time modifier handling
- **Problem:** With `@ts` set to `1000000000`, `display-message -p '#{t/f/%%Y:@ts}'` prints `2001` in tm and the current year in tmux 3.8, and `#{t/f/%%Y:#{@ts}}` prints `2001` in tm and `1000000000` in tmux. `tests/tmux_core_compat.rs` expected `2001` for the single-percent form, which tmux answers with the current year because `display-message` runs `strftime` over the whole message first; the case was never compared with the reference and now asserts the current year that tmux prints.
- **Recommendation:** Port `format_modifiers` time handling after probing the forms above and restore an exact assertion.
- **Fix with:** reference-parity
- **Effort:** S

### TMUX-24 · P3 · window_layout and the default list-panes line differ from tmux

- **Area:** Formats
- **Where:** `src/server.rs` `format_pane`, `list_panes`
- **Problem:** `#{window_layout}` expands to nothing; tmux 3.8 prints a JSON layout such as `{"V":2,"L":{"t":"p","w":80,"h":24,"x":0,"y":0,"a":true,"i":0,"I":"%4"}}`, so the default `list-windows` line differs in its `[layout ...]` column. The default `list-panes` line is `%4: 0 *` in tm and `0: [80x24] [history 0/10000, 960 bytes] %4 (active)` in tmux.
- **Recommendation:** Add `window_layout` from the layout tree and port the default `list-panes` template, then drop the layout exception in `windows_list_windows_default_format_matches_except_the_layout`.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-25 · P4 · window_flags lacks the activity, silence, and marked flags

- **Area:** Formats
- **Where:** `src/server.rs` `window_printable_flags`
- **Problem:** tm reports `*`, `-`, `!`, and `Z` for `#{window_flags}` and `#{window_raw_flags}`; tmux 3.8 also reports `#` (activity), `~` (silence), and `M` (marked pane), and escapes `#` in `window_flags`.
- **Recommendation:** Add the flags when tm tracks activity, silence, and the marked window, with a differential probe per flag.
- **Fix with:** reference-parity
- **Effort:** S

### TMUX-26 · P3 · Copy mode position indicator and copy formats differ from tmux

- **Area:** Copy mode
- **Where:** `src/server.rs` copy-mode rendering and format expansion
- **Problem:** On an attached client, tmux 3.8 draws `HH:MM [0/n]` on the first row (the time of the top line); tm draws `[0/n]` only. `#{copy_cursor_y}` is the screen row in tmux (7 on an 8-row pane) and the history-absolute row in tm (30). `#{scroll_position}` is empty outside a mode in tmux and `0` in tm.
- **Recommendation:** Port `copy-mode-position-format` with `top_line_time`, make `copy_cursor_y` viewport-relative, and print nothing for `scroll_position` outside a mode, each with a differential probe.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-27 · P3 · Attached base screen differs from tmux after the client resizes the window

- **Area:** Rendering
- **Where:** `src/server.rs` client attach resize and status rendering
- **Problem:** A session created at 40x8 and attached by a 60x10 client shows 7 of the last output rows in tm and 8 in tmux, and `set-option -g status-left MARK` renders `MARK 1:sh` in tm and `MARK1:sh` in tmux (tm pads the left status).
- **Recommendation:** The left-status padding is fixed (H2). Fix the resize row loss: when a window with output history grows, tmux pulls more rows from history. `ResizeOfScrolledWindow` in `tests/tmux_histories.rs` stops generating the shape.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-28 · P3 · choose-tree, choose-buffer and choose-client are client modes in tm

- **Area:** Modes
- **Where:** `src/server.rs` tree, buffer and client modes
- **Problem:** tm has no `choose-window` command (it exits 1 with `unknown command`; tmux 3.8 opens tree mode with the windows collapsed). tmux 3.8 puts these modes on the target pane (`#{pane_in_mode}` is 1, no client needed) with `(n)` shortcut lines and a template run at the selection. tm opens its own list on an attached client, renders different text, and C6c therefore rejects `-F`, `-f`, `-O`, `-r`, `-k`, `-h`, `-G`, `-N`, `-Z`, `-K`, `-s`, `-w`, `-y`, `-i` and the template.
- **Recommendation:** Rebuild the modes on the pane-mode model with the tmux line format, then re-enable the flags one at a time with raw-PTY differential tests.
- **Fix with:** reference-parity
- **Effort:** L

### TMUX-29 · P4 · Bound choose and display-panes commands still parse flags the CLI rejects

- **Area:** Bindings
- **Where:** `src/server.rs` `parse_tree_mode_command`, `parse_buffer_mode_command`, `parse_client_mode_command`, `parse_panes_mode_command`
- **Problem:** Commands run from bindings use these parsers, which accept flags the command line now rejects. No compiled binding uses them.
- **Recommendation:** Route bound commands through `crate::command::parse` or delete the parsers with the unused request fields.
- **Fix with:** delete
- **Effort:** S

### TMUX-30 · P3 · A window moved to a session of another size is resized to it

- **Area:** Layout
- **Where:** `src/server.rs` `relink_window`, `break_pane`, `reflow_session`
- **Problem:** tm sizes every window to its session, so `move-window`, `swap-window` and `break-pane` between detached sessions of different sizes resize the moved window. tmux keeps the window's size until a client attaches.
- **Recommendation:** Give a window its own size and resize it only for attached clients, with a differential history of cross-session moves between sessions of different sizes. `tests/tmux_histories.rs` `Exclusion::SessionSizeVariation` stops generating them.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-31 · P3 · Malformed targets are parsed differently from tmux

- **Area:** Targets
- **Where:** `src/server.rs` `resolve_session_index`, `resolve_window_target`, `resolve_pane_target`
- **Problem:** tm splits `a.b.c` at the last dot where tmux splits at the first, strips a leading `=` from error text, accepts `::` as a session, reads `a:b:c` and `zz:zz.zz` as a session named `a:b:c` or `zz:zz.zz`, resolves a bare `-` or `+` to a different window than tmux, and words `@`, `@x`, and `..` errors for a different object (`can't find pane: @x` against tmux's `can't find window: @x`).
- **Recommendation:** Replace the three resolvers with one parser that follows tmux's `cmd_find_target` grammar, with a differential probe per malformed shape. `tests/tmux_histories.rs` `Exclusion::MalformedTargetGrammar` lists the shapes it stops generating.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-32 · P3 · A relative window slot places new and moved windows differently

- **Area:** Windows
- **Where:** `src/server.rs` `relink_window`, `new_window`, `break_pane`
- **Problem:** `new-window`, `move-window` and `break-pane` with a `+N` or `-N` slot (for example `move-window -a -s c:x -t c:+`) moves window `x` after the window at `+`; tmux leaves it at index 2 where tm moves it to 3, because tmux resolves a `+N` or `-N` target to an index without a window and does not shuffle.
- **Recommendation:** Treat an offset target that names no window like an absent index in `relink_window`, with a differential probe for `-a` and `-b` with `+N` and `-N`. `tests/tmux_histories.rs` `Exclusion::RelativeWindowPlacement` stops generating the shape; the relative targets also leave other window state (such as the last-window flag) different afterwards.
- **Fix with:** reference-parity
- **Effort:** S

### TMUX-33 · P3 · A SIGKILLed client leaves its terminal in raw mode and the alternate screen

- **Area:** Clients
- **Where:** `src/client.rs` terminal setup and restore
- **Problem:** tmux's server owns the client's tty, so after `kill -9` of an attached client the server resets the alternate screen, bracketed paste, mouse modes and termios. tm's client restores its own terminal and cannot when killed.
- **Recommendation:** Pass the tty descriptor to the server, or have the server write the restore sequence and termios to the client's tty on connection loss; cover it with the raw-PTY `KillClientRendering` history.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-34 · P3 · Windows that no client displays are resized

- **Area:** Layout
- **Where:** `src/server.rs` `resize_session`
- **Problem:** tm resizes every window of a session to its client; tmux 3.8 resizes only the displayed window of an attached session and leaves other windows at their size until they are selected.
- **Recommendation:** Resize the active window on attach and resize, and the newly selected window on selection, then drop the `UnattachedWindowSize` exclusion in `tests/tmux_histories.rs`.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-35 · P3 · Two clients of different sizes on one session are drawn differently

- **Area:** Rendering
- **Where:** `src/server.rs` attach rendering
- **Problem:** With `window-size latest`, tmux draws a client larger than the window with a filler area and a client smaller than the window cropped; tm draws neither.
- **Recommendation:** Render the window at its size inside the client, with tmux's filler and crop rules, and drop the `SharedSessionClientSizes` exclusion.
- **Fix with:** reference-parity
- **Effort:** L

### TMUX-36 · P4 · Last window is a single slot, not tmux's stack

- **Area:** Windows
- **Where:** `src/model.rs` `last_window`
- **Problem:** When the current window of a session with two or more windows is removed (for example a window created, selected, and then exited), tmux returns to the window before it and keeps the older window as the last window; tm clears the last window.
- **Recommendation:** Replace the slot with a recency list, like TMUX-14, and drop the `WindowRemovalFromHistoryStack` exclusion.
- **Fix with:** reference-parity

### TMUX-37 · P3 · Resizing a window with several panes divides the space differently

- **Area:** Layout
- **Where:** `src/model.rs` layout resize
- **Problem:** When a client attach or resize changes the size of a window with two or more panes, tm and tmux round the pane rows or columns differently (a 3-pane column at 9 rows is 2/1/4 in tm and 2/2/3 in tmux).
- **Recommendation:** Redistribute the change over siblings as tmux's `layout_resize` does (see TMUX-15), and drop the `ResizeOfSplitWindow` exclusion.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-41 · P3 · A wide character that cannot fit at the right margin does not mark its row wrapped

- **Area:** Terminal
- **Where:** `src/terminal.rs` `write_terminal_character`; `vt100` 0.16.2 `Screen::text`
- **Problem:** tmux marks a row wrapped when a wide character cannot fit in the last column and moves to the next row. `vt100` sets the wrapped flag only when the last cell already has contents, so `capture-pane -J` leaves a break where tmux joins (`abcdefghi` then `中Z` in a 10-column pane). Pinned by the `wide_characters_margin_wrap` entries in `KNOWN_DIFFERENCES`.
- **Recommendation:** Carry the wrapped flag in tm's own grid layer, or patch the grid so a wide character that does not fit wraps with the flag set.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-42 · P2 · A resize does not reflow wrapped lines

- **Area:** Terminal
- **Where:** `src/model.rs` `resize_parser_to_rect`; `vt100` `Screen::set_size`
- **Problem:** tmux reflows wrapped lines and their history when a window narrows or widens, and restores them when it widens again. tm truncates rows on a narrower resize and never recovers the text; a program that does not redraw loses its output. Pinned by the `wide_characters_resize_narrower` and `wide_characters_resize_wider` entries in `KNOWN_DIFFERENCES`.
- **Recommendation:** Keep per-row wrapped flags and written extents in tm's grid and port `grid_reflow` from tmux 3.8 `grid.c`.
- **Fix with:** reference-parity
- **Effort:** L

### TMUX-43 · P3 · History trims one line at a time instead of a tenth of the limit

- **Area:** History
- **Where:** `src/model.rs` parser scrollback length
- **Problem:** When the history reaches `history-limit`, tmux 3.8 drops `limit / 10` lines at once (`grid_collect_history`), so the retained history oscillates between 90% and 100%. `vt100` drops one line per scrolled line, so `capture-pane -S -` starts at a different line after the limit is reached. Pinned by the `history_limit_trim_chunk` entries in `KNOWN_DIFFERENCES`.
- **Recommendation:** Own the scrollback in tm and trim it in tmux's chunks, with a differential probe that prints past the limit one line at a time.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-44 · P3 · resize-window is not implemented

- **Area:** Commands
- **Where:** `src/command.rs`
- **Problem:** tmux 3.8 has `resize-window` (`-x`, `-y`, `-A`, `-D`, `-U`, `-L`, `-R`); tm has no such command, so a detached session cannot be resized without an attached client. `tests/tmux_unicode_history.rs` resizes through attached raw-PTY clients instead.
- **Recommendation:** Add the command with the `window-size` option semantics and a differential test of each flag.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-45 · P3 · A window target given as a pane id is rejected

- **Area:** Targeting
- **Where:** `src/server.rs` `resolve_window_target`
- **Problem:** tmux 3.8 resolves a `%<pane id>` window target to the window holding that pane, so `kill-window -t %1` succeeds. tm exits 1 with `can't find window: %1`, while `display-message -t %1` accepts the same pane id.
- **Recommendation:** Resolve a `%` window target through the pane's window, and add a differential probe for each window-targeting command.
- **Fix with:** reference-parity

### TMUX-46 · P3 · The status window list omits a window that overflows the line

- **Area:** Status
- **Where:** `src/server.rs` status rendering
- **Problem:** On a 30-column client in a session with four windows, tmux 3.8 draws `(b) 1:sh 2:inpane 3:sleep 4:i>`, truncating the last window with `>`; tm draws `(b) 1:sh 2:inpane 3:sleep` and drops the fourth window. Seen as a screen difference in attached history seed 33 before the in-pane shell window was shortened.
- **Recommendation:** Reproduce on the raw-PTY runner with a differential screen test, then truncate the overflowing window entry as tmux does.
- **Fix with:** reference-parity

### TMUX-47 · P3 · Short format aliases are not expanded in window names

- **Area:** Formats
- **Where:** `src/server.rs` `rename_window`, `new_window`
- **Problem:** `rename-window '#S-#I'` names the window `#S-#I` in tm and `S-1` in tmux 3.8, and `new-window -n '#S-x'` likewise. The command prompt (`prefix r`) takes the same path. The long forms (`#{session_name}`) expand in both.
- **Recommendation:** Expand the single-letter aliases (`#S`, `#I`, `#W`, `#P`, `#F`, `#H`, `#h`, `#D`, `#T`) in the shared format expander and cover `rename-window`, `rename-session`, `new-window -n` and `new-session -s` with differential probes.
- **Fix with:** reference-parity
- **Effort:** S

### TMUX-48 · P4 · Ctrl+Shift+letter in a legacy pane reaches it as a control character

- **Area:** Input
- **Where:** `src/server.rs` pane key encoder
- **Problem:** With the pane not requesting extended keys, the client bytes `ESC [ 105 ; 6 u` (Ctrl+Shift+i) reach the pane as `0x09` in tm and as `I` in tmux 3.8.
- **Recommendation:** Follow `input_key_vt10x` and tmux's key parsing for Ctrl+Shift letters, with a differential probe over the whole letter range.
- **Fix with:** reference-parity
- **Effort:** S

### TMUX-49 · P3 · Copy-mode search prompt and formats differ

- **Area:** Modes
- **Where:** `src/copy_mode.rs` prompt, `src/server.rs` pane formats
- **Problem:** After `C-s` and `o` in copy mode tmux 3.8 draws `(search down) o` on the pane's last row and `[0/0] (3 results)` at the top right; tm draws `(search) o` on the status row and `[0/0]`. `#{search_present}` is unset in tm, `#{copy_cursor_word}` is empty where tmux gives the word, and `#{selection_present}` is 1 in tm for a zero-width selection where tmux gives 0.
- **Recommendation:** Move the prompt to the pane, count the matches, and add the formats, each with a raw-PTY differential test.
- **Fix with:** reference-parity
- **Effort:** M
