# A0 agent preflight task and evidence matrix

Frozen protocol commit: `70eaff8c5654396c309bf4420431a5fff51a28c3`. Base: S1b at `4d3ccb6c7c5279988f95178942c2107381d259f4`. The [contract](a0-agent-contract.md) owns semantics. This gate exercises synchronous M0 services before the reactive A1 stage.

| Files | Acceptance | Status | Evidence or remaining work |
| --- | --- | --- | --- |
| `examples/agent-contracts/Cargo.toml`, `examples/agent-contracts/src/lib.rs` | Typed run/model/tool/error/feedback operations | TESTED | Public SDK builds and portable macro expansion pass |
| `examples/agent/Cargo.toml`, `examples/agent/src/lib.rs` | Model/tool progression, request-local feedback, unique IDs, explicit optional limit | TESTED | Native model/tool/model, twenty tools, error feedback and limit cases pass |
| `examples/agent-model/Cargo.toml`, `examples/agent-model/src/lib.rs` | Model verifies actual preceding feedback and call ID | TESTED | Success and typed tool-error paths pass; invalid input yields InvalidTranscript |
| `examples/agent-alternate/Cargo.toml`, `examples/agent-alternate/src/lib.rs` | Replaceable model implementation | TESTED | Composition rebinding returns alternate answer with zero tool calls |
| `examples/agent-tool/Cargo.toml`, `examples/agent-tool/src/lib.rs` | Typed tool and actual admitted-effect count | TESTED | Exact count deltas; denied tool count remains zero |
| `examples/Cargo.toml`, `examples/Cargo.lock` | Standalone fixture workspace and reproducible dependencies | TESTED | Cargo builds pass; only five local package entries added to lock |
| `crates/phenix-conformance/src/bin/phenix-a0-agent.rs` | Independent kernel-side assertions and multiple root isolation | TESTED | Fourteen recorded cases plus post-failure isolation, exact-limit completion and deterministic preparation |
| `tests/a0-agent-product.py` | Independent canonical bytes and packaged CLI invocation | IMPLEMENTED | Python syntax passes; local CLI correctly rejects scratch packages; real Nix gate pending |
| `flake.nix` | SDK-only plugin packages, separate native/product gates and runnable demo | IMPLEMENTED | Optimized local builds in progress; pinned Nix CI pending |
| `.github/workflows/minimal-kernel.yml` | Full exact-head conformance and uploaded A0 reports | IMPLEMENTED | New gate runs first; all sixteen existing gates remain required |
| `spec/m1-roadmap.md`, `README.md` | Scope and runnable entry point | IMPLEMENTED | A0 is separate from A1 and kernel stages |

## Local evidence

The native gate loads four separately built shared libraries. It invokes the agent through the public kernel API rather than directly calling the loop implementation. Its report includes offline inspection and artifact/descriptor digests. Workspace unit and thirteen compile-fail tests pass on Rust 1.99.0. The local CLI requires actual Nix-store packages and rejects scratch artifacts, so local native success does not claim product acceptance.

The current local compiler is newer than the pinned Nix compiler. Its Clippy reports existing generated `drop_non_drop` and the kernel's deprecated atomic `fetch_update`; scoped local checks allow those existing diagnostics. Pinned full CI retains warnings denied without these allowances.

## Remaining work

Record exact implementation-head full Nix CI and uploaded package/output evidence before PROVEN. A1 still needs S2 live state, S3 authoring, S4 graph compilation and S5 serial reactive readiness. Streaming, asynchronous model/tool work, cancellation, full history, dynamic catalogs and real provider integration require later agent plugin gates.
