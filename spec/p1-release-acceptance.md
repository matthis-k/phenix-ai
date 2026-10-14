# P1 release acceptance and integration

P1 delivers the minimal graph-driven Basic agent product. This is an acceptance contract, not a claim of release completion.

## Required architecture

- Kernel owns one typed M1 resolver, authenticated graph scheduling, trigger admission, cancellation and generation identity; no agent loop in kernel.
- Typed Model, Tools, Skills, Context, Session, Journal, SQLite Storage, Basic Agent and Frontend remain selectable plugins. Prefer established crates for protocols, transports and persistence.
- Terminal, JSONL and official stable-v1 ACP SDK drive the same Basic graph and durable state.
- Verify multi-turn tool calls, caller-selected model/tool limits, usage, live output, cancellation, tool-error and Model-error continuation, and journal replay.
- Reject unauthorized effects, cross-session access, malformed or oversized frames and duplicate non-idempotent actions on restart.

## Required exact-head checks

Every item must pass on the **same final combined P1 SHA**, not merely on individual stacked PR heads:

1. Canonical conformance: workspace format, typed SDK, ABI/security and packaged Basic product negatives.
2. Full flake-aware matrix: every declared Nix check built and accounted for.
3. Generated Nix DAG: all prerequisites and shipping checks, with exact-head dependency evidence.
4. Independent official stable-v1 ACP SDK: typed protocol interoperability, tools, errors, multi-session and restart/replay.
5. Realized P1 Nix preflight: checks.x86_64-linux.p1-release-preflight, checking Terminal/JSONL/ACP, pending HTTP, crash/fault recovery, limits, cancellation, journal and M1 shipping. Its evidence has p1_accepted=false until external gates pass.
6. Combined diff boundary audit: no agent-specific kernel branches, M0 compatibility, second scheduler, handwritten standard protocol or storage implementation, or unexplained code-size expansion.
7. After integration, repeat authoritative checks on the **actual new main SHA**.

## Integration ledger

| Milestone | Implementation evidence | Final acceptance |
| --- | --- | --- |
| M1 kernel and G1–G4 | PR #771–#862 | Recheck combined SHA |
| B2 SQLite / Session / Journal | PR #829, #854–#855 and restart/fault fixtures | Recheck combined SHA |
| B3–B5 providers and Basic agent | Scripted and live HTTP, PR #865–#871 | Recheck combined SHA |
| F1 Terminal / JSONL | PR #867–#871 | Recheck combined SHA |
| F2 and R1 official ACP v1 | PR #843, #853, #864–#866 and SDK interop | Recheck combined SHA |
| P1 release evidence | PR #872 | Pending exact-head suite |
| Main integration | Not performed | Pending |

Tests-only heads #868 and #870 are intentionally RED and should not be made green by weakening the tests. Their subsequent implementation heads must pass.

## Safe integration procedure

1. Repair any failing test on its owning branch. Do not add speculative runtime abstractions.
2. Freeze one complete P1 head and run every authoritative gate on it; record all workflow run IDs and SHA.
3. Confirm oldmain preserves the outgoing main tip. Those branch histories matched when this checklist was drafted.
4. As old main and the minimal rewrite have divergent histories, do not naïvely merge those lineages. Perform the user-approved controlled replacement only after acceptance, preserving oldmain.
5. Run the required checks against the resulting main SHA; record the old and new SHAs. Mark the roadmap P1 row ACCEPTED only after verifying that integrated SHA.

Not P1 blockers: kernel management K1–K3, logging L1, introspection I1, ACP v2, synchronized multi-client frontends, Advanced agent features and future CI optimization.
