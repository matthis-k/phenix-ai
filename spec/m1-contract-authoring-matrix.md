# S3b generated authoring matrix

Status: partial implementation; native metadata slice tested locally, current-head full CI required
Frozen contract: m1-contract-authoring-contract.md, preceding implementation

| File or task | State | Evidence or remaining acceptance |
| --- | --- | --- |
| phenix-contract/src/descriptor/static_metadata.rs | IMPLEMENTED | Static schemas project into accepted owned metadata; definition-only bytes evaluate at compile time |
| phenix-macros/src/contract.rs | IMPLEMENTED | Trait declaration lowers to API module, typed operation markers and sorted complete contract |
| phenix-sdk/src/live.rs, src/lib.rs | IMPLEMENTED | Typed M1 operation declarations and macro export; eleven compile-fail cases |
| phenix-sdk/tests/contracts.rs | IMPLEMENTED | Independent expected owned metadata, reversed method order, nested ref schemas, raw Rust identifiers and full definition-package byte comparison |
| Definition-only plugin lowering | IMPLEMENTED | define annotations emit canonical const METADATA bytes, sort contract identities and reject duplicates; no State or native table |
| `phenix-contract/src/descriptor/static_metadata.rs` native encoder | TESTED | Const definitions, One/Optional/Many import slots, export slots and properties; canonical bytes match independently authored owned metadata; sorted contributions and duplicate/identity/revision/cardinality/size rejection |
| `phenix-contract/tests/static_native.rs` | TESTED | Four tests cover independent full-byte metadata, contribution-order invariance, invalid contributions and bounded encoding |
| `phenix-artifact/src/lib.rs`, `tests/m1-metadata-negative.py` | TESTED | Exact 16/128 KiB M1 sections, M0 exact 16 KiB rule, length/padding/profile/canonical checks; real ELF files have trapping constructors to prove offline inspection |
| Constant native plugin macro lowering | PENDING | require/provide and field annotations must feed the native encoder through public SDK macros |
| Plugin macro state profile | PENDING | Generated ABI 3 init/invoke/poll/cancel/stop; instance poisoning and cleanup |
| Exposed State fields and API property symbols | PENDING | Field-only provider, Cell rewrite and checked publication |
| ABI 3 pending import SDK and host envelopes | PENDING | Existing ticket manager, child authentication, domain recovery and cancellation |
| External definition/provider/decorator/caller fixtures | PENDING | SDK-only builds and independent canonical metadata/behavior gate |
| Full S3b acceptance | NOT PROVEN | Exact-commit CI, downloaded artifact and all predecessor gates |

The triggered Basic roadmap in `m1-roadmap.md` now owns follow-on work. G1 follows complete S3b acceptance. Native macro lowering, ABI 3 hooks, fields and pending imports still block that acceptance.
