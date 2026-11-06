Prefix: TMUX
Next: 72

TMUX-1, TMUX-2, TMUX-3, TMUX-5, TMUX-6, TMUX-7, TMUX-8, TMUX-9, TMUX-10, TMUX-16, TMUX-29, TMUX-38, TMUX-39, and TMUX-40 are closed
with tests and commits recorded in git history. TMUX-41, TMUX-42, and TMUX-43 are accepted as intentional differences
(`vt100` grid limits listed in `INTENTIONAL_DIFFERENCES`). Add a new entry here only for a reproduced defect that the current
suites do not cover.

### TMUX-50 · P1 · The default socket in shared /tmp trusts whoever listens there

- **Area:** Security
- **Where:** `src/server.rs:55-64`, `src/server.rs:88-98`, `src/server.rs:185-192`, `src/client.rs:181-184`
- **Problem:** Without `TMPDIR` (Linux default) the socket is `/tmp/tm-<euid>.sock` in sticky world-writable `/tmp`; `ensure_socket_parent` makes no private directory because `/tmp` exists. The client connects with no check of the socket's owner, and the daemon never checks the peer uid. Another local user who binds that name first receives every command and the keystrokes of each later `attach`, and can write arbitrary terminal bytes back; a pre-created regular file at the name, `.lock` or `.log` makes the victim's daemon fail to start, because the sticky bit stops the victim removing it. `bind` creates the socket under the umask before the chmod to 0600. Proven by reading the connect and bind paths; no second uid was available to run the attack. This host has `protected_symlinks=1` (mitigates symlink variants) and `protected_regular=0`.
- **Recommendation:** Default to a per-user directory created 0700 (`$XDG_RUNTIME_DIR/tm/` or `/tmp/tm-<uid>/`), verified with `symlink_metadata` to be owned by the euid with no group or other bits; have the client require the socket's owner to be the euid before sending; bind under a restrictive umask.
- **Fix with:** os-boundaries
- **Effort:** M

### TMUX-51 · P1 · After an upgrade the documented kill-server cannot stop the old daemon

- **Area:** Operability
- **Where:** `src/client.rs:138-141`, `src/client.rs:143-168`, `src/server.rs:425-432`, `src/protocol.rs:7`, `tests/protocol_version.rs:31-48`
- **Problem:** A new client sends its protocol version first; an older daemon answers with its own and hangs up before reading a request. The mismatch message tells the user to run `tm -S <socket> kill-server`, which goes through the same handshake and fails the same way. Proven: a daemon started from `target/dist/tm` refused both `ls` and `kill-server` from `target/debug/tm` with that message (rc 1); only the old binary's `kill-server` worked. `PROTOCOL_VERSION` changed about 12 times in the last 180 commits, and every change strands running daemons, whose only exit is finding the pid and signalling it. The mismatch test never sends `kill-server` to a mismatched daemon.
- **Recommendation:** Make shutdown version-independent: on a mismatch for `kill-server`, read the daemon pid with `SO_PEERCRED` and send SIGTERM (already handled gracefully), or accept a frozen shutdown frame before the version check. Fix the message to name a command that works, and test it against a mismatched daemon.
- **Fix with:** durable-state
- **Effort:** S

### TMUX-52 · P2 · `-S` naming a regular file deletes it

- **Area:** Data
- **Where:** `src/server.rs:185-190`
- **Problem:** When connecting to the socket path fails, the daemon removes whatever exists there without checking that it is a socket. Proven: `tm -S precious.txt new-session -d` replaced a regular file with content by a socket and created `.lock` and `.log` beside it. A mistyped `-S` or `TM_SOCKET` destroys user data.
- **Recommendation:** Check `symlink_metadata`; unlink only a socket owned by the euid, otherwise fail with an error naming the path.
- **Fix with:** os-boundaries
- **Effort:** S

### TMUX-53 · P2 · Pane programs run with SIGPIPE ignored

- **Area:** Errors
- **Where:** `src/pty.rs:258-302`
- **Problem:** The Rust runtime ignores SIGPIPE, and the forked child resets the signal mask but not dispositions, so the ignore survives `exec`. Proven: `/proc/self/status` in a pane shows `SigIgn: 0x1000`, and `(cat /dev/zero; echo $?) | head -c10` prints a write error with status 1 inside tm instead of exiting silently with 141. Programs that ignore write errors can spin after their reader exits.
- **Recommendation:** Reset SIGPIPE (and any other runtime-ignored signal) to `SIG_DFL` in the child before `exec`.
- **Fix with:** lifecycle-concurrency
- **Effort:** S

### TMUX-54 · P2 · An oversized window size aborts the daemon and every session

- **Area:** Errors
- **Where:** `src/model.rs:20-25`, `src/server.rs:692`, `src/server.rs:8745`, `src/protocol.rs:1587-1589`
- **Problem:** `Size::bounded` enforces only a minimum, and sizes from `new-session -x/-y`, attach and resize frames reach `vt100::Parser::new`, which allocates the whole grid. Proven: under `ulimit -v 4GB`, `tm new-session -d -x 20000 -y 20000` killed the daemon and an unrelated session; `-x 65535 -y 65535` without the limit made the daemon exit. A typo or any same-user process loses all sessions.
- **Recommendation:** Make the maximum part of the size type (tmux caps at 10000) and reject larger values at the command and decode boundaries with an error.
- **Fix with:** make-invalid-states-unrepresentable
- **Effort:** S

### TMUX-55 · P2 · run-shell output and pane input are buffered without bound

- **Area:** Concurrency
- **Where:** `src/server.rs:513-523`, `src/server.rs:7305-7312`, `src/pty.rs:126`, `src/server.rs:12435-12438`
- **Problem:** `run_shell_process` uses `Command::output()`, holding all helper stdout in the daemon, and `run-shell -b` then discards it. Proven: `run-shell -b yes` grew daemon RSS about 285 MB per second until the address-space limit. The pane input channel is an unbounded `mpsc::channel`, and terminal query replies queue there while a pane does not read stdin (about 2 MB/s growth under a DA-query flood on a debug build).
- **Recommendation:** Send background output to null, cap foreground output, and bound the pane input channel, coalescing or dropping query replies when full.
- **Fix with:** lifecycle-concurrency
- **Effort:** S

### TMUX-56 · P2 · A bound or prompted run-shell blocks every session while it runs

- **Area:** Concurrency
- **Where:** `src/server.rs:651`, `src/server.rs:4948-4951`, `src/server.rs:7282-7320`, `src/server.rs:455-477`
- **Problem:** The CLI path runs foreground `run-shell` outside the state lock, but key bindings and the command prompt reach `run_shell` from the attach input thread while it holds `state.lock()`, and run `Command::output()` and the `-d` sleep inline. Every pane reader, renderer and command waits until the script ends. Unproven: traced end to end by reading; not run in an attached client.
- **Recommendation:** Run bound and prompted `run-shell` on a detached thread, or release the lock around the spawn as the CLI path does.
- **Fix with:** lifecycle-concurrency
- **Effort:** M

### TMUX-57 · P2 · A daemon whose socket was removed cannot exit

- **Area:** Concurrency
- **Where:** `src/server.rs:318-326`, `src/server.rs:384-397`, `src/server.rs:493-495`
- **Problem:** Shutdown wakes the blocking `accept` by connecting to the public socket path and ignores failure. Proven: after `rm` of the socket, a daemon whose last session ended stayed alive, and SIGTERM was logged but did not stop it; only SIGKILL did. With `ulimit -n 16` the daemon and a `kill-server` call hung until another connection arrived (fd exhaustion as the cause is unproven). `/tmp` cleaners remove idle sockets the same way.
- **Recommendation:** Wake the accept loop with a self-pipe or eventfd and `poll` (or a nonblocking listener) instead of connecting to the path.
- **Fix with:** lifecycle-concurrency
- **Effort:** M

### TMUX-58 · P2 · Copy mode copies the whole scrollback for every key, frame and mouse event

- **Area:** Performance
- **Where:** `src/copy_mode.rs:1244-1262`, `src/copy_mode.rs:2246`, `src/copy_mode.rs:2369-2388`, `src/copy_mode.rs:1151`, `src/server.rs:9220-9240`, `src/server.rs:10344`, `src/server.rs:3687-3727`, `src/server.rs:14484`
- **Problem:** `history_rows` builds a `Vec<String>` of all scrollback and is called at least twice per operation; a `-s` source pane replays its retained bytes through a new parser each time. Measured on a release build: `send-keys -X cursor-up` takes about 66-70 ms per command with the default 10000-line history (200 commands in 13.3, 13.7 and 14.1 s) against 0.5 ms with 100 lines. Frames run the same helpers under the global lock (frame cost unproven). The render benchmark uses two-line panes, so its 16 ms gate cannot see this.
- **Recommendation:** Read history length and rows from the live parser without mutating its viewport (or cache a row index invalidated on output), drop the per-call replay parsers, and add a benchmark fixture with 10000 history rows in copy mode.
- **Fix with:** make-it-fast
- **Effort:** M

### TMUX-59 · P2 · Every format expansion replays the pane's whole output

- **Area:** Performance
- **Where:** `src/server.rs:14671`, `src/server.rs:14258-14273`, `src/server.rs:14240`, `src/server.rs:12548-12555`
- **Problem:** `format_pane` eagerly builds `__pane_content` by replaying up to the raw-output budget (about 1.9 MB per pane at 80 columns, capped at 32 MiB) through a new parser, though one reader uses it. Measured on a release build with 10000 history lines: `display-message -p -t c x` has a median of 28.5 ms (26.6-30.4, 20 samples) against 0.5 ms with 100 lines. Bindings, `run-shell` formats and choose-tree all expand pane formats.
- **Recommendation:** Compute pane content only for the consumer at `src/server.rs:14240`, or lazily when the format references it.
- **Fix with:** make-it-fast
- **Effort:** S

### TMUX-11 · P3 · Session format variables are missing or wrong

- **Area:** Formats
- **Where:** `src/server.rs` session format values
- **Problem:** `#{session_group}`, `#{session_created}`, `#{t:session_created}`, and `#{session_width}` expand to nothing in tm, and `#{session_group_size}` is `1` for ungrouped sessions; tmux 3.8 expands them.
- **Recommendation:** Add the values with a `compare_commands` probe of `display-message -p` for each.
- **Fix with:** reference-parity
- **Effort:** S

### TMUX-12 · P3 · Default pane_title is empty instead of the hostname

- **Area:** Formats
- **Where:** `src/server.rs` pane title initialisation
- **Problem:** tmux 3.8 sets a new pane's title to the host name; tm leaves `#{pane_title}` empty until the program sets one.
- **Recommendation:** Initialise the title from `gethostname` and add a `display-message -p '#{pane_title}'` probe against the reference.
- **Fix with:** reference-parity

### TMUX-13 · P3 · pane_current_command reports the start command

- **Area:** Formats
- **Where:** `src/server.rs` pane format values
- **Problem:** tm reports the pane's start command; tmux 3.8 reports the pane's foreground process name, so it changes when the shell runs a program.
- **Recommendation:** Read the foreground process group of the pane's PTY (`tcgetpgrp`) and its `/proc/<pid>/comm`; probe with a shell that execs `sleep`.
- **Fix with:** reference-parity

### TMUX-15 · P3 · resize-pane differs with three or more panes along one axis

- **Area:** Layout
- **Where:** `src/model.rs` `Layout::resize`
- **Problem:** tm lays panes out as a binary tree and moves only the border of the nearest enclosing split. tmux keeps one list of siblings and passes the remaining change on to the next siblings, so `resize-pane -R`, `-L`, `-x` in a row of three panes ends with different widths. The same tree gives a removed middle pane's space to the previous sibling where tmux gives it to the next (`kill-pane`, `break-pane`, `join-pane -s` in a window of three panes). `tests/tmux_histories.rs` `Exclusion::PaneRemovalFromWindowWithThreeOrMorePanes` stops generating them.
- **Recommendation:** Resize over the flattened sibling run of the same axis, with a differential probe for rows of three and four panes.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-17 · P3 · history_size and history_limit formats expand to nothing

- **Area:** Formats
- **Where:** `src/server.rs` pane format values
- **Problem:** `#{history_size}` and `#{history_limit}` are empty in tm; tmux 3.8 expands them to the lines of history and the option value.
- **Recommendation:** Add the values with a `display-message -p` probe after output and after `clear-history`.
- **Fix with:** reference-parity
- **Effort:** S

### TMUX-18 · P3 · display-panes is a client overlay in tm and a pane mode in tmux

- **Area:** Modes
- **Where:** `src/server.rs` `enter_panes_mode`
- **Problem:** tmux 3.8 puts the target pane in panes mode even with no client attached, and the mode zooms the pane. tm needs an attached client. `Request::DisplayPanes` still carries `source`, `no_zoom`, `no_mode`, and `kill_on_exit`, which the CLI can no longer set.
- **Recommendation:** Drop the unused fields and rebuild the mode on the pane, then compare it with a raw-PTY attach.
- **Fix with:** reference-parity
- **Effort:** L

### TMUX-20 · P3 · The format comparison operators compare numbers, tmux compares strings

- **Area:** Formats
- **Where:** `src/server.rs` `compare_format_values`
- **Problem:** `#{<:a,b}`, `#{>:a,b}`, `#{<=:a,b}`, and `#{>=:a,b}` compare numerically when both sides parse as numbers; tmux 3.8 uses `strcmp`, so `#{>:5,10}` is true there. `list-buffers -f '#{>:#{buffer_size},10}'` lists a 5-byte buffer in tmux and not in tm.
- **Recommendation:** Compare as strings and check `#{==:}`, `#{!=:}`, and the four orderings with a differential `display-message -p` probe.
- **Fix with:** reference-parity
- **Effort:** S

### TMUX-22 · P3 · send-keys knows few key names and no modifier combinations

- **Area:** Keys
- **Where:** `src/command.rs` `key_bytes`
- **Problem:** tm types `BSpace`, `DC`, `IC`, `Home`, `End`, `PageUp`, `PageDown`, `PgUp`, `PgDn`, `NPage`, `PPage`, `Insert`, `F1` to `F12`, `KP0`, `KP/`, `KPEnter`, `S-Tab`, `M-Enter`, `M-Space`, `C-Up`, `M-Up`, `S-Up`, `C-S-a`, `M-C-a`, `C-1`, `C-?`, `^a`, and any key name whose case differs (`enter`) as text, and sends `Home` and `End` as `ESC [ H` and `ESC [ F`. tmux 3.8 sends the key (`Home` is `ESC [ 1~`, `F1` is `ESC O P`, `C-Up` is `ESC [ 1;5 A`).
- **Recommendation:** Port tmux's key name table and the `input_key` encoding (cursor and keypad modes, modifier parameters), then probe every name against the reference.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-24 · P3 · window_layout and the default list-panes line differ from tmux

- **Area:** Formats
- **Where:** `src/server.rs` `format_pane`, `list_panes`
- **Problem:** `#{window_layout}` expands to nothing; tmux 3.8 prints a JSON layout such as `{"V":2,"L":{"t":"p","w":80,"h":24,"x":0,"y":0,"a":true,"i":0,"I":"%4"}}`, so the default `list-windows` line differs in its `[layout ...]` column. The default `list-panes` line is `%4: 0 *` in tm and `0: [80x24] [history 0/10000, 960 bytes] %4 (active)` in tmux.
- **Recommendation:** Add `window_layout` from the layout tree and port the default `list-panes` template, then drop the layout exception in `windows_list_windows_default_format_matches_except_the_layout`.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-26 · P3 · Copy mode position indicator and copy formats differ from tmux

- **Area:** Copy mode
- **Where:** `src/server.rs` copy-mode rendering and format expansion
- **Problem:** On an attached client, tmux 3.8 draws `HH:MM [0/n]` on the first row (the time of the top line); tm draws `[0/n]` only. `#{copy_cursor_y}` is the screen row in tmux (7 on an 8-row pane) and the history-absolute row in tm (30). `#{scroll_position}` is empty outside a mode in tmux and `0` in tm.
- **Recommendation:** Port `copy-mode-position-format` with `top_line_time`, make `copy_cursor_y` viewport-relative, and print nothing for `scroll_position` outside a mode, each with a differential probe.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-27 · P3 · Attached base screen differs from tmux after the client resizes the window

- **Area:** Rendering
- **Where:** `src/server.rs` client attach resize and status rendering
- **Problem:** A session created at 40x8 and attached by a 60x10 client shows 7 of the last output rows in tm and 8 in tmux, and `set-option -g status-left MARK` renders `MARK 1:sh` in tm and `MARK1:sh` in tmux (tm pads the left status).
- **Recommendation:** The left-status padding is fixed (H2). Fix the resize row loss: when a window with output history grows, tmux pulls more rows from history. `ResizeOfScrolledWindow` in `tests/tmux_histories.rs` stops generating the shape.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-28 · P3 · choose-tree, choose-buffer and choose-client are client modes in tm

- **Area:** Modes
- **Where:** `src/server.rs` tree, buffer and client modes
- **Problem:** tm has no `choose-window` command (it exits 1 with `unknown command`; tmux 3.8 opens tree mode with the windows collapsed). tmux 3.8 puts these modes on the target pane (`#{pane_in_mode}` is 1, no client needed) with `(n)` shortcut lines and a template run at the selection. tm opens its own list on an attached client, renders different text, and C6c therefore rejects `-F`, `-f`, `-O`, `-r`, `-k`, `-h`, `-G`, `-N`, `-Z`, `-K`, `-s`, `-w`, `-y`, `-i` and the template.
- **Recommendation:** Rebuild the modes on the pane-mode model with the tmux line format, then re-enable the flags one at a time with raw-PTY differential tests.
- **Fix with:** reference-parity
- **Effort:** L

### TMUX-30 · P3 · A window moved to a session of another size is resized to it

- **Area:** Layout
- **Where:** `src/server.rs` `relink_window`, `break_pane`, `reflow_session`
- **Problem:** tm sizes every window to its session, so `move-window`, `swap-window` and `break-pane` between detached sessions of different sizes resize the moved window. tmux keeps the window's size until a client attaches.
- **Recommendation:** Give a window its own size and resize it only for attached clients, with a differential history of cross-session moves between sessions of different sizes. `tests/tmux_histories.rs` `Exclusion::SessionSizeVariation` stops generating them.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-31 · P3 · Malformed targets are parsed differently from tmux

- **Area:** Targets
- **Where:** `src/server.rs` `resolve_session_index`, `resolve_window_target`, `resolve_pane_target`
- **Problem:** tm splits `a.b.c` at the last dot where tmux splits at the first, strips a leading `=` from error text, accepts `::` as a session, reads `a:b:c` and `zz:zz.zz` as a session named `a:b:c` or `zz:zz.zz`, resolves a bare `-` or `+` to a different window than tmux, and words `@`, `@x`, and `..` errors for a different object (`can't find pane: @x` against tmux's `can't find window: @x`).
- **Recommendation:** Replace the three resolvers with one parser that follows tmux's `cmd_find_target` grammar, with a differential probe per malformed shape. `tests/tmux_histories.rs` `Exclusion::MalformedTargetGrammar` lists the shapes it stops generating.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-32 · P3 · A relative window slot places new and moved windows differently

- **Area:** Windows
- **Where:** `src/server.rs` `relink_window`, `new_window`, `break_pane`
- **Problem:** `new-window`, `move-window` and `break-pane` with a `+N` or `-N` slot (for example `move-window -a -s c:x -t c:+`) moves window `x` after the window at `+`; tmux leaves it at index 2 where tm moves it to 3, because tmux resolves a `+N` or `-N` target to an index without a window and does not shuffle.
- **Recommendation:** Treat an offset target that names no window like an absent index in `relink_window`, with a differential probe for `-a` and `-b` with `+N` and `-N`. `tests/tmux_histories.rs` `Exclusion::RelativeWindowPlacement` stops generating the shape; the relative targets also leave other window state (such as the last-window flag) different afterwards.
- **Fix with:** reference-parity
- **Effort:** S

### TMUX-33 · P3 · A SIGKILLed client leaves its terminal in raw mode and the alternate screen

- **Area:** Clients
- **Where:** `src/client.rs` terminal setup and restore
- **Problem:** tmux's server owns the client's tty, so after `kill -9` of an attached client the server resets the alternate screen, bracketed paste, mouse modes and termios. tm's client restores its own terminal and cannot when killed.
- **Recommendation:** Pass the tty descriptor to the server, or have the server write the restore sequence and termios to the client's tty on connection loss; cover it with the raw-PTY `KillClientRendering` history.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-34 · P3 · Windows that no client displays are resized

- **Area:** Layout
- **Where:** `src/server.rs` `resize_session`
- **Problem:** tm resizes every window of a session to its client; tmux 3.8 resizes only the displayed window of an attached session and leaves other windows at their size until they are selected.
- **Recommendation:** Resize the active window on attach and resize, and the newly selected window on selection, then drop the `UnattachedWindowSize` exclusion in `tests/tmux_histories.rs`.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-35 · P3 · Two clients of different sizes on one session are drawn differently

- **Area:** Rendering
- **Where:** `src/server.rs` attach rendering
- **Problem:** With `window-size latest`, tmux draws a client larger than the window with a filler area and a client smaller than the window cropped; tm draws neither.
- **Recommendation:** Render the window at its size inside the client, with tmux's filler and crop rules, and drop the `SharedSessionClientSizes` exclusion.
- **Fix with:** reference-parity
- **Effort:** L

### TMUX-37 · P3 · Resizing a window with several panes divides the space differently

- **Area:** Layout
- **Where:** `src/model.rs` layout resize
- **Problem:** When a client attach or resize changes the size of a window with two or more panes, tm and tmux round the pane rows or columns differently (a 3-pane column at 9 rows is 2/1/4 in tm and 2/2/3 in tmux).
- **Recommendation:** Redistribute the change over siblings as tmux's `layout_resize` does (see TMUX-15), and drop the `ResizeOfSplitWindow` exclusion.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-44 · P3 · resize-window is not implemented

- **Area:** Commands
- **Where:** `src/command.rs`
- **Problem:** tmux 3.8 has `resize-window` (`-x`, `-y`, `-A`, `-D`, `-U`, `-L`, `-R`); tm has no such command, so a detached session cannot be resized without an attached client. `tests/tmux_unicode_history.rs` resizes through attached raw-PTY clients instead.
- **Recommendation:** Add the command with the `window-size` option semantics and a differential test of each flag.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-45 · P3 · A window target given as a pane id is rejected

- **Area:** Targeting
- **Where:** `src/server.rs` `resolve_window_target`
- **Problem:** tmux 3.8 resolves a `%<pane id>` window target to the window holding that pane, so `kill-window -t %1` succeeds. tm exits 1 with `can't find window: %1`, while `display-message -t %1` accepts the same pane id.
- **Recommendation:** Resolve a `%` window target through the pane's window, and add a differential probe for each window-targeting command.
- **Fix with:** reference-parity

### TMUX-46 · P3 · The status window list omits a window that overflows the line

- **Area:** Status
- **Where:** `src/server.rs` status rendering
- **Problem:** On a 30-column client in a session with four windows, tmux 3.8 draws `(b) 1:sh 2:inpane 3:sleep 4:i>`, truncating the last window with `>`; tm draws `(b) 1:sh 2:inpane 3:sleep` and drops the fourth window. Seen as a screen difference in attached history seed 33 before the in-pane shell window was shortened.
- **Recommendation:** Reproduce on the raw-PTY runner with a differential screen test, then truncate the overflowing window entry as tmux does.
- **Fix with:** reference-parity

### TMUX-47 · P3 · Short format aliases are not expanded in window names

- **Area:** Formats
- **Where:** `src/server.rs` `rename_window`, `new_window`
- **Problem:** `rename-window '#S-#I'` names the window `#S-#I` in tm and `S-1` in tmux 3.8, and `new-window -n '#S-x'` likewise. The command prompt (`prefix r`) takes the same path. The long forms (`#{session_name}`) expand in both.
- **Recommendation:** Expand the single-letter aliases (`#S`, `#I`, `#W`, `#P`, `#F`, `#H`, `#h`, `#D`, `#T`) in the shared format expander and cover `rename-window`, `rename-session`, `new-window -n` and `new-session -s` with differential probes.
- **Fix with:** reference-parity
- **Effort:** S

### TMUX-49 · P3 · Copy-mode search prompt and formats differ

- **Area:** Modes
- **Where:** `src/copy_mode.rs` prompt, `src/server.rs` pane formats
- **Problem:** After `C-s` and `o` in copy mode tmux 3.8 draws `(search down) o` on the pane's last row and `[0/0] (3 results)` at the top right; tm draws `(search) o` on the status row and `[0/0]`. `#{search_present}` is unset in tm, `#{copy_cursor_word}` is empty where tmux gives the word, and `#{selection_present}` is 1 in tm for a zero-width selection where tmux gives 0.
- **Recommendation:** Move the prompt to the pane, count the matches, and add the formats, each with a raw-PTY differential test.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-60 · P3 · server.rs holds 20k lines in one module and one impl

- **Area:** Architecture
- **Where:** `src/server.rs:1596-9987` (one `impl ServerState`, 488 functions), `src/server.rs:11739` (`execute_request_inner`, about 550 lines), `src/server.rs:15677-20206` (inline tests)
- **Problem:** 78% of the source is one file, so every change, review and conflict lands there, and helpers such as `copy_source_parser` are reused across unrelated paths without an owner (TMUX-58 and TMUX-59 grew that way). Cohesive clusters already exist: attached input and mouse (about 2378-3700), choose modes (about 3900-6200), prompt execution (about 4650-5200), buffers, options and environment (about 6226-6900), window and layout commands (about 7700-9000), rendering (about 9044-9980 and 10574-11720), the PTY reader (about 12292-12690) and the format engine (from 12732).
- **Recommendation:** Move `mod tests` to `server/tests.rs`, then rendering to `server/render.rs` and the format engine to `server/format.rs`; child modules keep access to private `ServerState` fields, so the state needs no redesign.
- **Fix with:** none
- **Effort:** M

### TMUX-61 · P3 · Option defaults and precedence live at each reader

- **Area:** Config
- **Where:** `src/options.rs:20-26`, `src/config.rs:4`, `src/server.rs:1609`, `src/server.rs:3637-3700`, `src/server.rs:10669-10679`, `src/server.rs:10777-10873`, `src/client.rs:264`, `src/client.rs:388`, `src/client.rs:432`
- **Problem:** Options are `HashMap<String, String>`; `OptionSpec` has no default, so defaults for options outside `COMPILED_OPTIONS` are `unwrap_or` literals at read sites (the scrollbar style default appears twice), and parsing repeats at each read. Precedence differs: the copy-mode scrollbar hit test reads window then global options, while rendering reads pane, window, global, so a `set -p pane-scrollbars-style` draws a bar the mouse does not hit. The client derives terminal setup from compiled `terminal-features` in three places while the daemon renders with the live option (whether the sequences diverge in practice is unproven).
- **Recommendation:** Give `OptionSpec` a default, resolve through one effective-option function with typed accessors, and have the client take terminal features from the daemon on attach.
- **Fix with:** single-source-of-truth
- **Effort:** L

### TMUX-62 · P3 · A daemon crash leaves no trace, and transient errors end the daemon

- **Area:** Operability
- **Where:** `src/client.rs:194-196`, `src/server.rs:106-154`, `src/server.rs:148`, `src/server.rs:182`, `src/server.rs:387-396`, `src/client.rs:411-425`, `src/main.rs:13`
- **Problem:** Panics abort and daemon stderr goes to `/dev/null` with no panic hook, so a crash loses every session and its message. The log is truncated at each start before the flock, so a racing second start truncates a live daemon's log. A transient `accept` error (EMFILE) or thread-spawn failure ends `serve`. Attach errors returned early skip the client's screen restore. Non-UTF-8 argv aborts the client (proven: SIGABRT, rc 134 from `std::env::args()`).
- **Recommendation:** Install a panic hook that writes to the daemon log, open the log after the flock and in append mode, retry transient accept errors, restore the screen on every attach exit, and read argv with `args_os`.
- **Fix with:** error-model
- **Effort:** S

### TMUX-63 · P3 · A pipe-pane helper that stops reading freezes the pane until killed

- **Area:** Concurrency
- **Where:** `src/server.rs:7022-7036`, `src/server.rs:12500-12512`
- **Problem:** The pane reader sends to a `sync_channel(64)` for the pipe and blocks when the helper stops reading; closing the pipe removes only the map entry, not the reader's sender or the helper. Proven: with `pipe-pane 'sleep 120'` on a looping pane, the pane stayed frozen, also after `pipe-pane` turned the pipe off; the stuck reader also skips `Pty::reap`.
- **Recommendation:** Give the pipe an owner that closes the helper's stdin and kills it on removal, so the blocked writer fails and the reader continues; never block the reader on the pipe.
- **Fix with:** lifecycle-concurrency
- **Effort:** S

### TMUX-64 · P3 · Pane spawn allocates after fork and leaks on partial failure

- **Area:** Concurrency
- **Where:** `src/pty.rs:100-137`, `src/pty.rs:286-299`
- **Problem:** The multithreaded daemon's child allocates `CString`s and calls `clearenv`/`setenv` after `fork`, which is not async-signal-safe (a musl deadlock is unproven). `chdir` and `exec` failures exit 127 silently. If `try_clone`, the writer thread or `set_cloexec` fails after `fork`, the child and both PTY fds leak, and a concurrent `Command::spawn` can inherit a pane master between `openpty` and `set_cloexec`.
- **Recommendation:** Build the environment before `fork`, set CLOEXEC before any other spawn can run, report `chdir`/`exec` failures through a CLOEXEC pipe, and unwind partial setup with a guard.
- **Fix with:** lifecycle-concurrency
- **Effort:** M

### TMUX-65 · P3 · Protocol requests admit combinations the commands forbid

- **Area:** Types
- **Where:** `src/protocol.rs:335-340`, `src/command.rs:1118-1124`, `src/protocol.rs:213-221`, `src/server.rs:8167`, `src/server.rs:8196`, `src/model.rs:9-26`
- **Problem:** `OptionFlags` is four independent bools, so exclusive scopes combine and `table_for_name` picks one silently (whether tmux rejects the combinations is unproven). `ResizePane` carries seven optional strings that are parsed and rejected only during execution, after a partial resize, and zoom silently discards the other fields. `Size` admits zero, so every constructor must call `bounded()`.
- **Recommendation:** Use an `OptionScope` enum plus a `global` flag, parse resize amounts in `src/command.rs` into an enum (`Zoom` or `Resize`), and make `Size` non-zero by construction (with TMUX-54's maximum).
- **Fix with:** make-invalid-states-unrepresentable
- **Effort:** M

### TMUX-66 · P3 · Builds do not pass --locked

- **Area:** Build
- **Where:** `Makefile:10`, `Dockerfile`, `.github/workflows/release.yml:88-109`
- **Problem:** No Makefile target, Dockerfile step or workflow uses `--locked` or `--frozen`, so a build can rewrite `Cargo.lock` silently, including through the crates.io source replacement in `.cargo/config.toml:6-10`.
- **Recommendation:** Add `--locked` to the cargo macro in the Makefile.
- **Fix with:** none
- **Effort:** S

### TMUX-67 · P3 · vt100 and laputa-ptytest are behind their upstreams

- **Area:** Dependencies
- **Where:** `Cargo.toml:13`, `Cargo.toml:16-17`, `Cargo.lock:34-37`, `Cargo.lock:180-183`, `tests/support/pty_reap.rs`, `tests/support/raw_pty.rs`, `tests/support/parity.rs`
- **Problem:** `vt100` is locked at 0.16.2; 0.17 fixes grid sizing and scrollback and 0.18 changes the API. `INTENTIONAL_DIFFERENCES` pins 0.16's grid limits, so an upgrade may retire entries. `laputa-ptytest` is locked at 0.1.1; its main branch now accepts `ESC ( B` and tolerates processes exiting mid-scan, but those fixes are unreleased, so `pty_reap.rs` and the raw-PTY runner remain. `rustybench` is at 0.1.0 with 0.4.0 available.
- **Recommendation:** Release ptytest with its fixes, then on a branch move tm to it and to vt100 0.18 together (ptytest also depends on vt100), and let the `INTENTIONAL_DIFFERENCES` failures show which grid limits are gone.
- **Fix with:** none
- **Effort:** M

### TMUX-14 · P4 · Last pane is a single slot, not tmux's history

- **Area:** Panes
- **Where:** `src/server.rs` `last_pane`
- **Problem:** After cross-window pane moves, tm keeps one previous pane per window; tmux 3.8 keeps a stack, so `select-pane -l` can pick a different pane after a moved or killed pane.
- **Recommendation:** Replace the slot with a per-window recency list and add a differential history in H2.
- **Fix with:** reference-parity

### TMUX-19 · P4 · pipe-pane -I and strftime expansion are missing

- **Area:** Commands
- **Where:** `src/command.rs` `parse_pipe_pane`, `src/server.rs` `pipe_pane`
- **Problem:** `-I` (command output into the pane) is rejected, and the command is expanded with `#{}` formats but not with strftime escapes as tmux's `format_expand_time` does.
- **Recommendation:** Read the helper's stdout into the pane and expand time escapes, with differential probes.
- **Fix with:** reference-parity
- **Effort:** M

### TMUX-21 · P4 · Choose modes treat the filter result "false" as false

- **Area:** Buffers
- **Where:** `src/server.rs` `build_buffer_entries` and the two other filter checks
- **Problem:** The choose-buffer, choose-tree, and choose-client filters treat an expanded filter of `false` as false; tmux 3.8 treats only an empty string and `0` as false. `list-buffers -f` now follows tmux.
- **Recommendation:** Share one `format_true` helper between the filters and add a differential probe with a filter that expands to `false`.
- **Fix with:** reference-parity
- **Effort:** S

### TMUX-23 · P4 · The t/f/ time modifier differs from tmux

- **Area:** Formats
- **Where:** `src/server.rs` time modifier handling
- **Problem:** With `@ts` set to `1000000000`, `display-message -p '#{t/f/%%Y:@ts}'` prints `2001` in tm and the current year in tmux 3.8, and `#{t/f/%%Y:#{@ts}}` prints `2001` in tm and `1000000000` in tmux. `tests/tmux_core_compat.rs` expected `2001` for the single-percent form, which tmux answers with the current year because `display-message` runs `strftime` over the whole message first; the case was never compared with the reference and now asserts the current year that tmux prints.
- **Recommendation:** Port `format_modifiers` time handling after probing the forms above and restore an exact assertion.
- **Fix with:** reference-parity
- **Effort:** S

### TMUX-25 · P4 · window_flags lacks the activity, silence, and marked flags

- **Area:** Formats
- **Where:** `src/server.rs` `window_printable_flags`
- **Problem:** tm reports `*`, `-`, `!`, and `Z` for `#{window_flags}` and `#{window_raw_flags}`; tmux 3.8 also reports `#` (activity), `~` (silence), and `M` (marked pane), and escapes `#` in `window_flags`.
- **Recommendation:** Add the flags when tm tracks activity, silence, and the marked window, with a differential probe per flag.
- **Fix with:** reference-parity
- **Effort:** S

### TMUX-36 · P4 · Last window is a single slot, not tmux's stack

- **Area:** Windows
- **Where:** `src/model.rs` `last_window`
- **Problem:** When the current window of a session with two or more windows is removed (for example a window created, selected, and then exited), tmux returns to the window before it and keeps the older window as the last window; tm clears the last window.
- **Recommendation:** Replace the slot with a recency list, like TMUX-14, and drop the `WindowRemovalFromHistoryStack` exclusion.
- **Fix with:** reference-parity

### TMUX-48 · P4 · Ctrl+Shift+letter in a legacy pane reaches it as a control character

- **Area:** Input
- **Where:** `src/server.rs` pane key encoder
- **Problem:** With the pane not requesting extended keys, the client bytes `ESC [ 105 ; 6 u` (Ctrl+Shift+i) reach the pane as `0x09` in tm and as `I` in tmux 3.8.
- **Recommendation:** Follow `input_key_vt10x` and tmux's key parsing for Ctrl+Shift letters, with a differential probe over the whole letter range.
- **Fix with:** reference-parity
- **Effort:** S

### TMUX-68 · P4 · The benchmark compiles the crate a second time and its scaffolding ships

- **Area:** Dead code
- **Where:** `benches/tm_performance.rs:1-25`, `src/main.rs:3-16`, `src/server.rs:889-992`, `src/server.rs:775-815`
- **Problem:** With no lib target, the bench includes every source file by path under `allow(dead_code, unused_imports)`, and `RenderBenchmark` is compiled into the release binary. Its delta path re-implements the attach loop without the full-frame guard for OSC, DCS and BEL or the sync bracketing, so the documented delta medians describe a different code path.
- **Recommendation:** Split `lib.rs` from `main.rs`, gate the fixture behind a feature, and extract one frame-to-wire function that both the attach loop and the benchmark call.
- **Fix with:** api-surface-and-module-boundaries
- **Effort:** M

### TMUX-69 · P4 · NewSession.detached is unused by the server

- **Area:** Dead code
- **Where:** `src/server.rs:11747`, `src/command.rs:195`, `src/client.rs:35`, `src/protocol.rs:13`, `src/protocol.rs:403`, `src/command.rs:160`, `src/command.rs:176`, `src/server.rs:7253`, `src/terminal.rs:388`
- **Problem:** The server ignores `NewSession.detached` (`detached: _`); the client already has the fact in `InvocationSpec.attach`. The 80x24 default size is written in four places.
- **Recommendation:** Drop the field at the next protocol change and add one `Size::DEFAULT`.
- **Fix with:** delete
- **Effort:** S

### TMUX-70 · P4 · Every build reports version 0.0.0

- **Area:** Operability
- **Where:** `Cargo.toml:3`, `src/main.rs:37`, `.github/workflows/release.yml:131`, `Makefile:54-56`
- **Problem:** `tm -V` prints `tm 0.0.0` for every build; the only build identity is the `release-<sha>` tag, so an operator cannot tell which binary or protocol a daemon runs, and `make install` keeps no previous binary to roll back to.
- **Recommendation:** Embed the git SHA and protocol version in `-V` output.
- **Fix with:** none
- **Effort:** S

### TMUX-71 · P4 · Each pane keeps up to 32 MiB of raw output to work around vt100 gaps

- **Area:** Performance
- **Where:** `src/server.rs:11716-11719`, `src/server.rs:12006-12008`, `src/server.rs:12445`, `src/server.rs:12548-12614`, `src/server.rs:14744-14745`, `src/copy_mode.rs:2250`, `src/copy_mode.rs:2422`
- **Problem:** `vt100` drops OSC 8 hyperlinks and tab cells, so each pane retains its raw byte stream (capped at 32 MiB), checkpoints it with a full history pass under the global lock, and clones it on every 8 KiB read while copy mode refreshes live. TMUX-58 and TMUX-59 are its most expensive consumers.
- **Recommendation:** Keep hyperlink and tab state beside the parser (or in an upgraded or patched `vt100`, see TMUX-67) and retire the raw-stream retention; until then, stop cloning it per read.
- **Fix with:** make-it-fast
- **Effort:** L
