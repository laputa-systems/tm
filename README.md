# tm

`tm` is a small Unix terminal multiplexer with sessions, windows, and panes.
Sessions run in a local daemon and can stay open after you detach.

## Install

Download a zip from [GitHub Releases](https://github.com/laputa-systems/tm/releases).
The release builds include macOS arm64 and Linux x86_64 and arm64 binaries.
Linux offers dynamic and static musl builds; use the static binary if musl is
not installed. Unzip the matching archive, rename its executable to `tm`, and
put it in a directory on your `PATH`.

## Use

```sh
tm                              # create a session and attach
tm new-session -d -s work       # create a detached session
tm split-window -h -t work      # split its window into side-by-side panes
tm ls                           # list sessions
tm attach-session -t work       # attach to the session
tm kill-session -t work
```

The default socket is `$TMPDIR/tm-$UID.sock`. Pass `-S path` to select another
socket. Setting `TM_SOCKET` also selects a socket; a daemon first started with
that variable uses the baseline settings and bindings.

The normal prefix is `C-a`. For example, `C-a %` and `C-a "` split panes,
`C-a h`, `C-a j`, `C-a k`, and `C-a l` move between panes, `C-a d` detaches,
and `C-a [` enters copy mode. A server first started with `TM_SOCKET` uses the
baseline bindings, including the `C-b` prefix. Settings and bindings are built
in; `tm` does not read tmux configuration files or `TM_CONFIG`.

Run `tm help` for the command summary. Report problems in the
[issue tracker](https://github.com/laputa-systems/tm/issues). The project is
licensed under [MIT](LICENSE).
