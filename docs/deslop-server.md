# Server audit

Scope: `src/server.rs` through `window_index_for_creation` (10,068 lines),
then `window_index` through the end (10,474 lines), at function boundaries;
`src/model.rs` (1,206 lines) belongs to both slices. Request dispatch
`execute_request_inner` belongs to the second slice. The binary registers both
modules in `src/main.rs`; the performance benchmark also imports them.

## Lifecycle slice: types (DSLP-11)

`cargo clippy --bin tm` with all nine deslop candidate warnings completed
successfully on macOS arm64. Counts below are diagnostic instances in this
slice plus `model.rs`, excluding test-only code. The latter is counted again
for the second slice.

| Candidate | Count | Disposition |
| --- | ---: | --- |
| `unsafe_code` | 19 | Reject: libc calls for sockets, process and signal handling; no safe replacement witness established. |
| `wildcard_enum_match_arm` | 9 | Reject: no defect witness; forwarding remaining requests is intentional at `server.rs:698`. |
| `match_wildcard_for_single_variants` | 0 | Reject: no witness. |
| `cast_possible_truncation` | 8 | Reject: no demonstrated defect or deleted trust; for example `model.rs:209` clamps to `u16` first. |
| `cast_sign_loss` | 4 | Reject: no demonstrated defect or deleted trust; the same clamp demonstrates a false positive. |
| `unwrap_used` | 23 | Reject: no demonstrated defect or deletion; dimensions are clamped before `NonZeroU16` construction at `server.rs:940`. |
| `expect_used` | 20 | Reject: no demonstrated defect or deletion; includes benchmark setup and compiled bindings. |
| `redundant_clone` | 4 | Reject: no typing witness or deleted escape hatch. |
| `needless_pass_by_value` | 33 | Reject: no typing witness or deleted escape hatch. |

No candidate was adopted. The escape-hatch search specified in the deslop
tools reference found 49 matching lines in the first server slice and 9 in
model, including tests and textual `unsafe` matches. Neither code nor the
count changed. The scan is evidence of candidates, not proof that each cast
or assertion is safe. No public contract changed.
