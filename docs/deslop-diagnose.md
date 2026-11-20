# Deslop Diagnose record

Bounded manual review of the riskiest boundaries at `69d397c` (base `6cf0b94` plus test-only fixes): PTY lifecycle, escape-sequence parsing, session and socket handling, and resource bounds. `ultrareview` was not run; the owner asked for a manual review that reproduces each defect against the built product (`target/debug/tm`, macOS arm64, isolation prefix). Findings are in `TODO-DESLOP.md`.

## Defects reproduced

- **DSLP-8, accept failure ends the daemon.** `serve` in `src/server.rs` turns any `accept` error other than `Interrupted` or `WouldBlock` into an exit of the whole daemon, and a failed connection-thread spawn does the same. `handle_connection` has no read deadline and no connection limit. Reproduction: `(ulimit -n 256; tm -S "$S" new-session -d "sleep 60")`, then hold about 250 idle `AF_UNIX` connections to `$S` from a Python script (`connect` is refused after 247). `tm -S "$S" list-sessions` prints `no server running`; the daemon log ends with `daemon: Too many open files (os error 24)`. With a limit of 64, 100 connections were enough. Every session and pane was lost. The same error is reachable without a hostile client when panes use the descriptor limit and the next `tm` invocation connects.
- **DSLP-9, unbounded SGR mouse buffer.** `feed_client_mouse` appends every byte to `client.mouse_buffer` once input begins with `ESC [ <`, and flushes it only at an `M` or `m` or an invalid prefix. A real SGR report is under 20 bytes. Reproduction: `set-option -g mouse on`, attach a client in a PTY, write `ESC [ <` and then 64 MiB of `1`. The daemon's resident size grew from 4.7 MiB to 117 MiB (about 1.8 bytes per input byte) and keeps growing with further input.
- **DSLP-10, sidecar files follow symlinks.** `DaemonLog::create` opens `<socket>.log` with `create`, `truncate` and no `O_NOFOLLOW`, then `set_permissions`; the lock file does the same (`run_daemon_inner`). With `$D/s.sock.log` a symlink to a file, `tm -S "$D/s.sock" new-session -d` replaced that file's contents (`precious data`) with the daemon log. With `$D/s.sock.lock` a symlink to a mode 644 file, the target became mode 600. This matters when `-S` names a path in a shared writable directory; the default socket directory is private.

## Reviewed without a defect

- Escape parsing: nine sequences differenced against tmux 3.8 through `capture-pane -p` gave identical bytes (CSI aborted by ESC, CAN inside CSI, OSC ended by CSI, DCS with an ST, REP, cursor movement past the margin with no-wrap, charset designation, UTF-8 selection). `OutputState` caps pending bytes at 1 MiB and control sequences at 4 KiB; both are tested in `src/terminal.rs`.
- Frame decoding: `read_frame` rejects zero and over-16 MiB lengths; strings, counts and offsets are checked before slicing. Its zeroed 16 MiB allocation per connection is lazy and was not shown to cost resident memory.
- Pane input and reply queues are bounded (16 MiB and 64 KiB) and tested in `src/pty.rs`.
- A pane that ignores `SIGHUP` and `SIGTERM` survives `kill-session` as an orphan; tmux 3.8 sends the same signals, so this is parity. The daemon exited after its last session.
