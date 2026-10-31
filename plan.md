# Bring tm to verified tmux parity within its existing scope

## Status

- **Goal:** make tm dependable as a daily multiplexer by matching pinned tmux 3.8 across tm's compiled normal profile, supported CLI, attached terminal, pane application, and lifecycle behavior.
- **Base:** code `05f4737bdfa5c032bf511bb98a8775134ca4e9ad` (the reviewed snapshot), toolchain `nightly-2026-09-15`.
- **Preflight:** 2026-10-30. Verdict: ready.
- **Accepted:** G0 reference build, provenance, and matrix skeleton. G1 hermetic adapter, normal-profile fixtures, single Attach, marker smoke, and conflicting config proof. G2 full option/binding matrix and compiled-profile cross-check. T1 client execution context, relative targets, prompt targeting, and all effective bindings from a second session. T2 atomic break/join/swap, sole-pane break, and chain parse/runtime semantics. W1 reference equal layouts, navigation/last pane, and zoom. W2 foreground cwd query, OSC 7 precedence, and split path inheritance. W3 named windows, renumbering, verbatim prompt values, and latest-client resize.
- **Baseline:** `make test` green (20 s), `make lint` green (4 s); no known red tests. Verified mismatches are in **Baseline** and become the first parity scenarios.
- **Next:** I1.
- **Resume:** "Execute `plan.md` from G0. One writer, no push, private sockets only." First command: `make test`.
- **Owner constraint:** this campaign runs inside the owner's tmux session. Every tmux invocation must use an explicit private `-S` path under `.work/tmp`, with `TMUX`, `TMUX_PANE`, and `TM_SOCKET` removed and `TMUX_TMPDIR` set to `.work/tmp`; never bare `tmux`, never `-L`, never a server-wide kill. A user server must never be touched.

## Charter

- **Decide and proceed.** Record a decision only when a later slice depends on it. Use the smallest reversible assumption for ordinary ambiguity; ask only for authority or product intent, which the owner granted below.
- **Authority:** edit this repo; add test-only scripts, fixtures, dev-dependencies, and CI config; install test-only reference build dependencies (libevent, ncurses, utf8proc); build the pinned reference; run Docker; commit locally as Conventional Commits (`type(scope): subject`, item ID in the body, no code comment cites this doc). Never push, publish, change repository settings, or touch the ambient tmux server. No formatters.
- **One writer.** All `src/*.rs` slices are serial. Parallel lanes only for the disjoint pair named in **Sequence**; worktrees live in `.work/wt/<lane>`. Manifests have one writer.
- **Steering.** After each accepted slice run the parent checkpoint, rewrite **Status** in place, and pick the next slice. Git history is the log; no logs, handoffs, or checkpoints in this doc.
- **Parity first.** Completeness and differential evidence before optimization. Fix session loss, daemon-wide hangs, unbounded growth, or an unrelated command delayed more than 2 s by a blocked helper in its own commit; record anything smaller. No compiled option, binding, or supported behavior is removed or changed; new support needs a concrete failing scenario within the agreed surface. Every slice lands implementation and its test; an audit, roadmap, or harness alone is not a slice. Commit each coherent green change; stage only campaign work.
- **Root cause.** Reproduce with the smallest failing test, fix the responsible layer, keep the test. No retry without a cause. Time-box an item at three attempts, then park it with branch, cause, and retry condition.
- **Checks.** Slice acceptance plus the narrowest affected suite on the integrated tree. Full suite at G1, after I3, after R5, and Z; report before any run expected to exceed five minutes. Never weaken, delete, or skip a test; never accept an unexplained golden update.
- **Oracle rule.** Every parity claim executes the pinned reference or cites a recorded, versioned reference observation. A test derived only from tm's output is not parity evidence.
- **Dependencies.** New dependencies are test-only unless the slice names a production need and passes the build, 16 ms, and 32 KiB gates. Prefer std; `unicode-width` is already transitive.
- **Paths.** Worktrees, reference builds, sockets, and test scratch live under `.work/`; run test/build commands with `TMPDIR=$PWD/.work/tmp`, and never `/tmp`, the ambient `$TMPDIR`, or a shared socket path.

## Decisions

**D1 — Contract inventory.** `docs/parity.md` has one row per COMPILED_OPTIONS entry (27), per effective binding key (27: VANILLA overlaid by COMPILED), per interactive mode those bindings invoke (prompt, display-panes, choose-*, copy-mode), and per CLI/terminal/input/lifecycle group (formats, targeting, buffers, capture, copy mode, session/window/pane lifecycle). Columns: contract item, selected semantics, scenario, implementation, test, status, intentional difference. A `parity_contract` test fails when an option or effective key has no row. Status is `pass`, `fail`, or `blocked`; a slice turns its rows `pass` only with linked behavioral evidence, never a `show-options` round-trip. Off values also need evidence that the prohibited behavior does not occur. An intentional difference needs a reference observation and a reason; never invent one to make a row green.

**D2 — Profiles.** `TM_SOCKET` selects baseline (C-b). Normal tests start with `-S <private path>` and explicitly remove inherited `TM_SOCKET`; every normal-profile assertion uses that profile. Baseline coverage stays in existing suites. The reference profile is selected for the profile under test, never tmux defaults.

**D3 — Reference build.** Pin tmux 3.8 source at `https://github.com/tmux/tmux/releases/download/3.8/tmux-3.8.tar.gz`, SHA-256 `e79c699c7e949dccd0a4a125e17b8d1261e311b16979dbc8eb34542f3966d82e`. `tests/support/install-tmux-ref.sh` verifies the hash, builds into `.work/ref/tmux-3.8` with `--enable-utf8proc --disable-jemalloc`, and writes provenance (`tmux -V`, URL/hash, configure flags, dependency versions). A clean build with those flags was verified on macOS arm64. CI installs macOS `libevent`, `utf8proc`, and `pkg-config`, or Debian/Ubuntu `build-essential`, `libevent-dev`, `libncurses-dev`, `libutf8proc-dev`, and `pkg-config`. Adapters use `TMUX_REF` or that path; a missing binary or a version other than 3.8 fails loudly. tmux stays out of tm's production dependency graph.

**D4 — Reference server.** Always `-S <private socket> -f tests/reference/tmux-3.8.conf` (normal) or `tmux-3.8-baseline.conf` (baseline); no user config or server. The static config installs the effective profile at the scopes listed in **Reference scopes**, clears the prefix table (`unbind -a -T prefix`) and installs the effective bindings, maps `__detach`→`detach-client`, `send`→`send-keys`, preserves `bind -r`, and is checked for drift against `src/config.rs` by a unit test. The baseline config mirrors `ServerState::new` defaults: `buffer-limit 50`, `history-limit 10000`, `mode-keys emacs`, `prefix C-b`, `mouse off`, `base-index 0`, `renumber-windows off`, `remain-on-exit off`. The adapter validates loaded options and `list-keys -T prefix` before running a scenario.

**D5 — Capability profiles.** Recorded profiles keyed by `TERM` give the sequences tm may use: alternate screen, bracketed paste, focus, sync, clipboard, extended keys. Apply `terminal-features` patterns from compiled options with tmux-style glob matching; unknown TERMs get the conservative ANSI baseline. Never assume a sequence exists on every TERM. Full terminfo parsing is a non-goal; the recorded set covers Ghostty, tmux-256color, xterm-256color, and the test profiles, and the matrix records the limitation.

**D6 — Execution context.** Thread a small `ExecContext { client_id, session_id, window_id, pane_id }` through `execute_request`. Omitted targets and relative `+N/-N` targets resolve against that context; stable IDs remain distinct from renumbered indexes. Remove the `-t` string-injection whitelist. A prompt captures its client and resolves at submission; a detached client is a visible error.

**D7 — Failure and chain semantics.** Parse and validate every command in a bound chain before running any; a parse or unknown-command error aborts the whole chain with no effects. Then execute in order; a runtime error stops the remaining commands, keeps earlier effects, and shows the error in the invoking client. No transaction-wide rollback. Preflight targets, occupancy, and geometry before mutation. A sole-pane `break-pane` moves the pane to a new window (next index for `:`) and keeps the session, matching the 3.8 probe.

**D8 — Equal layouts.** `even-horizontal`/`even-vertical` use `each = (size - (n - 1)) / n`, then distribute the remainder `size - n*(each+1) + 1` to the first panes; borders consume one cell. Minimum pane size is 1. Test 2–5 panes at odd/even dimensions, after border resize, repeated outer resize, middle-pane close, and zoom.

**D9 — Current directory.** `pane_current_path` refreshes from OSC 7 when present, otherwise queries the foreground process-group cwd: `/proc/<pgrp>/cwd` on Linux, `proc_pidinfo(pgrp, PROC_PIDVNODEPATHINFO)` on macOS, falling back to the pane pid then the start path. Cache per pane and invalidate on split/format use. Tests run a real shell `cd`, including a shell that emits no OSC 7.

**D10 — Prompt substitution.** Parse the prompt template once, then replace `%%` inside already-tokenized arguments with raw input; never re-tokenize user input. Empty input stays empty. Test spaces, apostrophes, double quotes, backslashes, `%`, Unicode, and empty input against reference.

**D11 — Resize with multiple clients.** The compiled profile selects tmux's default `window-size latest`: the most recently active attached client defines window size. A resize from a non-latest client updates that client's stored size but not the window; switching activity reflows to the new latest. Never let the last resize packet win.

**D12 — Input state.** One bounded byte-stream state machine with ordinary, prefix, repeat, escape/CSI, paste, and overlay states. Match 3.8 timing: with `escape-time=0`, a partial key gets a 1 ms deadline (a partial paste end up to 500 ms), never indefinite buffering; complete sequences are atomic. Repeat mode uses the 500 ms repeat-time deadline, can switch to another repeatable key, and ends on ordinary text. Lone ESC, Alt, incomplete, and unknown input are forwarded exactly once.

**D13 — Extended keys.** Decode CSI-u and xterm `27;mod;code~`; the protocol modifier is `1 + bitmask`, so cover all eight basic combinations plus Enter, Tab, Backspace, ASCII, Unicode, and modified specials. Keep a structured key until pane encoding: emit legacy bytes for panes that never requested extended input; for a pane in extended mode emit the selected `csi-u` format. Do not implement optional kitty extensions beyond the selected surface.

**D14 — Paste.** Paste is a payload state. Consume `CSI 200~`/`201~`; forward payload verbatim; forward markers only to a pane that requested bracketed paste; reset after 5 s of inactivity. Bound keys and literal prefix bytes inside a paste never execute. `paste-buffer -p` follows the same target-mode rule. Cover fragments, neighboring panes with different modes, and backpressure.

**D15 — Pane modes.** Per-pane vt100 modes own application cursor/keypad, focus, mouse, and paste state. Set the outer DECCKM/DECKPAM to the active pane's mode so arrows/keypad reach it in the requested representation, and switch on pane change. Forward focus in/out only to a pane that requested focus; send focus transitions when switching panes. Forward mouse only to the pane under the pointer that requested a mode, with its encoding and coordinates; wheel enters copy mode when the pane has no mouse mode. Never forward outer reports to an application that did not request them.

**D16 — Terminal ownership.** Track desired terminal modes per attached client on the server: alternate screen, mouse, focus, bracketed paste, sync, and keyboard capability. Emit enter/enable once at attach, only the diff when an option changes, and the matched disable/pop exactly once on detach or error. The client applies those bytes and holds a precomputed restore sequence for catchable signals (D24). Routine full and incremental redraws emit no mode setup. Never emit `CSI >1u` unless the recorded profile's enable pair is that sequence.

**D17 — Capability gating.** Honor `terminal-features` for `Sync` and `clipboard` (the compiled `tmux-256color` entry) and for test-only `focus`, `bpaste`, and `extkeys`. Clipboard output requires the clipboard capability; sync brackets a frame only when supported; focus/extended-key enablement follows the recorded pair. Inner-pane behavior still follows the compiled option values regardless of outer capabilities.

**D18 — Retained output.** `RetainedOutput` is the single owner per pane: bytes, checkpoint, replay seed (DECAWM, last printed character, alternate screen, pending bytes), and history floor. Process a chunk fully before checkpointing; a checkpoint serializes scrollback up to `history-limit` plus the visible screen and cursor, then appends the unparsed tail. Budget: `min(32 MiB, max(1 MiB, history_limit.saturating_mul((cols + 16) * 2)))`. Replay starts from the same seed. Never replay or clone the entire history on a key or redraw; copy/search/capture use the retained owner or the live parser consistently.

**D19 — Cell model.** Add one `src/cells.rs` using `unicode-width` (already transitive in vt100): display width, tab stops, wide cells, and combining marks. Use it in copy rendering, mouse column mapping, selection, and status truncation. No second width table.

**D20 — Status and prompts.** Build status output as styled spans, reset between spans, bound the row by display width, honor `status-left-length`, and truncate the window list so the active window stays visible for first/middle/last positions. Bound messages and prompts; never wrap the bottom row. Test Unicode names, many windows, narrow widths, prefix highlighting, zoom marker, and errors.

**D21 — Bell and clipboard.** Ring the outer BEL once per attached client of the alerting session when `bell-action=any` and `visual-bell=off`; never when visual-bell is on. With `set-clipboard=external`, tm-generated copies emit OSC 52 only when the clipboard capability exists, while application-originated OSC 52 is ignored (reference 3.8 behavior); with `on`, applications may create buffers. Clipboard pending is per client and emitted exactly once across full, delta, and unchanged frames.

**D22 — Liveness.** No child wait, pipe write, PTY input write, disk write, or unbounded parse runs while the shared state lock is held. Foreground commands complete through an explicit pending/re-lock path; pipe-pane has a bounded per-pipe writer queue with backpressure; pane input has an ordered bounded writer that never silently drops; a slow client blocks only its own render thread. Preserve ordering and handle completion after a pane exits or moves. Do not hide blocking with fire-and-forget.

**D23 — Children.** Reap every background `run-shell`, pipe helper, copy-pipe child, pane process, and failure-path child; a finite burst returns to baseline. Set close-on-exec on PTY descriptors and other inherited fds. Build argv, envp, cwd, and PATH resolution before `fork`; the child uses only async-signal-safe calls and `execve`. Preserve session/foreground-process-group, `TERM`, cwd, and PATH behavior; exec failure is observable.

**D24 — Signals.** Block signals process-wide and use a dedicated `sigwait` thread, never a handler that allocates or locks. On TERM/HUP/INT/QUIT restore termios, owned screen, cursor, keyboard/mouse/focus/paste/style state, and exit. On TSTP restore then stop; on CONT re-enter raw mode and force a redraw. Daemon signals shut down cleanly and remove the socket. SIGKILL and an unwind-less abort make no cleanup promise; test only handled paths. Release `panic=abort` is not a restore mechanism.

**D25 — Startup and diagnostics.** Serialize first-server startup with an exclusive `flock` on `<socket>.lock`; after acquiring it, probe for a live listener before unlinking a stale path. Concurrent initial commands converge on one daemon. The daemon writes `<socket>.log` (0600, truncated, capped at 1 MiB); a failed startup prints the log tail to the invoking client. Diagnostics never write to an attached terminal's screen.

**D26 — Idle clients.** The render wait loop checks the client's alive flag and existence after every wake; a closed peer unregisters and releases buffers even when no revision changed. `attach-session` performs exactly one Attach request (no duplicate registration or pre-attach 80×24 resize).

**D27 — Bounds.** Cap `OutputState.pending` at 1 MiB and recover at the next valid escape boundary after overflow. Clamp DEC REP to `cols - cursor_col`, matching 3.8. Reject or clamp oversized CSI/OSC parameters before allocation; do not truncate legitimate OSC 52 payloads below the cap.

**D28 — Parity comparison and test mechanics.** One minimal adapter runs the same logical scenario against tm and the reference; extend `TestEnv`, `ptytest`, and the existing fixtures, not a general framework. Use explicit dimensions, a fixed locale, cleared multiplexer variables, pinned `TERM`/`TERMINFO` profiles, and recorded capability profiles. Compare three boundaries: command results and state; attached semantic cells/attributes/cursor/modes/status/prompt/restoration; pane exact input bytes, requested modes, query replies, process state, cwd, and window-size signals. Normalize only socket paths, pids, timestamps, client names, and stable IDs by order; never geometry, indexes, content, dropped content, errors, or missing events. Deliberate presentation differences go in the matrix; they are not normalized. Do not require byte-identical renderer output. Assert bells, OSC 52, keyboard push/pop, focus, paste, and query replies as explicit raw events, not screen text. Keep `xterm-minimal-v1` for semantic screens; for sequences that profile does not admit (OSC 52, `>4;2m`, keyboard push such as `>1u`), run with `ProtocolProfile::disabled()` and assert exact `raw_output()` events with a shared extractor. Every subprocess, wait, and cleanup has a deadline; a watchdog fails a hung scenario; cleanup owns the daemon pid and works if `kill-server` hangs, and never kills a user server or unrelated processes. Fixture acknowledgements cannot be satisfied by echoed input; use markers/semantic state, not settle sleeps. Unit-test timeout machines with an injected clock; use bounded real-time tests for actual timers.

**D29 — CI.** CI runs on PRs and relevant main pushes, installs the pinned reference build dependencies, and runs `make parity` on Linux and macOS. The job caches `.work/ref` by source SHA, fails when the reference is absent, and uploads compact failure artifacts (parity diffs, ptytest bundles, daemon log). Perf stays a documented local gate, not a shared-runner pass/fail.

## Coverage

Every effective option and binding has one proving slice; T1 also presses all 27 effective bindings from a second session to prove context, while the focused slice proves the semantics. G2 fails if an option or binding in `docs/parity.md` is unowned; Z checks the TODO and source-area maps below are resolved or explicitly parked.

| Options (27) | Proving slice |
| --- | --- |
| `prefix` | T1 |
| `escape-time` | I1 |
| `extended-keys`, `extended-keys-format`, `mouse`, `focus-events` | I3 |
| `terminal-features` | C1 |
| `history-limit` | R1 |
| `default-terminal`, `set-titles` | R2 |
| `mode-keys` | R3 |
| `base-index`, `renumber-windows`, `automatic-rename`, `monitor-activity` | W3 |
| `status-position`, `status-bg`, `status-fg`, `status-left`, `status-left-length`, `status-right`, `window-status-format`, `window-status-current-format` | R4 |
| `set-clipboard`, `monitor-bell`, `bell-action`, `visual-bell` | R5 |

| Bindings (27) | Proving slice |
| --- | --- |
| `p`, `Enter`, `/`, `C-Left`, `C-Right`, `Left`, `Right` | T1 |
| `%`, `"`, `h`, `j`, `k`, `l`, `\`, `-`, `z` | W1 |
| `x`, `&`, `m`, `C-n` | T2 |
| `c`, `n`, `r` | W3 |
| `b`, `C-s` | I1 |
| `[` | R2/R3 |
| `d` | L2 |

| TODO | Resolved by |
| --- | --- |
| TMUX-1 (lock-held waits, REP) | L1 and L4 |
| TMUX-2 (escape after prefix) | I1 |
| TMUX-3 (signal leaves raw mode) | L3 |
| TMUX-5 (setenv after fork) | L1 |
| TMUX-6 (TM_CONFIG test writes no file) | G1 |
| TMUX-7 (discarded diagnostics) | L2 |

| Source area | Slices |
| --- | --- |
| Contract and matrix | D1, G1, G2 |
| Differential oracle | G1, D28 |
| Context, targeting, atomicity | T1, T2 |
| Window, pane, cwd, prompt, resize | W1, W2, W3 |
| Input, capabilities, paste, modes | I1, C1, I2, I3 |
| Rendering, copy, history, status, notifications | R1–R5 |
| Liveness and lifecycle | L1–L4 |
| Performance | P1 |
| Real apps, CI, release, closeout | A1, V1, V2, Z |

## Baseline

- **Reference probe (2026-10-30):** source tarball above, SHA verified, built and run on macOS arm64; `tmux -V` = `tmux 3.8`. Private socket; `-f /dev/null` or the compiled reference config; ambient `TMUX`/`TMUX_PANE` cleared. Its binary SHA-256 and dependency provenance are recorded in `tests/reference/PROVENANCE` when G0 lands.
- **Loops:** `make build` 5 s; `make test` 20 s; `make test-ci` 29 s; `make lint` 4 s; `cargo test --test attached_client_pty` 2 s; `make perf` 15 s after compilation.
- **Baseline suite:** green. Perf medians: 28.7 µs (80×24, 1 pane), 32.3 µs (80×24, 2), 76.2 µs (120×40, 4), 327.1 µs (delta, 120×40, 4); budgets remain 16 ms and 32 KiB.
- **Verified mismatches to fix:**
  - `even-horizontal` with 3 panes at 80×24 yields 40/19/19 in tm; reference gives 26/26/26. Four panes: reference 20/19/19/19.
  - Breaking the sole pane of a sole window removes the session in tm; reference keeps the session and moves the pane to window index 1. `break-pane -t :` chooses the next free index; an occupied index errors with `index in use` and leaves state intact; an unknown join target errors with `can't find window` and leaves state intact.
  - A command-chain parse error prevents every command in the chain from running; a runtime error preserves earlier successful effects and stops the rest.
  - With the compiled profile and `TERM=tmux-256color` or `xterm-ghostty`, reference enters the alternate screen, enables bracketed paste, syncs frames, enables mouse/focus after capability negotiation, and honors the clipboard capability; it does not emit `CSI >1u`. tm currently emits `CSI >1u` and `CSI ?1004h` without capability gating and never enters the alternate screen or syncs.
  - Interactive targets: omitted `-t` injects only a session name, relative targets fall back to the first session, and binding context is lost; this affects Left/Right, C-n, and the m prompt.
  - `C-a k` is kill-pane in `COMPILED_BINDINGS` while README describes h/j/k/l as navigation. Preserve the binding; fix the README.
  - `escape-time` is stored but never read; repeat mode has no deadline; bracketed paste is not a payload state; prefix parsing holds partial matches indefinitely.
  - Retained output checkpoints before parsing, drop the crossing chunk, lose scrollback, and reset parser state; copy rendering indexes attributes by character rather than cell; status output is not width-bounded.
  - Blocking `run-shell`, copy-pipe waits, pipe-pane writes, and PTY writes run under the state lock; idle disconnects leak; startup unlinks live sockets; the `TM_CONFIG` and core-smoke tests do not prove their claims.

## Sequence

Each slice ends on one local acceptance command. Prerequisites are the previous accepted item unless stated. Split a feature item if its acceptance exceeds 20 minutes; keep the ID with a letter suffix. G0/G1 build the loop; V1/V2/P1/Z are integration milestones and may exceed 20 minutes (report before starting). `tests/tmux_parity.rs`, `tests/support/parity.rs`, `tests/reference/capabilities.rs`, and `docs/parity.md` are shared serial files: every slice adds its scenarios/rows there. Every slice also runs its named parity scenario with `cargo test --features parity --test tmux_parity -- <filter>`; the focused command below is the fast loop.

| ID | Owned files | Slice and acceptance |
| --- | --- | --- |
| G0 | `tests/support/install-tmux-ref.sh`, `tests/reference/PROVENANCE`, `docs/parity.md` skeleton | Read `AGENTS.md`, `README.md`, `TODO.md`, `docs/architecture.md`, `docs/testing.md`, `docs/ptytest.md`, `docs/performance.md`, `docs/release.md`, `src/config.rs`, the command parser, and existing tests; record the commit, recheck every finding, then land the reference build/provenance and baseline record. `tests/support/install-tmux-ref.sh && .work/ref/tmux-3.8/bin/tmux -V && make test` |
| G1 | `Cargo.toml`, `src/client.rs`, `tests/support/parity.rs`, `tests/support/pty_harness.rs`, `tests/tmux_parity.rs`, `tests/reference/tmux-3.8.conf`, `tests/reference/tmux-3.8-baseline.conf`, `tests/reference/profile.rs`, `tests/reference/capabilities.rs`, `tests/attached_client_pty.rs`, `tests/core_smoke.rs`, `tests/tmux_config_compat.rs`, `tests/support/pty_fixture.rs`, `docs/ptytest.md` | Hermetic adapter/three boundaries/comparator, normal-profile fixtures, marker-based core smoke replacing the `printf hello` echo assertion, valid conflicting `TM_CONFIG`/`.tmux.conf` proof. Fix the duplicate Attach registration before multi-client work (one Attach per attach; L2 keeps the idle-cleanup regression). Add a `parity` feature and a `[[test]]` target with `required-features`; default `make test` stays reference-free. Self-check: compare 80×24 3-pane `even-horizontal` geometry and assert tm (40/19/19) differs from reference (26/26/26); W1 flips it to equality. Acceptance: `cargo test --features parity --test tmux_parity && cargo test --test attached_client_pty && cargo test --test core_smoke && cargo test --test tmux_config_compat` |
| G2 | `docs/parity.md`, `README.md`, `tests/parity_contract.rs` | Full matrix; README corrected for `C-a k`; every option/key row names its proving slice and test, and existing tests are linked where they already prove behavior; `parity_contract` cross-checks every option/key row against `COMPILED_OPTIONS`/`COMPILED_BINDINGS`; unowned rows are a plan error. `cargo test --test parity_contract` |
| T1 | `src/server.rs`, `src/command.rs`, `tests/tmux_targeting.rs` | `ExecContext`, omitted/relative resolution, per-client prefix/prompt/repeat state, all 27 effective bindings from a second session with overlapping indexes, slash-prompt targets (omitted, relative, fully qualified, stable ID), two-client interleaving, visible prompt errors. Include Left/Right swap plus select chains, Ctrl-Left/Ctrl-Right, C-n break-pane, and the m join prompt. `cargo test --test tmux_targeting` |
| T2 | `src/server.rs`, `src/command.rs`, `tests/tmux_atomicity.rs` | Break/join/move/swap preflight and atomicity; sole-pane reference behavior; occupied/invalid/insufficient-space/spawn failures; parse-vs-runtime chain semantics; no partial mutation. `cargo test --test tmux_atomicity` |
| W1 | `src/server.rs`, `src/model.rs`, `tests/tmux_layouts.rs` | Reference equal distribution for 2–5 panes at odd/even sizes, after border resize, repeated outer resize, middle-pane close, and zoom/unzoom; split-plus-layout chains, `\`, `-`, `%`, `"`, navigation, pane numbers, `z`, `k`, last pane/window. Replace G1's inverted layout assertion with equality. `cargo test --test tmux_layouts` |
| W2 | `src/server.rs`, `src/model.rs`, `src/pty.rs`, `tests/tmux_cwd.rs` | Foreground-pgrp cwd query on Linux/macOS, OSC 7 precedence, actual `cd` in a non-OSC7 shell (`SHELL=/bin/sh`), split with `C-a \`, assert `#{pane_current_path}`. `cargo test --test tmux_cwd`; Linux rerun at V2. |
| W3 | `src/server.rs`, `src/client.rs`, `tests/tmux_windows.rs` | Named windows, rename, base-index/renumbering, active/last tracking, detach/reattach, prompt values (spaces, apostrophes, double quotes, backslashes, `%`, Unicode, empty), latest-client resize and two-client interleaving. Use two clients at 80×24 and 100×30; resizing the non-latest must not change the window, and input from the other makes it latest. `cargo test --test tmux_windows` |
| I1 | `src/server.rs`, `src/config.rs`, `tests/tmux_input.rs`, `docs/testing.md` | Input state machine, `escape-time=0`, lone/ordinary Escape after prefix, Alt, incomplete/unknown input, repeat deadline/switching and ordinary text after expiry, fragmented/coalesced input at every byte boundary, C-s two C-a bytes, literal prefix forwarding; record the 1 ms/500 ms timing assumptions in `docs/testing.md`. `cargo test --test tmux_input` |
| C1 | `src/server.rs`, `src/client.rs`, `src/terminal.rs`, `src/protocol.rs`, `tests/tmux_terminal_modes.rs` | Capability profiles, `terminal-features` matching, one-time mode ownership, alternate-screen enter/leave, sync bracketing, matched keyboard enable/disable, shell screen restoration, repeated redraws, repeated attach/detach with preexisting keyboard state. Lands before paste/modes because those consume its capability pairs. `cargo test --test tmux_terminal_modes` |
| I2 | `src/server.rs`, `src/client.rs`, `tests/tmux_paste.rs`, `tests/support/pty_fixture.rs` | Bracketed paste payload state, outer enable/disable through C1's capability pairs, pane requests paste on/off, fragmented markers at every byte boundary, C-a d/k, newlines, escapes, Unicode, backpressure, neighboring-pane modes, `paste-buffer -p`. `cargo test --test tmux_paste` |
| I3 | `src/server.rs`, `src/terminal.rs`, `tests/tmux_modes.rs`, `tests/support/pty_fixture.rs` | CSI-u/xterm decoding for all eight modifiers plus ASCII, Unicode, Enter, Tab, Backspace and modified specials; pane-side extended/legacy encoding; application cursor/keypad; focus; mouse routing/coordinates/buttons/release/wheel in ordinary and copy modes; two panes with different modes; fragmented arrows, CSI-u, SGR and focus at every byte boundary; gate outer enablement through C1. `cargo test --test tmux_modes` |
| R2 | `src/terminal.rs`, `src/server.rs`, `tests/terminal_corpus.rs` | Audited vt100 corpus: cursor/save/restore/appearance, erase/editing, scrolling regions, autowrap, CR progress, DEC REP clamp, alternate screens/styles and application-exit restoration, wide/combining/tabs/emoji, wrap/resize, and pane-boundary replies to DA1/DA2, DECRQSS, XTGETTCAP and OSC colour queries actually used by the corpus; record anything outside it as not required. Fix demonstrated gaps in the smallest layer; no second emulator, and a vt100 patch needs its failing case. `cargo test --test terminal_corpus` |
| R1 | `src/server.rs`, `src/model.rs`, `src/terminal.rs`, `tests/tmux_history.rs` | Post-parse retention, checkpoint across UTF-8/CSI/OSC/REP, no-wrap and alternate screen, multiple checkpoints (small history-limit), exact replay/capture/copy/search and subsequent output, 10,000-line bound. `cargo test --test tmux_history` |
| R3 | `Cargo.toml`, `src/main.rs`, `src/cells.rs`, `src/copy_mode.rs`, `src/server.rs`, `tests/tmux_copy_cells.rs` | Shared cell/width model; colored `界X`, combining marks, styled ASCII, wide continuations, hyperlinks, selection/mouse, narrow resize, copied bytes/attributes/cursor; `mode-keys emacs` copy actions. `cargo test --test tmux_copy_cells` |
| R4 | `src/server.rs`, `tests/tmux_status.rs` | Width-bounded status/prompt/error spans, style resets, active window visible when first/middle/last, zoom marker, prefix highlight, long and Unicode names, narrow terminals, many windows. `cargo test --test tmux_status` |
| R5 | `src/server.rs`, `src/terminal.rs`, `src/client.rs`, `tests/tmux_notifications.rs` | Audible bell and clipboard side effects, external vs on semantics, per-client exactly-once across full/delta/unchanged frames, application OSC 52. `cargo test --test tmux_notifications` |
| L1 | `src/server.rs`, `src/pty.rs`, `tests/tmux_liveness.rs` | Off-lock foreground waits, copy-pipe, pipe-pane backpressure, ordered PTY input, child reaping, FD_CLOEXEC, pre-fork env/PATH, burst baseline. Barrier tests hold a helper, nonreading pipe, copy-pipe child, pane app, and slow client; prove another session runs, renders, and detaches before release, then verify completion/order. `cargo test --test tmux_liveness` |
| L2 | `src/server.rs`, `src/client.rs`, `src/main.rs`, `tests/tmux_lifecycle.rs` | Idle disconnect, single Attach, startup flock/stale-socket convergence, `<socket>.log` diagnostics, actionable failures, preserved 0700/0600 socket permissions. `cargo test --test tmux_lifecycle` |
| L3 | `src/client.rs`, `src/main.rs`, `src/terminal.rs`, `src/server.rs`, `tests/tmux_signals.rs` | sigwait restoration on TERM/HUP/INT/QUIT, TSTP/CONT, daemon shutdown; repeated attach/detach and preexisting keyboard state. `cargo test --test tmux_signals` |
| L4 | `src/terminal.rs`, `src/server.rs`, `tests/terminal_bounds.rs` | 1 MiB pending cap, malformed recovery, REP clamp, parameter bounds, fuzz-style unit cases. `cargo test --test terminal_bounds` |
| A1 | `tests/tmux_apps.rs`, `docs/testing.md` | Deterministic real-app pass with `/bin/sh` or `bash` editing, `less`, `vim -u NONE`, multiline `cat > file` paste, copy/clipboard, resize, detach/reattach; stable full-vs-delta and silence assertions. `cargo test --test tmux_apps` |
| V1 | `.github/workflows/ci.yml`, `Makefile`, `Cargo.toml`, `docs/testing.md` | `make parity` depends on the reference installation and runs `cargo test --features parity --test tmux_parity`; CI installs `vim` and `less` for A1, runs `make test` and `make parity` on relevant main pushes and PRs, caches the reference, uploads failure artifacts, and uses Linux/macOS runners; pin any new action by commit SHA. `make parity && make lint`; the hosted run is owner-pushed. |
| V2 | `docs/release.md`, `Dockerfile` (only if the existing image cannot run the documented test command; record why) | Native macOS release with `make release TARGET=aarch64-apple-darwin`, Linux musl `x86_64-unknown-linux-musl`/`aarch64-unknown-linux-musl` builds with static verification, Linux PTY tests in Docker; keep existing packaging and pinned actions. Run the documented Docker commands; report before starting. |
| P1 | `benches/tm_performance.rs`, `docs/performance.md`, `src/server.rs` (only for a measured regression) | Re-measure existing fixtures and blocked-helper responsiveness, sustained output, large paste, idle cleanup; preserve 16 ms/32 KiB; interleave baseline/candidate runs, compare medians against observed spread, and fix regressions. Report any fixture change instead of raising a budget. `make perf` with `perf-gate-ready` before and after; memory/size always. |
| Z | `TODO.md`, `README.md`, `docs/parity.md`, `docs/architecture.md`, final report | Resolve/reclassify the TODO map in **Coverage** without duplicating entries, remove dead code, final matrix, full suite, independent review (`review`; ultrareview if available), fix P1/P2. `make test && make test-ci && make lint && make parity && cargo test --test tmux_apps` |

Existing suite checks: each slice also runs the existing suite it can affect — T1/T2/W1/W2/W3/R1/L1 → `tmux_core_compat`; R3 → `tmux_copy_mode_compat`; R5 → `tmux_buffers_compat`; C1/I2/I3 → `attached_client_pty`; L2 → `socket_permissions`; G1/G2/Z → all. The milestone full suite is the backstop.

Default one writer. The only possible parallel pair is V1 beside a source slice that touches none of `.github/`, `Makefile`, `Cargo.toml`, or `docs/testing.md`; verify disjointness first, otherwise serialize. Full-suite milestones: after G1, after I3, after R5, and Z.

## Acceptance and completion

A slice is accepted when its command and parity scenario pass on the integrated tree and its claimed matrix rows turn `pass` with a test link. The campaign is complete only when:

- Every compiled option and effective binding has observable evidence, including normal-profile attached coverage; off options have negative evidence.
- Required CLI, terminal, input, and lifecycle scenarios match pinned tmux, except differences recorded in the matrix with a reference observation.
- Multi-session targeting, sole-pane break/move, malformed input, slow children, paste, disconnect, and restoration have regressions.
- No session-loss or daemon-wide-hang finding from this campaign remains unresolved.
- Existing tests, parity gates, lint, release, and documented performance checks pass on the environments reported.
- No required test is skipped, weakened, or replaced by a golden update.

Final report: starting/ending commit IDs, reference version/provenance, user-visible fixes, contract-area pass/fail/blocked summary, exact commands and outcomes per environment, performance changes, remaining intentional differences, and precise missing evidence. If an environment blocks a check, continue useful work and report the blocked evidence instead of claiming parity.

## Needs environment

- **GitHub Actions run:** V1/V2 workflows run after the owner pushes; the agent verifies YAML, local commands, and the same Docker images, not a hosted run.
- **Ghostty GUI/manual checks:** actual keyboard protocol, focus, clipboard, paste, audible bell, resize, and visual restoration in Ghostty; deterministic PTY evidence covers protocol bytes. Owner command: attach `tm -S .work/tmp/manual.sock`, run the `docs/testing.md` checklist, detach, confirm the shell screen.
- **Hosted Linux runners:** local Docker covers x86_64/aarch64 musl; native hosted-runner behavior and CI caching are owner-observed.
- **Actual audio/clipboard devices:** tests assert BEL/OSC 52 bytes; audible sound and system clipboard contents need a human.

## Boundary and non-goals

**Authorized:** the repo, test-only scripts/fixtures, dev-dependencies, the reference build, Docker, local Conventional Commits. **Forbidden:** pushing, publishing, repository settings, CI-only workflow changes outside V1, user tmux config/servers, `/tmp`, and user data.

**Contract changes to surface in the landing commit and README:** `set-clipboard=external` means tm-generated copies use the clipboard capability while application OSC 52 is ignored; attach enters the alternate screen and restores the shell screen; focus/extended-key enablement is capability-gated and may be inert on a TERM; `C-a k` remains kill-pane and README is corrected; interactive errors are visible; two-client resize follows latest-client; one Attach per attach; `<socket>.log` is created.

**Non-goals:** tmux configuration-file parsing, `TM_CONFIG` or user tmux config, arbitrary option combinations, plugins, hooks, control mode, floating panes, new feature families, full terminfo parsing, every optional keyboard protocol extension, Windows, and non-listed platforms. No `server.rs` rewrite, async runtime, runtime configuration parser, or parallel legacy/new implementation; no temporary harness shortcut or broad snapshot update left in the result.

## Reference scopes

Verified against 3.8 with `show-options -s/-g/-gw`; the static configs use exactly these scopes.

| Scope | Options |
| --- | --- |
| Server (`-s`) | `default-terminal`, `terminal-features`, `set-clipboard`, `extended-keys`, `extended-keys-format`, `escape-time`, `focus-events` |
| Session (`-g`) | `base-index`, `bell-action`, `history-limit`, `mouse`, `prefix`, `renumber-windows`, `set-titles`, `status-bg`, `status-fg`, `status-left`, `status-left-length`, `status-position`, `status-right`, `visual-bell` |
| Window (`-gw`) | `automatic-rename`, `mode-keys`, `monitor-activity`, `monitor-bell`, `window-status-current-format`, `window-status-format` |

## Capability profiles

The reference was probed on 2026-10-30 with the compiled profile. The adapter records the exact sequences it observes in `tests/reference/capabilities.rs`; these are the starting values, not guesses.

| TERM/profile | Alternate | Paste | Focus | Sync | Clipboard | Extended keys |
| --- | --- | --- | --- | --- | --- | --- |
| `tmux-256color`, compiled features | `?1049h/l` | `?2004h/l` | `?1004h/l` after negotiation | `?2026h/l` | `OSC 52` via `Ms`/feature | none |
| `xterm-ghostty`, compiled profile | `?1049h/l` | `?2004h/l` | after negotiation | `?2026h/l` | `OSC 52` via `Ms` | none |
| `xterm-256color` | `?1049h/l` | `?2004h/l` | after negotiation | none | `OSC 52` via `Ms` | none |
| runtime `terminal-features ...:extkeys` | as TERM | as TERM | as TERM | as TERM | as TERM | `>4;2m` enable, `>4m` disable |

## Reference

Verified at `05f4737`: `src/config.rs` (options/bindings), `src/server.rs` (`execute_bound_commands`, `select_layout`, `break_pane`, `join_pane`, `handle_client_input`, `spawn_reader`, `retain_raw_output`, `render_session_with_clear`, `execute_request`), `src/client.rs` (`attach_loop`), `src/pty.rs` (`spawn`), `src/terminal.rs` (`OutputState`). Pinned reference sources: `cmd-break-pane.c`, `layout-set.c`/`layout.c` (`layout_spread_cell`), `input.c` (REP clamp, OSC 52, queries), `input-keys.c` (extended encoding), `tty.c`/`tty-keys.c` (modes, paste, sync), `alerts.c` (bell). Row-level evidence lives in `docs/parity.md`; the reference profile is `tests/reference/tmux-3.8.conf`.

## Source anchors

- [Reviewed project snapshot](https://github.com/laputa-systems/tm/tree/05f4737bdfa5c032bf511bb98a8775134ca4e9ad)
- [Compiled profile](https://github.com/laputa-systems/tm/blob/05f4737bdfa5c032bf511bb98a8775134ca4e9ad/src/config.rs)
- [Existing findings](https://github.com/laputa-systems/tm/blob/05f4737bdfa5c032bf511bb98a8775134ca4e9ad/TODO.md)
- [Existing testing contract](https://github.com/laputa-systems/tm/blob/05f4737bdfa5c032bf511bb98a8775134ca4e9ad/docs/testing.md)
- [Reference release](https://github.com/tmux/tmux/releases/tag/3.8)
- [Versioned upstream manual](https://github.com/tmux/tmux/blob/3.8/tmux.1)
- [Input and REP behavior](https://github.com/tmux/tmux/blob/3.8/input.c)
- [Pane-input encoding](https://github.com/tmux/tmux/blob/3.8/input-keys.c)
- [Equal layouts](https://github.com/tmux/tmux/blob/3.8/layout-set.c)
- [Sole-pane break behavior](https://github.com/tmux/tmux/blob/3.8/cmd-break-pane.c)
- [Keyboard encoding and mode-stack protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/)
