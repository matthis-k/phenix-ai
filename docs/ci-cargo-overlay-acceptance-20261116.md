# Shared Cargo dependency pilot acceptance

This branch is an **independent, disposable verification head** based on
the existing experimental consumer PR #1305; it does not change the FBP
feature stack or the prepared coordinated-CI rebase anchor.

## Invariants

- Use an immutable Phenix Flake CI library revision and matching NAR hash.
- Keep the **full workspace Cargo.toml and Cargo.lock** for all test groups.
  No group-specific rewritten workspace manifest, and no --locked downgrade.
- Each source-filtered group overlays only its selected real crates and their
  local dependencies onto a complete dummy workspace source supplied by Crane.
- Default suites partition all real workspace packages exactly once, while
  the separate no-default profile remains independent.
- A failing group must not prevent unrelated Nix roots from being attempted.
- Keep CLIPPY, format, kernel-smoke, runtime handoff, untrusted PR fallback,
  final acceptance and specialized FBP benchmark workflows.

## Evaluation

The exact-head GitHub workflow must pass all eight jobs. Then compare
repeated cold/warm runs against the fully green non-Crane FBP CI baseline,
counting actual Cargo compilations and runner-minutes rather than inferring
performance from wall time alone.

Do not merge the experimental pilot into the FBP stack unless these
criteria are met. This document is an acceptance marker for the independent
test PR and does not modify the production implementation.
