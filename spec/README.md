# Specification index and precedence

**Purpose:** This directory records intended behavior, migration plans, design reviews, and historical acceptance evidence. **It does not describe the live state of the repository.** Source code and tests at a pinned Git revision determine what is implemented. A specification describes what should hold, even when implementation is incomplete. A merged PR supplies change history; its verified checks supply acceptance evidence.

**Latest graph redesign target:** [Graph Architecture v2.4](graph-architecture-v2.4.md), drafted 25 October 2026. **Proposed, not merged as runtime behavior.** Earlier graph designs (v1 and v2.1–v2.3) are not competing normative targets. The existing M1 `phenix.graph.v1` code and its tests remain the current implementation until graph-v2 reaches tested P1 parity and replaces it.

## Read the right source

| Need | Authoritative source |
| --- | --- |
| What actually runs or compiles? | Source, current composition, and its tested Git commit; inspect `crates/`, `flake.nix`, and product tests |
| What should the graph redesign implement? | [Graph-v2.4 target design](graph-architecture-v2.4.md) and the applicable owning implementation PR |
| Which behavior must a migration preserve? | Current M1 tests, accepted product evidence, and current kernel/SDK contracts |
| Which milestone is open or closed? | Reconcile the relevant roadmap entry with current PR merge state and exact-head acceptance; older markdown status is a dated snapshot |
| How should contributions be reviewed? | [CONTRIBUTING.md](../CONTRIBUTING.md) |

A specification may be approved as a design without being implemented. An open PR may contain working code without being integrated. A merged PR may pass limited tests without satisfying a larger product milestone. Record those separately.

## Migration order

1. **P1 stabilization.** Fix current user-visible behavior using the M1 implementation.
2. **Graph-v2 P1 parity.** Add v2.4 mechanisms behind tested migrations. Preserve existing authority, cancellation, session, tool, streaming, and frontend behavior; remove graph-v1 paths only after parity.
3. **P2.** Implement new agent capabilities on the tested architecture.

This order does not supersede current M1 security guarantees while the migration is incomplete. Where v2.4 requires new graph mechanisms or revises earlier graph design decisions, it takes precedence as the **design target**, not as a claim about the current runtime.

## Document classification

The categories below are an audit of the top-level `spec/` Markdown files on `main` at `ead8fb49bb58180e2c0d109d984376eaf11580c5`, 26 October 2026. Dates and status lines inside older documents describe their own snapshot. The index does not turn those snapshots into current assertions.

### Graph-v2 target, not implemented

- [`graph-architecture-v2.4.md`](graph-architecture-v2.4.md)

### Current M1 architecture and API references (verify source before claiming implementation)

- [`agent-product-decision.md`](agent-product-decision.md)
- [`m1-only-runtime-contract.md`](m1-only-runtime-contract.md)
- [`m1-metadata-state-contract.md`](m1-metadata-state-contract.md)
- [`m1-reference-wire-contract.md`](m1-reference-wire-contract.md)
- [`m1-value-sdk-contract.md`](m1-value-sdk-contract.md)
- [`m1-live-state-contract.md`](m1-live-state-contract.md)
- [`m1-contract-authoring-contract.md`](m1-contract-authoring-contract.md)
- [`m1-authoring-acceptance-contract.md`](m1-authoring-acceptance-contract.md)
- [`m1-epoch-snapshot-contract.md`](m1-epoch-snapshot-contract.md)
- [`plugin-state-ownership.md`](plugin-state-ownership.md)

### Graph-v1 implementation and milestone contracts (not Graph-v2 prescriptions)

- [`m1-graph-declarations.md`](m1-graph-declarations.md)
- [`m1-definition-graph-contract.md`](m1-definition-graph-contract.md)
- [`m1-pending-graph-contract.md`](m1-pending-graph-contract.md)
- [`m1-serial-graph-contract.md`](m1-serial-graph-contract.md)
- [`basic-agent-contract.md`](basic-agent-contract.md)
- [`basic-agent-pending.md`](basic-agent-pending.md)
- [`basic-agent-parity.md`](basic-agent-parity.md)
- [`basic-http-model-contract.md`](basic-http-model-contract.md)
- [`basic-observer-contract.md`](basic-observer-contract.md)

### Superseded legacy proposals (redirects; full source retained in Git history)

- [`m1-values-reactive-dag-rfc.md`](m1-values-reactive-dag-rfc.md)
- [`m1-plugin-contracts-rfc.md`](m1-plugin-contracts-rfc.md)

### Roadmaps, migration trackers and exact-head evidence snapshots

- [`pr-integration-queue.md`](pr-integration-queue.md) — dated active PR dependencies and removal gates
- [`m1-roadmap.md`](m1-roadmap.md)
- [`p2-advanced-agent-roadmap.md`](p2-advanced-agent-roadmap.md)
- [`m1-ci-matrix.md`](m1-ci-matrix.md)
- [`m1-contract-authoring-matrix.md`](m1-contract-authoring-matrix.md)
- [`m1-value-sdk-matrix.md`](m1-value-sdk-matrix.md)
- [`m1-live-state-matrix.md`](m1-live-state-matrix.md)
- [`ci-post-p1-cutover.md`](ci-post-p1-cutover.md)
- [`p1-release-acceptance.md`](p1-release-acceptance.md)
- [`a0-agent-matrix.md`](a0-agent-matrix.md)

### Historical freezes and analysis (context/evidence, not current normative target)

- [`clean-start-rfc.md`](clean-start-rfc.md)
- [`a0-agent-contract.md`](a0-agent-contract.md)
- [`m1-consolidation-audit.md`](m1-consolidation-audit.md)
- [`m1-consolidation-contract.md`](m1-consolidation-contract.md)
- [`m1-r1-library-audit.md`](m1-r1-library-audit.md)
- [`p0-decision.md`](p0-decision.md)
- [`plugin-source-preparation.md`](plugin-source-preparation.md)
- [`declarative-plugin-cleanup.md`](declarative-plugin-cleanup.md)
- [`plugin-responsibility-audit.md`](plugin-responsibility-audit.md)
- [`usage-first-plugin-architecture.md`](usage-first-plugin-architecture.md)

### Historical M0 documents (retired formats; preserve acceptance evidence)

These files describe milestones and previous wire/profile contracts. M1 is the sole supported product. Their security and behavior tests may remain relevant, but their historical formats are not reintroduced.

- [`m0-abi-review.md`](m0-abi-review.md)
- [`m0-acceptance.md`](m0-acceptance.md)
- [`m0-contracts.md`](m0-contracts.md)
- [`m0-matrix.md`](m0-matrix.md)
- [`m0.1-acceptance.md`](m0.1-acceptance.md)
- [`m0.1-contracts.md`](m0.1-contracts.md)
- [`m0.1-matrix.md`](m0.1-matrix.md)
- [`m0.2-acceptance.md`](m0.2-acceptance.md)
- [`m0.2-import-contracts.md`](m0.2-import-contracts.md)
- [`m0.2-lifecycle-contracts.md`](m0.2-lifecycle-contracts.md)
- [`m0.2-matrix.md`](m0.2-matrix.md)
- [`m0.2-pending-contracts.md`](m0.2-pending-contracts.md)
- [`m0.3-acceptance.md`](m0.3-acceptance.md)
- [`m0.3-contracts.md`](m0.3-contracts.md)
- [`m0.3-matrix.md`](m0.3-matrix.md)
- [`m0.4-acceptance.md`](m0.4-acceptance.md)
- [`m0.4-contracts.md`](m0.4-contracts.md)
- [`m0.4-matrix.md`](m0.4-matrix.md)

### Immutable evidence

- [`evidence/`](evidence/) contains dated verification records. Do not rewrite them to claim later behavior.

## Supersession policy

- For **new graph-v2 decisions**, edit or amend v2.4. Remove contradictory planned behavior from active requirements; retain older RFCs only as explicitly labelled history.
- For **existing M1 behavior**, test and inspect the implementation. If a document claims a type, plugin, or command already exists but source disagrees, fix the document's claim; do not invent the missing code.
- For **an unfinished migration**, link its owning issue or open PR. No specification status, unmerged code, or draft test is a completion claim.
- When a PR changes accepted design semantics, update the latest target spec in the same change. When it changes implementation, include exact-head conformance evidence and reconcile the owning roadmap.
- Keep old acceptance documents if a current test or roadmap links them. Mark them historical instead of deleting traceable proof. Delete redundant unreferenced stale drafts after checking references.

## Known stale statements to ignore

- [`m1-roadmap.md`](m1-roadmap.md) formerly named the older reactive DAG RFC as generally normative. That RFC remains historical for graph-v1 development; v2.4 defines the latest **future graph redesign**.
- [`usage-first-plugin-architecture.md`](usage-first-plugin-architecture.md) was written before `ProviderRegistrations` and `ToolRegistrations` existed on `main`. Those contracts have now been added. The proposed manager and authoring ergonomics still require separate implementation evidence.
- Dated roadmap statuses for provider authentication, model selection, and tool registration cannot be used to infer current state; verify each owning PR and current source.

This file is navigation and precedence, **not a second milestone tracker**.
