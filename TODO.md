Prefix: TMUX
Next: 11

### TMUX-8 · P3 · An exited pipe helper stays a zombie while pipe-pane is active

- **Area:** Lifecycle
- **Where:** `src/server.rs:6608`, `src/server.rs:1127`
- **Problem:** `pipe-pane -t S -- 'true'` leaves the helper `<defunct>` under the daemon until the pipe is turned off. The writer thread blocks in `recv()` and only calls `child.wait()` after `PipeMessage::Close`, so a helper that exits early is not reaped while the pipe remains registered.
- **Recommendation:** Poll `try_wait` while the pipe is active, or reap the child in a separate wait thread, so an exited helper is collected before `pipe-pane` is turned off.
- **Fix with:** lifecycle-concurrency
- **Effort:** S

### TMUX-10 · P3 · A session-scoped status-left is not rendered

- **Area:** Status
- **Where:** `src/server.rs` status rendering
- **Problem:** `set-option -t S status-left X` changes the option but the status line keeps the global value; `set-option -g status-left X` renders. tmux 3.8 renders the session value.
- **Recommendation:** Resolve status options through the session scope before the global table when rendering, with a differential raw-stream test.
- **Fix with:** reference-parity
- **Effort:** S

TMUX-1, TMUX-2, TMUX-3, TMUX-5, TMUX-6, TMUX-7, and TMUX-9 are closed with
tests and commits recorded in git history. TMUX-8 is an open P3 from the final
review. Add a new entry here only for a reproduced defect that the current
suites do not cover.
