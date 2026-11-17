# Pane fixture completion evidence

Base: `8fffdf00b5319dfd95573dd1a83e411359df99b2`. On Linux ARM CI run
44132882993, `pipe-pane -t S 'cat >> OUT'` compared `m1\r\nm1\r\n` with
`m1`: the old file wait returned on the first marker. On native macOS, the
fresh base suite compared an extra `S:1.1 ''` pane against a topology in which
it had exited. The empty command runs as a shell command and can exit after the
respawn command returns. These observations are consistent with incomplete
fixture observations; neither log alone proves the exact scheduling.

The pipe wait now observes both complete terminal echo and `cat` lines before
returning the entire file. The empty-command respawn waits for the original
pane ID to disappear before comparing the unchanged complete `list-panes`
and topology results. Other commands keep their original ordering.

The controlled fragmentation witness presents `m1` on the first file read,
then completes the file; it checks the exact bytes. The controlled exit witness
keeps tm's respawned pane in `read`, releases the reference pane first, then
releases tm after observing that its pane is still present. It compares command
outcomes, complete `list-panes`, and complete topology. It models delayed exit;
it does not recreate the CI or native host's exact schedule.

On Darwin arm64 with tmux 3.8, set this prefix from the repository root for
every command below:

```sh
env -u TMUX -u TMUX_PANE -u TM_SOCKET -u TM_CONFIG \
  TMPDIR="$PWD/.work/tmp" TMUX_TMPDIR="$PWD/.work/tmp" \
  CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 RUSTC_WRAPPER=kache cpu-guard --cores 1
```

Append `cargo test --test tmux_cli_flags panes::pipe_file_waits_for_fragmented_output -- --exact`
or `cargo test --test tmux_cli_flags panes::pane_exit_barrier_handles_delayed_exit -- --exact`:
both passed. With the shared mutation lock held and no concurrent build, replace
`text.ends_with(&expected)` with `text.contains(marker)` and run the first
command: it exits 101 with `[109, 49]` instead of the eight complete bytes.
Restore it, insert `return;` at the start of `wait_pane_exit`, and run the
second: it exits 101 on the full `list-panes` comparison, with the held tm pane
still present and the reference pane gone. Restore the helper before further
checks. These are two targeted fixture faults, not a broad mutation campaign;
no unrun mutants or survivors have been classified.

With both faults restored, the prefix plus `cargo test --test tmux_cli_flags
panes::panes_pipe_pane -- --test-threads=1` passed four tests; the prefix plus
`cargo test --test tmux_cli_flags panes::panes_respawn_pane_flags_match_the_reference
-- --exact` passed one. The prefix plus `make test` passed, including 149 CLI
tests, and the prefix plus `/usr/bin/time -l make lint` passed. The latter
reported 180830208 bytes maximum RSS for that invocation; it is not a qualified
cost comparison. The debug binary was 5724112 bytes. Runtime code and
dependencies were unchanged; no runtime speed or memory change is claimed.
Cold, warm, and lint costs remain unqualified, and Linux ARM was not rerun here.
After testing, `pgrep -af "$PWD/(target|\.work)"` produced no output.
