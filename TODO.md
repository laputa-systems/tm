Prefix: TMUX
Next: 26

### TMUX-10 · P3 · A session-scoped status-left is not rendered

- **Area:** Status
- **Where:** `src/server.rs` status rendering
- **Problem:** `set-option -t S status-left X` changes the option but the status line keeps the global value; `set-option -g status-left X` renders. tmux 3.8 renders the session value.
- **Recommendation:** Resolve status options through the session scope before the global table when rendering, with a differential raw-stream test.
- **Fix with:** reference-parity
- **Effort:** S

### TMUX-11 · P3 · Session format variables are missing or wrong

- **Area:** Formats
- **Where:** `src/server.rs` session format values
- **Problem:** `#{session_group}`, `#{session_created}`, `#{t:session_created}`, and `#{session_width}` expand to nothing in tm, and `#{session_group_size}` is `1` for ungrouped sessions; tmux 3.8 expands them.
- **Recommendation:** Add the values with a `compare_commands` probe of `display-message -p` for each.
- **Fix with:** reference-parity
- **Effort:** S

TMUX-1, TMUX-2, TMUX-3, TMUX-5, TMUX-6, TMUX-7, TMUX-8, and TMUX-9 are closed
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
- **Problem:** tm lays panes out as a binary tree and moves only the border of the nearest enclosing split. tmux keeps one list of siblings and passes the remaining change on to the next siblings, so `resize-pane -R`, `-L`, `-x` in a row of three panes ends with different widths.
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
