# S3b generated authoring matrix

Status: partial implementation; full CI pending
Frozen contract: m1-contract-authoring-contract.md, preceding implementation

| File or task | State | Evidence or remaining acceptance |
| --- | --- | --- |
| phenix-contract/src/descriptor/static_metadata.rs | IMPLEMENTED | Static schemas project into accepted owned metadata; definition-only bytes evaluate at compile time |
| phenix-macros/src/contract.rs | IMPLEMENTED | Trait declaration lowers to API module, typed operation markers and sorted complete contract |
| phenix-sdk/src/live.rs, src/lib.rs | IMPLEMENTED | Typed M1 operation declarations and macro export; eleven compile-fail cases |
| phenix-sdk/tests/contracts.rs | IMPLEMENTED | Independent expected owned metadata, reversed method order, nested ref schemas, raw Rust identifiers and full definition-package byte comparison |
| Definition-only plugin lowering | IMPLEMENTED | define annotations emit canonical const METADATA bytes, sort contract identities and reject duplicates; no State or native table |
| Constant native plugin metadata lowering | PENDING | require/provide and property declarations, canonical section bytes |
| Plugin macro state profile | PENDING | Generated ABI 3 init/invoke/poll/cancel/stop; instance poisoning and cleanup |
| Exposed State fields and API property symbols | PENDING | Field-only provider, Cell rewrite and checked publication |
| ABI 3 pending import SDK and host envelopes | PENDING | Existing ticket manager, child authentication, domain recovery and cancellation |
| External definition/provider/decorator/caller fixtures | PENDING | SDK-only builds and independent canonical metadata/behavior gate |
| Full S3b acceptance | NOT PROVEN | Exact-commit CI, downloaded artifact and all predecessor gates |

Original Basic parity follows this stage as A1a. Reactive A1b remains after S5.
