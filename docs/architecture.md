# Architecture

The foreground client and daemon are separate processes. The daemon owns
sessions, windows, panes, PTYs, and terminal state; clients send commands over
a local Unix socket and attached clients render that state in the user's
terminal.

The entry point and command parser are in [`src/main.rs`](../src/main.rs) and
[`src/command.rs`](../src/command.rs), and the wire format is in
[`src/protocol.rs`](../src/protocol.rs). The client and daemon are in
[`src/client.rs`](../src/client.rs) and [`src/server.rs`](../src/server.rs).
PTY creation and terminal output handling are in [`src/pty.rs`](../src/pty.rs)
and [`src/terminal.rs`](../src/terminal.rs); copy mode and capture are in
[`src/copy_mode.rs`](../src/copy_mode.rs) and [`src/capture.rs`](../src/capture.rs).
The tmux option table is [`src/options.rs`](../src/options.rs). Display width,
wide cells, combining marks, tab stops, and column mapping live in
[`src/cells.rs`](../src/cells.rs).

Interactive startup settings and key bindings live in
[`src/config.rs`](../src/config.rs) as compiled data. A normal daemon applies
that profile; a daemon first started on the socket named by `TM_SOCKET` uses the baseline
settings and bindings, including the `C-b` prefix, for isolated use. The
program does not parse tmux configuration files. Changing the normal
interactive profile requires editing the compiled data and rebuilding.

The command and interactive surface is a tested subset of tmux, not a promise
of full CLI parity. Add support when a concrete workflow or regression test
needs it. A daemon can outlive its launching client and continues running the
binary that started it; stop it before checking rebuilt daemon behavior,
as described in [Wire protocol](#wire-protocol).

Long-lived helpers follow the same separation as attach clients: foreground
commands run outside the shared state lock, pane input and pipe-pane writes use
ordered writer threads, and copy-pipe helpers run asynchronously. A pane's
retained output checkpoints parsed scrollback plus the visible screen under the
history budget, so replay, capture, and copy-mode search do not depend on
replaying the entire stream on each key.

## Lifecycle

The daemon exits when its last session ends. A command that finds no daemon
on the socket fails with `no server running on <socket>`, except the commands
that start one, such as `new-session`.

A successful `kill-server` response means the listener is closed and the
socket path no longer accepts connections. Panes may still be tearing down. A
connection accepted during shutdown gets the error `server is shutting down`,
never a silent EOF. The client does not retry a request, because a mutating
request may already have run.

The daemon writes `<socket>.log` with mode `0600`. It holds the listening
address, daemon errors, and the number of each received signal. The daemon
truncates it at start and again when it would pass 1 MiB. The daemon handles
`SIGTERM`, `SIGHUP`, `SIGINT`, and `SIGQUIT` by closing the listener and
removing the socket.

## Wire protocol

Every connection starts with a little-endian `u32` `PROTOCOL_VERSION` from the
client, and the daemon answers with its own before any other byte. On a
mismatch, or on EOF before any response byte, which is how an older daemon
reacts, the client prints
`tm server at <socket> runs a different protocol; run tm -S <socket> kill-server`
and exits nonzero. An old daemon refuses a new client's version preamble before it reads any
request, so no request can reach it. `kill-server` therefore handles a mismatch
itself: the client reads the daemon's pid and uid from the connected socket
(`SO_PEERCRED` on Linux, `LOCAL_PEERPID` and `getpeereid` on macOS) before it
sends anything, requires the uid to equal its own, sends `SIGTERM`, and waits
until the socket stops accepting. The daemon removes its socket on `SIGTERM`.
The next command then starts a daemon of the new version. The encoder refuses more than 1024 command
arguments, so the client fails before it connects. Golden-byte tests in
[`src/protocol.rs`](../src/protocol.rs) fail when the format changes without a
version bump.

## Pane context

Pane context follows the rules in the [README](../README.md#use): the client
sends `TM_PANE` with a request only when it connects to the socket named by
`TM_SOCKET`, and a pane that no longer exists resolves as outside any pane.
