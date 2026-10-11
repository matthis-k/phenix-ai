# Phenix minimal kernel

This branch replaces the previous implementation with a domain-neutral native plugin kernel. It starts at root commit `11cdf0eca7e80965d0dfca3762d1fd65bb19e10f` and follows [the normative RFC](spec/clean-start-rfc.md). P0 and M0.0 are accepted; M0.1 adds composition and scoped authority.

The public contract crate owns values, schemas and ABI types. The SDK generates contributions and C entry glue. The kernel verifies packages offline, freezes explicit bindings, and mediates synchronous calls. External example plugins transform text through a typed import.

[Implementation contracts](spec/m0-contracts.md) fix this milestone's wire formats and ownership. [M0.0 acceptance](spec/m0-acceptance.md) records the foundation review. [M0.1 contracts](spec/m0.1-contracts.md) and its [acceptance matrix](spec/m0.1-matrix.md) record composition behavior, evidence and remaining work.

Use `nix flake check -L` for canonical validation. The flake builds kernel and example plugins separately. Later milestones require their own contract gates.
