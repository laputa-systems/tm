Prefix: TMUX
Next: 12

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
