## Change

State the concrete behavior and owning milestone. Link the acceptance matrix and exact-head evidence.

## Complexity review

| Question | Evidence |
| --- | --- |
| Which named invariant requires this? | |
| Which existing crates or `std` APIs cover its generic protocol, I/O, parser, transport or persistence work? | |
| Which crate/version/features were selected, or what concrete incompatibility justifies custom code? | |
| What do the crate's license, dependency/build cost, maintenance and cancellation/size semantics imply? | |
| Which duplicate protocol/parser implementation is removed, or which follow-up PR must remove it before milestone acceptance? | |
| Can an ordinary plugin implement it? | |
| Does provider lookup use the canonical resolver? | |
| Does execution retain one root owner and authority boundary? | |
| Do annotations remain the only metadata source? | |
| What is the state's lifetime and teardown rule? | |
| What simpler milestone contract was considered? | |
| Which superseded code is deleted? | |
| Which external public-SDK fixture exercises it? | |

## Validation

Record tested, implemented and unproven requirements separately. Include the kernel commit, artifact identities, Nix outputs and CI run. Compilation alone does not accept a milestone.
