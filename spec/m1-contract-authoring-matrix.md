# S3b generated authoring matrix

Status: partial implementation; native authoring slice tested locally, current-head full CI required
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
| `phenix-macros/src/state.rs` native lowering | TESTED | SDK define/require/provide and field annotations emit canonical metadata and the 128 KiB section; named One/Optional/Many typed slot helpers; sealed contract membership; const handler coverage/schema validation |
| Plugin macro state profile | TESTED | Generated ABI 3 create/initialize/invoke/poll/cancel/stop; initialization binds fields once before start; v2 panic boundaries; poisoned invocation rejects before work, poll/cancel remain available; pending handlers require a native ticket |
| `phenix-macros/src/property.rs`, exposed State fields | TESTED | Authored portable fields rewrite to Cell<T>; stable IDs and declared get/set/watch produce typed property modules; API-only aliases match independently authored synthetic schemas; plain fields remain local |
| `phenix-sdk/tests/native_authoring.rs`, SDK native compile failures | TESTED | Real generated ABI callbacks, single-use binding, typed imports, domain/pending/cancel/poll, panic/drain/stop; eight new compile-fail cases include undeclared/wrong-contract imports, missing/duplicate exports, invalid fields and mismatched handlers |
| `phenix-artifact/src/lib.rs`, `phenix-kernel/src/resolve.rs`, `runtime.rs` | TESTED | Native definitions contribute only a maximum delegation ceiling; explicit grants, selections and resources still authorize effects; ABI 3 quiescent failures propagate as typed failures |
| ABI 3 pending import SDK and host envelopes | PENDING | Existing ticket manager, child authentication, domain recovery and cancellation |
| `examples/authoring-api/`, `authoring-provider/`, `authoring-client/`, `authoring-field/` | TESTED | SDK-only annotated API package, mixed synchronous/pending provider, typed intermediary and field-only provider; no build scripts or handwritten native descriptors/table glue |
| `phenix-conformance/src/bin/phenix-m1-authoring.rs`, `flake.nix`, workflow evidence | TESTED locally | Existing kernel loads independent native packages; named slots, Optional/Many, synthetic property read, pending/cancel, ceiling and resource/import/operation denial with zero committed effects; new m1-native-authoring full-CI gate and artifact |
| External pending caller and complete import security fixtures | PENDING | ABI 3 parent/child calls, child ticket slot authentication, scoped requests, domain recovery, cancellation and abandonment |
| Full S3b acceptance | NOT PROVEN | Exact-commit CI, downloaded artifact and all predecessor gates |

The triggered Basic roadmap in `m1-roadmap.md` now owns follow-on work. G1 follows complete S3b acceptance. Authenticated pending imports, native resource context, property control during a physical pending call, and the complete external security suite still block acceptance. The metadata implementation commit `fef00f549ec3fc944e9520307fa13f6509fc34d8` passed full CI 38178581812. The native authoring slice requires its own exact-head full-CI evidence.
