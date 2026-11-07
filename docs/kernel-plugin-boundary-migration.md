# Plugin/kernel boundary — migration slice 1

Status: experimental extraction; no graph-engine replacement yet.

## Actual changes
- New graph-independent phenix-plugin crate owns native library load, ABI dispatch, stop and drop.
- Kernel runtime keeps host API creation, root admission, authorization, pending ticket tracking and existing graph scheduler. It passes a host-created StateHostApi to the plugin loader.
- Typed Registrar stores host-selected contribution provenance and validates duplicate keys before atomic merge. It has no graph dependencies or second executor.
- Cargo.lock reflects the transferred libloading dependency.

## Remaining old responsibilities
- The current main graph v1/v2 remains in phenix-kernel. Delete only after the separate Iteration 12 phenix-graph prototype is integrated, compiled, and conformance-verified.
- Existing resolve::Prepared still selects plugin packages and handles composition; migrate selection to KernelInstance composition in a later bounded slice.
- This registry is in-process only. Native plugin registration requires an ABI-safe descriptor/handle adapter; TypeId and Rust trait objects are not a stable native ABI.
- Registrar::new must be called by the host with authenticated owner identity. Owner strings are not grants; the kernel enforces effective authority.

## Tests and acceptance
Run cargo test -p phenix-plugin -p phenix-kernel and existing kernel conformance. This environment lacks cargo/rustc; compilation is unverified.

## Next steps
1. Move package catalog/factories and code leases into phenix-plugin without duplicating verification or kernel policy.
2. Add an external plugin/graph bridge contribution that wraps the existing PatchBuilder; no separate patch implementation.
3. Stage selected contributions into a single compiled graph generation; implement atomic activation and retired-generation pinning.
4. Migrate graph execution from legacy graph paths to Iteration 12 only after tests pass, then delete legacy scheduling/declaration paths and unused glue.
