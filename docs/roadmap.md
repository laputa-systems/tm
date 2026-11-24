# Roadmap

This is the single plan for this repository: all current and future work, and the handoff for resuming it. Campaigns are sections below, and their items interleave in **Sequence**. "Resume the roadmap" means continue from **Status**.

## Status

- **State:** idle; no active campaign or slice is recorded.
- **Last accepted revision:** parity's final gate passed at `eb58f2d` on 2026-11-06; the current integration branch's full gate is not established. Later test attempts and their environment are documented in [Ground](deslop-ground.md).
- **Parked branches:** none recorded for an active roadmap item.
- **Next slice:** none planned; select and plan work from Later.

## Sequence

No slices are ready; plan an item from Later before execution.

## Later

Details and priorities are in [TODO](../TODO.md).

- Match session and pane format values: TMUX-11, TMUX-12, TMUX-13, TMUX-17.
- Match layout, placement and resize behavior: TMUX-15, TMUX-24, TMUX-27, TMUX-30, TMUX-32, TMUX-34, TMUX-35, TMUX-37, TMUX-44.
- Match pane modes and copy mode: TMUX-18, TMUX-26, TMUX-28, TMUX-49.
- Match format comparisons and aliases: TMUX-20, TMUX-23, TMUX-25, TMUX-47.
- Match key names and modifiers: TMUX-22, TMUX-48.
- Match target parsing and pane targets: TMUX-31, TMUX-45.
- Restore terminal state after a client is killed: TMUX-33.
- Match status window overflow: TMUX-46.
- Preserve last-pane and last-window history: TMUX-14, TMUX-36.
- Match pipe input and time expansion: TMUX-19.
- Match choose-mode filter truth: TMUX-21.
- Split the server and centralize option defaults: TMUX-60, TMUX-61.
- Improve daemon crash reporting and transient error handling: TMUX-62.
- Prevent a stalled pipe helper from freezing a pane: TMUX-63.
- Make pane spawning safe across fork and partial failure: TMUX-64.
- Restrict protocol requests to valid command combinations: TMUX-65.
- Use locked dependency builds: TMUX-66.
- Upgrade vt100 and ptytest after the ptytest fixes are released: TMUX-67.
- Explain the killed-pane targeting failure: TMUX-72.
- Bound unterminated SGR mouse input: TMUX-73.
- Prevent log and lock sidecars from following symlinks: TMUX-74.
- Diagnose oversized control sequence and external-stop test flakes: TMUX-75, TMUX-76.
- Remove benchmark duplication and shipped scaffolding: TMUX-68.
- Resolve the unused detached field: TMUX-69.
- Report the actual build version: TMUX-70.
- Reduce raw output retention required by vt100 gaps: TMUX-71.
