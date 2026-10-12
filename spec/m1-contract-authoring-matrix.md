# S3b generated authoring matrix

Status: S3b accepted at `75a255b9289ec43db32bc7f592816fc78d5884af`, CI 38205255204
Frozen contract: m1-contract-authoring-contract.md, preceding implementation

| File or task | State | Evidence or remaining acceptance |
| --- | --- | --- |
| phenix-contract/src/descriptor/static_metadata.rs | PROVEN in CI 38205255204 | Static schemas project into accepted owned metadata; definition-only bytes evaluate at compile time |
| phenix-macros/src/contract.rs | PROVEN in CI 38205255204 | Trait declaration lowers to API module, typed operation markers and sorted complete contract |
| phenix-sdk/src/live.rs, src/lib.rs | PROVEN in CI 38205255204 | Typed M1 operation declarations and macro export; eleven compile-fail cases |
| phenix-sdk/tests/contracts.rs | PROVEN in CI 38205255204 | Independent expected owned metadata, reversed method order, nested ref schemas, raw Rust identifiers and full definition-package byte comparison |
| Definition-only plugin lowering | PROVEN in CI 38205255204 | define annotations emit canonical const METADATA bytes, sort contract identities and reject duplicates; no State or native table |
| `phenix-contract/src/descriptor/static_metadata.rs` native encoder | PROVEN in CI 38205255204 | Const definitions, One/Optional/Many import slots, export slots and properties; canonical bytes match independently authored owned metadata; sorted contributions and duplicate/identity/revision/cardinality/size rejection |
| `phenix-contract/tests/static_native.rs` | PROVEN in CI 38205255204 | Four tests cover independent full-byte metadata, contribution-order invariance, invalid contributions and bounded encoding |
| `phenix-artifact/src/lib.rs`, `tests/m1-metadata-negative.py` | PROVEN in CI 38205255204 | Exact 16/128 KiB M1 sections, retired-format rejection, length/padding/canonical checks; real ELF files have trapping constructors to prove offline inspection |
| `phenix-macros/src/state.rs` native lowering | PROVEN in CI 38205255204 | SDK define/require/provide and field annotations emit canonical metadata and the 128 KiB section; named One/Optional/Many typed slot helpers; sealed contract membership; const handler coverage/schema validation |
| Plugin macro state profile | PROVEN in CI 38205255204 | Generated ABI 3 create/initialize/invoke/poll/cancel/stop; initialization binds fields once before start; v2 panic boundaries; poisoned invocation rejects before work, poll/cancel remain available; pending handlers require a native ticket |
| `phenix-macros/src/property.rs`, exposed State fields | PROVEN in CI 38205255204 | Authored portable fields rewrite to Cell<T>; stable IDs and declared get/set/watch produce typed property modules; API-only aliases match independently authored synthetic schemas; plain fields remain local |
| `phenix-sdk/tests/native_authoring.rs`, SDK native compile failures | PROVEN in CI 38205255204 | Real generated ABI callbacks, single-use binding, typed imports, domain/pending/cancel/poll, panic/drain/stop; eight new compile-fail cases include undeclared/wrong-contract imports, missing/duplicate exports, invalid fields and mismatched handlers |
| `phenix-artifact/src/lib.rs`, `phenix-kernel/src/resolve.rs`, `runtime.rs` | PROVEN in CI 38205255204 | Native definitions contribute only a maximum delegation ceiling; explicit grants, selections and resources still authorize effects; ABI 3 quiescent failures propagate as typed failures |
| ABI 3 pending import SDK and host envelopes | PROVEN in #780, CI 38197645494 | Existing ticket manager, named slots and parent/root/provider/operation authentication; native imported-child completion, scoped denial and recursive cancellation pass locally |
| `examples/authoring-api/`, `authoring-provider/`, `authoring-client/`, `authoring-field/` | PROVEN in CI 38205255204 | SDK-only annotated API package, mixed synchronous/pending provider, typed intermediary and field-only provider; no build scripts or handwritten native descriptors/table glue |
| `phenix-conformance/src/bin/phenix-m1-authoring.rs`, `flake.nix`, workflow evidence | PROVEN in CI 38205255204 | Existing kernel loads independent native packages; named slots, Optional/Many, synthetic property read, pending/cancel, ceiling and resource/import/operation denial with zero committed effects; new m1-native-authoring full-CI gate and artifact |
| External pending caller and complete import security fixtures | PROVEN in CI 38205255204 | M1 parent/child, immediate child, frozen attenuation, domain errors, wrong-parent handle reuse, cancellation, wake propagation and abandonment pass. Completion branch adds raw-ABI slot, Many index, operation, selected-provider, parent-ticket, unknown-ticket and cancel forgeries; live-child state and valid completion prove zero effects |
| Stateless handlers and prepared cell execution | TESTED locally in consolidation branch | Generated empty state, concise typed arguments and ordinary results; selected get/set/watch during a real native worker; denied cell grants/resources/root scopes produce no writes or subscriptions; see `m1-consolidation-audit.md` |

## Completion after consolidation

The [acceptance addendum](m1-authoring-acceptance-contract.md) owns current
property-discovery and pending-slot semantics. It supersedes earlier proposed
pending list envelopes. PR #780 passed all contemporary M1 gates at
`367420e33bbaaa55b6d680394b9cc99a3c6f43e6` in CI 38197645494.

| Changed files | State | Executable evidence or remaining gate |
| --- | --- | --- |
| `phenix-kernel/src/state_effects.rs`, `value_state.rs` | PROVEN in CI 38205255204 | Exact selected property target, shared state authority, typed location lookup; no cell or watch allocation |
| `phenix-macros/src/property.rs`, `phenix-sdk/src/live.rs` | PROVEN in CI 38205255204 | Generated get/set/watch locate helpers; runtime schema identity check; native targets and absent selections reject |
| `examples/authoring-api`, `authoring-client`, `phenix-m1-authoring.rs` | PROVEN in CI 38205255204 | Independent field-only read, pending-owner discovery, operation/resource/root denials, schema mismatch, zero writes/subscriptions |
| `phenix-macros/src/state.rs`, `examples/pending-consumer` | PROVEN in CI 38205255204 | Sealed begin/begin_scoped and indexed Many helpers reuse Host/PendingImport; real pending tree and attenuation fixture |
| `phenix-sdk/src/live.rs` compile-fail cases | PROVEN in CI 38205255204 | Wrong-contract pending and scoped indexed imports fail; existing authoring/ref restrictions remain |
| `examples/pending-security`, `phenix-m1-imports.rs` | PROVEN in CI 38205255204 | Raw native ABI forgeries reject before poll/cancel/settlement; live descriptor, unchanged cancellation/delivery state, valid completion and release; read-only resource survives polling and scoped resource switches reject |
| `examples/Cargo.toml`, `Cargo.lock`, `flake.nix`, workflow | PROVEN in CI 38205255204 | Security fixture builds with public SDK only and runs inside existing imports gate; complete M1 release retains every behavioral gate |
| `tests/shipping-product.py` | PROVEN in CI 38205255204 | Named adversarial producer requires recorded forged-handle evidence; ordinary authoring still rejects handwritten native glue |
| Full S3b acceptance | PROVEN at 75a255b9 | Full Nix CI 38205255204 and downloaded artifact 11727835414 match the source commit and GitHub archive digest. All 22 shipping/public-SDK gates and 56 JSON report hashes verified. |

The adversarial fixture writes its native table by hand to bypass SDK handle
protections. Ordinary provider/client fixtures remain declarative. It imports
only the public SDK and example contracts, with no private kernel access.

The [Basic roadmap](m1-roadmap.md) owns G1-G4. Lifecycle and reactive graphs
share ordering algorithms with distinct semantics. A0 stays an ordinary
synchronous plugin fixture; the final Basic agent uses triggered nodes.

## Accepted implementation evidence

[CI 38205255204](https://github.com/matthis-k/phenix-ai/actions/runs/38205255204)
passed on `75a255b9289ec43db32bc7f592816fc78d5884af`.
[Saved evidence](evidence/m1-authoring-completion-ci.json) verifies artifact
11727835414, its ZIP digest, the matching checkout, 56 report hashes, realized
Nix gates and the production closure. Native evidence contains 20 authoring cases,
10 property-discovery cases and 27 imported-child cases. Rust tests and 36 compile
failures passed. Every predecessor and contemporary behavioral gate remains.

The first run failed the shipping scan's ordinary-authoring rule because the
adversarial producer intentionally has a handwritten table. The accepted check
names that fixture, requires its forged-handle evidence and still rejects
handwritten native glue in ordinary authoring examples. Runtime and fixture
behavior did not change in that correction.

This acceptance covers S3b. G1-G4 graph declarations, preparation, execution and
control pumping remain subsequent work. Evidence-only commits retain the accepted
implementation bytes and require their own current-head CI status.
