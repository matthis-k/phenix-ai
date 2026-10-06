---
name: rust
description: "Rust implementation and review. Use whenever writing, changing, reviewing, or designing Rust code, especially before adding dependencies or building common infrastructure locally. Inspect the workspace first, prefer established ecosystem crates when they fit, and research current evidence when a dependency choice may have changed."
---

Use Rust's existing ecosystem before creating a project-local substitute.

## Start

1. Inspect the workspace `Cargo.toml`, relevant crate manifests, enabled features, `Cargo.lock`, toolchain/MSRV policy, existing abstractions, and project concurrency/runtime policy.
2. Classify the requested capability. For common dependency roles, load `references/ecosystem.md` with `phenix_skill_resource_read`.
3. Prefer an already-used workspace crate when it satisfies the requirement cleanly.
4. Preserve typed domain boundaries. Convenience libraries must not erase useful error, authority, identity, or state semantics.

## Dependency choice

For a new or replacement crate:

1. Establish required semantics: blocking/threaded vs async, runtime policy, supported backends, `no_std`, native dependencies, wire/storage format, performance constraints, and MSRV.
2. Check the baseline candidates in `references/ecosystem.md`.
3. When the choice is material, contested, unfamiliar, or likely stale, load `references/research.md` and run that research protocol.
4. Compare semantic fit, maintenance, interoperability/adoption, API/docs, advisories, license/MSRV, dependency/native-build cost, and feature control.
5. Choose the smallest established dependency that fits the required semantics. Downloads alone do not decide quality.
6. When an obvious baseline crate is not selected, record the concrete reason the alternative or local implementation fits better.

## Concurrency

Do not equate concurrency with async Rust.

1. Follow the project's declared runtime model first.
2. Prefer a synchronous call when the caller can legitimately wait.
3. For long blocking I/O that must not stall unrelated work, use an OS thread or a bounded blocking worker pool plus typed channels/events.
4. Use explicit cancellation handles instead of relying on future/task cancellation when the project is thread-based.
5. Use Rayon for CPU-bound data parallelism when it fits. Do not use a CPU work-stealing pool as the normal home for long-lived blocked socket/process workers.
6. Introduce Tokio or another async runtime only when the project permits async and the workload or surrounding API has a concrete async requirement. Async is not the default answer to ordinary concurrency.

## Audit

When reviewing a Rust project:

1. Inventory direct dependencies by role.
2. Search for local code duplicating established crate functionality.
3. Classify each role as `aligned`, `intentional alternative`, `candidate dependency`, or `unnecessary duplicate`.
4. Recommend migration only when it improves correctness, maintenance, interoperability, or materially removes code. Matching the list alone is not a reason to churn working code.
5. Keep errors typed inside libraries and domain code. `thiserror` is the normal boilerplate reducer; reserve `anyhow` mainly for application or tooling boundaries where callers do not need structured variants.
6. Scope dependency features to what the code uses. Avoid broad/default feature sets when a narrower set is clear.
7. Flag runtime-model drift: async dependencies in a blocking/threaded project, blocking calls inside an async runtime, or executor-specific types leaking into domain contracts.

## Repeatability

The ecosystem reference is a maintained shortlist, not permanent truth.

- Re-research a role when no listed crate fits, the recommendation is disputed, maintenance has changed, a major version changes the tradeoff, or current correctness depends on fresh ecosystem state.
- Treat official crate documentation, crates.io metadata, upstream repositories, and RustSec as primary evidence. Use curated lists for discovery and cross-checking.
- Update `references/ecosystem.md` only when research establishes a durable recommendation change.
- Keep project-specific exceptions in the project, not in the global ecosystem list.

## Completion

Run the repository's Rust formatting, Clippy, and test commands. If the dependency graph changed, run available advisory, license, and duplicate-dependency checks. Report non-obvious crate choices and the reason for rejected alternatives.