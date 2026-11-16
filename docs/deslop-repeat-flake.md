# Repeat binding test qualification

The navigation test now checks each prefix/repeat sequence before issuing state
probes. Nine independent cases retain the full session/window/pane comparison
against locked tmux 3.8 and assert literal window order and active-window values.
The shipped runtime, reference profile and 500 ms repeat period are unchanged.

## Demonstrated fixture defect

The [first tm#32 CI attempt](https://github.com/laputa-systems/tm/actions/runs/43697795774/attempts/1)
timed out waiting for the reference to react to `repeated C-Left`; the unchanged
[second attempt](https://github.com/laputa-systems/tm/actions/runs/43697795774/attempts/2)
passed. Its log has no inter-key timestamps, so it cannot uniquely identify the
historical cause.

At base `54ec9ef`, each `Pair::press` performed several CLI state queries
between separate prefix and repeat writes. Those probes could exhaust the
reference's 500 ms repeat window. A controlled 650 ms pause reproduces the
reported timeout. Twelve ordinary native baseline runs passed; they do not
rule out that scheduling-dependent failure.

The new cases send the prefix and repeats in one PTY write, then observe the
result. The same 650 ms delay after every probe fails the old test and passes
the new one. All nine old observations remain covered, including wrapping and
both swap directions, with explicit expected window states.

The expiry case retains its 650 ms wait. Its barrier now swaps the active
window, preserving which window was selected before the barrier. The old
new-window barrier hid an incorrect selection by always selecting the new
window. Changing only tm's expiry to five seconds passed the old test and
fails the new full-state comparison. Dropping a repeated C-Left from both
sides' input fails the new literal-state assertion. All diagnostic delays,
instrumentation and runtime faults were restored.

## Socket-path prerequisite

Descriptive fixture labels made the reference socket pathname exceed macOS's
limit in a normal `.work/wt/` checkout. Scratch paths now use the process ID
and atomic counter, with the label retained in diagnostics. Creation is
exclusive, so a pre-existing path is refused rather than reused. The regression
`descriptive_scenario_names_leave_room_for_private_unix_sockets` binds an actual
Unix listener: it fails with the old helper and passes with the short path.

## Verification

Run from the repository root with the pinned nightly and tmux 3.8 reference:

```sh
env -u TMUX -u TMUX_PANE -u TM_SOCKET -u TM_CONFIG \
  TMPDIR="$PWD/.work/tmp" TMUX_TMPDIR="$PWD/.work/tmp" \
  CARGO_BUILD_JOBS=2 cpu-guard --cores 4 -- make test
```

The final native macOS `make test` and `make lint` pass. The parity-feature
bindings suite and socket regression pass. Post-run process checks find no
private worktree daemon. The reference archive SHA-256 is
`e79c699c7e949dccd0a4a125e17b8d1261e311b16979dbc8eb34542f3966d82e`;
no upstream reference source or profile changed. No dependency, CI, formatter,
release or managed installation changed. Runtime performance is unaffected;
quiet-host test-cost measurements remain unqualified on the constrained host.
This finding does not complete the broader deslop pass.
