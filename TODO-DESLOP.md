Prefix: DSLP
Next: 7

Deslop campaign backlog ([plan](https://github.com/laputa-systems/stuff/blob/main/docs/campaigns/deslop.md), tier C). One agent works this file with the `deslop` skill: take the lowest-numbered open ticket whose area does not start with `Needs owner ·`. After DSLP-5, work the open P1 and P2 findings, then fill in `## Report` below.

## Report

Not started.

## Tickets

### DSLP-1 · P2 · Ground: baseline, boundaries, and CI test entry point

- **Area:** deslop · ground
- **Where:** AGENTS.md, Makefile, .github/workflows/
- **Problem:** Base d508573; code lines Rust 51.6k (fleet/loc/v1, vendored included); timings not yet measured. Open fleet lint findings: linux-musl-toolchain 2, binary-release-static 1, laputa-rustix 1, make-clean-target 1, mdtool 1, owned-tool-support 1. Over 50k lines, so Types, Tests, and Dead code run per component; largest first-party groups: src/(top-level files) (32k), tests/(top-level files) (19k), tests/support (2k). Perf gate: decide in Ground whether a user-visible hot path needs a workload.
- **Recommendation:** Run the deslop Ground step. Accept: CI runs make test and is green, and AGENTS.md has a Verification section.
- **Fix with:** deslop
- **Effort:** M

### DSLP-2 · P2 · Diagnose: ultrareview into this backlog

- **Area:** deslop · diagnose
- **Where:** whole repository
- **Problem:** No review has ranked this repository's risks for the deslop pass.
- **Recommendation:** Run the deslop Diagnose step. Accept: the diff touches only docs, comments, and this file, and every finding names its Fix with.
- **Fix with:** deslop
- **Effort:** M

### DSLP-3 · P2 · Types: strict profile and typing findings

- **Area:** deslop · types
- **Where:** type checker and lint configuration; Rust sources
- **Problem:** The strict-profile candidates in the deslop skill's tools reference have not been measured here.
- **Recommendation:** Run the deslop Types step. Accept: clean at the adopted profile, escape hatches not increased, witnesses recorded.
- **Fix with:** deslop
- **Effort:** M

### DSLP-4 · P2 · Tests: coverage, boundary gaps, mutation

- **Area:** deslop · tests
- **Where:** test suites and CI
- **Problem:** Coverage and mutation results are unmeasured; no deslop mutation lane exists.
- **Recommendation:** Run the deslop Tests step. Accept: no undispositioned survivor in critical code, the test cost budget holds, and the lane is green where the tools are mirrored.
- **Fix with:** deslop
- **Effort:** L

### DSLP-5 · P2 · Dead code: tools and debloat

- **Area:** deslop · dead code
- **Where:** whole repository
- **Problem:** No dead-code scan has run.
- **Recommendation:** Run the deslop Dead code step. Accept: tools clean or allowlisted with reasons; walk-backs filed as Needs owner tickets.
- **Fix with:** deslop
- **Effort:** M


### DSLP-6 · P2 · window_navigation_bindings_repeat_like_the_reference fails intermittently on macOS

- **Area:** Tests
- **Where:** the test window_navigation_bindings_repeat_like_the_reference (CI job CI (xcode-27))
- **Problem:** On the docs-only seeding PR (tm#32) the macOS CI job failed this test (9 passed, 1 failed) and passed on rerun with no code change, while main was green; a flaky test hides real failures and blocks unattended merges.
- **Recommendation:** Root-cause it (run it in a loop on macOS; look for timing, terminal-size, or ordering assumptions), fix the cause rather than retrying, and keep the test as the regression.
- **Fix with:** root-cause
- **Effort:** S

