Prefix: TMUX
Next: 8

### TMUX-1 · P2 · The daemon mutex is held across shell waits and unbounded terminal replay

- **Area:** Concurrency
- **Where:** `src/server.rs:130`, `src/server.rs:5833`, `src/server.rs:4727`, `src/server.rs:13187`, `src/server.rs:10271`, `src/terminal.rs:163`
- **Problem:** `handle_connection` holds the only `ServerState` mutex for the whole of `execute_request`. Foreground `run-shell` calls `Command::output` on that stack, and copy-pipe calls `child.wait` from `execute_copy_action`, which runs on the same mutex from the attach input thread. The pane reader also holds the mutex while `OutputState::process` expands DEC REP (`CSI Ps b`) once per repeat, with no cap on `Ps`. One session therefore freezes every other command, including `kill-server`, until the wait or the expansion finishes. On a dev build, `run-shell sleep 2` made a concurrent `ls` take 1.755s; three idle `ls` runs took 0.022s, 0.023s, and 0.024s. One `ls` during a pane's `CSI 50000000 b` took 1.542s (one sample).
- **Recommendation:** Release the mutex before any child wait. Cap a DEC REP to the pane width, or another small bound, before replaying it.
- **Fix with:** lifecycle-concurrency
- **Effort:** M

### TMUX-2 · P2 · A lone escape after the prefix is held as an unfinished binding

- **Area:** Architecture
- **Where:** `src/server.rs:1580`, `src/server.rs:1606`, `src/config.rs:13`, `src/config.rs:175`, `src/config.rs:269`
- **Problem:** The compiled profile's `Left`, `Right`, `C-Left`, and `C-Right` bindings all start with `ESC`. After the prefix, `handle_client_input` keeps a partial key in `key_buffer` until some later byte makes it stop being a prefix of a binding. `escape-time` is compiled as `0` and is never read. The following key is appended, and on a mismatch the daemon writes the prefix byte plus the buffered bytes into the pane. Reproduced with `tm -S` and no `TM_SOCKET` (so the compiled table is active): attach, send `C-a`, `ESC`, then `n`; `capture-pane` showed `^A^[n` instead of the `n` binding. Integration tests set `TM_SOCKET`, which skips `COMPILED_BINDINGS`, so they never load this table.
- **Recommendation:** Honor `escape-time`. At `0`, do not keep a partial prefix match past the bytes already in hand. If those bytes are not a binding, forward them and parse the rest as new input.
- **Fix with:** none
- **Effort:** M

### TMUX-3 · P2 · A signal leaves the attached terminal in raw mode

- **Area:** Concurrency
- **Where:** `src/client.rs:104`, `src/client.rs:139`, `src/terminal.rs:299`, `src/terminal.rs:311`
- **Problem:** `RawTerminal` restores termios in `Drop` and `attach_loop` restores it only on the normal return. Nothing installs a handler for `TERM`, `HUP`, `INT`, or `QUIT`. Those signals terminate the process without running destructors. A separate Rust process confirmed that: after `SIGTERM`, its `Drop` impl did not run. Release builds set `panic = "abort"`, and `make release` adds `-Cpanic=immediate-abort`, so a panic skips `Drop` as well. The user's shell then stays raw until they reset it.
- **Recommendation:** On `TERM`, `HUP`, `INT`, and `QUIT`, restore the saved termios and then exit. Do not use `Drop` as the only restore path.
- **Fix with:** lifecycle-concurrency
- **Effort:** S

### TMUX-5 · P3 · Pane spawn calls setenv after fork in a multithreaded daemon

- **Area:** Concurrency
- **Where:** `src/pty.rs:92`, `src/pty.rs:207`, `src/server.rs:95`, `src/server.rs:10262`
- **Problem:** `Pty::spawn` uses `fork` after the daemon already has client and reader threads. `child_setup` then calls `setenv` and `execvp`. `setenv` is not async-signal-safe. If another thread holds the allocator lock at the `fork`, the child can deadlock before `exec` and the pane never starts. The deadlock was not reproduced.
- **Recommendation:** Build the child's environment before `fork` and pass it to `execve`, or use `posix_spawn`. The child path should only call async-signal-safe functions.
- **Fix with:** os-boundaries
- **Effort:** M

### TMUX-6 · P3 · The TM_CONFIG test never writes a config file

- **Area:** Tests
- **Where:** `tests/tmux_config_compat.rs:14`, `tests/tmux_config_compat.rs:61`
- **Problem:** `compiled_defaults_ignore_tm_config_before_the_first_session_exists` points `TM_CONFIG` at a path it does not create, then asserts the compiled prefix and window index. A daemon that read and applied `TM_CONFIG` would still pass, because there is no file to apply. README and AGENTS.md state that startup ignores tmux configuration, and nothing checks that.
- **Recommendation:** Write a file that sets `prefix` and `base-index` to values other than the compiled ones, and assert the server still reports `C-a` and base index `1`.
- **Fix with:** none
- **Effort:** S

### TMUX-7 · P3 · Daemon diagnostics are discarded

- **Area:** Operability
- **Where:** `src/client.rs:57`, `src/main.rs:22`
- **Problem:** `connect_or_start` sets the daemon's stdin, stdout, and stderr to null before `spawn`. `main` reports a daemon failure with `eprintln`, which then goes nowhere. After the socket accepts, an operator cannot see a later daemon error; the client only notices a dead socket on the next command.
- **Recommendation:** Leave stderr inherited until `bind` succeeds so startup failures are visible, and keep a single stderr stream for the running daemon that is not the attached client's screen.
- **Fix with:** none
- **Effort:** S
