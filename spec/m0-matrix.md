# P0 and M0.0 acceptance matrix

PR [#771](https://github.com/matthis-k/phenix-ai/pull/771) owns this matrix. Branch `rewrite/minimal-kernel` starts at `11cdf0eca7e80965d0dfca3762d1fd65bb19e10f`. The root has no parents. Its merge base with observed main `1dd0ffc8ba882d46c4e6c6d5a8cee328fa8f8063` is that root, 1,215 commits behind. No architecture was merged or cherry-picked. Existing branches and PRs remain references.

Implemented means code exists. Tested means the named semantic check has passed locally. CI evidence is authoritative for the exact PR head; the PR body links that run. M0.0 acceptance requires all four Nix checks on the same head. An earlier green run does not certify a changed head.

| Task | Concrete files | Acceptance criterion | Status | Evidence |
| --- | --- | --- | --- | --- |
| Root-based branch | Git ancestry, PR #771 | Parentless root is ancestor of main | Tested | Local rev-list and GitHub compare agree |
| Contract freeze | spec/m0-contracts.md, include/phenix.h | Concrete wire, contributions, ABI, authority and inspection | Implemented | Executable model is frozen for this milestone |
| P0 extraction decision | contract/src/descriptor/, artifact/src/lib.rs, spec/p0-decision.md | Inspect optimized ELF without executing code | Tested | Nix descriptor check; embedded section is the sole path |
| P0 retention | tests/descriptor.py, flake.nix | GNU/lld, LTO off/thin/fat, three strip modes, clean repeat | Tested | m0-descriptor/evidence.json records final hashes and paths |
| Canonical wire and schemas | contract/src/value.rs, descriptor/mod.rs | Bounded decoding, canonical maps and U64 descriptor scalars; typed request/result/error | Tested | Contract field-tag vectors and rehashed native descriptor rejection without loading |
| Single-source contributions | macros/src/lib.rs, descriptor/encode.rs | Declare/Require/Provide and glue derive from annotations | Tested | Duplicate export compile-fail; structural schema comparison |
| Typed public SDK | sdk/src/lib.rs, macros/src/lib.rs | Typed operation markers and scoped import stubs | Tested | Separate fixture compilation; six compile-fail cases |
| Public distribution boundary | flake.nix, examples/Cargo.toml, examples/Cargo.lock | Plugins consume packaged SDK with no private kernel sources | Tested | Native derivations physically build from the SDK distribution |
| Offline package verification | artifact/src/lib.rs | Final artifact/descriptor hashes and embedded equality before loading | Tested | Tampering, wrong ownership, unsupported ABI and mismatch cases |
| Canonical resolver | kernel/src/resolve.rs | Explicit selection; missing, ambiguous, incompatible and absent choices reject | Tested | Native conformance; artifact-order permutation |
| Native loader | kernel/src/native.rs, tests/abi-negative.c | Header before table; null/short/incompatible tables reject | Tested | Actual faulty native files; final native check owns evidence |
| Root and scoped dispatch | kernel/src/runtime.rs | Same path for root and nested effects; host authenticates caller | Tested | Real consumer -> provider invocation |
| Operation authority | kernel/src/runtime.rs | Deny before effect; immutable selected grants and ceilings | Tested | Denied import leaves provider counter zero |
| Thread and scope ownership | kernel/src/runtime.rs | Forged, expired and wrong-thread scopes reject before pointer access | Tested | Scope boundary tests; Host is not Send |
| Instance reentrancy | examples/cycle/src/lib.rs, kernel/src/runtime.rs | Self-call rejects before another instance entry | Tested | Native cycle fixture; final native check owns evidence |
| Buffer ownership | contract/src/abi.rs, SDK glue | Producing-side release once for success, domain and malformed replies | Tested | Counted-release tests, C/Rust layout assertions, native round trips |
| Startup rollback | native.rs, tests/abi-negative.c, examples/consumer/src/lib.rs | Release acquired replies and instances once; do not admit a root or invoke provider after failed create | Tested | Actual native lifecycle witness; null release and unknown status reject |
| Repeated stop | native.rs | Reject duplicate host stop before callback; drop does not repeat it | Tested | Counted producer stop test in canonical contract gate |
| ABI/trust review | spec/m0-abi-review.md | Review layouts, pointer/lifetime rules, and read-only-source assumptions | Reviewed | Documented boundary audit and matching native fixtures |
| Recoverable panics | SDK glue, examples/provider/src/lib.rs | Catch FFI unwind; reject future calls to poisoned state | Tested | Native panic/poison fixture; process-fatal events remain fatal |
| Trusted CLI / inspection | kernel/src/bin/phenix-kernel.rs, tests/product.py, tests/inspection-golden.json | Parsed JSON, typed result and coherent failure records, including handled errors and root admission | Tested | Native handled-denial, replacement and admission recovery; CLI golden; subsequent-success preservation |
| Pre-load nonexecution | examples/provider/src/lib.rs, conformance/src/main.rs, tests/product.py | Constructor witness stays absent during offline checks | Tested | Witness appears only after actual native loading |
| Nix deployment closure | flake.nix | Separate kernel, SDK, tooling and plugin products; no Nix resolver | Tested | m0-contract, m0-descriptor, m0-native, m0-product |
| Exact-head CI | .github/workflows/minimal-kernel.yml | All canonical checks, commit stamp, paths, digests and CI run | Local passed; CI tracked in PR | Live status and exact run are recorded in the PR body |

## Remaining acceptance work

The full local gate passed at `b7dc0fc3f1a24a5c90f00951c1e34ba0bb517cba`, including loaded-ABI, cycle and panic-poison cases. [Persistent local evidence](evidence/m0-local.json) records exact artifacts and Nix outputs. The initial follow-up evidence commit changes documentation only. The later ABI review closes null-release rejection, failed-create rollback, repeated host stop and failure phase/context inspection. [Follow-up local evidence](evidence/m0-abi-local.json) records realized outputs, final artifact hashes, rollback events and CLI failure fixtures. All four Nix gates passed again on that follow-up working tree; exact commit and run are recorded by CI and linked in the PR body.

The current inspection follow-up replaces separate failure/context fields with one observation record. Its external consumer handles an undeclared import and replaces a provider error; neither behavior needs a new kernel intrinsic. Descriptor validation also rejects signed representations of unsigned metadata fields before JSON conversion. All four Nix checks passed on the final follow-up tree. [Observation follow-up evidence](evidence/m0-observation-local.json) records realized outputs, descriptor/artifact hashes, handled and replaced failures, root admission recovery and CLI inspection. The final exact-head CI run remains tracked in the PR body.

1. Record GitHub CI for the final PR head. Its generated commit stamp and run identify that head without a self-referential document hash.
2. The [ABI/trust review](m0-abi-review.md) now records completed boundary review and executable rollback/ownership evidence. Remaining acceptance is the exact-head gate for the follow-up, not an unimplemented later milestone. Raw pointer validity and process-fatal cleanup remain outside the trusted-native profile.

## Mechanism ownership

| Mechanism | Executable need | Lifetime | Simpler alternative rejected |
| --- | --- | --- | --- |
| Offline ELF reader | Constructor witness must stay absent during inspection | Preparation | dlopen inspection executes constructors |
| Constant descriptor encoder | External declarations need identical canonical bytes | Artifact lifetime | Handwritten manifests duplicate declarations |
| Resolver snapshot | Consumer requires explicit compatible provider | Loaded process | Discovery during nested calls can change selection |
| Scoped dispatcher | Denied imports must leave provider state unchanged | Synchronous call | Direct Rust calls bypass native ABI and policy |
| Scope records | Forged/expired/wrong-thread callbacks must reject safely | Provider entry until return | A raw kernel pointer cannot authenticate before dereference |
| Native image owner | Instance and reply callbacks need resident code | Create through stop and release | Unowned dlopen handles permit early unloading |
| Failure context | A handled or replaced nested failure needs accurate JSON context | Last coherent observation | Separate mutable failure/context fields can pair unrelated observations |

Later work starts with M0.1 contract addenda after M0.0 evidence is accepted. Optional imports, substitutions, richer attenuation, asynchronous lifecycle, generations and product conformance expansion remain deferred. This PR does not implement those milestones.
