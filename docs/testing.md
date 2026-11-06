# Testing

Run the host test suite with `make test`. `make test-ci` runs it in the release
profile. `make parity` installs the pinned tmux 3.8 reference when needed and
runs the differential suites listed in the `parity` target of the
[`Makefile`](../Makefile) with `--features parity`.

## Isolation

On a machine where you run tmux, a test run, `tm`, or `tmux` can reach your
own server through `TMUX`, `TMUX_PANE`, or the default socket. Run every build,
test, and manual command through this prefix from the repository root:

```sh
env -u TMUX -u TMUX_PANE -u TM_SOCKET -u TM_CONFIG \
  TMPDIR="$PWD/.work/tmp" TMUX_TMPDIR="$PWD/.work/tmp" <command>
```

Pass `-S "$PWD/.work/tmp/<name>.sock"` to every manual `tm` or `tmux`
command, including `kill-server`. Never run a bare `tm` or `tmux`, `tmux -L`,
or `tmux attach` in your shell, and never run `make install` while testing: it
overwrites `~/.local/bin/tm`. After a run, `pgrep -af "$PWD/(target|\.work)"`
must print nothing. A daemon left behind is a lifecycle bug.

## Reference tmux

[`tests/support/install-tmux-ref.sh`](../tests/support/install-tmux-ref.sh)
downloads tmux 3.8, checks its SHA-256, and builds it into
`.work/ref/tmux-3.8/bin/tmux`. It needs the libevent, ncurses, and utf8proc
development packages. The tests start the reference on a private socket with
[`tests/reference/tmux-3.8.conf`](../tests/reference/tmux-3.8.conf), which
mirrors tm's compiled profile. Never point the tests at another tmux.

Without the reference, the differential tests fail instead of skipping.

Linux PTY coverage runs in the release container with the platform and Rust
target that match the host CPU. On an arm64 Mac use `--platform linux/arm64`
and `aarch64-unknown-linux-musl`; the amd64 image is emulated and much slower.
See [release](release.md) for the commands.

Integration tests use private `TM_SOCKET` or `-S` paths and never connect to
an existing server; only the differential suites start the reference tmux. [`tests/tmux_core_compat.rs`](../tests/tmux_core_compat.rs)
covers command, session, window, pane, PTY, capture, format, option, and
`run-shell` behavior. Copy mode, buffers, and compiled startup settings have
focused suites in [`tests/tmux_copy_mode_compat.rs`](../tests/tmux_copy_mode_compat.rs),
[`tests/tmux_buffers_compat.rs`](../tests/tmux_buffers_compat.rs), and
[`tests/tmux_config_compat.rs`](../tests/tmux_config_compat.rs). The daemon
lifecycle smoke test is [`tests/core_smoke.rs`](../tests/core_smoke.rs).

[`tests/attached_client_pty.rs`](../tests/attached_client_pty.rs) covers the
real terminal boundary for tm alone; see [PTY integration tests](ptytest.md).
Passing the suite establishes compatibility only for the exercised flows, not
for the whole tmux command or configuration surface.

## Differential tests

A differential test runs one scenario against tm and the reference and
compares exit status, stdout, and stderr byte for byte, then the resulting
`list-*` state. Only the scratch paths, the version string, and the
`tm: `/`tmux: ` error prefix are normalized. A difference that comes from a
limit of tm's `vt100` grid is pinned in `INTENTIONAL_DIFFERENCES` in
[`tests/support/parity.rs`](../tests/support/parity.rs). The test fails if the
difference changes or disappears, so replacing the grid forces the entries
out. Each entry names its `GridLimit`, which names its row in the
[parity matrix](parity.md).

Attached observations use the raw-PTY runner in
[`tests/support/raw_pty.rs`](../tests/support/raw_pty.rs): it opens a PTY,
reads it on a thread, keeps screen state with `vt100`, and answers the DA1
query without which tmux withholds focus reporting. `ptytest` cannot attach
tmux, because it rejects any escape with an intermediate byte such as
`ESC ( B`, which tmux emits on attach. Use `ptytest` for tm-only scenarios.

Suites beyond `tmux_parity`:

- `tests/tmux_cli_flags.rs` probes every `## CLI flags` row.
- `tests/tmux_histories.rs` runs generated histories, described below.
- `tests/tmux_unicode_history.rs` covers wide characters at the margin,
  resize, `history-limit` trimming, copy across wrapped rows, and
  `capture-pane -e -J`.
- `tests/tmux_input.rs` and `tests/tmux_paste.rs` split prefix bindings,
  extended keys, SGR mouse, focus, and bracketed paste at every byte boundary
  and require the pane to receive the unsplit bytes, as the reference does.
- `tests/tmux_bindings.rs`, `tests/tmux_option_rows.rs`,
  `tests/tmux_mode_rows.rs`, and `tests/tmux_lifecycle_rows.rs` carry the
  evidence for the matrix rows that are not CLI flags.

## Generated histories

`tests/tmux_histories.rs` has two corpora. The topology corpus generates
64 seeded sequences of session, window, and pane commands, including
invalid targets, and compares every outcome plus `list-sessions`,
`list-windows`, and `list-panes -a -F` after each step. The attached corpus
generates 48 sequences on the raw-PTY runner with two clients of different
sizes: attach, detach, `switch-client`, resize, commands typed inside panes,
pane exit, and killed clients. It compares `list-clients`, each client's
screen, and the restored terminal modes. Each corpus runs in about 20 seconds
under `make parity`.

The seeds are fixed. A failure panics with the seed, the full and the reduced
operation list, both observations, `tmux -V`, and a replay command. Run one
case with:

```sh
TM_HISTORY_SEED=<n> cargo test --features parity --test tmux_histories topology
```

Use `attached` instead of `topology` for the attached corpus. Turn each
reduced failure into a named `regression_*` test in the same file. An
operation that exposes a known difference is excluded by a typed `Exclusion`
whose reason names its `TODO.md` id.

## Qualifying the release binary

`TM_BIN=<absolute path>` makes every harness run that executable instead of
the cargo-built `tm`. [`tests/support/tm_bin.rs`](../tests/support/tm_bin.rs)
is the only place that resolves it, and `tests/parity_contract.rs` fails when a
test uses `CARGO_BIN_EXE_tm` directly. `make qualify` runs `make release` and
`make verify-release`, then the full suite and `make parity` against
`target/$(TARGET)/dist/tm`. The release workflow does not run it, so run it by hand before a release.
It takes about 90 seconds on x86_64 Linux.

Input timing follows the compiled profile. With `escape-time=0`, a partial
terminal sequence gets a 1 ms deadline before it is forwarded and a key that
is unbound in the prefix table is dropped, as tmux drops it, while
repeat mode uses tmux's 500 ms `repeat-time` window and refreshes on each
repeat. Paste payloads and repeat deadlines are covered by their focused
suites; tests wait for pane bytes or terminal state instead of assuming a
settle delay.
