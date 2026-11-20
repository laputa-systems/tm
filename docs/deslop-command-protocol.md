# Command and protocol audit

## Types

Scope: `src/command.rs` and `src/protocol.rs`, 4,119 lines. The Rust
escape-hatch search from deslop's tools reference found 38 matching lines,
all in tests (`expect`); production has none of those matches. No type or
runtime behavior changed, so the count remains 38.

A `cargo clippy --bin tm` run with all nine deslop Rust candidate warnings
finished successfully. Counts below are diagnostics in the two scoped files,
excluding tests because this was the binary target:

| Candidate | Count | Disposition |
| --- | ---: | --- |
| `unsafe_code` | 0 | Reject: no defect witness or deleted escape hatch. |
| `wildcard_enum_match_arm` | 0 | Reject: no witness or deletion. |
| `match_wildcard_for_single_variants` | 0 | Reject: no witness or deletion. |
| `cast_possible_truncation` | 3 | Reject: no demonstrated defect or deletion; frame and argument counts are bounded, and an oversized byte payload is rejected before writing. |
| `cast_sign_loss` | 1 | Reject: the delay parser already excludes negative and non-finite inputs. |
| `unwrap_used` | 0 | Reject: no witness or deletion. |
| `expect_used` | 0 | Reject: no witness or deletion. |
| `redundant_clone` | 0 | Reject: no witness or deletion. |
| `needless_pass_by_value` | 0 | Reject: no witness or deletion. |

No candidate is adopted. The three truncation diagnostics and the sign-loss
diagnostic are false positives for the existing input and frame guards; they
exceed the owner's limit of one per 5,000 lines in this 4,119-line scope.
The remaining candidates have no positive witness. Existing protocol enums,
optional fields and parser results were kept: their callers and wire format
require those states. No public contract changed.

## Tests

The scoped inventory has 9 parser unit tests, 15 protocol unit tests after
this audit, 149 CLI flag tests, and 5 protocol-version tests (one ignored
child fixture). The CLI suite took 140.33 seconds in the baseline full run
and 158.92 seconds in the separately invoked focused run on this busy host;
these are observations, not a controlled performance comparison. The full
baseline ran 503 tests and ignored one across 33 binaries.

| Contract | Guard | Gap and disposition |
| --- | --- | --- |
| Accepted, rejected and tm-only CLI flags, status and output | `tests/tmux_cli_flags.rs` and the `docs/parity.md` CLI flag rows; `tests/parity_contract.rs` checks command-row coverage | Retain the matrix and its differential and internal cases; no demonstrated missing flag case. |
| Parser command arguments and selected flags | `src/command.rs` tests and the CLI cases | Retain; no demonstrated gap in the audited cases. |
| Request and message wire bytes and tags | `wire_format_matches_goldens_for_the_current_version`, `wire_format_matches_goldens_for_messages`, and random round trips in `src/protocol.rs` | Retain goldens and round trips. Unknown request tags are asserted; invalid client and server tags have no direct named assertion. Their wider malformed-input coverage needs environment. |
| Version preamble and mismatched daemon handling | `version_preamble_is_never_a_valid_frame_length_for_an_older_daemon` and `tests/protocol_version.rs` | Retain, including the old-daemon kill path. |
| Argument and frame limits | `command_arguments_at_the_limit_round_trip_and_beyond_it_are_refused`, `oversized_frames_are_refused_by_encoder_and_decoder`, and `over_long_argument_lists_are_refused_before_any_byte_is_sent` | Retain boundary and no-send checks. |
| Trailing frame data | `messages_reject_trailing_frame_data` | Added: disabling `Decoder::finish` passed all 14 prior protocol unit tests but accepted an extra byte. The new test rejects it for client and server messages. |
| Other malformed values, including invalid booleans, UTF-8 and truncated fields | Decoder checks in `src/protocol.rs`; no direct named assertions found | Gap: systematic coverage and mutation are `Needs environment`; no claim that the existing cases exercise each rejection. |

Hand-injected controls, each restored byte for byte: accepting trailing frame
data made the new test fail (exit 101); changing the first request tag made
the request golden fail (101); writing the version in big endian made the
preamble test fail (101); lowering the argument limit to 1023 made the
protocol-version boundary test reject its valid 1024-argument case (101);
and ignoring `new-session -d` made the CLI differential test report status
1 versus the reference's status 0 (101).

`cargo-llvm-cov` and `cargo-mutants` are unavailable. Coverage, systematic
mutation, their before/after counts, and survivor dispositions are
**Needs environment** under the owner's ruling; no tools or CI lane were
installed. The test-pyramid tool is unavailable; the unit and process-level
tiers and the observed CLI cost are recorded above. No test was removed.
