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

The default socket is `tm-<uid>/default` in `$TMPDIR`, or in `/tmp` when
`TMPDIR` is unset, where `<uid>` is the effective user id. tm keeps that
directory private (mode 0700, owned by you) and refuses to use it otherwise,
and it refuses to connect to a socket another user owns. `TM_SOCKET` overrides
the default path and `-S path` overrides both. A daemon first started on the
socket named by `TM_SOCKET` uses the baseline settings and bindings, including
the `C-b` prefix. The daemon writes errors and received signals to
`<socket>.log` beside the socket.

Each pane runs with `TM_SOCKET` set to its server's socket and `TM_PANE` set to
`%<pane id>`, so a `tm` command run in a pane talks to that pane's server, and
an omitted target resolves from that pane, as tmux does with `TMUX` and
`TMUX_PANE`. tm never reads or sets `TMUX` or `TMUX_PANE`, so it can run inside
a real tmux.

The client and daemon check a protocol version on every connection. After an
upgrade, other commands fail against a daemon started by the old version until
you stop it with `tm -S <socket> kill-server`, which works across versions.

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
- Client and daemon check a protocol version on every connection, and a
  command with more than 1024 arguments is refused by the client.
- The default socket moved from `<tmpdir>/tm-<uid>.sock` to the private
  `<tmpdir>/tm-<uid>/default`. A daemon started by an older build is not found
  there; stop it with that build's `kill-server` or `SIGTERM`. A `-S` or
  `TM_SOCKET` path naming an existing file that is not your socket is an error
  instead of being deleted.
- Panes export `TM_SOCKET` and `TM_PANE`, and omitted targets resolve from the
  calling pane. A daemon started with `-S` from inside a pane no longer
  inherits the baseline profile unless `TM_SOCKET` names its own socket.

Run `tm help` for the command summary. Report problems in the
[issue tracker](https://github.com/laputa-systems/tm/issues). The project is
licensed under [MIT](LICENSE).
