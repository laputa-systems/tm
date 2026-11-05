Prefix: TMUX
Next: 15

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
