# Phenix minimal kernel

This branch replaces the previous implementation with a domain-neutral native plugin kernel. It starts at root commit `11cdf0eca7e80965d0dfca3762d1fd65bb19e10f` and follows [the normative RFC](spec/clean-start-rfc.md). P0 and M0.0 through M0.4 are accepted for trusted native code on x86_64 Linux.

The public contract crate owns values, schemas and ABI types. The SDK generates contributions and C entry glue. The kernel verifies packages offline, freezes explicit bindings and mediates typed synchronous and pending calls through one dispatcher. External example plugins exercise these contracts without private kernel dependencies.

Pending imports retain their parent, root, operation and frozen authority. Cancellation revokes effects before cleanup. Physical executable leases keep native images resident until work drains. Immutable generations support offline candidate preparation, explicit trial, promotion, rollback and safe retirement. Existing roots remain pinned to their admitted generation.

| Milestone | Accepted behavior | Evidence |
| --- | --- | --- |
| P0 / M0.0 | Offline descriptors, generated ABI, typed native imports, explicit grants and inspection | [Foundation acceptance](spec/m0-acceptance.md) |
| M0.1 | Deterministic composition, provider selection, optional imports and resource attenuation | [Composition acceptance](spec/m0.1-acceptance.md) |
| M0.2 | Pending roots and imports, cancellation trees, physical leases and ordered lifecycle | [Pending and lifecycle acceptance](spec/m0.2-acceptance.md) |
| M0.3 | Immutable generation identities, pinned roots, promotion, rollback and retirement | [Generation acceptance](spec/m0.3-acceptance.md) |
| M0.4 | Standalone public SDK, integrated conformance and production closure | [Product acceptance](spec/m0.4-acceptance.md) |

Use `nix flake check -L` for canonical validation. All fourteen gates passed on implementation commit `f4fbe2ffd14a6ef7c71dcfc8694e5f073392b23d`. [CI run 38110344943](https://github.com/matthis-k/phenix-ai/actions/runs/38110344943) uploaded the complete release evidence. [PR #771](https://github.com/matthis-k/phenix-ai/pull/771) records validation for the current branch head and remains ready, open and unmerged.

The default Nix package contains the kernel and its runtime dependencies. SDK, descriptor tools, fixtures, demos and conformance evidence are separate products. `nix run .#phenix-m0-demo` exercises the packaged CLI. `nix build .#phenix-m0-release` materializes every gate, standalone SDK evidence and the production closure.

M0.0 through M0.4 have no remaining implementation tasks. Agent API and other domain plugins follow this accepted kernel. Process-fatal native faults remain outside the in-process guarantee. Noncooperative native work may keep executable images resident until process exit.
## Next architecture milestone: M1 (proposed)

M0.0-M0.4 are accepted for the trusted-native profile. The next work is a distinct staged extension rather than a rewrite of those gates: [plugin contract authoring](spec/m1-plugin-contracts-rfc.md), [kernel-owned Value/ValueRef and contributed reactive DAGs](spec/m1-values-reactive-dag-rfc.md), and the [file-level M1 implementation roadmap](spec/m1-roadmap.md). None of these M1 features are implemented in the accepted M0 kernel. Keep the M0 acceptance artifacts unchanged and use new external non-agent fixtures to prove each M1 gate.

## Basic agent preflight

`nix run .#phenix-a0-demo` runs separate agent, scripted model and uppercase-tool plugins through the kernel. The answer is `mock:PHENIX`. `nix build .#checks.x86_64-linux.a0-agent-product` verifies the native cases and the independent CLI producer. [The A0 contract](spec/a0-agent-contract.md) fixes this synchronous single-tool profile; [its matrix](spec/a0-agent-matrix.md) records acceptance. Live provider I/O and the reactive A1 agent remain subsequent work.
