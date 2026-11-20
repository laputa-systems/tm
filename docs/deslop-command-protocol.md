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
