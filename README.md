# tm

`tm` is a small Unix terminal multiplexer with sessions, windows, and panes.
Sessions run in a local daemon and can stay open after you detach.

## Install

Download a zip from [GitHub Releases](https://github.com/laputa-systems/tm/releases).
The archives are statically linked Linux x86_64 and arm64 binaries. Unzip the
matching archive, rename its executable to `tm`, and put it on your `PATH`.

## Use

```sh
tm                              # create a session and attach
tm new-session -d -s work       # create a detached session
tm split-window -h -t work      # split its window into side-by-side panes
tm ls                           # list sessions
tm attach-session -t work       # attach to the session
tm kill-session -t work
```

The default socket is `tm-<uid>.sock` in `$TMPDIR`, or in `/tmp` when `TMPDIR` is
unset, where `<uid>` is the effective user id. `TM_SOCKET` overrides that
path and `-S path` overrides both. A daemon first started on the socket named by `TM_SOCKET` uses the baseline settings and
bindings, including the `C-b` prefix.

Each pane runs with `TM_SOCKET` set to its server's socket path and `TM_PANE`
set to `%<pane id>`. A `tm` command run in a pane without `-S` therefore talks
to that pane's server, and an omitted target resolves from that pane: its
session, that session's current window, and the window's active pane, as tmux
does with `TMUX` and `TMUX_PANE`. tm never reads or sets `TMUX` or `TMUX_PANE`,
so it can run inside a real tmux. The client sends `TM_PANE` only when the
socket it connects to is the one named by `TM_SOCKET`. If that pane no longer
exists, the command resolves as if run outside any pane.

The client and daemon check a protocol version on every connection. After an
upgrade, a client that finds a daemon started by another version prints
`tm server at <socket> runs a different protocol; run tm -S <socket> kill-server`
and exits nonzero. That command fails the same way from the new binary. See
[Wire protocol](docs/architecture.md#wire-protocol) for the recovery steps. A command with more than 1024 command arguments is refused
by the client before it connects.

The normal prefix is `C-a`. `C-a %` and `C-a "` split panes, `C-a h`, `C-a j`,
and `C-a l` move between panes, `C-a k` kills the current pane, `C-a d`
detaches, and `C-a [` enters copy mode. Settings and bindings are compiled in.
`tm` does not read tmux configuration files or `TM_CONFIG`. Attaching uses the
alternate screen and restores your shell screen on detach; mouse, focus, paste,
sync, and extended-key sequences are enabled only when the terminal profile
advertises them.

## Changes

These changes alter documented or scripted behavior.

Flags and forms that tmux 3.8 accepts and tm does not implement, or that tm
used to give another meaning, are now rejected with
`tm: unsupported flag <flag> for <command>` and a nonzero exit. The
[CLI flags table](docs/parity.md#cli-flags) lists every command and flag as
`match`, `rejected`, or `tm-only`. Notable rejections are `link-window`,
`new-pane`, `split-window -f`, `select-pane -n` and `-p`, `copy-mode -k`,
`-M` and `-S`, `set-option -a` and `-o`, `send-prefix -2`, and every flag of
`kill-server`. Positional targets and extra arguments now fail with tmux's
`too many arguments`.

Changed behavior:

- `send-prefix` sends the session's `prefix` instead of `C-b`.
- `new-session -E` runs the command or the default shell instead of creating
  an empty pane. `new-session` prints nothing unless you pass `-P` or `-F`.
- `kill-server` returns only after the socket stops accepting connections.
  A request that arrives during shutdown fails with `server is shutting down`.
- The daemon exits when its last session ends.
- Unknown session names in `-t` fail with `can't find session: <name>`
  instead of falling back to the first session. `new-window` without `-d`
  no longer attaches the calling terminal.
- A window is named after its command (or the shell) instead of its index.
- `capture-pane` without `-p` writes a paste buffer. `show-buffer` and
  `save-buffer` to stdout add no newline.
- `show-options` prints only values set at the target unless you pass `-A`.
  Sessions have their own environment, and panes start from the global
  environment overlaid by the session's.
- A client killed by `SIGTERM` or `SIGHUP` prints `[terminated]` or
  `[lost tty]` and exits 1, not 128 plus the signal number. Detaching or
  ending a client prints `[detached (from session <name>)]` or `[exited]`.
- Status messages disappear after `display-time` instead of at the next
  request.

Wire protocol: client and daemon exchange a protocol version on every
connection, and the version changes whenever the format does. Before an
upgrade, stop the daemon with `kill-server` from the old binary. After an
upgrade, send `SIGTERM` to the `tm --daemon <socket>` process instead. A client
that meets a daemon of another version prints the message above and exits
nonzero. A command with more
than 1024 arguments is refused by the client.

Environment: panes export `TM_SOCKET` and `TM_PANE` and omitted targets resolve
from the calling pane. A daemon started with `-S` from inside a pane no longer
inherits the baseline profile unless `TM_SOCKET` names its own socket.

The daemon writes `<socket>.log` next to the socket. It holds the listening
address, daemon errors, and received signals, is truncated when the daemon
starts, and is capped at 1 MiB.

Run `tm help` for the command summary. Report problems in the
[issue tracker](https://github.com/laputa-systems/tm/issues). The project is
licensed under [MIT](LICENSE).
