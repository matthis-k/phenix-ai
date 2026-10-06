# Rust dependency research protocol

Use this when the ecosystem baseline needs verification or extension.

## Evidence checklist

For each candidate crate, record only evidence that can change the choice:

- required capability and semantic fit;
- current stable release and supported Rust/MSRV when published;
- maintenance state and recent release activity;
- crates.io adoption and reverse-dependency signal when useful;
- upstream ownership and repository health;
- RustSec vulnerability, unsoundness, malicious-package, or unmaintained advisories;
- license and native/system dependency requirements;
- relevant feature flags and default-feature cost;
- interoperability with the project's existing runtime and crates;
- migration cost from an existing implementation.

## Decision

1. Reject candidates that fail required semantics, project policy, or security constraints.
2. Prefer an existing project dependency when it remains a strong fit.
3. Among viable candidates, prefer mature ecosystem integration and clear maintenance over novelty.
4. Use popularity as corroboration, never as the sole reason.
5. Keep a local implementation only when project-specific semantics or dependency costs make the established crate a worse fit.

## Baseline update

Change `ecosystem.md` only when the result is likely to remain useful across Rust projects. Keep one-off project choices in project documentation or code comments where the decision is enforced.