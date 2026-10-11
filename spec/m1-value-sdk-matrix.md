# S3a typed value SDK and evidence

Status: TESTED locally; full twenty-two-check Nix CI pending
Base: S2 PR #776; frozen SDK contract precedes implementation

| File or task | State | Semantic evidence |
| --- | --- | --- |
| `phenix-contract/src/static_schema.rs`, public exports | TESTED | Constant canonical encodings and SHA-256 match fifteen independent schemas, including nested refs and multiple hash blocks |
| `phenix-sdk/src/live.rs`, public exports | TESTED | Immutable values without Clone bound, typed locations, snapshots, explicit vector drafts, CAS conflicts, heterogeneous batch, epoch snapshot and subscription cleanup |
| `phenix-macros/src/lib.rs` LivePortable derive | TESTED | Explicit M1 trait for named structs and unit variants; M0 derive unchanged |
| `phenix-sdk/tests/live-values.rs` and compile-fail docs | TESTED | Immutable non-Clone payload; exact structural conversion; mutable ref/value/cell use, borrowed/pointer fields and cross-thread host/watch reject |
| `examples/value-provider`, `examples/value-client` | TESTED | Independently built public SDK-only cell owner and typed caller; no private kernel link |
| `phenix-conformance/src/bin/phenix-m1-sdk.rs` | TESTED | Fourteen native behaviors, including stale draft, abandoned draft zero effects, coalescing, selected ref grants no authority and denied zero effects |
| `phenix-contract/src/bin/phenix-static-schema-probe.rs`, `tests/m1-value-sdk-vectors.py` | TESTED | Fifteen constant byte/hash vectors compared to independent producer and accepted S1a vectors |
| `tests/m1-value-sdk-product.py` | IMPLEMENTED | Genuine Nix packaged CLI; no fabricated store paths; pending CI |
| `flake.nix`, workflow | IMPLEMENTED | Two SDK gates plus twenty predecessor gates, independently packaged native fixtures and exact-commit evidence |
| Local evidence | TESTED | Debug and ThinLTO release reports and source hashes |
| CI acceptance | NOT YET PROVEN | Exact commit, downloaded archive and all twenty-two checks required |

S3a adds no kernel mechanism. S3b owns `#[contract]`, define/require/provide,
generated exposed-field storage/initialization and native ABI glue. The current
SDK fixtures deliberately retain handwritten ABI glue until that independent
generated-authoring gate. Graph hooks remain S4/S5. Basic group parity is pending.

CI 38170416751 passed the SDK native and independent schema gates, then found
an incorrect product-test lookup of `after_stop`. The CLI has always returned
its post-stop inspection at the top level. The corrected test reads that
existing shape; no runtime change is needed. Full acceptance remains pending.
