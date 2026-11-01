# Architecture

The foreground client and daemon are separate processes. The daemon owns
sessions, windows, panes, PTYs, and terminal state; clients send commands over
a local Unix socket and attached clients render that state in the user's
terminal.

The entry point and command parser are in [`src/main.rs`](../src/main.rs) and
[`src/command.rs`](../src/command.rs). The client and daemon are in
[`src/client.rs`](../src/client.rs) and [`src/server.rs`](../src/server.rs).
PTY creation and terminal output handling are in [`src/pty.rs`](../src/pty.rs)
and [`src/terminal.rs`](../src/terminal.rs). Display width, wide cells,
combining marks, tab stops, and column mapping live in
[`src/cells.rs`](../src/cells.rs).

Interactive startup settings and key bindings live in
[`src/config.rs`](../src/config.rs) as compiled data. A normal daemon applies
that profile; a daemon first started with `TM_SOCKET` uses the baseline
settings and bindings, including the `C-b` prefix, for isolated use. The
program does not parse tmux configuration files. Changing the normal
interactive profile requires editing the compiled data and rebuilding.

The command and interactive surface is a tested subset of tmux, not a promise
of full CLI parity. Add support when a concrete workflow or regression test
needs it. A daemon can outlive its launching client and continues running the
binary that started it; use `tm kill-server` before checking rebuilt daemon
behavior.

Long-lived helpers follow the same separation as attach clients: foreground
commands run outside the shared state lock, pane input and pipe-pane writes use
ordered writer threads, and copy-pipe helpers run asynchronously. A pane's
retained output checkpoints parsed scrollback plus the visible screen under the
history budget, so replay, capture, and copy-mode search do not depend on
replaying the entire stream on each key.
