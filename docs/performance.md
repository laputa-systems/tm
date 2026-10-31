# Rendering performance

Run the optimized benchmark suite with `make perf`, or use
`cargo bench --bench tm_performance`. It covers 80×24 with one and two panes,
120×40 with four panes, and incremental attached rendering at 120×40 with four
panes. The fixtures use deterministic terminal state and do not start shells.

Every measured render must finish within 16 ms and serialize no more than
32 KiB. The benchmark uses 20 samples of 10 iterations each. To collect JSON
output, run:

```sh
cargo bench --bench tm_performance -- --format json
```

The attached-client PTY test also checks that output stops growing when pane
state is unchanged and that its pane scenario emits one full-screen clear.
Attached rendering suppresses byte-identical frames and waits for a render
revision, so stable serialization and idle wakeups are part of the contract.

Measured 2026-10-30 on the aarch64 macOS host with the pinned toolchain,
interleaved against the campaign checkpoint `af1d5d7`:

| Fixture | Candidate median | Checkpoint median |
| --- | --- | --- |
| 80×24, 1 pane | 41.7 µs | 41.8 µs |
| 80×24, 2 panes | 50.8 µs | 50.9 µs |
| 120×40, 4 panes | 124.0 µs | 123.2 µs |
| 120×40 delta, 4 panes | 392.2 µs | 389.3 µs |

All medians stayed inside the observed spread. `tests/tmux_liveness.rs`
covers blocked helpers, nonreading pipes, nonreading panes, ordered input,
and a stalled attached client; `tests/tmux_apps.rs` covers resize, detach,
reattach, and idle silence.
