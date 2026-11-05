# Make tm's tmux-compatibility claims executable and true

## Status

- **Goal:** within tm's compiled profile and narrow scope, every behavior advertised as matching tmux 3.8 has evidence that runs both implementations, observes the effect exactly, and passes at the qualified commit. The known CLI and lifecycle mismatches are fixed. Generated histories cover multi-step use, and the release artifact on this host passes the suite.
- **Base:** `d171bbd`, toolchain `nightly-2026-09-15` (pinned; do not change), host x86_64 Alpine (`x86_64-unknown-linux-musl`).
- **Preflight:** 2026-11-05. Verdict: ready.
- **Accepted:** G0 (baseline reconfirmed at `4e84fe0`, which adds only this doc to `d171bbd`), G1, G2, G3, O1 (`KNOWN_DIFFERENCES` empty: exact comparison exposed no difference in the existing differential tests), O2 (raw runner `tests/support/raw_pty.rs`; fixed a resize row-loss mismatch), O2.1–O2.4, O2.5 (`src/capture.rs` matches tmux `grid_string_cells` for `-J`, `-e`, `-N`), C1 (`Request::SendPrefix { target }`; `-2` is rejected, so the `secondary` field D4 sketched would be constant and is omitted), C2 (`Request::NewSession` loses `empty`; detached `new-session` no longer prints the session name without `-P`/`-F`), M1 (Evidence column; 51 rows that were `pass` on tm-only evidence are now `fail`/`internal`), L1 (TMUX-9 closed: the listener closes before `kill-server` replies; this exposed a timing-dependent sync-bracket decision, fixed in `f160c95`), L2 (TMUX-8 closed; `#{pane_pipe}` added), V1 (version preamble `0x544d_0001`; the 1024-argument cap is enforced in the encoder; full gate green at `f968ee5`), C3 (83 flag rows; shared `unsupported_flag` and `scan_flags` in `src/command.rs`; unknown `-t` session names no longer fall back to the first session), C3.1–C3.3 (the daemon exits after the last session; most-recently-active default session; tmux no-server text), C4 (window flags; first window named after the command), C4.1–C4.5 (`link-window` rejected; `PROTOCOL_VERSION` now 4), C5a with C4.6 (pane flags; `new-pane`, `split-window -f`, and `select-pane -n`/`-p` rejected; `list-panes -a`/`-s`/`-f` implemented; `PROTOCOL_VERSION` now 5), C5b (resize, respawn, display, pipe, capture and clear flags; `capture-pane` without `-p` writes a buffer; `PROTOCOL_VERSION` now 6; dev builds now abort on panic like release, after a panic left an unkillable daemon), C6a (buffer, `send-keys`, `display-message`, `run-shell` and `list-windows` flags; TMUX-16 closed; `PROTOCOL_VERSION` now `0x544d_000a`), C6b (option and environment flags; `src/options.rs` holds tmux 3.8's option table, so the table is chosen by option name; session environments; panes start from the global environment overlaid by the session's; `set-option -a`/`-o`/`-F` rejected; `PROTOCOL_VERSION` now `0x544d_000b`), C6b.1 (style and colour values validated like tmux, including the X11 colour names), C6c (copy-mode and choose-* flags; copy-mode pages by height minus 2 like tmux; `tests/parity_contract.rs` fails when a command arm in `src/command.rs` has no `## CLI flags` row), H1 (64-seed topology corpus, 24 s, under `make parity`; 14 reduced regression tests; tm fixes in `87673f1`, including a daemon abort in cross-session `break-pane`; TMUX-30 to TMUX-32 filed and excluded by typed `Exclusion`s), H2 (48-seed attached corpus on the raw-PTY runner, about 20 s, stable over 3 runs; tm now prints the detach and exit messages, `command-prompt` shows `:`, and the status line lost a stray pad column; TMUX-33 to TMUX-37 filed and excluded; TMUX-38 filed because tm ignores the caller's pane for omitted targets, so H2 drives omitted targets through the command prompt).
- **Red / parked:** O2.6, capture of cells erased after being written (`fail` row `capture of erased cells`, two `KNOWN_DIFFERENCES` entries). tmux keeps each line's written extent and wrapped flag through erase, scroll, and resize; tm's vt100 grid keeps neither, and vt100 maps `38;5;9` to the same cell colour as `91`. *Retry when* tm's grid records per-line written extent and indexed versus bright colour; otherwise Z moves the entries to intentional differences with this reason. Baseline reds are listed in **Baseline**.
- **Size risk for P1:** `make release` is 1,635,704 bytes at `bc7360c`, 10.1% over the 1,485,880 baseline against a 5% budget. Growth by slice: G0–C4 +27 KB, C5 +35 KB, C6a +18 KB, C6b +27 KB, C6b.1 +43 KB (style parser and X11 colour table). P1 first shrinks without touching runtime paths (compact static tables, monomorphization, dead code from Z); if it is still over, P1 reports the gate red with the per-slice breakdown for the owner rather than raising the budget.
- **Next:** H3 (byte-split input), then H4 and the full gate, M1.1, Q1, D1, P1, Z. Z also deletes request fields C6c left constant (`copy-mode` `-k`/`-M`/`-S`, choose-* options) and the `DisplayPanes` fields (`source`, `no_zoom`, `no_mode`, `kill_on_exit`) unless TMUX-18 restores them. TMUX-29: the server-side parsers for bound commands still accept flags the CLI rejects; bindings are compiled, so this is not user-reachable, but Z should share one parser. TMUX-10 to TMUX-38 are filed.
- **Isolation incident (C6c):** an unquoted backtick in a worker's shell command ran the installed `~/.local/bin/tm`, which started daemon pid 26811 on `/tmp/tm-1000.sock` at 22:47. The Charter forbids killing it or touching that socket, so it is left for the owner. Later briefs require single-quoted search patterns.
- **Ruling (affects O2, H2, H3):** ptytest 0.1.1–0.3.0 rejects any escape with an intermediate byte, such as `ESC ( B`, under every profile, so it cannot attach the reference. Differential attached observations use a raw-PTY runner in `tests/support/` (openpty, a reader thread, and the existing `vt100` dev-dependency for screen state; it answers DA1, without which tmux withholds `?1004h`); ptytest stays for tm-only scenarios. The upstream ptytest fix is listed under **Needs environment**.

## Charter

- **Owner's tmux is live.** The owner runs tmux, and this campaign may run inside it, so `TMUX`, `TMUX_PANE`, and `/tmp/tmux-1000/default` are present. Run every build, test, `tmux`, and `tm` command through the isolation prefix:

  ```sh
  env -u TMUX -u TMUX_PANE -u TM_SOCKET -u TM_CONFIG TMPDIR="$PWD/.work/tmp" TMUX_TMPDIR="$PWD/.work/tmp" <command>
  ```

  Every manual `tmux` or `tm` invocation also passes `-S "$PWD/.work/tmp/<name>.sock"`. The following are forbidden:
  - bare `tmux` or `tm`, `tmux -L`, and any `kill-server` without `-S`;
  - `pkill`, `killall`, or `kill` against a process whose argv does not contain this repo's absolute `.work/` or `target/` path;
  - `tmux attach` or `tm attach` in the foreground shell; PTY tests use ptytest's own PTY;
  - reading or writing `~/.config/tmux`, `/tmp/tmux-*`, or `/tmp/tm-*.sock`;
  - `make install`, which overwrites `~/.local/bin/tm`.

  After each milestone, `pgrep -af "$PWD/(target|\.work)"` must print nothing. A leftover daemon is a lifecycle bug: kill it by PID and fix the cause.
- **Decide and keep going.** Record a decision here only when a later slice depends on it. Use the smallest reversible assumption for ordinary ambiguity. The owner is not available during execution.
- **Authority:**
  - **Allowed:** edit this repo; add test files, fixtures, and helpers; make the contract changes in **Decisions**; build the pinned reference into `.work/ref`; run Docker with the repo's `Dockerfile`; make local Conventional Commits.
  - **Forbidden:** pushing, publishing, GitHub settings, editing `.github/`, changing `rust-toolchain.toml` or `.cargo/config.toml`, installing system packages, and anything outside the repo. An item that needs any of these is parked.
- **Commits:** `type(scope): subject`, with the workstream as scope (`oracle`, `cli`, `lifecycle`, `protocol`, `histories`, `qualify`, `docs`, `perf`). The item ID goes in the body. A contract change takes `!`. Code comments never cite this doc. No formatters.
- **One writer.** Slices run serially except the pairs named in **Sequence**. Lane worktrees live at `.work/wt/<lane>` and follow `orchestrate`'s worktree lifecycle (`~/.claude/skills/orchestrate/SKILL.md`, "Worktree lifecycle").
- **Steering.** After each accepted slice, on resume, and after a second failure of the same slice, run the parent steering checkpoint (`~/.claude/skills/orchestrate/references/campaigns.md`, "Parent steering checkpoint"). Then rewrite **Status** in place. Git history is the log. Never append logs, handoffs, or dated checkpoints here.
- **Parity before optimization.** Fix the following on sight, in their own commit:
  - session loss;
  - a daemon-wide hang;
  - unbounded memory growth;
  - an unrelated command delayed more than 2 s;
  - a leaked daemon or child process;
  - a single test taking more than 10 s of wall time, or a test binary more than 30 s, without a stated reason.

  Record anything smaller in `TODO.md` as `TMUX-<n>`, with the next number from the file header (currently 39).
- **Root cause first.** Reproduce with the smallest failing test, fix the responsible layer, and keep the test (`root-cause`). Never retry without a cause. A failure that shows up only under host load is a synchronization bug until shown otherwise. Other campaigns share this host. After 3 failed attempts, park the slice on branch `park/<id>` with the cause and a retry condition in **Status**, then move on.
- **Oracle rule.** A parity claim must either execute the reference at `.work/ref/tmux-3.8/bin/tmux` or cite a recorded reference observation in `tests/reference/`. A test derived only from tm output is `internal` evidence. Never set `TMUX_REF`, and never use `/usr/bin/tmux` as the reference.
- **Checks.** A slice ends on its acceptance command plus the narrowest affected suite, run on the integrated tree. The full gate is `cargo test --no-fail-fast && make parity && make lint`, about 2.5 min. It runs after G3, after V1, after H4, and at Z. Never weaken, delete, or skip a test, and never accept an unexplained golden update. When a test was itself wrong, the commit body says so.
- **Dependencies.** Production: none expected; a new one needs a named need and must pass the P1 gates. Dev-only: allowed when std plus about 100 lines will not do. The commit body names the dependency's license and its effect on build time, and the `cargo test` build time must not grow by more than 10%.

## Decisions

Owner decisions (2026-11-05):

- **O1 Isolation:** the owner's running tmux must not be disturbed at any point in the campaign. Enforced by the **Charter** isolation rules.
- **O2 Reference:** the owner installed `libevent-dev`, `utf8proc-dev`, `ncurses-dev`, and `vim`. The executor builds the hash-pinned reference with `tests/support/install-tmux-ref.sh` (about 10 s).
- **O3 Formal methods:** Lean and Aeneas proofs and pure-core extraction are out of scope. The protocol's admissible domain is enforced by types and tested instead (D10).

Prescribed decisions. Each one states what would reverse it.

- **D1 Scope.** Keep the compiled profile and the non-goals listed under **Boundary**. The target is reliable compatibility for the selected profile, plus explicit exceptions. *Reverse:* an owner request for a broader surface.
- **D2 Flag rule.** For every command and flag tm accepts, one of these holds:
  - **match:** tm behaves as tmux 3.8 does;
  - **rejected:** tm exits nonzero with `unsupported flag <f> for <command>`;
  - **tm-only:** the command or flag is listed as an intentional difference (for example `copy-mode-and-page`, `help`, and aliases tmux lacks).

  An accepted flag with a different meaning is never allowed. Inputs that tmux accepts and tm rejects, such as unique-prefix command names, are `rejected` rows, not defects. *Reverse:* a workflow the owner names.
- **D3 `new-session -E`.** In tmux 3.8, `-E` skips `update-environment`. tm has no `update-environment`, so `-E` becomes a no-op: the pane runs the given command or the default shell. `new-window -E` and `split-window -E` already match tmux (empty pane; a command is an error) and stay as they are. Tests that used `new-session -E` to get an empty pane switch to `-- sleep 300`. This is a contract change. *Reverse:* tm gains `update-environment`, in which case `-E` must skip it.
- **D4 `send-prefix`.** Becomes a semantic request (`Request::SendPrefix { target, secondary }`), resolved on the server against the session's `prefix`. The CLI and interactive paths share one implementation, and the literal `vec![2]` is deleted. tm has no `prefix2`, so `-2` must match the reference's observed bytes with tmux's default `prefix2` (None); if no bytes are observed, tm rejects `-2`.
- **D5 `kill-server` postcondition.** A success response means the listener is closed and the socket path no longer accepts connections. The daemon may still be tearing down panes. A connection accepted during shutdown gets an explicit error response, never a silent EOF. The client does not blanket-retry, because a mutating request may already have run. A retry is allowed only for a connect that fails before any request byte is written.
- **D6 Pipe helpers.** A pipe-pane helper that exits is reaped within 1 s, and the pane's pipe state (`#{pane_pipe}`) then matches the reference.
- **D7 Exact observations.** These rules govern the `Outcome` type in `tests/support/parity.rs`.
  - `Outcome` records `ExitStatus::{Code(i32), Signal(i32)}` and raw `Vec<u8>` stdout and stderr. It is compared on every field, including stderr on success.
  - Normalization covers only the scratch socket, root, and home paths, the version string, and the `tm: ` / `tmux: ` error prefix. Delete the no-op `("tm ", "tm ")` pair at `tests/support/parity.rs:292`.
  - The runner drains both pipes on threads while the child runs.
  - Differences exposed by exact comparison go into a typed `KNOWN_DIFFERENCES` list in `tests/support/parity.rs`. Each entry is keyed by test and command, links a `fail` row in `docs/parity.md`, and asserts that the difference still exists, so fixing it fails the test until the entry is removed. Z requires the list to be empty, or each remaining entry moved to an intentional difference with its reason.
- **D8 Evidence labels.** `docs/parity.md` gains an `Evidence` column with three values:
  - `differential`: the cited test constructs `Mux::reference` or calls a helper that does;
  - `recorded`: compared against a reference observation stored under `tests/reference/`;
  - `internal`: tm only.

  A row may be `pass` only with `differential` or `recorded`; `internal` rows are not `pass`. `tests/parity_contract.rs` checks that each cited file and function exists and, for `differential`, that the function body reaches `Mux::reference`. Running `make parity` at the commit is the run evidence. No generated report or results file is kept: the draft asked for one, but recording results duplicates the test run.
- **D9 Terminal observation.** The vt100 reparse in `tests/terminal_corpus.rs:139` stays for screen equivalence. Promised effects are also asserted on the raw outer byte stream for both implementations:
  - OSC 52 clipboard;
  - BEL;
  - alternate-screen, bracketed-paste, focus, and mouse mode set and reset;
  - sync brackets;
  - no title escape.

  Do not add a second terminal parser dependency.
- **D10 Protocol.** Three requirements.
  - **Admissible domain:** every request value the client can build must be decodable. Enforce the 1024-argument cap (`src/protocol.rs:1576`) on construction or in the encoder (`src/protocol.rs:1423`), and return an error, never a truncation. A round-trip test covers requests generated inside the domain, and the boundary cases 1024 and 1025.
  - **Version check:** every connection starts with a `PROTOCOL_VERSION: u32`. On mismatch the client prints `tm server at <socket> runs a different protocol; run tm -S <socket> kill-server` and exits nonzero. An old daemon that cannot decode the new framing closes the connection, so a client that gets EOF before any response byte prints the same message.
  - **Golden bytes:** a golden-bytes test of one representative request of each variant forces a `PROTOCOL_VERSION` bump whenever the format changes.

  This is a contract change.
- **D11 Generated histories.** Follow these rules; histories never write artifact files.
  - Use no new dependency: a seeded xorshift64 generator and ddmin shrinking over the operation list.
  - The default corpus is fixed seeds, completes in 60 s or less, and runs under `make parity`.
  - A failure panics with the seed, the full and the reduced operation list, both implementations' observations, and `tmux -V`.
  - `TM_HISTORY_SEED=<n>` replays one case.
  - Each reduced failure becomes a named regression test.
  - Use real time only. Controlled scheduling and logical time need the pure-core extraction that O3 defers.
- **D12 Artifact under test.** All test harnesses resolve the tm binary through one helper in `tests/support/`. The helper returns `TM_BIN` when set, otherwise `env!("CARGO_BIN_EXE_tm")`. 14 files use `CARGO_BIN_EXE_tm` today.

Delegated decisions. The executor decides within the stated bound and records the choice only if a later slice depends on it.

- How to split C3–C6 within each command family, at no more than about 12 commands per slice.
- Test names, which must name the behavior.
- Whether the G2 and G3 fixes land in the harness or the product, as the root cause dictates.
- How to fix the musl `time_t` deprecation in G1, without `#[allow]`.
- The operation weights in H1 and H2, provided every operation in the item appears at least once per 10 seeds.

## Baseline

Measured 2026-11-05 at `d171bbd` on this host, under concurrent load from other jobs, through the isolation prefix:

| Check | Result | Wall time |
| --- | --- | --- |
| `cargo build --all-targets` | green | 38 s |
| `cargo test --no-fail-fast` | 1 flaky red (below; passed on the G0 rerun) | 86–132 s, of which `tmux_modes` takes 55–86 s |
| `make parity` (`tmux_parity`) | green, 6 tests | 7 s |
| `tests/support/install-tmux-ref.sh` | green, tmux 3.8 | 10 s |
| `make lint` | **red**: 10 `deprecated libc::time_t` errors (musl) at `src/server.rs:13221-13245`; green since G1 | 6 s |
| focused suite, e.g. `cargo test --test tmux_core_compat` | green | 2 s |
| `make release && make verify-release` | green; dist `tm` is 1,485,880 bytes (P1 size baseline) | 60 s |

Known red before any change:

- TMUX-9 reproduces 10 of 10: `tm -S S kill-server; tm -S S new-session -d` gives `failed to fill whole buffer`.
- TMUX-8 reproduces: after `pipe-pane -t S -- true`, a `[sh] <defunct>` child stays under the daemon.

`make test` stops at the first failing binary. Use `cargo test --no-fail-fast` to see all of them. Without the reference, 16 tests in `terminal_corpus`, `tmux_copy_cells`, `tmux_cwd`, `tmux_history`, `tmux_layouts`, and `tmux_windows` fail at `tests/support/parity.rs:80`. Without `vim`, `tmux_apps` fails.

## Sequence

The phases run in order: ground (G), oracle (O, M), CLI and lifecycle parity (C, L, V), generated histories (H), qualification and docs (Q, D), optimization (P), close (Z). Run each slice's acceptance command through the isolation prefix. Each slice also reruns the existing suites it touches.

| ID | Size | Prereq | Owns | Work | Acceptance |
| --- | --- | --- | --- | --- | --- |
| G0 | S | — | `parity-campaign.md` | Build the reference. Confirm **Baseline**. Record the dist binary size from `make release && make verify-release` for P1. Commit this doc. | `tests/support/install-tmux-ref.sh && make parity` |
| G1 | S | G0 | `src/server.rs` (time formatting) | Fix the musl `time_t` deprecation at its cause. | `make lint` |
| G2 | S | G1 | `tests/tmux_signals.rs`, `tests/support/pty_harness.rs`, `src/client.rs` if the cause is there | Root-cause the SIGTSTP flake. Replace the settle wait with a barrier on a terminal-state or fixture marker. | `for i in $(seq 20); do cargo test --test tmux_signals || exit 1; done` |
| G3 | S | G2 | `tests/tmux_modes.rs`, the harness or `src/server.rs` as the cause dictates | Root-cause the 4 tests over 60 s. | `cargo test --test tmux_modes` green in ≤ 10 s; full gate |
| O1 | M | G3 | `tests/support/parity.rs`, new `tests/parity_oracle.rs`, every caller of `Outcome` | Apply D7: exact `Outcome`, threaded pipe draining, stderr always compared, minimal normalization, and `KNOWN_DIFFERENCES`. Add a comparator self-test that mutates an exit code, one stdout byte, successful-command stderr, and a terminal mode, and requires each to be detected. Add a 4 MiB stdout command that completes. | `cargo test --test parity_oracle && make parity` plus the suites that use `Outcome` |
| O2 | M | O1 | `tests/support/pty_harness.rs`, `tests/terminal_corpus.rs`, `tests/tmux_terminal_modes.rs`, `tests/tmux_notifications.rs` | Apply D9 with raw-stream effect assertions for both implementations. Replace the `terminal-features` evidence with a `differential` test of `tmux-256color:Sync:clipboard`, or relabel the row `recorded` with a stored observation. | `cargo test --test terminal_corpus --test tmux_terminal_modes --test tmux_notifications` |
| M1 | S | O1 | `docs/parity.md`, `tests/parity_contract.rs`, renames in cited tests | Apply D8: add the evidence column and the stronger validator, relabel every row, and rename or rewrite `tests/tmux_input.rs:134` (`literal_prefix_bindings_match_reference_bytes` never runs the reference). | `cargo test --test parity_contract`; temporarily citing a nonexistent function must fail |
| C1 | S | O1 | `src/command.rs`, `src/protocol.rs`, `src/server.rs`, `tests/tmux_input.rs` | Apply D4. Add a differential test: CLI `send-prefix` in the normal profile writes the same pane bytes as tmux (`01`), and `-2` follows D4. | `cargo test --test tmux_input && cargo test --features parity --test tmux_parity` |
| C2 | S | C1 | `src/command.rs` (`new-session` parser, `:213`), `tests/tmux_core_compat.rs` | Apply D3. Add a differential test for `new-session -d -E -- cmd` and migrate the empty-pane uses. | `cargo test --test tmux_core_compat` |
| L1 | M | C2 | `src/server.rs` (accept loop `:214`, kill-server), `src/client.rs` (`:53`), `tests/tmux_lifecycle.rs`, `TODO.md` | Apply D5 and close TMUX-9. | 200 kill/new cycles with 0 failures; an attach during shutdown gets an error or a new server, never EOF; tmux behaves the same in a differential run; `cargo test --test tmux_lifecycle --test core_smoke --test socket_permissions` |
| L2 | S | L1 | `src/server.rs` (`pipe_pane` `:6608`, writer `:6648`), `tests/tmux_lifecycle.rs`, `TODO.md` | Apply D6 and close TMUX-8. | Test: no defunct child of the daemon 1 s after the helper exits (`/proc` on Linux, `ps` on macOS), and `#{pane_pipe}` matches the reference; `cargo test --test tmux_lifecycle --test tmux_liveness` |
| V1 | M | L2 | `src/protocol.rs`, `src/client.rs`, `src/server.rs` (`handle_connection`), protocol tests | Apply D10. | Protocol tests: round-trip, 1024/1025, golden bytes, a mismatched version gives the message and a nonzero exit, and EOF before a response gives the same; full gate |
| C3 | M | V1 | `src/command.rs`, `src/server.rs`, new `tests/tmux_cli_flags.rs`, `docs/parity.md` (new **CLI flags** table) | D2 inventory, sessions and clients: `new-session`, `attach-session`, `detach-client`, `has-session`, `kill-session`, `kill-server`, `list-sessions`, `list-clients`, `rename-session`, `switch-client`, `refresh-client`. Each accepted flag gets a differential probe that compares the outcome and the resulting `list-*` topology. | `cargo test --features parity --test tmux_cli_flags sessions` |
| C4 | M | C3 | same | Windows: `new-window`, `kill-window`, `select-window`, `next-window`, `previous-window`, `last-window`, `link-window`, `unlink-window`, `move-window`, `swap-window`, `rotate-window`, `rename-window`, `respawn-window`, `select-layout`. | `... tmux_cli_flags windows` |
| C5 | M | C4 | same | Panes: `split-window`, `select-pane`, `kill-pane`, `break-pane`, `join-pane`/`move-pane`, `swap-pane`, `resize-pane`, `respawn-pane`, `display-panes`, `pipe-pane`, `capture-pane`, `clear-history`. | `... tmux_cli_flags panes` |
| C6 | M | C5 | same | The rest: keys, buffers, options, environment, display, `run-shell`, `copy-mode`, `choose-*`, `help`, and the tm-only commands. | `... tmux_cli_flags misc`; the **CLI flags** table lists every `Some("…")` arm in `src/command.rs` |
| H1 | M | C6 | new `tests/support/history.rs`, new `tests/tmux_histories.rs` | D11 topology histories covering: new/kill session, window, and pane; split; select; swap; join; break; move; rename; resize; `send-keys`; and invalid targets. Compare every outcome, plus `list-sessions`/`list-windows`/`list-panes -a -F` after each step. A failed target must leave the topology unchanged. | `cargo test --features parity --test tmux_histories topology` in ≤ 60 s |
| H2 | M | H1 | `tests/support/history.rs`, `tests/tmux_histories.rs`, `tests/support/pty_harness.rs` | Attached histories covering: two clients at different sizes; attach, detach, and `switch-client`; omitted-target commands from each client; resize; a pane child exiting with unread output; SIGKILL to a client during rendering; and detach during a redraw. Observe `list-clients`, the semantic screen per client, and the outer modes and termios restored. | `... tmux_histories attached` in ≤ 60 s |
| H3 | S | H2 | `tests/tmux_input.rs`, `tests/tmux_paste.rs` | Split each of these at every byte boundary with no delay: prefix bindings, CSI-u and `27;mod;code~` keys, SGR mouse, focus, and a bracketed paste. The pane bytes must equal the unsplit input, for both implementations. One case with a delay past `escape-time` must change meaning exactly as the reference does. | `cargo test --test tmux_input --test tmux_paste` |
| H4 | S | H3 | new `tests/tmux_unicode_history.rs` | Differential test: a wide character at the right margin, then wrap, narrower and wider resize, trimming at `history-limit`, a copy-mode copy, and `capture-pane -e -J`. | `cargo test --test tmux_unicode_history`; full gate |
| Q1 | M | H4 | new `tests/support/` binary helper, the 14 `CARGO_BIN_EXE_tm` users, `Makefile` (`qualify` target) | Apply D12. `make qualify` runs `release`, then `verify-release`, then `TM_BIN=target/$(TARGET)/dist/tm` with `cargo test --no-fail-fast` and `make parity`. | `make qualify` green on this host |
| D1 | S | Q1 | `docs/testing.md`, `docs/parity.md`, `docs/architecture.md`, `README.md`, `TODO.md` | Document the isolation prefix, evidence labels, histories and replay, `make qualify`, the `kill-server` postcondition, and the version check. Write each contract change in README. | `mdtool check docs README.md` |
| P1 | S | D1 | `benches/tm_performance.rs`, `docs/performance.md`, `src/` only for a measured regression | Run `make perf` interleaved against `d171bbd`, gated by `perf-gate-ready`. Check the dist binary size against G0. Keep the 16 ms and 32 KiB budgets. | Medians inside the observed spread; size growth ≤ 5% or explained in the commit body; otherwise "perf gate deferred: host busy" |
| Z | M | P1 | anything dead, `plan.md`, this doc | Empty or justify `KNOWN_DIFFERENCES`. Delete dead code (`dead-code-archaeology`; liveness proven by build plus `rg`). Delete the finished `plan.md`, which nothing links. Run `ultrareview` and fix its P1 and P2 findings. Rewrite **Status** as the final report. | `cargo test --no-fail-fast && make parity && make lint && make qualify`; `pgrep -af "$PWD/(target|\.work)"` prints nothing |

Parallel lanes are allowed only for **M1 ∥ L1** and **M1 ∥ L2**. Their file sets are disjoint provided L1 and L2 leave `docs/parity.md` alone; the executor confirms with `git diff --name-only` before merging. Everything else is serial, because the `src/` slices share `src/server.rs` and `src/command.rs`, and the C and H slices share their test files.

Any new mismatch a slice exposes is handled one of two ways. If the fix is small (S effort), fix it in its own `fix(<ws>)` commit inside the slice, with a differential test. Otherwise add it to `KNOWN_DIFFERENCES` with a `fail` row and schedule it as item `<slice>.<n>` right after the current slice.

## Acceptance

The campaign is done when all of the following hold:

- **Exact comparison:** every `pass` row in `docs/parity.md` is `differential` or `recorded`; the validator rejects a missing function, a missing file, and a `differential` row that never runs the reference; and the comparator self-test detects all four mutations (O1, O2, M1).
- **Command semantics:** `send-prefix` on both entry points sends the session prefix; `new-session -E` keeps the command and the shell; and every `Some("…")` arm in `src/command.rs` appears in the **CLI flags** table as match, rejected, or tm-only, with a differential probe for each match (C1–C6).
- **Lifecycle:** 200 `kill-server`/`new-session` cycles fail 0 times; an attach during shutdown never sees a silent EOF; and an exited pipe helper is reaped within 1 s while `#{pane_pipe}` matches the reference (L1, L2).
- **Protocol:** every request the client can build round-trips; 1025 arguments are refused before sending; a version mismatch or an old daemon gives the clear message and a nonzero exit; and the golden bytes pin the format (V1).
- **Histories:** the topology, attached, fragmentation, and unicode-history corpora pass against the reference within their time budgets, and every reduced failure is a named regression test (H1–H4).
- **Artifact:** the `make release` binary for `x86_64-unknown-linux-musl` passes the full suite and parity through `TM_BIN` (Q1).
- **Baseline cleared:** lint is green, the SIGTSTP test passes 20 of 20, `tmux_modes` takes 10 s or less, and TMUX-8 and TMUX-9 are closed in `TODO.md`.
- **Isolation held:** no process from this repo remains after any milestone, and the owner's tmux server was never addressed.
- **Optimization:** the P1 gates pass or are reported as deferred.

The final report rewrites **Status**. It gives:
- the start and end commits;
- user-visible fixes;
- each contract change;
- pass, fail, and blocked counts per matrix section;
- the commands run and their outcomes;
- remaining intentional differences;
- the **Needs environment** items.

## Needs environment

These checks cannot run on this host. The campaign does not wait for them.

- **Hosted CI.** The workflows have not run on the result. The owner pushes, then runs `gh run watch`.
- **macOS crate mirror.** The hosted macOS job got HTTP 403 from `crates.17166969.xyz`. That is the owner's registry infrastructure (`stuff-registries`); local Linux fetches work.
- **aarch64 musl, native.** On an arm64 host: `docker build --platform linux/arm64 -t tm-builder . && docker run --rm --init --platform linux/arm64 -v "$PWD:/tm" -w /tm tm-builder sh -ec 'make qualify TARGET=aarch64-unknown-linux-musl'`. This needs the reference dependencies in the image, or a host build.
- **macOS arm64.** `make qualify TARGET=aarch64-apple-darwin` on the owner's Mac.
- **Ghostty workflows.** Shell editing, the owner's editor and coding-agent TUIs, paste, copy, resize, detach and reattach, and suspend and resume. Run them in `tm -S .work/tmp/manual.sock` under Ghostty, outside the owner's tmux.
- **ptytest escape scanner.** `laputa-ptytest` rejects nF escapes (`ESC` + 0x20–0x2f intermediates + final), which tmux emits on attach. Fixing it is outside this repo; once fixed, the raw-PTY runner can be replaced by ptytest.
- **Real clipboard and audio.** Tests assert the OSC 52 and BEL bytes. The system clipboard contents and the audible bell need a human.

## Boundary and non-goals

**Authorized effects:** listed in the **Charter**. **Forbidden effects:** listed in the **Charter**. The owner's tmux server, configuration, and sockets are never addressed.

**Contract changes**, surfaced in `!` commits and README:

1. `new-session -E` no longer creates an empty pane (D3).
2. CLI `send-prefix` sends the configured prefix, not `C-b` (D4).
3. `kill-server` returns only after the socket stops accepting connections (D5).
4. The wire protocol gains a version check, so a new client refuses an older daemon with a clear message (D10).
5. Over-long argument lists are refused on the client (D10).
6. Matrix rows carry evidence labels; former `pass` rows with only `internal` evidence become `fail` or `blocked` until proven (D8).

**Non-goals:**
- Lean, Aeneas, or extraction of a pure core (O3).
- Controlled scheduling and logical time (D11).
- Parsing tmux configuration files, `TM_CONFIG`, or the user's tmux config.
- Arbitrary option combinations, plugins, hooks, control mode, and floating panes.
- Full terminfo parsing; Windows.
- Interoperability with tmux's `imsg` protocol.
- Implementing new tmux features beyond what D2 needs. Rejecting a flag is preferred over implementing it.
- A rewrite of `server.rs` or an async runtime.

## Reference

Verified at `d171bbd`:

| What | Where |
| --- | --- |
| CLI `send-prefix` sends a literal `C-b` | `src/command.rs:100` |
| Interactive `send-prefix` sends the configured prefix | `src/server.rs:4360` |
| `new-session -E` maps to an empty pane | `src/command.rs:213`, `src/server.rs:1729` |
| `new-window -E` and `split-window -E` (correct as is) | `src/command.rs:434`, `src/command.rs:511` |
| Accept loop polls shutdown every 20 ms | `src/server.rs:214` |
| Client connects, then writes immediately with no version check | `src/client.rs:17`, `src/client.rs:53` |
| Pipe helper `wait` only after `Close` | `src/server.rs:6608`, `src/server.rs:6648` |
| Encoder and decoder disagree on the argument cap | `src/protocol.rs:1423`, `src/protocol.rs:1576` |
| Lossy `Outcome`; stderr compared only on failure; pipes drained only after exit | `tests/support/parity.rs:245`, `:251`, `:299` |
| Matrix validator checks only that the file exists | `tests/parity_contract.rs:63` |
| `terminal-features` row cites a tm-only extkeys test | `tests/tmux_terminal_modes.rs:135` |
| Shared vt100 reparse | `tests/terminal_corpus.rs:139` |

Reference behavior was observed on 2026-11-05 with tmux 3.8 on a private socket:
- `new-session -d -E 'sleep 30'` runs `sleep 30`.
- `new-window -E` and `split-window -E` create empty panes.
- `new-window -E 'echo x'` fails with `command cannot be given for empty pane`.

Upstream sources: [tmux 3.8 `cmd-send-keys.c`](https://github.com/tmux/tmux/blob/3.8/cmd-send-keys.c), [`cmd-new-session.c`](https://github.com/tmux/tmux/blob/3.8/cmd-new-session.c), [`tmux.1`](https://github.com/tmux/tmux/blob/3.8/tmux.1).
