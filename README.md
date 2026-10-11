# Phenix minimal kernel

This branch replaces the previous implementation with a domain-neutral native plugin kernel. It starts at root commit `11cdf0eca7e80965d0dfca3762d1fd65bb19e10f` and follows [the normative RFC](spec/clean-start-rfc.md). P0, M0.0 and M0.1 are accepted. M0.2 adds synchronous lifecycle and native pending roots. Its full gate remains open until pending imports and their cancellation tree are proven.

The public contract crate owns values, schemas and ABI types. The SDK generates contributions and C entry glue. The kernel verifies packages offline, freezes explicit bindings, and mediates typed synchronous and pending calls. External example plugins transform text through a typed import.

[Implementation contracts](spec/m0-contracts.md) fix this milestone's wire formats and ownership. [M0.0 acceptance](spec/m0-acceptance.md) records the foundation review. [M0.1 contracts](spec/m0.1-contracts.md) and its [acceptance matrix](spec/m0.1-matrix.md) record composition behavior, evidence and remaining work.

[M0.1 acceptance](spec/m0.1-acceptance.md) records all six exact-head CI gates. [M0.2 lifecycle contracts](spec/m0.2-lifecycle-contracts.md) and [its matrix](spec/m0.2-matrix.md) track native start/stop, rollback, inspection and remaining work. [Pending contracts](spec/m0.2-pending-contracts.md) define generated ABI 2 hooks, wake permits, exactly-once results, cancellation and executable leases. The external pending plugin proves native worker ownership without private kernel dependencies. Two independent SDK plugins prove dependency order and typed failure propagation through native loading.

Use `nix flake check -L` for canonical validation. The flake builds kernel and example plugins separately. Later milestones require their own contract gates.

The default package is the kernel. SDK and descriptor tools are separate packages. Example plugins belong to conformance and demo outputs. Agent API and other domain plugins can be added after M0.4 accepts the kernel contracts.

The minimal implementation includes typed pending imports, cancellation trees, physical executable leases and selectable immutable generations. The public `Generations` control API installs offline candidates, trials them explicitly, promotes or rolls back admission defaults and reports retirement blockers. Roots retain the existing Kernel/Root dispatcher and their original generation.

`nix build .#phenix-m0-release` materializes every conformance gate, standalone SDK tests, shipping evidence and production closure. See spec/m0.2-matrix.md, spec/m0.3-matrix.md and spec/m0.4-matrix.md for acceptance evidence. PR #771 remains open and unmerged.
