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

The default socket is `$TMPDIR/tm-$UID.sock`. Pass `-S path` to select another
socket. A daemon first started with `TM_SOCKET` uses the baseline settings and
bindings, including the `C-b` prefix.

The normal prefix is `C-a`. `C-a %` and `C-a "` split panes, `C-a h`, `C-a j`,
and `C-a l` move between panes, `C-a k` kills the current pane, `C-a d`
detaches, and `C-a [` enters copy mode. Settings and bindings are compiled in.
`tm` does not read tmux configuration files or `TM_CONFIG`.

Run `tm help` for the command summary. Report problems in the
[issue tracker](https://github.com/laputa-systems/tm/issues). The project is
licensed under [MIT](LICENSE).
