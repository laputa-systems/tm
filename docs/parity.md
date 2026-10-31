# Parity matrix

The contract is pinned to tmux 3.8, built by
[`tests/support/install-tmux-ref.sh`](../tests/support/install-tmux-ref.sh) from
the verified release tarball recorded in
[`tests/reference/PROVENANCE`](../tests/reference/PROVENANCE). A row may turn
`pass` only after a scenario runs against that reference and the linked test
proves both the selected behavior and the prohibited behavior when the option
is off.

Status values are `pass`, `fail`, and `blocked`. `fail` has a reproducing test;
`blocked` names the missing environment or prerequisite.

## Options

| Contract item | Selected semantics | Scenario | Implementation | Test | Status | Intentional difference |
| --- | --- | --- | --- | --- | --- | --- |
| `default-terminal` | Pane `TERM=tmux-256color` | R2 | `src/server.rs` environment | pending | fail | none |
| `terminal-features` | `tmux-256color:Sync:clipboard` capability matching | C1 | `src/terminal.rs` | pending | fail | none |
| `set-clipboard` | `external`: tm copies use OSC 52, application OSC 52 ignored | R5 | `src/server.rs` | pending | fail | none |
| `extended-keys` | `on`: pane-requested CSI-u input | I3 | `src/server.rs` | pending | fail | none |
| `extended-keys-format` | `csi-u` encoding for extended panes | I3 | `src/server.rs` | pending | fail | none |
| `monitor-bell` | `on`: record bell for status | R5 | `src/server.rs` | pending | fail | none |
| `bell-action` | `any`: alert all clients of the session | R5 | `src/server.rs` | pending | fail | none |
| `visual-bell` | `off`: audible BEL only | R5 | `src/server.rs` | pending | fail | none |
| `escape-time` | `0`: no partial prefix or escape held | I1 | `src/server.rs` | pending | fail | none |
| `focus-events` | `on`: forward focus to requesting panes | I3 | `src/server.rs` | pending | fail | none |
| `prefix` | `C-a` | T1 | `src/config.rs` | pending | fail | none |
| `mouse` | `on`: route pointer events | I3 | `src/server.rs` | pending | fail | none |
| `history-limit` | `10000` retained lines | R1 | `src/terminal.rs` | pending | fail | none |
| `mode-keys` | `emacs` copy-mode keys | R3 | `src/copy_mode.rs` | pending | fail | none |
| `base-index` | `1` | W3 | `src/model.rs` | pending | fail | none |
| `renumber-windows` | `on` | W3 | `src/server.rs` | pending | fail | none |
| `automatic-rename` | `off` | W3 | `src/server.rs` | pending | fail | none |
| `status-position` | `bottom` | R4 | `src/server.rs` | pending | fail | none |
| `status-bg` | `black` | R4 | `src/server.rs` | pending | fail | none |
| `status-fg` | `white` | R4 | `src/server.rs` | pending | fail | none |
| `window-status-format` | `#[dim]#I:#W#{?window_zoomed_flag, (Z),}` | R4 | `src/server.rs` | pending | fail | none |
| `window-status-current-format` | `#[fg=green]#I:#W#{?window_zoomed_flag, (Z),}` | R4 | `src/server.rs` | pending | fail | none |
| `status-left` | `#{?client_prefix,#[fg=yellow],}(#S) ` | R4 | `src/server.rs` | pending | fail | none |
| `status-left-length` | `32` | R4 | `src/server.rs` | pending | fail | none |
| `status-right` | empty | R4 | `src/server.rs` | pending | fail | none |
| `monitor-activity` | `off` | W3 | `src/server.rs` | pending | fail | none |
| `set-titles` | `off` | R2 | `src/server.rs` | pending | fail | none |

## Bindings

| Contract item | Selected semantics | Scenario | Implementation | Test | Status | Intentional difference |
| --- | --- | --- | --- | --- | --- | --- |
| `n` | prompt for name, new window | T1 | `src/config.rs` | pending | fail | none |
| `p` | display panes | T1 | `src/config.rs` | pending | fail | none |
| `Enter` | display compiled-profile message | T1 | `src/config.rs` | pending | fail | none |
| `C-s` | send two literal prefix bytes | T1 | `src/config.rs` | pending | fail | none |
| `/` | command prompt | T1 | `src/config.rs` | pending | fail | none |
| `r` | prompt rename window | T1 | `src/config.rs` | pending | fail | none |
| `C-Left` | previous window, repeating | T1 | `src/config.rs` | pending | fail | none |
| `C-Right` | next window, repeating | T1 | `src/config.rs` | pending | fail | none |
| `Left` | swap and select previous window, repeating | T1 | `src/config.rs` | pending | fail | none |
| `Right` | swap and select next window, repeating | T1 | `src/config.rs` | pending | fail | none |
| `\` | split horizontal in current path, even-horizontal | W1 | `src/config.rs` | pending | fail | none |
| `-` | split vertical in current path, even-vertical | W1 | `src/config.rs` | pending | fail | none |
| `z` | toggle pane zoom | W1 | `src/config.rs` | pending | fail | none |
| `k` | kill pane, then display | T2 | `src/config.rs` | pending | fail | none |
| `m` | prompt join target | T1 | `src/config.rs` | pending | fail | none |
| `C-n` | break pane to next index | T1 | `src/config.rs` | pending | fail | none |
| `%` | split horizontal | W1 | `src/config.rs` | pending | fail | none |
| `"` | split vertical | W1 | `src/config.rs` | pending | fail | none |
| `h` | select pane left | W1 | `src/config.rs` | pending | fail | none |
| `j` | select pane down | W1 | `src/config.rs` | pending | fail | none |
| `l` | select pane right | W1 | `src/config.rs` | pending | fail | none |
| `x` | kill pane | T2 | `src/config.rs` | pending | fail | none |
| `&` | kill window | T2 | `src/config.rs` | pending | fail | none |
| `c` | new window | W3 | `src/config.rs` | pending | fail | none |
| `b` | send prefix | I1 | `src/config.rs` | pending | fail | none |
| `[` | copy mode | R2 | `src/config.rs` | pending | fail | none |
| `d` | detach | L2 | `src/config.rs` | pending | fail | none |

## Interactive modes

| Contract item | Selected semantics | Scenario | Implementation | Test | Status | Intentional difference |
| --- | --- | --- | --- | --- | --- | --- |
| command prompt | tokenized template, `%%` replaced with raw input | T1 | `src/server.rs` | pending | fail | none |
| display-panes | pane number overlay for `display-panes-time` | T1 | `src/server.rs` | pending | fail | none |
| choose-tree | choose a session/window from an attached client | T1 | `src/server.rs` | pending | fail | none |
| choose-window | choose a window from an attached client | T1 | `src/server.rs` | pending | fail | none |
| choose-client | choose an attached client | T1 | `src/server.rs` | pending | fail | none |
| choose-buffer | choose a paste buffer | T1 | `src/server.rs` | pending | fail | none |
| copy mode | vi/emacs navigation, selection, copy actions | R3 | `src/copy_mode.rs` | pending | fail | none |

## CLI and lifecycle

| Contract item | Selected semantics | Scenario | Implementation | Test | Status | Intentional difference |
| --- | --- | --- | --- | --- | --- | --- |
| formats | tmux format expansion for supported names | T1 | `src/command.rs` | pending | fail | none |
| targeting | omitted and relative targets resolve in client context | T1 | `src/server.rs` | pending | fail | none |
| buffers | set/show/paste/delete buffers | R5 | `src/server.rs` | pending | fail | none |
| capture | capture-pane visible and history ranges | R1 | `src/server.rs` | pending | fail | none |
| copy mode | selection, copy, search, cursor | R3 | `src/copy_mode.rs` | pending | fail | none |
| session lifecycle | create, rename, kill, attach, detach | L2 | `src/server.rs` | pending | fail | none |
| window lifecycle | create, rename, select, swap, kill | W3 | `src/server.rs` | pending | fail | none |
| pane lifecycle | split, break, join, move, swap, kill | T2 | `src/server.rs` | pending | fail | none |
| attached terminal | alternate screen, modes, restoration | C1 | `src/client.rs` | pending | fail | none |
| input | prefix, repeat, escape, paste, extended keys | I1 | `src/server.rs` | pending | fail | none |
| daemon lifecycle | startup lock, private socket, diagnostics | L2 | `src/server.rs` | pending | fail | none |
| signals | restore termios and screen on handled signals | L3 | `src/client.rs` | pending | fail | none |

## Baseline

Recorded 2026-10-30 from commit `05f4737` before the first product slice.

- `make test`: green, 20 s.
- `make lint`: green, 4 s.
- `make test-ci`: green, 29 s.
- `cargo test --test attached_client_pty`: green, 2 s.
- `make perf`: 28.7 µs (80×24, 1 pane), 32.3 µs (80×24, 2), 76.2 µs (120×40, 4), 327.1 µs (delta, 120×40, 4).
- Reference probe: `tmux 3.8` from the verified tarball, private `-S` socket, ambient multiplexer variables cleared.
