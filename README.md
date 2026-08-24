# tm

`tm` is the deliberately small, Unix-only core of a tmux-style terminal
multiplexer. A local daemon owns sessions, windows, panes, PTYs, and terminal
state; the foreground client attaches to it over a Unix socket.

The supported interactive surface is compiled into `src/config.rs`, rather
than loaded from a user configuration file or presented as universal tmux
parity. The compiled contract covers startup options (including the pane
`TERM` value from `default-terminal`), a custom `C-a` prefix,
repeatable key bindings, chained commands, command prompts,
window and pane navigation, relative window swapping, pane movement, layouts,
and the status line. Copy mode remains part of the core because it
is a primary interactive tmux contract, not because every tmux feature is
supported.

Web sharing, SDKs, Windows support, runtime config plugins, hooks, control mode,
menus/popups, and unrelated advanced commands are intentionally outside this
contract. They should only be added when the user's configuration or a focused
regression test gives the core a reason to carry them.

## Usage

```sh
cargo run                         # create session 0 and attach
cargo run -- new-session -d -s work
cargo run -- split-window -h -t work
cargo run -- list-sessions
cargo run -- attach-session -t work
```

The daemon socket defaults to `$TMPDIR/tm-$UID.sock`. Set `TM_SOCKET` or
use `-S path` to choose another local socket.

The daemon persists independently of the client process. After rebuilding `tm`,
stop an existing daemon (`tm kill-server`) before starting a new session so the
attached client is served by the rebuilt renderer and key table.

The normal daemon starts with the settings and bindings from `src/config.rs`.
An explicit `TM_SOCKET` starts an isolated vanilla daemon for tooling and test
harnesses. `TM_CONFIG` is ignored; tm never reads a tmux configuration file.
`source-file` is unavailable because changing the interactive contract requires
an explicit source change and rebuild.

The implementation currently targets macOS and Linux. The Unix PTY and
terminal interfaces are kept in `pty.rs` and `terminal.rs`; raw mode, terminal
size, and PTY resize use `rustix::termios`.

## Compatibility checks

The tests are deterministic contracts for the implemented core and compiled
interactive behavior. The headless suites cover session, target, pane, window,
PTY, buffer, capture, copy-mode, chooser-mode, format, client-registry, and
run-shell behavior. `tests/attached_client_pty.rs` is the real
terminal boundary: it launches `tm attach-session` through `openpty`, drives input
and resize events, and parses rendered output with `vt100`. Its fixture is
`tests/support/pty_fixture.rs`; it uses marker barriers and never invokes the
platform `script` utility. Every integration test owns a private `TM_SOCKET`; the
suite never contacts tmux or any existing tmux session.

Run the tm-only checks with:

```sh
cargo test
```

Passing these tests establishes compatibility for the exercised flows. It is
not a claim that the deliberately smaller command surface implements every
tmux CLI feature; the durable compatibility target is the configured workflow
described above.
