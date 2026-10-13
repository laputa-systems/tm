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
