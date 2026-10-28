# M1 kernel implementation

Specifications describe **intended architecture**, not necessarily the current implementation. Before changing code, check [the spec index](spec/README.md), the exact source and conformance tests, and the owning PR's merge status. [Graph Architecture v2.4](spec/graph-architecture-v2.4.md) is the latest **proposed graph-v2 design**; graph-v1 M1 code remains current until the P1-stable → graph-v2 P1-parity → P2 migration proves replacement. Old graph RFCs are historical where they disagree with v2.4. Update the latest target specification when design decisions change, and use source plus tests to state what is implemented.

M1 is the sole supported product. Follow [the M1-only transition contract](spec/m1-only-runtime-contract.md), [the consolidation contract](spec/m1-consolidation-contract.md) and [the roadmap](spec/m1-roadmap.md). They supersede historical M0 compatibility requirements. Preserve behavioral guarantees in contemporary M1 conformance tests. Historical commits and acceptance documents retain evidence for retired formats.

Keep one canonical ContractMetadata/ValueSchema model, one declarative Composition, one preparation mechanism, one dispatcher, one authority lineage, one pending-ticket manager and one generation lifecycle. StaticSchema is a compile-time encoding. Native and cell operations share preparation and authority while retaining distinct effect semantics. Lifecycle and future reactive graphs share algorithms, with separate semantic projections. Agent logic belongs in plugins.

For dynamic operations and observation, follow the single checklist in
[the M1 roadmap](spec/m1-roadmap.md). Kernel code owns authorized plugin
management, desired-state reconciliation, structured diagnostic emission and
authenticated internal inspection queries. The independent logging plugin
consumes diagnostic events and owns sinks/retention. The independent introspection
plugin projects authorized kernel queries into normal typed contracts. Neither
plugin depends on the other, and neither holds unrestricted kernel authority.
Basic's durable semantic event journal is separate from diagnostic events.
Persistence, external execution adapters and OS isolation belong to plugins.

Use `nix flake check -L` for canonical acceptance. `nix run .#phenix-m1-demo` exercises the packaged CLI. `nix build .#phenix-m1-release` materializes all contemporary conformance and shipping evidence. The public SDK and external fixtures must build without private kernel sources. Keep implementation PRs ready and unmerged. Record exact-head CI and changed files, semantic tests, evidence and remaining tasks in the owning matrix and PR.

Freeze addenda before implementing new mechanisms. C1/C2 replace legacy representations and consumers together. S3b pending imports and G1-G4 follow consolidation acceptance. Do not add compatibility adapters, registries, workflow interpreters, graph-patch engines or another executor during consolidation. Reuse tickets, leases, dispatch, attenuation and generations.
