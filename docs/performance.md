# Rendering performance

`tm` has two performance contracts: the in-process renderer must be fast enough
for a 60 Hz terminal, and the attached PTY must not emit redraws when the pane
state is unchanged.

Run the Rustybench suite in an optimized build with:

```sh
cargo bench --bench tm_performance
```

The benchmark fixture uses empty panes with deterministic `vt100` state, so its
measurements cover frame construction rather than shell startup or scheduling.
Each sample has two enforced budgets:

- frame construction: at most 16 ms;
- serialized frame: at most 32 KiB.

The suite currently covers 80×24 with one and two panes, and 120×40 with four
panes. Use `--format json` when collecting machine-readable results for a
performance report:

```sh
cargo bench --bench tm_performance -- --format json
```

The checked-in attached PTY capture enforces the wire-level idle contract: after
the initial frame settles, `PtyTest::raw_output` must not grow while pane state
is unchanged, and the scenario permits exactly one `ESC[2J` full-screen clear.
For a quantitative terminal comparison, capture `PtyTest::raw_output` under a
fixed pane-output workload and report `ESC[?25l` frame starts, `ESC[2J` full
clears, total bytes, and elapsed time. Run that same workload through tmux with
your `~/.config/tmux/tmux.conf`; keep this as an external comparison so tmux's
configuration and process scheduling are not conflated with the renderer
microbenchmark.

The benchmark attributes set 20 samples of 10 iterations each. The assertions
run inside every measured iteration, including the 16 ms and 32 KiB budgets.
For a repeatable check, run `make perf` without `RUSTYBENCH_*` sample overrides;
Rustybench intentionally allows those options to be changed for exploration.

An unchanged frame must serialize identically. This is important because the
attach loop suppresses duplicate frames byte-for-byte; nondeterministic escape
ordering turns an idle session into a continuous full-screen redraw stream.
The attach loop also waits on a render revision condition variable, so an idle
client does not repeatedly rebuild a frame just to discover that no bytes
changed.
