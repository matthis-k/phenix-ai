# Superseded M1 value and reactive DAG RFC

**Status: superseded design proposal.** The 11 October 2026 RFC no longer specifies the target for new graph work. Its original content remains in Git history, but keeping two competing graph descriptions in the working tree caused stale guidance.

- **For future graph architecture:** [Phenix Graph Architecture v2.4](graph-architecture-v2.4.md). This is proposed and not implemented on `main`.
- **For current runtime behavior:** inspect `crates/phenix-contract/`, `crates/phenix-kernel/`, selected compositions, and contemporary conformance tests.
- **For current M1 structural value and ABI contracts:** [M1-only runtime](m1-only-runtime-contract.md), [reference wire contract](m1-reference-wire-contract.md), [live-state contract](m1-live-state-contract.md), and [authoring contract](m1-contract-authoring-contract.md).
- **For migration order:** [M1 roadmap](m1-roadmap.md), [specification index](README.md), and the owning implementation PR.

The old RFC described `Epoch` propagation, `after` ordering, and early value-wire evolution. Do not use its historical graph design to override v2.4, and do not claim v2.4 is executable until accepted code and tests establish that fact.

Historical full text: [pre-supersession revision](https://github.com/matthis-k/phenix-ai/blob/ead8fb49bb58180e2c0d109d984376eaf11580c5/spec/m1-values-reactive-dag-rfc.md).
