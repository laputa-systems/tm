# macOS repeat binding investigation

DSLP-6 remains open. The cause of the original failure is unknown.

The [first tm#32 CI attempt](https://github.com/laputa-systems/tm/actions/runs/43697795774/attempts/1) timed out waiting for the reference to react to `repeated C-Left`. The unchanged [second attempt](https://github.com/laputa-systems/tm/actions/runs/43697795774/attempts/2) passed that test. The first log does not record the interval between keys. The 500 ms repeat window is a hypothesis, not a demonstrated cause.

At base `54ec9ef9c4f009d06c9aeb9f4c7c2269e3524f4b`, `Pair::press` reads reference state, writes each side, waits for reference state to change, and compares state. The navigation test calls it separately for prefix C-Left and repeated C-Left. See `tests/support/attached_pair.rs:118` and `tests/tmux_bindings.rs:52`.

On the assigned macOS worktree, the following command exited 101 twice before reaching the navigation keys:

```sh
env -u TMUX -u TMUX_PANE -u TM_SOCKET -u TM_CONFIG \
  TMPDIR="$PWD/.work/tmp" TMUX_TMPDIR="$PWD/.work/tmp" \
  CARGO_BUILD_JOBS=2 RUSTC_WRAPPER=kache cpu-guard --cores 4 -- \
  cargo test --features parity --test tmux_bindings \
  window_navigation_bindings_repeat_like_the_reference -- --exact --nocapture
```

Both runs reported `error connecting to .../reference/server.sock (File name too long)` during reference `new-session`. `Scratch::new` constructs the socket root from `CARGO_MANIFEST_DIR` in `tests/support/parity.rs:21`, and `Mux::reference` appends `reference/server.sock` at line 101. Shortening the bindings suite's private fixture labels resolved that setup failure without changing the scenarios.

After that correction, the focused command passed. Twelve further native runs, instrumented to show the C-Left writes, all passed. A diagnostic fault inserted a 650 ms delay after `prefix C-Left`. The focused command then exited 101 with the same `timed out waiting for the reference to react to repeated C-Left` message as CI. The diagnostic delay and instrumentation were removed. Expiry can produce the observed failure, but the original log does not establish that expiry occurred in CI. No flake fix was attempted.

The bindings suite passed all 10 tests with `cargo test --features parity --test tmux_bindings -- --test-threads=4` under the same isolation prefix, build settings, and CPU guard. The other suites, full `make test`, and lint were not run after the investigation stopped with the cause unknown.

The remaining gates are a native reproduction or deterministic fault tied to the original failure, a cause-based fix preserving each observable assertion and reference behavior, the affected PTY and helper suites, native `make test`, lint, and a private-daemon cleanup check. No performance conclusion was drawn.
