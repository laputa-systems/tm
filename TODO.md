Prefix: TMUX
Next: 20

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

### TMUX-16 · P3 · show-buffer and list-buffers differ from tmux

- **Area:** Buffers
- **Where:** `src/command.rs` buffer commands, `src/server.rs` `show_buffer`, `list_buffers`
- **Problem:** `show-buffer` prints the buffer and then a newline of its own; tmux prints the bytes unchanged. The default `list-buffers` format prints the sample unescaped and unquoted; tmux prints `name: N bytes: "escaped sample"`.
- **Recommendation:** Write the buffer bytes unchanged and port the default format with a differential probe.
- **Fix with:** reference-parity
- **Effort:** S

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
