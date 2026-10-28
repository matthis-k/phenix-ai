# Active Phenix PR integration sequence (26 October 2026)

**Status:** Operational dependency and removal gate, not an implementation specification or claim of current CI. Graph Architecture [v2.4](graph-architecture-v2.4.md) is the target design; source and exact-head product checks define implemented behavior. The integration baseline is `main` after documentation-only #1169 (`ca31506a6dce`). This inventory covered all 29 open PRs found immediately before #1169 was merged and #1159 was closed. Recheck open status, actual merge base and CI before taking any action.

**Policy:** Stop adding parallel mechanisms. Remove duplicate, demonstrably superseded code as soon as the replacement has a reproducible parity gate; never delete the sole working P1 path merely because v2.4 prescribes its future replacement. For every candidate removal, require a call-site inventory, selected-composition parity, negative/security cases, no fallback to the old implementation, and exact-head CI. Do not collapse independent PR lanes into an artificial single Git history.

## Integration lanes and dependency order

| Lane | Ordered PRs | Merge/reconciliation gate |
| --- | --- | --- |
| Canonical spec | **#1169 merged** | Proposed graph-v2 semantics are now discoverable without claiming runtime support |
| P1 credential-free evidence | #1164 → #1165 → #1166 → #1167 | Each starts from `main`; order is *integration order*, not Git ancestry. Re-evaluate overlapping `flake.nix` patches after each merge |
| P1 authorization | #1158; #1160 → #1161 | Fix #1158 ACP credential-initialize failure; #1160 library-backed SIWC transport must pass exact-head product checks before #1161 pre-session selection can integrate |
| Competing P1 implementation | #1136 | Independent concrete API-key/ChatGPT handler migration; audit against #1140–#1146 and #1160. Transfer unique behavioral tests/guards into the selected shared-host path, then close or integrate once. Do **not** merge two authoritative auth pipelines |
| Provider authoring and deletion | #1139 → **#1143** → #1140 → #1141 → #1142 → #1144 → **#1145** → #1146 | #1143 removes old imperative provider builders once #1139 data declarations exist. #1145 replaces duplicated Responses-family policy. Both are presently **sibling PRs**: integrate their edits into the downstream tree and update bases before declaring this linearized |
| Generic event and tool migration | #1147 → #1148 → #1149 → #1150 → #1151 → #1152 → #1153 → #1154 → #1155 → #1156 | Existing physical parent stack; preserve kernel ownership, manager-owned catalogs and tool execution attestation. Intermediate red checks must be fixed or subsumed by verified later checks, never ignored |
| Separate ABI slice | #1163 | Typed chunk envelope only; keep draft until stream capability, SDK transport and product adoption. Not a prerequisite for provider P1 or graph-v2 V0 |
| Future graph-v2 | V0 → V1 → V2 → V3 → V4 → V5 → V6 → V7 | Implement canonical metadata + finite-wave semantics, then native Source/Invoke/Flow/Invoke, then operators, deterministic patches, deferred waves, SDK parity, plugin acceptance and inspection. No version-specific second dispatcher or domain state in kernel |

Arrows between independent test PRs indicate preferred *review/integration* order only. Actual Git ancestry is the source of truth for every stacked branch.

## Current dependency corrections needed

1. **#1143 is based on #1139, but #1140 is also based on #1139.** #1143 must be integrated into #1140 without reintroducing deleted builders. After that, update the downstream ancestry; merely changing the PR's base branch without carrying the deletion is invalid.
2. **#1145 is based on #1144, but #1146 is also based on #1144.** The shared Responses-policy change must be carried into #1146 (and descendants) before restacking.
3. **#1136 is not an ancestor of #1139.** Reconcile concrete auth behavior and tests before promoting either path. A green CI result on two independent heads does not establish compatible product composition.
4. **#1159 closed unmerged.** It attempted a selective libcurl IPv4 retry; #1160 replaces that bespoke SIWC transport. Do not resurrect both.
5. **#1160 → #1161** is already physically stacked. Keep it so, and do not treat #1161's draft head as pre-session model selection on `main`.
6. **#1164–#1167 and #1158 are `main`-based independent candidates.** Resolve changes to the shared CI flake after each integration rather than retargeting them to graph branches.

## CI failure and removal queue (snapshot, 26 October 2026)

| Priority | PR | Observed failure or gate | Work already performed |
| --- | --- | --- | --- |
| 1 | #1143 | `phenix-contract-check`: unused imports following removal of legacy builders | Production/test imports separated at `323a09f82b`; new checks pending |
| 1 | #1145 | Dead `selected_connection` host wrapper and dotted Responses model versions misclassified | Unused wrapper removed; shared family matcher tightened and tests extended; new checks pending |
| 1 | #1160 | `phenix-contract-check`: unused `mut` in discovery cache tests | Corrected at `9c9fbb1401`; new checks pending |
| 2 | #1158 | `phenix-basic-acp-credentials-product`: initialize failure | Unresolved; reproduce in isolated auth product before integration |
| 2 | #1141–#1144, #1152–#1156 | `phenix-provider-host-api-key-product`: missing successful response at initial API-key assertion (seen on #1156) | Real product failure; not a mere lint problem; inspect selected auth and model-routing admission |
| 2 | #1149 | Formatting, typed-v1-v2 and Nix shard failures | Fix source and tests in owning branch before relying on descendant passes |
| 2 | #1161 | Multiple Nix shard failures at draft head | Requires #1160 exact-head acceptance plus frontend pre-session checks |
| 3 | #1163 | Transport capability, SDK and frontend integration not yet implemented | Explicitly partial, no release claim |

Historical checks on older heads are not current acceptance after a fix or restack. Re-run `nix flake check -L`, the selected native product compositions, and the P1 frontend tests on each candidate integration commit.

## Spec-conformance migration: delete first, then replace graph mechanics

- **Safe early removal:** unused/dead legacy provider declarations, duplicated metadata/executor registration, multiple authoritative published catalog copies, duplicated model eligibility rules and obsolete bespoke OAuth paths, each with selected-product parity.
- **Do not delete yet:** the functioning graph-v1 trigger executor, serial root/pending-ticket logic, ACP default operations, or session/auth persistence. Those are required P1 behavior and must survive through v2.4 native V1/V5 parity.
- **First graph-v2 code:** canonical offline `GraphContributionV2`, typed port/edge declarations, structural authorization and wave/closure rules (V0), without a second runtime scheduler.
- **First native parity demonstration:** one Source → Invoke → Flow → Invoke chain with typed success/error outputs, pending/cancel tests and original M1 direct-call parity (V1).
- **Removal completion:** prove provider/tool/agent/ACP equivalence at selected composition, then remove v1 lowering bridge and deprecated APIs together. No indefinite compat layers.

## Exit criteria

- Every open PR has exactly one owner/lane and its base names an actual ancestry relation.
- Sibling branches #1143/#1145 are reconciled, not merely relabelled.
- Only one selected P1 auth/model-transport route remains; credential-source grants and account selection stay explicit.
- Stale code is gone from the shipping graph/composition and public authoring surfaces, not merely unused in a demo.
- Accepted v2.4 design, implemented exact source, CI evidence and merged distribution remain separate facts.
