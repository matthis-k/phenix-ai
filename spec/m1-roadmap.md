# Phenix M1 implementation roadmap: contract-first reactive values and DAGs

> **Status note (26 October 2026):** This is a dated implementation/evidence ledger, not a live description of `main`. Source, tests, and current PR merge state determine what exists. The [specification index](README.md) separates current M1 code from the proposed v2.4 graph target. Keep historical evidence, but do not promote prior per-stage `pending` or `draft` lines to current facts.

Status: P1 integrated product accepted at 68f46f63; C4 authoritative CI accepted on main at b7017042; post-P1 work resumed
Updated: 2026-10-20
Baseline: [clean-start RFC](clean-start-rfc.md), accepted M0.0-M0.4 in PR #771
Implemented M1 graph-v1 semantics: [G1/G2 graph declaration contract](m1-graph-declarations.md) and current source/tests. Future graph-v2 design target: [Graph Architecture v2.4](graph-architecture-v2.4.md), which is not implemented. The [older value/reactive DAG RFC](m1-values-reactive-dag-rfc.md) is historical and no longer governs future graph redesign.
This file is the single task/evidence tracker for M1. Do not duplicate completion percentages in PR bodies.

## Current integrated acceptance and next stages

### Graph-v2 implementation-gap matrix (27 October 2026)

Inspected baseline: `31f86f454fd4cd3316379b5802b02ba19ecae69c`.
This is the canonical graph-v2 gap matrix within the existing M1 tracker.
The v2.4 target does not establish implementation or product acceptance.
The isolated rewrite #1171 is not a replacement for the working P1 composition.

| Area | Spec reference | Current code | Gap | Migration | Verification | PR/status |
| --- | --- | --- | --- | --- | --- | --- |
| Graph metadata | v2.4 section 2 | `phenix-contract/src/graph/v2/declarations.rs`, shared artifact graph section parser | Base declarations integrated by #1172; Switch/Merge (#1173) and Join (#1175) merged; Optional/StaticMany readiness implemented on a follow-up; deferred and patches still explicitly reject | V0/V2, retain v1 | Canonical permutation, malformed metadata, ownership, bounds, readiness wire and native fixture tests | In progress; base+operators integrated, readiness pending CI |
| Finite waves | v2.4 section 3 | Existing serial M1 graph/root machinery, private initial-wave v2 readiness | Initial-wave intrinsic delivery through Switch/Merge/Join and Optional/StaticMany merged or on follow-up; multiple waves and deferred admission remain absent | V0/V1/V2 | Native Source/Invoke/Constant, Switch/Merge, Join and readiness fixtures pass; follow-up exact-head CI pending | In progress; base+operators integrated |
| Graph patches | v2.4 section 4 | No v2 patch engine | Exact deterministic patch algebra and exports | V3, after V1 | No patch acceptance | Not started |
| Security | v2.4 sections 3.4.1, 5 | Offline product lattice, trusted composition policy and inherited-resource runtime delivery | Independent topology/delivery/operation checks implemented; intrinsic label propagation tested. Declassification/endorsement and selected classified observation remain absent | V0/V1 base integrated; V2 follow-ups | Native scoped delivery/ref-without-read negatives passed in #1172; follow-up intrinsic/control/resource tests pass locally | In progress; base slice integrated |
| Scheduler | v2.4 section 6 | Existing `GraphExecution` with private v2 readiness, same Root/dispatcher/tickets | Bounded initial-wave One inputs, typed intrinsics through Join, and Optional/StaticMany terminal reduction implemented; deferred execution remains absent | V1/V2/V4 through existing dispatcher/tickets | Native result routing, terminality, failure precedence, cardinality closure and physical-drain fixtures; follow-up exact-head CI pending | In progress; V1 base + operators integrated |
| SDK/macros | v2.4 section 7 | Existing M1 `Operation`, graph-v1 authoring | Typed graph/subgraph authoring and temporary lowering bridge | V5 | Existing APIs are not v2 acceptance | Not started |
| Plugin state/exposure | Plugin state ownership | Ordinary plugin fields and M1 selected property contracts | Explicit normative opt-in publication amendment and concrete authoring trials | Preserve private ownership; no global state registry | No new exposure acceptance | Not started |
| Providers/auth | Provider responsibility audit | Existing selected provider contracts/runtime | Consolidate stacked shared-host lineage and auth parity | #1139-#1156; reconcile #1143/#1145, do not combine competing #1136 auth | Refresh each exact-head product gate before integration | In progress, unmerged stack |
| Sessions/frontends | Basic/P1 contracts | Working graph-v1 Basic/session/frontend path | Model-before-session control, auth restoration, graph-v2 P1 parity | #1158, #1160 -> #1161; V5 | No graph-v2 product acceptance | In progress, unmerged P1 fixes |
| Testing | v2.4 section 9 | M1 conformance and new offline security unit tests | Native V1 and full property/negative/product gates | V0 through V7 | #1164 exact head `76fc59da16b6552373d43ff452e1de98c8f08d94` CI green; evidence stack remains unmerged | In progress |

#### Stacked execution acceptance (29 October 2026)

The following unmerged increments extend the dated baseline above; they do not
change the working V1 default or establish V2 P1 parity. The supplied heads of
#1206 (`2d1ff0c4`), #1207 (`4bd4a7f8`) and #1208 (`2edc7cfa`) were verified and
left intact. #1171 and the Python experiments remain isolated.

| Increment | Exact accepted head | Evidence and scope |
| --- | --- | --- |
| #1209: repeated turns | `d3be19319af21f56dd237f72fe3e9385281260c1` | [Full exact-head CI](https://github.com/matthis-k/phenix-ai/actions/runs/40965359696), coordinator 18m10s. Explicit Switch and bounded FIFO Deferred Next activations, final/one-tool/two-tool-round/budget/tool-failure native cases, packaged repeated turns/restart/journal failure. Tools within a batch remain serial. |
| #1210: continuation drain | `8c27045cc7674cec8af86b94edaab4983cc60f00` | [Full exact-head CI](https://github.com/matthis-k/phenix-ai/actions/runs/40968268658), coordinator 17m14s. Controlled pending initial and repeated continuation cancellation, exactly-once completion, late-success suppression and physical lease drain. |
| #1211: recursive Deferred | `5161d14651f6db34fd6974e1c6d77e8963c4e9f1` | [Full exact-head CI](https://github.com/matthis-k/phenix-ai/actions/runs/40972410804), coordinator 23m38s. Internal/cross-boundary Deferred materialization, provenance, exact effective-ID policy and native execution. |
| #1212: activation observation | `baba485426aa61f17044e1071b02148ab2602890` | [Full exact-head CI](https://github.com/matthis-k/phenix-ai/actions/runs/40975538667), coordinator 23m32s. Trusted `(root, activation, node)` correlation, once-only pending observations, model activations 0/1/2. Not durable identity or concurrent-root isolation. |
| #1213: packaged cancellation | `20f3ad16d473696085123aa076d57512d409e3e0` | [Full exact-head CI](https://github.com/matthis-k/phenix-ai/actions/runs/40981365668), coordinator 20m48s. Physical-worker-witnessed initial/continuation cancellation, same-host/session recovery and stdin EOF after cancellation held until drain. |
| #1214: deferred-gap recovery | `14cd8d3d9139a046a8f3490d21395100c21c5b39` | [Full exact-head CI](https://github.com/matthis-k/phenix-ai/actions/runs/41021476597), coordinator 21m23s. Zero-lease cancellation, rejection before authorized durable finalization, and same-session recovery afterward. Native explicit host finalizer, not packaged gap timing. |
| #1215: independent invocation batches | `7a90b64ec404195190b85b89f9a1bf7fc9c13adf` | [Full exact-head CI](https://github.com/matthis-k/phenix-ai/actions/runs/41059981242), coordinator 21m35s. Kernel/contract units and native distinct-provider overlap, ordered collection, empty batches, same-provider serialization, denial, cancellation and physical drain. |
| #1216: Basic tool fan-out | `c79c44ff1ec808f57e487fd122cee5e9d7736dea` | [Full exact-head CI](https://github.com/matthis-k/phenix-ai/actions/runs/41060020967), coordinator 20m59s. Native and packaged JSONL fan-out, durable partial outcomes, cancellation/EOF, restart recovery, active malformed collection and receipt-write failure. Focused opt-in lint/host compilation passes in 47s. |
| #1217: nested typed boundaries | `9ec4481b69994e10d9d56a3ba0c6e1788f9469cf` | [Full exact-head CI](https://github.com/matthis-k/phenix-ai/actions/runs/41100857560), coordinator 24m25s. Three-level input/output aliases, repeated native occurrences, complete alias provenance and pre-constructor malformed-boundary/policy rejection. |
| #1218: scoped anchors and Order | `4ba135ba0569715881832d52cefe4e32baacf033` | [Full exact-head CI](https://github.com/matthis-k/phenix-ai/actions/runs/41105358932), coordinator 20m11s. Public/private resolution, exact-node cross-boundary Order, held-A/ready-B counterfactual, no-Flow nonactivation and complete scoped provenance. |

**Accepted bounded fan-out (30 October 2026):** The v2.4 section 10.5 InvokeEach
contract freezes homogeneous selected routes, atomic bounded list admission,
host-issued branch ordinals and original-order typed collection after drain.
Distinct providers overlap; the existing physical provider-busy guard serializes
same-provider calls. No second scheduler or agent-specific kernel mechanism is
introduced. The initial profile rejected recursive placement; the dependent C1
section 7.15 increment adds that placement and is gated separately.

Basic V2 connects admit/checkpoint -> sealed Tools plan -> InvokeEach -> correlated
Join/collect -> Basic ordered checkpoint -> Deferred Model. Both typed outcomes
echo the complete session/run/batch/call identity. The synchronous effect loop is
excluded from V2 compilation, while V1 ToolRegistrations remains intact.

Pre-publication review corrected two correctness gaps: pure selected validation
now precedes durable pending admission, and each executor persists its immutable
outcome through an independent Storage-backed receipt provider before returning.
Receipt creation atomically checks the batch fence; collection verifies exact
receipts before consuming the active batch. Executors do not mutate Session or
Running, and model continuation waits for complete collection and physical drain.

After cancellation drain or fenced owned-run recovery, a separately authorized
receipt recovery root imports known results through GraphToolSession.recover.
Known B remains durable if A is cancelled or fails; only A stays pending, and
operator resolution of A cannot replace B. Recovery assembles original call order
without restarting Model or replaying tools. Session's persisted V1 format and
its existing reconcile operation are unchanged. Receipt -> Session/Storage and
Contributor -> Receipt dependencies are acyclic, with explicit selected imports
and grants, not inferred permissions from tool names or instance membership.

The existing V2 gate now contains native and packaged invalid-argument, matching
active-batch malformed collection, and B-success-then-A-cancel/infra cases. Package
recovery inspects persisted per-call records and Session pending membership across
a fresh host process. These pass the exact-head gate above. This is scripted native
and packaged JSONL acceptance, not full V2 ACP/Neovim/provider parity. Local checks
remain formatting and static Python/TOML/Nix validation, with no compilation.

The packaged JSONL cancellation acceptance is distinct from the controlled native
fixtures and does not prove arbitrary direct kernel shutdown during pending work.
A dependent deferred-gap recovery
increment admits a new run only after a fresh authorized host finalizer has
durably marked the same old run Cancelled with no pending tool effects. Cancellation
callbacks gain no write authority. The deterministic gap fixture explicitly uses
that manual host finalizer; it is not packaged deferred-gap scheduling evidence.

The next acceptance milestone is recursive InvokeEach placement through repeated
source-free occurrences, globally attested routes and original-owner grants,
exact effective-ID entry/exit delivery, shared-provider serialization and physical
drain. Normative requirements and executable source assertions are present in the
dependent placement increment; native execution remains pending its exact-head gate.

The five C1 bundles are tracked independently in v2.4 section 7.12. Typed
boundaries, Flow/Order/Deferred semantics and public/private anchor resolution
have frozen contracts and exact-head acceptance for this trusted-host profile.
Complete implemented-transformation provenance and placement/ownership/security
remain open until the recursive invocation placement gate passes. This profile
does not apply patches; actual patch application/transformation is future V3.
Scoped concurrent state, durable graph recovery and real V2 ACP/Neovim/provider/auth
parity remain open.

CI cost evidence: expensive local builds were zero. The #1209 planner contains
89 checks and 3,353 unique closure derivations, not 3,353 rebuilt outputs. #1209
and #1210 each scheduled 454 unique remote derivations; 419 derivation paths were
identical across the two runs. GitHub PR cache scopes prevent cross-PR archive
reuse, while the existing shared publisher excludes PR events. Changing that
requires an explicit trusted publication policy and approved destination/read
configuration, with provider changes merged before consumer pins are regenerated.
No unchanged-SHA reruns were requested; superseded known-failing coordinator runs
were cancelled, and warning/security/shipping gates were not weakened.

Integrated base/initial-wave scope: #1172 merged at
`12e6f7becd0d3cd0ee1a63aec8b7b4ff4acb3c64` after all five exact-head flake
shards and aggregate, official ACP SDK, cache isolation, formatting and workflow
generation passed at `1af1abfca4cc1f9bdf8972362e05f01f22324be6`
([matrix run](https://github.com/matthis-k/phenix-ai/actions/runs/40496223602)).
Local Nix `m1-graph`, `m1-pending`, `m1-imports`, `basic-history-verification`
and `basic-streaming` were rerun successfully at the actual merge commit.
This verifies the named slice and preserved current P1 fixtures, not full V0/V1
or graph-v2 P1 acceptance. Full local `nix flake check -L --keep-going` exceeded
20 minutes; the completed remote matrix supplies the full-gate evidence.

V2 continuation (merged): Switch and Merge::ExactlyOne shipped via #1173 on the
same private wave/GraphExecution machinery. Native fixtures exercise text/unit
case routing, cross-source convergence, pending candidate waiting, late failure
precedence, multiple-value rejection and all-closed termination; focused tests
include direct wire negatives, whole-input classification, selected-resource
attenuation and confidential control outcomes. General classified observation
remains open.

CI continuation for #1173: the user-selected single-store full-DAG backend
replaces only the automatic PR matrix execution, not its complete check
inventory or the independent ACP/security gates. Provider
`phenix-flake-ci` #33 merged at
`25029578a370c0b1f57ae88881b6018ed1ac08df`; reusable workflow/runtime pins are
updated together to that merged revision. Trusted-main remote execution keeps
its existing pin and authority. The stale matrix-pin policy test was updated
with adversarial wrong-pin, OIDC, publisher and reduced-inventory cases. The
final #1173 head `64f6274a3ad025d1440eedd354a52d99dd0df894` passed the complete
single-store full-DAG run before merge; the superseded `acf46e51f` head's earlier
policy failure is retained only as history.

Join continuation (merged via #1175): named One inputs assemble
one canonical struct, with conservative output security and intersected resource
scope. Offline compilation rejects independent-only and direct Switch-exclusive
combinations, diagnoses dormancy and reports Unknown for unproven composed
paths. Native fixtures passed pending fan-in, actual field delivery, no early
consumer invocation, required closure followed by late success/failure, producer
non-cancellation and physical draining. Private terminal bookkeeping releases
payloads for non-runnable joins instead of retaining late successes. Patch
algebra, deferred waves, SDK/lowering and graph-v2 P1 parity remain open. The
user's single-store CI is inherited; no second test inventory or local full-flake
rebuild is introduced.

Join review correction in #1175: late discarded successes must not attempt
destination data access. Outgoing closures now resolve before selected payloads,
eliminating edge-order-dependent denial when the same producer closes another
required Join input. A native cross-plugin regression narrows away the late
value's destination resource and still requires successful producer completion,
zero consumer calls and physical drain. Runnable recipients retain independent
denial checks. This is not a grant exemption for actual deliveries.

Integrated operator scope: Switch and Merge::ExactlyOne merged via #1173 at
`89c6f7f5c83e29819a05062f22d1333ad22d6093`, and named-One Join::All merged via
#1175 at `52b1c3261db182b9f595aac85d48fc238bddb83c`, both after the user-selected
single-store full-DAG CI passed at their exact heads. The single-store backend
replaces only automatic PR matrix execution, not the complete check inventory or
the independent ACP/security gates. Trusted-main remote authority is unchanged.

Readiness continuation (follow-up on the merged Join base): Optional wraps a raw
candidate as `Some`/`None` and StaticMany collects a finite candidate set in
stable EdgeId order, closing on under/overflow and letting active failure
dominate closure. `InputPort.schema` is the complete consumed value; edges match
its unwrapped candidate schema and Invoke still consumes one complete request.
Readiness never creates a source: Flow-only relevance gates absence activation,
so disconnected/Order-only Optional and min-zero nodes stay dormant. Synthesized
absence carries kernel-owned causal evidence intersected with the effective root
and prepared flow grant; it never restores root scope, and an explicit empty
resource set stays distinct from missing authority. Native fixtures cover
Some/None, nested `Option<Option<T>>`, ordered/under/overflow collections,
pending terminality, failure precedence, dormancy and narrowed-resource denial;
exact-head single-store CI and full V2 acceptance remain required. Deferred
waves, patch algebra, SDK/lowering and graph-v2 P1 parity stay open.

Remaining architectural question: repository documents still label v2.4 a
proposal; this implementation follows the requested target without changing
that review status or claiming approval of unfinished interfaces. Source-ceiling
helpers require trusted callers and do not turn plugin self-description into
authority. No graph-v1 path or persistence implementation is removed.

V0 security foundation evidence: [#1172](https://github.com/matthis-k/phenix-ai/pull/1172),
implementation commit `a9087986f366f9a7c8a77d29d199edccafb23176`.
At that exact commit, `cargo test -p phenix-contract` passed 16 unit and four
integration tests inside `nix develop`; warnings-denied all-target contract
Clippy, new-module Rustfmt and
`nix build .#checks.x86_64-linux.m1-reference-wire --no-link -L` passed.
`devenv test` could not start because the executable is unavailable.
`nix flake check -L` evaluated 231 checks but exceeded the local command limits
on both attempts (200 and 600 seconds). This is not full-gate acceptance.
Remote exact-head CI and review remain required; no merge or P1 parity is claimed.

Base-plan milestone: `c11fe1a226a24b529a45366a5b170b21a722dcfb`.
Native delivery milestone: `72933c2a28f202bbd9234c1f42a619f5f7dbe057`.
At the latter exact commit, focused contract/artifact/kernel tests (25 + 2 + 36
unit tests and four contract integration tests), warnings-denied Clippy and
workspace formatting passed. Nix `m1-graph` passed the existing native graph-v1
suite plus the initial-wave v2 fixture. Nix `m1-pending`, `m1-imports`,
`basic-history-verification` and `basic-streaming` also passed. The product checks
prove preserved current P1 behavior in their selected fixture scope, not v2 P1
parity. Follow-up native tests cover authorized reference delivery with denied
dereference and kernel failure remaining outside domain `err` routing; their
exact committed-head verification is recorded in #1172. Full V0/V1 acceptance,
classified observation, later operators/waves and product migration remain open.

P1 is accepted on `main` at `68f46f63`; `oldmain` remains available.
The tested CI migration [#890](https://github.com/matthis-k/phenix-ai/pull/890)
merged at `b7017042`. Its first authoritative 4×2-builder release,
independent official ACP SDK interoperability, Rust formatting and
generated-policy checks all passed on that main commit.
See [the C4 evidence ledger](ci-post-p1-cutover.md) for exact run links,
the 38-check cold/warm comparison and trust policy. GitHub ruleset
configuration is optional administration, not a CI or merge prerequisite.

Post-P1 code has resumed. [#886](https://github.com/matthis-k/phenix-ai/pull/886)
merged the opt-in deterministic Basic session-history product at `b96182d5`
after its full PR matrix, DAG, official ACP SDK and dedicated verification
passed. The latest production run on the resulting main commit is separate
release evidence. [#892](https://github.com/matthis-k/phenix-ai/pull/892)
merged at `d674b366` after exact-head CI. It publishes the composition-derived
ACP model binding and replays accepted user prompts through standard ACP
notifications. Model selection mutation remains separate.
Follow [runtime management #891](https://github.com/matthis-k/phenix-ai/issues/891)
and [multi-client projection #852](https://github.com/matthis-k/phenix-ai/pull/852),
then the existing S5/F3 reactive, K1-K3 operations, L1/I1 observability
and Advanced agent dependencies. Keep client UI state derived from runtime
state and use established protocol libraries.

Older per-stage entries below retain development-phase evidence where
milestone-local acceptance was narrower than the final P1 release. The
current integrated P1 and CI acceptance statements above supersede their
pre-integration projections.

### Current merged-code reconciliation (20 October 2026)

This section is the current implementation status, not a new architecture or a second acceptance tracker. Initially checked runtime `main` at `e6228d51` (#1022 merged), then reconciled later merges #996/#1026/#1027/#1031; the older #890/C4 acceptance and #886/#892 references above are **historical evidence**, not pending work. When a dated PR description says `draft` or `pending`, use the PR's **current** GitHub merge state and exact-head acceptance results instead. Merge state does not itself prove a broader end-to-end feature.

- **Completed plugin source preparation:** #977 added the single v3 composition and source/builder strategy; #988 added persistent local Cargo reuse (both merged 18 October). Do not queue their implementation again.
- **Provider operations partially integrated:** #958 (idempotent ACP model re-selection), #962 (pure account-bound admission), #1006 (read-only installed provider catalog), #1009 (scoped credential import/revoke), #1010 (auth-status/model queries), #1011 (launcher-granted credential profile), #1012 (offline/local/plugin-contributed providers), #1019 (validated `AdmittedModelChoice`), #1026 (fail-closed explicit HTTP key handling), #1027 (exact credential identity) and #1031 (documented built-in inference endpoints) are merged. Draft #1030 adds the checked HTTP endpoint binding and has been reconciled onto current runtime main, preserving #1026 credential hardening; its new integrated SHA requires full exact-head acceptance before merge. This does **not** prove verified login, live provider-selected HTTP dispatch, durable CAS model switch, or approved OAuth. These remain under #891; read-only registration and stored key bytes do not imply entitlement.
- **Contract invariants:** #1016–#1022 have merged as recorded in #1021. #1023 is a draft cross-crate closed transcript migration, not yet accepted. Do not duplicate those data structures in new provider work.
- **CI:** #996 has **merged** with all ten A–J derivation-isolation scenarios and exact-head product/ACP checks; #1024's shared Basic composition prebuild was **rejected and closed unmerged** after increasing wall time by ~1m44s. #1029 has **merged** after artifact-inspected D/E/F/H/I/J physical edit-build evidence, deletion of the temporary probe workflow and green post-cleanup exact-head checks. The controlled H edit materializes only the changed composition (0.163 s) once the unchanged HTTP plugin is included in the baseline. These do not establish sub-10-minute full CI or signed external binary-cache substitution.
- **Neovim:** original client #46 closed unmerged; #69 merged its *presentation-only* editable queue. Client #78–#85 merged catalog/race/editor protections, #86–#90 merged provider inspection and scoped API-key UX, and #92–#94 merged callback generation isolation, private-buffer read denial and honest unsupported client-tool reporting. Durable input admission and genuine model selection remain new integration slices; do not revive #46 or redo the merged client code.
- **Distribution:** #147 and #148 merged installed read-only provider management plus matching source pinning. D4 is **partially implemented**, not completed; a real authenticated model choice, turn, restart and denial still require runtime/client integration.


## Immediate delivery queue: fast CI to usable Neovim (reconciled 20 October 2026)

This is a **dependency and integration order**, not an alternative implementation
specification or release gate. The existing owner roadmaps and #891 supply
behavioral contracts and exact acceptance tests. Do not introduce a second
protocol, catalog, storage owner, or legacy composition branch to complete it.

| Order | Owning implementation and dependency | Next action | Evidence needed before promotion |
| --- | --- | --- | --- |
| 1. CI and incremental builds | [#986](https://github.com/matthis-k/phenix-ai/issues/986), merged [#996](https://github.com/matthis-k/phenix-ai/pull/996), rejected [#1024](https://github.com/matthis-k/phenix-ai/pull/1024) and merged [#1029](https://github.com/matthis-k/phenix-ai/pull/1029), `phenix-flake-ci` | #996 is merged; #1024 was measured and rejected (~1m44s slower). #1029 physical D/E/F/H/I/J artifact evidence and cleanup are merged. Independently address full-CI wall/runner-minute runtime and remote-cache proof; historical branches remain separately tracked under #1028. | Full Nix/ACP/negative gates, unchanged plugin derivations under isolated edits, actual critical-path, runner-minute and cache metrics; trusted publishing only. |
| 2. Plugin source preparation **(MERGED)** | [#977](https://github.com/matthis-k/phenix-ai/pull/977) and [#988](https://github.com/matthis-k/phenix-ai/pull/988) | **No implementation PR pending for these slices.** Only new, evidenced gaps in source identity, native package validation or cache behavior should be filed separately. | Prior native source/builder and persistent Cargo cache evidence; preserve one v3 composition, not a compatibility reader. |
| 3. Authorized interactive management **(PARTIAL)** | [runtime #891](https://github.com/matthis-k/phenix-ai/issues/891), merged #1006/#1009–#1012/#1019 | Reuse installed catalog and scoped API-key operations. Next: credential-verified selected executable Model transport, persisted revision-CAS effective selection, dynamic account-specific usability, optional supported OAuth, and durable session/control operations. | Fresh auth/selected-provider real Model fixture, revoked/expired/wrong account denial, later-turn selection, restart and two-client revision conflict; no secrets in observations. |
| 4. Standalone Neovim experience **(PARTIAL)** | [client N4–N6](https://github.com/matthis-k/phenix-ai.nvim/blob/main/spec/p1-p2-client-roadmap.md); #46 closed unmerged; #69, #78–#90 and #92–#94 merged | Connect #69's editable queue presentation to **runtime-admitted** follow-ups only when supported. Finish privilege-aware client tools and N4 runtime-authorized selection/verification without recreating merged UI/race/credential-profile work. | No duplicate admission, stale callbacks or forged selection; packaged Neovim restart, secret and denial tests. |
| 5. Distribution D4 **(PARTIAL)** | [distribution roadmap](https://github.com/matthis-k/phenix-nvim/blob/main/spec/p1-p2-distribution-roadmap.md), merged #147/#148 | Keep read-only provider registration and scoped-auth pins; add only next accepted runtime/client SHA pair after upstream gates. | Installed authenticated model, real turn and restart with denied/stale cases; test actual pins and preserve Basic default. |
| 6. P2 Advanced/UI | [Advanced roadmap](p2-advanced-agent-roadmap.md), client C1–C7 and distribution E1–E4 | Resume Advanced composition, negotiated A2UI, workspace/edit/job and independent multi-client views after the usable P1 path. | Existing Advanced fixtures and independent transport/schema tests, while the default Basic package and UI regression stay green. |

Parallelism: CI benchmarking, source builders and unprivileged Neovim
transcript/editor-tool work need not wait for #891; runtime-backed auth,
model mutations and durable queue acceptance do. Merge/pin in runtime →
standalone client → distribution order, verifying the actual integrated
commit at each boundary. Do not mark an individual roadmap row accepted
from tests of a parent PR, a provisional lockfile or a stale branch head.

### Runtime N4 implementation packets, 19 October 2026

These are implementation entry points for existing [management issue #891](https://github.com/matthis-k/phenix-ai/issues/891); its accepted decisions and acceptance cases are unchanged. Split by the first observable end-to-end result. Keep one selection authority in the runtime and one ACP transport in the client.

| Order | Existing code | Smallest verifiable result | Release gate |
| --- | --- | --- | --- |
| R-N4a provider discovery | `crates/phenix-provider-contracts/src/lib.rs`, `crates/phenix-plugin-provider-builtins/src/lib.rs`, `crates/phenix-provider-discovery/src/lib.rs` | An authorized Basic host query returns installed provider/connection/method identities and current catalog provenance. An offline registration does not claim entitlement. | Separate account/connection catalog identities, zero/one/both authentication methods, stale/denied reads; packaged product test |
| R-N4b credential actions | `crates/phenix-provider-contracts/src/credentials.rs`, `crates/phenix-plugin-provider-secret-service/src/lib.rs`, existing opt-in 0600 file provider | A typed, resource-granted host operation imports, resolves and revokes one named API-key profile without journal/config/Neovim secret persistence. ProviderAuth checks actual readiness. | Cross-account and revoked-grant denial, error redaction, restart, `tests/provider-credential-store-product.py` and `tests/provider-secret-service-product.py` |
| R-N4c effective selection | `crates/phenix-provider-contracts/src/selection.rs`, the existing Basic Session/Storage plugins, `crates/phenix-basic-runtime/src/acp_sdk.rs` | Runtime validates current catalog + credential profile, stores an effective scoped selection with revision CAS, and applies it only to later turns. Standard ACP session config reflects the admitted session choice. | Conflict, stale catalog, revoked profile, cross-session/connection spoofing, restart and two-client race. Run `tests/acp-stdio-product.py` and independent ACP SDK interop |
| R-N4d client/distribution | [Client N4](https://github.com/matthis-k/phenix-ai.nvim/blob/main/spec/p1-p2-client-roadmap.md), [distribution D4](https://github.com/matthis-k/phenix-nvim/blob/main/spec/p1-p2-distribution-roadmap.md) | Expose only operations actually admitted by the host. Pin accepted runtime, then client, then distribution; test an installed fake provider before manual live-provider setup. | `:Phenix auth` -> `:Phenix model` -> prompt -> restart, zero secret disclosure and no fallback to a different account |

**N4 evidence boundary (20 October):** R-N4a registered provider/catalog read-only admission is implemented by #1006/#1010; R-N4b exact-resource credential import/revoke is implemented by #1009/#1011, but is not verified provider authentication. R-N4c operational selected-provider Model rebinding, persisted mutation/CAS, and the complete Neovim journey remain unaccepted. The table above describes contract targets and should not be read as all rows being new implementation tasks.

### Provider plugin ownership and legacy removal

A provider enters the runtime only through an installed, selected plugin.
Provider plugins may contribute multiple connections and choose local in-process
models, literal-loopback HTTP, or remote HTTPS. Declared static model lists and
plugin-owned dynamic discovery are both valid. No remote URL is required for an
in-process connection, and no endpoint can be inferred from its protocol name.
Model discovery and authentication remain provider operations, not kernel logic.

A provider descriptor is metadata, not proof of a usable model. Actual model
selection requires a fresh, account-scoped catalog, an authorized Model binding
and a real executable provider. Plugins may expose zero, one or several auth
methods. Explicit composition selects provider contributions; the provider
catalog aggregation work must reject duplicate full provider identities and
unselected bindings. No hardcoded provider allowlist belongs in the kernel,
client or provider contract library.

**Legacy removal gate:** replace `crates/phenix-plugin-model-http/src/request.rs`
environment-driven connection and secret loading, `phenix-basic-http`'s
environment-driven launch defaults and the parallel HTTP-model composition with
the provider-selected execution path. Delete those old branches and their
variables, rather than shipping compatibility aliases, when the new selected
Model provider passes the existing HTTP/ACP streaming, cancellation and
authorization tests. Retain their test scenarios as the new provider product
acceptance. The preexisting code remains until that replacement exists; this
roadmap entry is not implementation acceptance.

The completed gate must demonstrate independently contributed offline and
online provider plugins in one composition, no-auth and API-key connections,
a provider-owned dynamic catalog, deterministic duplicate-ID rejection, typed
unavailable states, remote plaintext and loopback-hostname rejection, and exact
credential-origin binding. A registered but unimplemented model must not
appear as selectable.

R-N4a read-only registration and R-N4b scoped credential actions are now implemented by merged PRs, but actual verified readiness, selected Model execution, and session-scoped CAS remain release gates. The existing provider contract tests and packaged runtime checks must fail on the missing behavior before the corresponding production integration is accepted. [Client #78](https://github.com/matthis-k/phenix-ai.nvim/pull/78) already handles pushed ACP catalog removal; picker invalidation (#79) and provider distribution pinning (#147/#148) have also merged. These prove the named slices, not full R-N4c host mutation or provider-verified authentication. N5 unprivileged Neovim editor integrations may proceed in parallel. Do not treat a passing fixture UI as a completed auth or model-selection product.

### Runtime environment locations

`phenix.environment@1` is an optional plugin import. Its default native
implementation resolves a resource-bound snapshot with a launch cwd hint,
config, state, data, cache and optional runtime directory. Each plugin asks
for its own namespace through `resolve_for(host, "Environment", namespace)`.
This provider computes locations but never creates directories, opens files,
changes process cwd, stores configuration or grants filesystem authority.

On Linux, the default provider follows absolute XDG home variables with
HOME-based fallbacks; it ignores relative variables and does not invent an
XDG runtime directory. The launch cwd is a hint, not an authorized project
root. Workspace selection and changes remain project/session configuration
owned by selected plugins, without `chdir` for the entire process. A
filesystem/tool provider must validate its resource grant and resolve paths
within its admitted workspace before every effect.

The kernel remains unaware of XDG, project filenames, config precedence and
SQLite schema. The selected runtime/project/session configuration providers
own effective values and revision CAS under #891; Storage and ProjectStorage
providers own durable bytes. Plugin packages own bundled assets. The product
host resolves pinned plugin sources to verified native package paths before
passing the one v3 composition to the kernel. Never use paths returned by an
Environment import as a substitute for those artifact paths.

Acceptance for the default provider is a native, separately composed
Environment contract/product test with absolute and relative XDG roots,
optional runtime, denied foreign resource, namespace traversal and zero
filesystem effects. Adoption by Basic SQLite, per-session cwd overrides and
config mutations are separate integration work and are not claimed by
this directory-resolution slice.

## P2 advanced agent

[The P2 advanced agent roadmap](p2-advanced-agent-roadmap.md) owns the detailed
P2-0 through P2-10 stage order and acceptance tests. The Advanced product
extends the accepted P1 kernel and Basic composition with memory, reversible
compaction, deterministic code-entity lineage, plugin-contributed graph
updates, workspaces, durable jobs, session-tree sub-agents and inspection.

P1 and the C4 CI cutover are accepted. P2 implementation now proceeds as
separate tests-first, packaged native plugin slices. Early composition and
source-evidence work do not constitute acceptance of the Advanced product.
The older `oldmain` Advanced implementation is a behavioral reference.
Post-P1 multi-client projection and I/O remain a separate roadmap track.

P2-6A adds architecture decisions backed by project-scoped cross-session
memory. Its acceptance cases AV-35 through AV-41 cover deterministic,
source-backed design decisions, supersession and recovery across sessions.
The Architecture plugin can invoke existing Skills, Context, Git and Workspace
facilities for optional grilling, glossary/ADR capture and vertical ticket
slicing. This is not a second architecture CI gate: the compiled contracts
and runtime remain authoritative; functional and denial tests own acceptance.
Model-based design-quality benchmarks remain optional, outside CI.

## Consolidation intervention

The 12 October decision makes M1 the sole supported product after consolidation.
[The consolidation contract](m1-consolidation-contract.md) supersedes historical
compatibility requirements for this transition. [The verified audit and file
matrix](m1-consolidation-audit.md) record corrections, executable changes and
remaining removal work. M0 acceptance below is historical evidence. C1 and C2
must replace descriptors and their consumers together, while migrating semantic
conformance to M1. S3b pending imports and G1 follow that coordinated removal.
PR #779 implements prepared native/cell effects, shared ordering and concise
stateless handlers. Its exact head `712b9898` passed full CI 38193609248.
The `impl/m1-only-runtime` transition now removes legacy schemas, descriptors,
wire/ABI profiles, macros, synthetic normalization and preparation input forms.
[Its frozen contract](m1-only-runtime-contract.md) owns the coordinated C1/C2
migration. Local M1 native, lifecycle, pending/import, generation, state, SDK,
authoring and agent suites pass. Independent wire, metadata and C ABI gates pass.
Consolidation passed exact-head full Nix CI 38197645494 at `367420e33b`.
S3b completion adds property discovery, sealed pending slots and adversarial
import tests. It passed full CI 38205255204 at `75a255b9`;
[its matrix](m1-contract-authoring-matrix.md) binds the downloaded artifact.
G1-G4 now follow the accepted consolidated kernel. Historical profiles and acceptance claims below describe
their pinned commits, not the active product.

## 1. Branch and PR ownership

PR #771 remains the unchanged M0 implementation and validation baseline. The S0 specification PR #772 is stacked on `rewrite/minimal-kernel` and includes documentation only. The separate implementation PRs below own their executable changes and must not amend M0 acceptance claims or rewrite implementation history.

Each numbered stage below owns one **focused implementation PR** after the specification is accepted. Do not open empty placeholder PRs. Use the agreed base branch and add actual executable changes before opening. Mark implementation PRs ready for review to trigger full CI; keep them unmerged until the requested integration decision. Stage-dependent PRs may be stacked, but their dependency and base branch must be explicit.

| ID | Proposed PR scope | Depends on | Exit gate | Status |
| --- | --- | --- | --- | --- |
| S0 | M1 RFC, this roadmap and M0-to-M1 links | #771 head | Consistent contract and file-level scope | Spec authored; review pending |
| S1a | Portable schema/ref identities and explicit v2 wire profile | S0 | Independent Python/Rust vectors and M0 regression gates | PROVEN at `2682be5f` in CI 38125329024 |
| S1b | Definition-only contracts, property/import metadata and negotiated state ABI | S1a | Metadata-only package and independent C/native ABI vectors | PROVEN at `51017e07` in CI 38128644908 |
| S2 | Kernel-owned ValueRef cells, CAS, subscriptions, transactions | S1b; stacked on #775 | External native state and security fixtures | PROVEN at `6ce818c4`, CI 38170397949; `m1-live-state-matrix.md` |
| S3a | Typed Rust value SDK and static schemas | S2 | Independent vectors, native host fixture and compile failures | PROVEN at `b47f7d95`, CI 38170898637; `m1-value-sdk-matrix.md` |
| S3b | Contract/property macros and generated ABI | S3a | External SDK-only annotated plugin | Accepted at `75a255b9`, CI 38205255204; generated native metadata, ABI 3, typed fields/slots, pending imports and property/security completion proven |
| S4 | Contributed graph descriptor, resolve, validation, identity | S3b | Independent plugin contributions compile deterministically | Refined into G1/G2: shipping CI passed, remaining acceptance cases open |
| S5 | Serial readiness execution and reactive on-change integration | S4 | Correct multi-root DAG partial order and cancellation; typed macro-generated change/event handlers with no manual subscription plumbing, proven by independent plugins | G3/G4 slices implemented; committed-state triggers and declarative hook authoring remain unaccepted |
| S6 | Parallel-ready tasks and multi-admission concurrency | S5 | Independent branches run concurrently, safe instance lifetimes | Not started |
| A0 | Basic synchronous agent plugin preflight | Accepted M0 services; S1b branch baseline | Replaceable model/tool imports and real CLI gate | PROVEN at `4d47e783` in full CI 38134340209; `spec/a0-agent-matrix.md` |
| Basic | Triggered agent, providers, durable sessions and frontends | G4 and Basic providers | Terminal, JSONL and ACP run the same agent | Partial implementation; B1-B5, F1-F2 and P1 below own evidence and remaining gates |

S1/S2/S3 may refine their Rust API shape based on conformance evidence, but MUST maintain the semantic boundaries in the M1 RFC. S6 is a distinct optimization/capability gate: S5 must implement the correct partial order without relying on parallelism.

## 2. S1: contract and wire freeze

S1a owns the [frozen portable reference wire contract](m1-reference-wire-contract.md), `crates/phenix-contract/src/reference.rs`, `crates/phenix-contract/src/value.rs`, the contract dependency/lockfile updates, `tests/m1-contract-vectors.json`, `tests/m1-wire-negative.py`, and the public-contract probe. `flake.nix` and the conformance workflow own its executable gate. This split proves canonical bytes before live state callbacks can use them.

S1b depends on S1a and owns definition-only metadata, property/import-slot descriptors and the separately negotiated state-host ABI. The table below names the combined S1 scope. Freeze each profile before runtime code.

| File | Required change |
| --- | --- |
| `crates/phenix-contract/src/lib.rs` | Stable `SchemaId`, `ValueId`, ref/error vocabulary and contract role definitions |
| `crates/phenix-contract/src/value.rs` | A version-negotiated reference-capable portable encoding; keep v1 semantics frozen |
| `crates/phenix-contract/src/descriptor/mod.rs` | Definition-only contracts and typed property/import-slot metadata with ownership |
| `crates/phenix-contract/src/descriptor/encode.rs` | Canonical descriptor bytes and deterministic contribution order |
| `crates/phenix-contract/src/abi.rs` | Versioned state-host API extension proposal with explicit length/ownership |
| `include/phenix.h` | Exact C ABI declaration for the negotiated extension |
| `crates/phenix-artifact/src/lib.rs` | Verify versioned metadata-only packages offline, with separate package and descriptor digests |
| `crates/phenix-artifact/src/bin/phenix-describe.rs` | Extract and inspect definition-only metadata without loading executable code |
| `examples/reactive-contracts/` | SDK-only contract definitions and generated offline metadata fixture |
| `tests/m1-contract-vectors.json` (new) | Independent canonical byte and identity fixtures |
| `tests/m1-wire-negative.py` (new) | Malformed, forged, noncanonical and incompatible encoding cases |
| `spec/m1-values-reactive-dag-rfc.md` | Record frozen discriminants and compatibility decision when proven |

Gate: independent decoder/encoder agrees on reference/descriptor vectors; a metadata-only contract package verifies without dlopen; malformed references and duplicate/conflicting property declarations reject. The M0 conformance profile still passes. Do not accept a new ABI merely because a Rust unit test round-trips.

### S1a implementation evidence

The frozen contract commit is `c2365e1199b8bf15184f8e7246f15d853f2fe366`. [PR #773](https://github.com/matthis-k/phenix-ai/pull/773) owns the implementation on `impl/m1-reference-wire`, stacked on `spec/m1-values-reactive-dag`.

S1a is PROVEN at implementation commit `2682be5f1b7293a8a3305d5b09186821b4167305`. [CI run 38125329024](https://github.com/matthis-k/phenix-ai/actions/runs/38125329024) passed all fifteen checks, including the fourteen unchanged M0 gates. The downloaded artifact's `kernel-commit.txt` matches that commit. Its wire report confirms 52 accepted and 148 rejected independent cases; the vector digest matches the frozen fixture. [CI evidence](evidence/m1-reference-wire-ci.json) records the exact Nix outputs and hashes of 27 JSON reports. [Local evidence](evidence/m1-reference-wire-local.json) records source hashes, isolated SDK tests and optimized wire tests.

The first CI attempt stopped on Clippy 1.96 `collapsible_match`. The corrected expression passes the pinned toolchain. Reference payloads use a Box so portable scalar storage stays 32 bytes on the frozen target; location data takes 112 bytes. This changes no wire bytes.

This acceptance covers portable shape, identities and compatibility. S1b owns contract metadata and the state-host ABI. S2 owns authenticated cells and their lifecycle.
| File or task | State | Evidence or remaining gate |
| --- | --- | --- |
| `crates/phenix-contract/src/reference.rs` | PROVEN | Distinct identity types, structural schema hashing, validated `DynValue`; independent schema and mismatch cases |
| `crates/phenix-contract/src/value.rs` | PROVEN | Explicit v2 selection, nested refs, v1 refusal and all reference truncations |
| Contract manifest and both Cargo lockfiles | PROVEN | Pinned SHA-256 dependency; workspace and external examples compile |
| `crates/phenix-contract/src/bin/phenix-value-probe.rs` | PROVEN | Public-contract process, no private kernel dependency |
| `tests/m1-contract-vectors.json` | PROVEN | Fixed wire bytes and schema identities agree with independent Python |
| `tests/m1-wire-negative.py` | PROVEN | 52 accepted and 148 rejected cases |
| `flake.nix`, `.github/workflows/minimal-kernel.yml` | PROVEN | Exact implementation-head CI passed all fifteen checks and uploaded release evidence |
| Owner/cell issuance, live authority and lifecycle | NOT STARTED | S2 owns these; decoded locations grant no access |
| Definition-only descriptors, slots/properties and state-host ABI | PROVEN | S1b accepted at `51017e07`; see independent CI evidence below |

### S1b implementation and acceptance gate

The [frozen S1b profile](m1-metadata-state-contract.md) is commit
`295067bbcc2ea78f13cb26c68351080cacc1d0a5`, created before implementation.
[PR #774](https://github.com/matthis-k/phenix-ai/pull/774) on
`impl/m1-metadata-state` is stacked on PR #773. S1b is PROVEN at implementation
commit `51017e07067bae786131775e18686044b78eb625`. [CI run 38128644908](https://github.com/matthis-k/phenix-ai/actions/runs/38128644908)
passed all sixteen checks, including all fourteen M0 gates and S1a.
The downloaded [artifact 11698043204](https://github.com/matthis-k/phenix-ai/actions/runs/38128644908/artifacts/11698043204)
records that exact commit, 1,544 independent cases (23 accepted, 1,521 rejected),
12 package cases, and native C/Rust interoperation with 17 rejected tables.
[CI evidence](evidence/m1-metadata-state-ci.json) records the realized Nix outputs,
archive digest and hashes of all 30 JSON reports. No live cell, provider
selection, field macro or graph execution is claimed.

Optimized standalone linkage needed ThinLTO. Both initial CI attempts then
exposed Cargo's separate target libraries and host proc-macro libraries. The
corrected search paths reproduce locally with Cargo's explicit target layout;
the final pinned Nix run proves the correction.

| File or task | State | Evidence or remaining gate |
| --- | --- | --- |
| `crates/phenix-contract/src/descriptor/metadata.rs`, `descriptor/mod.rs` | PROVEN | Typed definitions, named cardinality slots and plugin-owned properties; canonical ordering and duplicate rejection |
| `crates/phenix-contract/src/state.rs`, `lib.rs` | PROVEN | Exact state request opcodes, v2 envelopes, sorted batch identities and domain error shapes |
| `crates/phenix-contract/src/abi.rs`, `include/phenix.h` | PROVEN | ABI 3 header-first negotiation, initialization callback, v2 owned replies and unchanged M0 decoding |
| `crates/phenix-artifact/src/lib.rs`, `bin/phenix-describe.rs` | PROVEN | Offline definition-only verification, independent descriptor/package digests, containment and no native artifact |
| `crates/phenix-artifact/src/bin/phenix-metadata-probe.rs` | PROVEN | Process boundary for independent malformed descriptors and state envelopes |
| `examples/reactive-contracts/`, examples manifest/lockfile | PROVEN | Public SDK only; reversed authored declaration order produces canonical Python bytes |
| `tests/m1-metadata-vectors.json`, `tests/m1-metadata-negative.py` | PROVEN | Frozen independent vectors, metadata/schema/cardinality/property negatives, all truncations, state requests/errors and 12 package cases |
| `tests/m1-state-abi.c`, `tests/m1-state-abi.rs` | PROVEN | C producer/Rust host interoperate; 4 host callbacks, 10 C/3 Rust releases, 17 rejected tables and incompatible reply bodies |
| `flake.nix`, conformance workflow | PROVEN | All sixteen gates passed in CI 38128644908; both M1 products uploaded |
| `descriptor/encode.rs` | UNCHANGED | Frozen const v1 encoder stays intact; canonical M1 encoding belongs to the owned metadata module; macro lowering is S3 |
| Native metadata packages and runtime admission | NOT STARTED | S2 integrates ABI 3 with the existing native loader/resolver; this gate accepts offline definition packages only |
| `spec/evidence/m1-metadata-state-local.json` | PROVEN | Source hashes and debug, optimized and explicit-target layout reports |
| `spec/evidence/m1-metadata-state-ci.json` | PROVEN | Exact commit and run, independent reports, realized Nix outputs and 30 report hashes |

## 3. S2: kernel-owned live cells

Owner: one S2 implementation PR, no reactive DAG. Native package verification,
ABI 3 admission and selected slot normalization are prerequisites for its external
state fixtures and belong to S2. S4 extends that same prepared representation
with graph contributions; it must not introduce another resolver. The S2
addendum must freeze initialization outcomes, state grant/resource mapping,
owner incarnation issuance and subscription/ticket ownership before mechanisms.

| File | Required change |
| --- | --- |
| `crates/phenix-kernel/src/value_state.rs` (new) | Cell ownership, revision, snapshot, CAS and atomic multi-cell commit |
| `crates/phenix-artifact/src/lib.rs` | Verify native M1 packages and bind canonical metadata to exact executable bytes before loading |
| `crates/phenix-kernel/src/native.rs` | Negotiate ABI 3 in the existing loader; create then initialize before admission, preserving instance/stop ownership |
| `crates/phenix-kernel/src/resolve.rs` | Normalize M1 definitions and selected named slots into the existing resolver/dispatcher; no parallel metadata resolver |
| `crates/phenix-conformance/src/bin/phenix-m1-state.rs` (new) | Independent native state effects, denied-effect counters, transaction and retirement traces |
| `crates/phenix-kernel/src/lib.rs` | Export narrowly scoped trusted APIs, not unfenced raw cell access |
| `crates/phenix-kernel/src/runtime.rs` | Reuse central dispatcher for read/write/watch and authenticated caller scopes |
| `crates/phenix-kernel/src/pending.rs` | Subscription tickets, cancellation, coalesced notification cursors and leases |
| `crates/phenix-contract/src/scope.rs` | State resource restrictions if operation/resource grants cannot express them |
| `crates/phenix-kernel/src/generation.rs` | Owner-incarnation and retirement behavior for live refs |
| `examples/reactive-provider/` (new) | Independently compiled provider with typed cells |
| `examples/reactive-client/` (new) | Independent authorized/unauthorized accessor and subscriber |

Gate: two refs clone the same identity; old snapshots remain unchanged; racing CAS commits produce one winner; atomic read+subscribe has no lost update; batch publish is all-or-nothing; stale owner and forged ref reject; denying write yields zero provider effects; abandoned subscriptions release leases. Run M0 tests unchanged.

### S2 implementation gate

The frozen live profile is commit `80a40cc86f41d3e3c464685699251e861e4f13e8`.
The consistent epoch snapshot import was frozen separately at
`0690c37d578782fe6b865235132f6bb29938e797` before its mechanism.
`impl/m1-live-state` is based on the accepted A0 head so the basic loop gates stay
in every regression run; its functional prerequisite remains accepted S1b.
The [S2 matrix](m1-live-state-matrix.md) owns file-level evidence and remaining
acceptance. Native ABI 3 callbacks use the same loader, operation/resource
policy check, resolver, generation manager and pending tickets as M0. The
[Basic parity checklist](basic-agent-parity.md) records the original group target.

## 4. S3: Rust SDK, macros and ergonomic plugin state

S3 is split before implementation: S3a owns typed values, static schemas and
SDK state access; S3b owns generated contract/property authoring and ABI glue.
The dependency and file ownership are frozen in `m1-value-sdk-contract.md`.
Neither split changes the S4/S5 order or the complete S3 acceptance gate.

| File | Required change |
| --- | --- |
| `crates/phenix-sdk/src/lib.rs` | Typed `Value<T>`, `ValueRef<T>`, `Snapshot<T>`, `Edit<T>` and async/ref host methods |
| `crates/phenix-macros/src/lib.rs` | Canonical `#[plugin]`, `#[contract]`, `define/require/provide` and field-helper parsing |
| `crates/phenix-contract/src/descriptor/mod.rs` | Only schema changes required by actual generated contracts; no duplicate registration |
| `examples/reactive-contracts/` (new) | Public API crate with generated property symbols |
| `examples/reactive-provider/` | `#[get]`, `#[set]`, `#[watch]` on authored plain Rust fields |
| `examples/reactive-client/` | `Value<T>` trait ergonomics, remote refs and denied field access |

Gate: `Value<T>: Clone + Deref + AsRef<T> + From<T>` where bounds allow; `ValueRef<T>: Clone`, but no `Deref` or `DerefMut`; `Edit<T>` supports ordinary `Vec::push` and explicit, fallible commit. Macro-generated state storage has no silent direct write path. Standalone SDK builds without private kernel sources. Compile-fail fixtures check duplicate exported IDs, invalid field types, mismatched contracts, undeclared imports and forbidden mutable ref deref. Definition-only plugins need no state or export.

## 5. S4: multi-source graph preparation

Owner: one S4 implementation PR. Graphs remain inert until S5.

| File | Required change |
| --- | --- |
| `crates/phenix-contract/src/descriptor/mod.rs` | Graph node, edge, trigger, join/failure declaration schemas |
| `crates/phenix-kernel/src/graph.rs` (new) | Composition, activation-independent DAG validation, stable node IDs and conflict reports |
| `crates/phenix-kernel/src/resolve.rs` | Resolve graph service imports through the existing selected binding map |
| `crates/phenix-kernel/src/generation.rs` | Include canonical graph inputs, bindings and policy in generation identity |
| `crates/phenix-macros/src/lib.rs` | Generate static hook/edge contributions from `#[on_change]` and `#[after]` |
| `tests/m1-graph-vectors.json` (new) | Permutation-invariant deterministic graph identities |
| `examples/graph-search/`, `examples/graph-context/` (new) | Contributions from independent plugin artifacts |

Gate: `A -> C <- B` and disconnected `A -> C, B -> D` compose identically regardless of contribution enumeration; duplicates, stale anchors, ambiguous bindings, cycles, unauthorized edges, conflicting writes, and undeclared operations fail before graph activation. Dependency order is validated separately from plugin start/stop order. No plugin callback runs during graph resolution.

## 6. S5: serial DAG execution and reactive propagation

Owner: one S5 implementation PR. Reuse one kernel runtime and central dispatcher.

| File | Required change |
| --- | --- |
| `crates/phenix-kernel/src/graph.rs` | Compute affected/activated set and per-epoch predecessor counters |
| `crates/phenix-kernel/src/runtime.rs` | Execute ready nodes using the existing authorized dispatcher under one admitted root |
| `crates/phenix-kernel/src/pending.rs` | Associate pending tickets with nodes, failure/cancellation, and join settlement |
| `crates/phenix-kernel/src/value_state.rs` | Publish committed epoch notifications into selected graph executions |
| `crates/phenix-sdk/src/lib.rs` | Typed change context and on-change invocation access |
| `examples/graph-search/`, `examples/graph-context/` | Executable hook/consumer end-to-end tests |
| `tests/m1-graph-order.py` (new) | External trace assertions for ordering and dormant predecessor behavior |

Gate: A and B complete before C if both active; no global barrier for `A->C`, `B->D`; only A active does not wait for B; `after` alone does not trigger a node; transactional updates trigger one epoch; dependent failure never masquerades as successful completion; cancellation and retirement observe existing leases; no duplicate executor. Serial topological order is an allowed implementation, not a cross-branch promise.

### S5 declarative reactivity and macro ergonomics

S5 is incomplete if `ValueRef<T>::watch` and `Watch::next` work but each plugin
must still register, poll, dispatch and clean up subscriptions itself. Extend
the existing `#[plugin]` macro and typed SDK to let an independently authored
plugin declare reactions. Prefer `#[on_change(...)]` for committed properties
and `#[on_event(...)]` for typed semantic events. These spellings are proposed,
not implemented or frozen. Generate registrations, metadata, typed callback
bindings and lifecycle cleanup from the declarations. Reuse the existing
resolver, graph executor, dispatcher, pending tickets and authority checks.

The generated API must preserve two distinct delivery contracts:

- **State changes:** a committed `ValueRef<T>` update activates an authorized
  handler with a typed snapshot, revision and epoch. Coalescing is permitted.
  Atomic read/subscription bootstrap prevents missed commits; writes made by a
  handler start a new epoch rather than reentering the current callback.
- **Semantic events:** typed event handlers consume ordered events with stable
  identities and session/run correlation. Progress and tool events cannot be
  silently coalesced or dropped. Durable replay and observer acknowledgements
  remain the EventJournal plugin's responsibility; reconnects may redeliver
  events, so consumers must handle at-least-once delivery.
- **Lifecycle and authority:** declaration metadata specifies source bindings,
  schemas, required grants and ordering. Generated bindings respect selected
  providers and the admitted root, and release watches/leases on cancellation,
  generation retirement and plugin shutdown. Denied sources cannot invoke a
  handler. A source with no permission is not implicitly subscribed.

Acceptance: compile two independently packaged plugins against the public SDK.
One exposes a watched property; the other declares an `on_change` handler and
an ordered `on_event` handler. Exercise committed changes, simultaneous
changes, event ordering and replay, cross-session denial, errors, cancellation,
and owner retirement. The plugin source must contain no manual watch polling,
subscription registration, raw ABI callbacks or custom dispatch loop. Generated
code uses the single kernel executor. This authoring gate does not add a new
P1 prerequisite; the post-P1 F3 gate applies it to real frontends.


## 7. S6: concurrency and independent branches

Owner: one S6 implementation PR. Do not fold the runtime ownership refactor into S5.

| File | Required change |
| --- | --- |
| `crates/phenix-kernel/src/runtime.rs` | Replace serial root/thread constraints with synchronized admission and execution state |
| `crates/phenix-kernel/src/pending.rs` | Wake/cancel/ticket ownership safe under multiple concurrent tasks |
| `crates/phenix-kernel/src/native.rs` | Explicit plugin-instance concurrency/reentrancy policy and lifetime safety |
| `crates/phenix-kernel/src/generation.rs` | Concurrent root pins, promotion/rollback, safe physical retirement |
| `crates/phenix-contract/src/abi.rs`, `include/phenix.h` | Versioned cross-thread call semantics as required; do not repurpose M0 ABI |
| `tests/m1-concurrency.py` (new) | Independent overlap, join ordering, cancellation and old-generation residency fixtures |

Gate: independent A and B execute concurrently when both providers advertise safe concurrency; C waits for both; unrelated `A->C` and `B->D` branches do not serialize due to a global root lock. Stateful serial-only providers remain serialized, with explicit diagnostics. Concurrency never bypasses root-scope authentication, cancellation, resource grants, or physical leases. Noncooperative trusted native work still has no forced-termination guarantee.

## 8. Working Basic agent roadmap

The 11 October Basic roadmap replaces A1a/A1b's direct-call product plan.
A0 remains an accepted synchronous model-tool-model fixture. It does not prove
triggered execution, full history, durable sessions or frontend parity.

Frontends submit typed events. Nodes declare triggers. The kernel admits events,
resolves the selected graph and invokes nodes through its existing dispatcher.
Frontends observe ordered output events. Product startup never calls a special
`Agent::run()` entry. The kernel owns authority, generation selection, readiness,
cancellation and native lifetimes. Plugins own agent and frontend semantics.

Status is an acceptance checklist: `[x]` means proven on an exact implementation head; `[ ]` means further acceptance work. Passing CI is necessary but does not alone close a semantic gate.

| Stage | Depends on | Concrete ownership | Acceptance | State |
| --- | --- | --- | --- | --- |
| S3b | S3a | `phenix-macros/src/`, `phenix-sdk/src/live.rs`, `phenix-contract/src/descriptor/`, `phenix-artifact/src/lib.rs`, `phenix-kernel/src/native.rs`, `pending.rs`, `runtime.rs`, external examples | SDK annotations generate complete native metadata, fields, typed imports and ABI 3 hooks; independently built provider/decorator/field-only/pending caller | [x] PROVEN at `75a255b9`, CI 38205255204 |
| G1 | S3b | Contract descriptors, macros, SDK trigger contexts, narrow graph inspection API | Offline typed event/trigger declarations and discoverable action IDs, schemas, descriptions and required authority; duplicate IDs, invalid schemas and undeclared handlers reject | [x] PROVEN at `fbe55d3d`, full CI 38231650549; definition-only, owner/schema/write and native active/dormant cases |
| G2 | G1 | New `phenix-kernel/src/graph.rs`, existing `resolve.rs`, `generation.rs`, contract descriptors | Static multi-source multi-root DAG; discovery-order-independent graph; cycles, missing anchors, ambiguous bindings, denied contributions and conflicting writes reject before activation; preparation executes no plugin code | [x] PROVEN at `fbe55d3d`, full CI 38231650549; definition-only, owner/schema/write and native active/dormant cases |
| G3 | G2 | `graph.rs`, existing `runtime.rs`, `pending.rs`, SDK trigger contexts | Authenticated event activates two external nodes in declared order and a third reads the resulting state; frontend never invokes handlers directly | [ ] TESTED LOCALLY: follow-up on #785 fixes resource grants, implements Skip and transitive suppression, observes correlated serial results, and bounds delegation by root resource grants; serial shipping CI 38228548634 passed at `672b7f09`; definition/native dormant CI 38231650549 passed; generation-aware admission context remains |
| G4 | G3 | Existing tickets, cancellation, leases and live values through one same-root control pump | Pending model emits progress; cancellation revokes child effects; retained leases drain; committed changes activate the same DAG | [ ] PARTIAL: G4a epoch projection #856 and G4b state-trigger preparation #857 passed all exact-head CI workflows. G4c same-root hook dispatch #858, G4d chained epochs #860, the bounded synchronous cycle #861 and native pending-hook cancellation #862 are implemented but await exact-head acceptance. No second scheduler, registry, event queue or authority store. |
| CI0 | Cross-cutting infrastructure; not a Basic dependency | Existing Nix checks, GitHub Actions shard orchestration and cache | Matrix must run all affected checks with exact-head evidence; full release/shipping parity remains mandatory before replacing serial acceptance | [x] ACCEPTED for P1: full exact-head matrix and release evidence on production main `b7017042`; historical #790 speed baseline retained in the CI evidence ledger. |
| AI-CI1 | CI0; parallel with Basic | `flake.nix`, compatible example and Basic plugin builds | Share Cargo compilation across compatible fixtures, package each plugin independently, narrow Nix source sets; preserve isolated ABI, LTO/linker and malicious fixtures | [ ] PLANNED: same-head before/after benchmark; one fixture edit must not rebuild unrelated fixtures |
| AI-CI2 | AI-CI1 | Phenix AI Rust/Nix test profiles | Provide fast semantic checks and separate optimized shipping proofs without weakening native ABI, negative/security or production closure tests | [ ] PLANNED: compare exact check coverage, artifacts and failure cases for both profiles |
| FC1 | CI0 | `phenix-flake-ci` opt-in Nix check discovery | Derive exact-head check inventory and plan manifest from `checks.<system>`; new checks cannot be omitted by stale YAML | [x] ACCEPTED: `phenix-flake-ci` pinned inventory/evaluation and generated caller on main `b7017042`. |
| FC2 | FC1 | `phenix-flake-ci` checked Nix source-impact planner | Extend existing Cargo reverse-dependency rules with provable Nix input impact; unknown or shared changes fall back to full checks | [ ] DESIGNED in #17; implementation pending |
| FC3 | FC1 | `phenix-flake-ci` check grouping, DAG and evidence gate | Schedule shared Nix outputs, shard without new compiles, account for every check and validate exact-head shipping evidence | [x] ACCEPTED: pinned Nix DAG, 4 remote workers and exact-head shipping aggregation on main `b7017042`. |
| FC4 | FC1; may parallelize FC2/FC3 | `phenix-flake-ci` trusted cache policy and CI metrics | PRs restore verified Nix outputs read-only; validated trusted runs publish cache; report wall time, build counts and transfer costs | [x] ACCEPTED for trusted remote releases: isolated cold/warm cache verified with 38 checks in each phase, run 38604657851; PRs remain credential-free. |
| AI-CI3 | AI-CI1, AI-CI2, FC1, FC3 | Phenix AI generated workflow migration | Adopt the Nix-aware planner; prove full check and shipping-evidence parity on the same head; retire redundant serial workflow | [x] ACCEPTED: consolidated #890 merged at `b7017042`; first promoted `main` remote release and all independent checks passed. Legacy canonical/DAG workflows are manual fallbacks. |
| AI-CI4 | AI-CI3, FC2; FC4 for cache rollout | Phenix AI selective PR checks and shared cache | Small edits run only proven affected checks; main/release/manual-full run complete inventory and shipping | [ ] PLANNED: cold/warm timing and unaffected-check evidence required |
| B1 | G1 | New `phenix-basic-contracts/` | Independently compiled Model, ToolCatalog, ToolExecute, Skills, Context, Session, EventJournal, Storage, AgentInput and AgentOutput contracts; no kernel agent dependency | [ ] IMPLEMENTED on impl/m1-basic-contracts: eleven typed public contracts and an independently built definition package; exact-head acceptance pending |
| B2 | B1 | `phenix-plugin-basic-session/`, `phenix-plugin-event-journal/`, `phenix-plugin-storage-sqlite/` | Restart restores history, call IDs/results, checkpoints and observer offsets; two observers replay independently; cross-session access rejects; interrupted non-idempotent tools remain indeterminate until reconciled | [ ] PARTIAL: observer checkpoint #813, SQLite injected rollback #854 at `334ee551` and schema v0-to-v1 migration #855 at `aa9f97a1` passed their focused exact-head gates. Full cross-process fault and integrated P1 acceptance remain open. |
| B3 | B1, B2 | New Basic tools, skills, context, executor and optional workspace-backed shell plugins | Selectable independent tools; denied execution has no effect; query-bound pagination and revisions reject stale cursors; skills/context persist; catalog discovery grants no execution permission | [ ] IMPLEMENTING: typed native ToolCatalog/ToolExecute and a new Skills/Context plugin with selected Storage CAS, session checks and two-process SQLite restart fixture on impl/m1-basic-skills-context. Exact-head CI and complete B2 integration remain open |
| B4 | B1 | New `phenix-plugin-model-scripted/`, `phenix-plugin-model-http/` | Same portable contract carries complete history, text, structured calls, usage and failures; local HTTP fixture verifies streaming, malformed replies, provider errors and cancellation | [ ] IMPLEMENTING: scripted reference and pending HTTP/HTTPS provider with live journal deltas, typed usage, function calls, limits and draining on impl/m1-model-http. Exact-head CI and pending Basic-agent integration remain |
| B5 | G4, B2-B4 | New `phenix-plugin-basic-agent/` | Prompt trigger starts a multi-turn run with complete history, multiple call IDs, skill/context use, tool-error recovery, usage, partial output, session continuation and cancellation; limits are caller-selected | [ ] IMPLEMENTING on impl/m1-basic-agent: independently packaged BasicInput/BasicOutput plugin with prompt graph trigger, selected providers, persistent checkpoints, journal events, multiple tool calls and caller limits. Pending model streaming, cancellation, full continuation and CI acceptance remain |
| F1 | G3, B1 | Terminal and JSONL frontend plugins | Input decoding submits discovered actions; observers render progress and completion without agent-specific kernel entry points | [ ] IMPLEMENTING: independently packaged terminal and JSONL Frontend adapters from shared Rust codecs; native decode/render/session checks on impl/m1-basic-frontends. Live input/output control pump and cancellation remain |
| F2 | F1 contracts | ACP frontend plugin and protocol integration fixtures | ACP drives the same input/output contracts and passes session, progress and cancellation integration tests | [ ] PARTIAL: ACP v1 multi-session stdio host, live HTTP progress/cancellation and durable replay are implemented. The independent official SDK client covers prompt/tool turns and two-process `session/load` (#831). #832 through #836 passed canonical conformance, flake matrix, generated DAG and official ACP SDK checks on their exact heads. These slices cover load validation, bounded stdin, JSON-RPC envelopes, invalid prompt parameters and pending-queue isolation. Active `session/cancel` requests are rejected without deferral on `impl/m1-p1-acp-cancel-request-admission`, pending exact-head checks. Active ACP stdin-disconnect cancellation is implementing on `impl/m1-p1-acp-eof-cancellation`; the real HTTP fixture checks prompt cancellation before the upstream model responds, pending exact-head acceptance. The SDK-only stable-v1 host passed exact-head checks in #843; R1-ACP4 parity remains open. |
| R1 | Cross-cutting; before P1 acceptance | Audit common infrastructure in each plugin and host; select maintained Rust libraries or document narrow exceptions | Thin provider adapters over established protocol, transport, parser, storage and process crates; keep Phenix-specific contracts, graph admission, authority, session and replay rules | [ ] PARTIAL: `rusqlite` Storage #815, `sse-parse` SSE #823, `url` endpoint parsing #840 and the official SDK-only ACP v1 host/codec #843 passed their exact-head CI. #843 passed canonical conformance, the flake matrix, generated DAG and independent SDK interoperability at `fb1ba478`. The [plugin ownership audit](m1-r1-library-audit.md) remains the crate-first review reference. R1-ACP4 full negative parity, including independent resource-link replay in #853, and integrated P1 release evidence remain open. |
| P1 | B5, F1, F2, R1 | Basic package composition, executable product fixtures and release evidence | Start terminal, submit prompt, discover/execute tools across turns, stream answer, restart/continue session; JSONL and ACP use unchanged agent/kernel logic | [x] ACCEPTED: main `68f46f63` passed integrated P1 product, independent ACP, canonical, matrix, DAG and release preflight; see [P1 release acceptance](p1-release-acceptance.md). |
| F3 | P1, S5; S6 only for concurrent-session acceptance | Migrate named reactive plugin consumers/producers and the frontend host to generated hooks; prove ACP and Terminal/JSONL parity | All required plugins in the F3 migration inventory use typed declarative handlers instead of redundant watch registration, journal polling or event dispatch; preserve durable event order, cancellation, scopes and transport ownership | [ ] PLANNED POST-P1: inventory and migration gates specified; implementation pending |

| P2a | P1 accepted | Common host-side session execution driver and SDK handles | Submit, observe, cancel and drain through one kernel-owning controller; remove frontend-specific polling without a second kernel scheduler | [ ] PLANNED: post-P1 only |
| P2b | P2a | Session hub, independently typed client projections and A2UI capability selection | Multiple authorized frontends observe one canonical session with separate subscriptions/cursors; a target client advertises its supported standard A2UI catalogs and receives only validated compatible UI | [ ] PLANNED: post-P1 only |
| P2c | P2a; P2b for projection routes | Plugin-declared typed I/O ports, configurable forwarding and standard A2UI message/action transport | Multiple ports and one-to-many sinks including pipes/sockets; preserve unmodified A2UI envelopes and authorized return actions over compatible existing transports; no new UI wire format or kernel dispatch per raw byte | [ ] PLANNED: post-P1 only |
| P2d | P2b, P2c | Multi-client, A2UI and I/O fixtures | ACP v1, official ACP v2 and WebUI share session state without sharing streams; verify A2UI catalog negotiation, validation, actions, fallback, isolation, reconnect and multi-sink routing where transport support exists | [ ] PLANNED: post-P1 only |
| K1 | S3b | Kernel generation and prepared composition APIs in `phenix-kernel` | Authorized stage/load/replace/unload/trial/promote/retire of verified plugins; preserve grants, active roots, rollback and leases; builds stay outside kernel | [ ] PLANNED: generation lifecycle exists, management API incomplete |
| K2 | K1, G2 | Kernel desired-state diff/preview/reconciliation using existing resolver, DAG compiler and generation manager | No-effect preview; deterministic diff; atomic promotion or rollback; unchanged roots and safe draining; no second registry or dispatcher | [ ] PLANNED: generation transition exists, desired-state reconciliation absent |
| K3 | G3, G4 | Kernel diagnostic event emission and authenticated internal inspection API | Structured execution/lifecycle/state/denial facts with correlation; authorized consistent snapshots; dropped diagnostic events detectable; no consumer affects execution | [ ] PLANNED: basic inspect methods exist, diagnostic emitter missing |
| L1 | K3 | `phenix-plugin-logging` | Independent optional consumer of kernel diagnostics; logging formats, filtering, sinks, export and retention owned by plugin; zero logging code in kernel | [ ] NOT STARTED |
| I1 | K3 | `phenix-plugin-introspection` | Independent optional typed, discoverable contract projection over authorized kernel query APIs; no direct unrestricted host handles, no dependency on L1 | [ ] NOT STARTED |

G1-G4 refine S4/S5 using one resolver and dispatcher. S6 remains outside the Basic critical path. B1 can proceed after G1, F1 after G3, and P1 requires G4. K1-K3 and L1/I1 are independent operations milestones, not Basic product blockers.

### Deterministic Basic capability matrix

The scripted (`phenix.model.scripted`) and history-test (`phenix.model.history-test`)
Models drive the deterministic product fixtures; no credentials or network are
required. This table records where each Basic capability is proven. "Product"
means a packaged `tests/*.py` fixture over `jsonl`/`terminal`/`acp`; "native"
means a `phenix-b*` conformance binary or crate unit test. Items marked "gap"
are not exercised through the synchronous deterministic Model and remain owned
by their existing HTTP/pending or native gates — this section does not accept P2
or claim real-model/live-provider verification.

| Capability | Deterministic coverage | Status |
| --- | --- | --- |
| fresh session + prompt | `jsonl-multisession-product.py`, `terminal-sessions-product.py`, `acp-stdio-product.py`, `basic-history-verification-product.py` | covered |
| assistant text / streaming | `basic-streaming-product.py` (synchronous TextDelta projection); `basic-pending-http-product.py` (incremental pending HTTP stream) | partial across separate gates; synchronous fixture is not live streaming |
| single and multiple tool calls | `basic-history-verification-product.py` (one call), `jsonl-multisession-product.py`, `acp-stdio-product.py` (two calls) | covered |
| tool-call argument validation | `phenix-plugin-basic-tools` and `phenix-plugin-model-scripted` unit tests (invalid JSON, unavailable tool) | covered (unit) |
| tool-result continuation | `basic-history-verification-product.py`, `jsonl-multisession-product.py` | covered |
| tool errors and recovery | `basic-history-verification-product.py` (`tools-fail:hello`), `terminal-sessions-product.py` | covered |
| multiple turns | `basic-history-verification-product.py`, `jsonl-multisession-product.py` | covered |
| previous user/assistant recall | `basic-history-verification-product.py` (`recall:`/`verify-assistant:`) | covered |
| history preservation without tools | `basic-history-verification-product.py` (restart `recall`/`verify-assistant`) | covered |
| skills/context inclusion | `phenix-b5-agent` (`skills-and-context-imports`) | covered (native) |
| provider substitution | `basic-history-verification-product.py` (scripted→history-test), `basic-http-preset-product.py` | covered |
| model selection/connection identity | `acp-stdio-product.py` (`session/set_config_option`); connection identity in provider track | covered (model selection) |
| usage reporting and limits | limits: `basic-history-verification-product.py`, `jsonl-multisession-product.py`; usage: `phenix-b5-agent` | covered (limits product / usage native) |
| cancellation and effect revocation | idle cancel: `basic-history-verification-product.py`; active cancel/revocation: `basic-pending-http-product.py` | partial (active path is HTTP-only) |
| persisted restart/replay | `basic-history-verification-product.py`, `jsonl-multisession-product.py`, `observer-product.py` | covered |
| multiple observer cursors | `observer-product.py` (independent Terminal/JSONL checkpoints and failed delivery replay), `basic-history-verification-product.py` | covered by observer products, not basic-streaming |
| cross-session authority isolation | `jsonl-multisession-product.py`, `basic-history-verification-product.py`, `terminal-sessions-product.py` | covered |
| conflicting state revisions | `phenix-plugin-storage-sqlite` CAS, `phenix-b2-sqlite` injected fault; owner-marker conflict in `basic-pending-http-product.py` | covered (native) |
| invalid/unavailable operations | invalid session identity, unavailable model/provider (`acp-stdio-product.py`), `model-error` | covered |
| ACP error propagation | `acp-stdio-product.py`, `basic-history-verification-acp.py` | covered |

The synchronous `basic-streaming-product.py` proves journal event order, not
incremental token emission, partial-response observability, or independent
observer replay. Those are separate pending-HTTP and observer product gates.
Never infer a live-stream or cross-cursor guarantee from an ordered
`TextDelta` emitted after a synchronous Model has returned.

Remaining deterministic gaps (by design, not addable with the synchronous
scripted/history Model): active mid-stream cancellation/effect revocation,
product-surface usage reporting, product-surface skills/context mutation,
product-surface conflicting-revision races and invalid-tool-argument injection.
Each stays covered by its existing pending-HTTP or native gate.

### Post-P1 P2: shared session projections and typed I/O routing

**Start gate.** Begin P2a-P2d only after P1 passes complete independent
product and shipping acceptance. These stages do not expand P1 scope, alter
its CI gates or block its integration. Keep separate follow-up implementation PRs.

**Goal.** One Phenix instance owns the canonical session. Multiple frontend
connections attach with independent read/action grants, subscriptions,
protocol adapters, replay cursors and delivery queues. They synchronize
their authorized view of state and events, not raw sockets, pipes or frames.
ACP v1, ACP v2, ACP extensions and WebUI may coexist in one process.

**Runtime configuration and credentials.** [Runtime management #891](https://github.com/matthis-k/phenix-ai/issues/891) owns provider-supplied model/routing catalogs, authorized mutations and persisted revisions. Effective setting precedence is session override, project override, runtime default. Return both the effective value and its source; new sessions inherit the current applicable defaults. Changes apply to later turns, while the admitted turn retains its model, routing policy and plugin generation. A documented routing policy may make a fallback selection during that turn. A resumed session with an unavailable selected provider reports incompatibility rather than silently substituting another. A different authorized composition may resume a session only when its storage and data contracts are compatible. Credential provider plugins resolve secrets at runtime outside world-readable Nix store paths and diagnostic output; the client never supplies the authoritative provider catalog.

**P1 usability / P2 release-critical provider onboarding (#891).** See the [R1 provider protocol and crate audit](m1-r1-library-audit.md#provider-protocol-and-authentication-audit-17-october-2026) for the three selected adapters, provider endpoint matrix, OAuth2/OIDC library selection, credential backends and plugin split. Reuse its decisions rather than defining another provider contract here. A logical provider (for example `openai`) may expose multiple provider-owned **connections**, each declaring an explicit protocol adapter, validated inference base URL where applicable, supported authentication methods, and dynamic discovery and/or declared static model sources. Keep connection identity and authentication-method identity explicit under the one user-facing provider label; an OpenAI API connection and a supported ChatGPT/Codex subscription connection can have different transports, credentials, billing and model catalogs without creating duplicate user-facing providers. Endpoint URL alone never implies protocol, OAuth support or a compatible model-list API. Provider plugins supply metadata and authentication state; the application normalizes and authorizes it. Frontends do not hard-code provider IDs, endpoints, supported methods, model lists, or billing semantics.

For a connection requiring authentication, an authenticated provider is **ready if at least one of its supported authentication methods is currently valid**; a connection not requiring credentials is ready independently. This aggregate status is for UI display only: execution must select an authorized, currently valid **specific connection + authentication context + model**, and cannot borrow credentials from another connection, silently change auth method, or silently switch billing. When both API key and OAuth are supported, offer an explicit method choice; when exactly one is supported, skip the choice and begin that method; when neither is required, skip authentication. Multiple valid methods can coexist, with explicit selection/override and independent revocation. Missing, expired or revoked credentials and stale selections fail closed. Credential submission uses runtime-authorized, secret-safe input; OAuth uses only a provider-approved PKCE/browser or device flow with lifecycle/revocation handling, not captured web cookies or another application's tokens.

Discover models dynamically only when the selected protocol/provider supports a documented enumeration mechanism. Otherwise use provider-declared static models; where both exist, merge with provenance, refresh and retirement semantics. Refresh after authentication, on request and with a bounded cache policy. The runtime filters discovered models by connection-specific authentication, actual inference transport support, required capabilities and selection authority; the `:Phenix model` picker shows only **usable** model variants from ready connections. Provider onboarding still lists unauthenticated providers in `:Phenix auth`. Each model selection records provider, connection, model, effective scope/origin and revision; thinking variants come from the provider. A change applies to future turns and is replayable across reconnects, with no implicit fallback on restart or token expiry.

ChatGPT Plus/Pro subscription access is **not interchangeable with OpenAI API-key billing**. Offer a ChatGPT/Codex OAuth subscription connection only when an officially supported delegated usage flow explicitly authorizes this client and its selected model transport; do not promise generic ChatGPT OAuth inference or reuse proprietary Codex/ChatGPT tokens. If Phenix is not eligible for that authorization, expose the method as unavailable with a clear explanation, while normal API-key onboarding remains functional.

**Accepted provider UX and runtime behavior (2026-10-17, Q1–Q6):** A user may create an authorized generic provider connection from Neovim using a URL and an installed protocol, without writing a plugin. One connection supports multiple named OAuth/API-key profiles, each with separate billing and credential identity. Use a supported OS secret service first; an explicit opt-in, protected `0600` file fallback keeps headless setups possible with a security warning. Basic and Advanced register the six selected providers before authentication and perform model discovery on demand. The model picker may visually group duplicate names but must preserve visible provider, connection and account choices. A failed catalog refresh retains stale entries for inspection only; selection/inference must revalidate authorization and availability. **Accepted follow-up (Q7–Q9):** A runtime-managed connection binds a validated explicit protocol ID and remote HTTPS URL. Local HTTP is allowed only for approved loopback endpoints; Unix sockets require an authenticated local transport. Credential-bearing redirects and URL origin changes are rejected, including DNS-rebinding paths. A selected credential profile has independent persistence, revision-checked scope and revocation. No silent substitution on expiry/revocation. Provider-supported OAuth refresh rotates tokens atomically and offers reauthentication on failure. Use provider-declared model refresh rules or a bounded default of **15 minutes**. A stale/offline catalog is inspection-only and cannot authorize new selection or inference. Static no-auth local catalogs do not require network access.

**Grill tracking and build order:** All **17 decisions are resolved; 0 open and 0 deferred**. [#891](https://github.com/matthis-k/phenix-ai/issues/891#provider-grill-decision-tree-and-first-frontier-2026-10-17) owns the accepted decision ledger, and this roadmap owns the behavior and functional acceptance. Provider contracts and reusable libraries are now integrated on `main`: [#948](https://github.com/matthis-k/phenix-ai/pull/948) (provider/auth/catalog definitions), [#949](https://github.com/matthis-k/phenix-ai/pull/949) (bounded model-list decoders), [#950](https://github.com/matthis-k/phenix-ai/pull/950) (secure endpoint and credential-origin validation), [#953](https://github.com/matthis-k/phenix-ai/pull/953) (opt-in `0600` credential-file backend, merge `de3f26f4`), [#954](https://github.com/matthis-k/phenix-ai/pull/954) (six provider registrations, merge `f0c89630`), [#964](https://github.com/matthis-k/phenix-ai/pull/964) (independently packaged base provider catalog/auth plugin, merge `86eb50cd`; re-lands the #955 work after its stale stacked base) and [#962](https://github.com/matthis-k/phenix-ai/pull/962) (fresh account-bound selection admission, merge `152f7a42`). This completes PV1 and the reusable metadata/credential-storage pieces. Subsequent merges #1006/#1009/#1010/#1011 now expose installed read-only catalog/model/auth queries and tightly authorized credential import/revoke. They do **not** establish provider-verified readiness, actual selected-provider HTTP binding, persisted revision-CAS model mutation or a completed Neovim journey. The endpoint library validates configuration; network resolution, redirect behavior and credential grants remain runtime/transport obligations. Keep all later features in #891 rather than adding competing owners.

**PV2 credential-store slice (implementation, not acceptance):**
`phenix.provider.credentials@1` supplies separately granted import, resolve and
revoke operations over one exact provider/connection/profile resource. The
independently selected `phenix-plugin-provider-credential-files` reuses the
existing explicit-consent file backend; it is not installed as a silent default.
`PHENIX_CREDENTIAL_FILE_DIRECTORY` must name an existing absolute, owned `0700`
directory, and `PHENIX_CREDENTIAL_FILE_CONSENT=1` explicitly enables the weaker
plaintext fallback. Values have redacted Debug and no ordinary serde/config
serialization. The `provider-credential-store` installed native check covers
restart and rotation/restart, isolation, independent import/resolve/revoke grant
denials with unchanged vault bytes, consent, revocation, identity boundaries and
diagnostic redaction. Identities retain the existing 64-byte component limit and
must total at most 113 bytes: their encoded file leaf is at most 239 bytes and
their canonical resource at most 247 bytes (below the authority's 256-byte cap).
The Linux fallback now requires filesystem `O_TMPFILE` support and `/proc/self/fd`;
unsupported environments fail closed without a second persistence backend.
Anonymous files hold uncommitted plaintext until sync. A reserved
`.credential-pending` publish slot is linked and atomically renamed while holding
an exclusive lock on the pinned directory. Open/read/store/revoke recover that
slot under the same lock; they never scan arbitrary candidate names or delete a
live cooperating writer's staging file. Existing committed filenames are
unchanged; no migration or alternate backend is introduced. Backend subprocess
tests interrupt imports and rotations after sync and after link, verify restart
and revocation, and check live-writer exclusion, pinned-directory authority and
same-instance directory/file permission revalidation. The unchanged production
CLI performs only one invocation per loaded generation, so same-generation
checks are library evidence, not claimed native CLI coverage.
Provider-auth profile metadata/revision management,
non-journaled ACP management admission, selected HTTP resolution and end-to-end
Neovim onboarding remain open under #891. Stored bytes alone do not prove
provider authentication, model entitlement or PV2 completion. Credential writes
and profile metadata mutations must be coordinated by that management owner;
the low-level secret store is not another selection/revision authority.
The kernel deliberately forbids changing an inherited root resource. These
operations therefore run under a dedicated credential-resource root; importing
`resolve` directly from a session-resource Model call cannot work. PV3 must
first establish the trusted controller's existing credential admission/binding
path without weakening that rule or putting a secret in `ModelRequest`. This
slice does not claim that transport integration is solved.

**PV2 OS Secret Service slice (18 October 2026; acceptance pending).**
`phenix-plugin-provider-secret-service` implements the existing typed
`ProviderCredentials` import/resolve/revoke operations, with the same exact
credential-resource root and separate kernel grants. R1 records the selected
maintained keyring-core/Secret Service backend. The installed native
`provider-secret-service` check uses an isolated real GNOME Keyring and private
D-Bus session; no user keyring is read. It covers profile/connection isolation,
native-process and service restart, rotation, revocation, missing operation
grants, session-resource denial, diagnostic redaction and service failure with
zero automatic file fallback. The initial functional RED fails against the
existing file package because it cannot provide the new OS backend. This is
storage integration only, not provider authentication or a packaged PV2 gate.
Local verification passed: `nix build --no-link -L
.#checks.x86_64-linux.provider-secret-service
.#checks.x86_64-linux.m1-contract` (installed real-service fixture, strict
workspace Clippy, formatting, Rust tests, C ABI and architecture checks).
The independent package's Nix source contains only public SDK/contracts and
this plugin, not private kernel sources. Exact-head full GitHub CI and merge
evidence belong to the implementation PR; no live provider credentials tested.

**PV2/PV3 implementation boundary (reconciled 20 October 2026).**
Merged #1009 authorizes ACP credential import/revoke under a dedicated exact credential-resource root; #1010 adds read-only auth status/model queries, and #1011 advertises the launcher's one admitted credential profile before prompting. These replace the historical assertion that no credential ACP binding was defined. The resulting stored credential is **not** verified provider authentication or an executable Model selection. Remaining #891 work: authorize selected secret resolution at inference time, verify the specific provider/account/model/protocol transport binding, implement revocation/expiry/readiness and revision-CAS persistent model mutations, and test approved OAuth separately. No extra management protocol, frontend secret store, kernel provider routing or unsafe session-resource escalation.

#### Provider implementation packets: current coding queue

**Reusable inference codec packet (Responses + Anthropic, #974).** The shared
`phenix-provider-protocols` library now carries all three R1 inference wire
codecs — `openai-chat` (unchanged default), `openai-responses` and
`anthropic-messages` — as pure request/stream codecs over the existing
`curl`/`sse-parse`/`url` transport and the canonical `Model` contract. The
`phenix-plugin-model-http` transport selects an explicit protocol
(`PHENIX_MODEL_HTTP_PROTOCOL`) with `openai-chat` preserved as the default, so
accepted P1 behavior is unchanged; the Anthropic path sends `x-api-key` and
`anthropic-version`. The real local HTTP/SSE fixtures (`tests/http-model.py`,
`tests/basic-pending-http-product.py`, `tests/model_protocol_fixture.py`) drive
valid text, multiple tool calls plus continuation, malformed replies, usage
reporting, error and cancellation drain for every protocol. This is the audit's
"generic codecs" step, not credential-bound model selection: PV2/PV3 identity,
binding and PV4-PV7 remain OPEN, and no live provider or onboarding is claimed.

This is the coding handoff for #891. The accepted method/connection/model
semantics above remain the behavior source. The [R1 audit](m1-r1-library-audit.md)
owns standards and library selection; do not duplicate its provider matrix.
Build one independently runnable slice at a time. Paths here are entry points,
not prescriptions for private module layout.

| Order | Inspect / change | First implementation and RED case |
| --- | --- | --- |
| **PV1: finish discovery contracts** | `crates/phenix-provider-contracts/src/lib.rs`, #948 and stacked #949/#950, provider-contracts Nix package | Verify the already-defined `ProviderDescriptor`, `ConnectionDescriptor`, `AuthenticationStatus`, `CredentialProfile` and model provenance round-trip, including malformed, duplicate and too-large catalogs. Reuse bounded provider-specific decoders and strict URL validation. A configured URL never chooses a protocol. Run `cargo test --locked -p phenix-provider-contracts` and the package descriptor check. Integrate or reconcile overlapping PRs before creating another definition. |
| **PV2: usable API-key connection** | Existing typed provider contract, `crates/phenix-plugin-model-http/src/{request,transport,stream}.rs`, selected runtime Storage/authority | Add one provider-declared API-key connection, a secret-safe submission operation and selected credential profile. Resolve secret bytes only at the transport's authorized call, never in `ModelRequest`, persistent session history or frontend event. Use the OS secret service where available; add the accepted explicit `0600` headless fallback without silently enabling it. Test save/restart/revoke, wrong-session grant, transcript/log redaction and no secret in generated Nix artifacts. |
| **PV3: actual credential-bound Model request** | Selected `Model` import, existing `phenix-plugin-model-http` curl/SSE stack, `phenix-basic-runtime` model selection | Pass an explicit provider/connection/profile/model/protocol identity to the selected transport. Add `openai-chat` first and keep its request, stream and tool-call fixtures passing. A second valid account must never be used when the selected account expires. Deny before HTTP on missing grant, stale model, protocol mismatch or revoked credential. Preserve pending cancellation and physical drain. |
| **PV4: discovery and static fallback** | Provider contract `DiscoverySource`, #949 response normalization, existing Catalog query/selection surfaces | Query a provider's documented model-list endpoint lazily and within bounds. Merge declared and dynamic choices with source identity and freshness; a stale snapshot remains readable but cannot authorize selection or inference. Add static-only/no-auth provider fixtures and authenticated discovery fixtures. Verify pagination, unknown types, retired models and transport/thinking capability filters; no default `/models` guess. |
| **PV5: authorized model mutation** | `crates/phenix-basic-runtime/src/acp_sdk.rs`, ACP SDK session config options, existing Session/Journal/Storage, #958 | Extend the already supported runtime-controlled model selector instead of inventing a second ACP management protocol. Persist the specific provider + connection + credential profile + model with scope, source and CAS revision. Changes apply to later turns. Return typed denied/stale conflicts and publish an observable update; a running turn retains its binding. Test two clients racing, restart and an unavailable saved selection. Any management operation absent from stable ACP must use a verified standard extension/facility or stay unavailable. |
| **PV6: OAuth only for approved connections** | Chosen `oauth2`/OIDC crates and credential backend from R1, the PV2 provider operation | Add PKCE/browser or device flow only where the provider permits this client and transport. Store refresh/revocation per named profile. Expired or revoked OAuth must not switch to another account or API key; both methods can remain separately ready. Test state/nonce/issuer/audience checks where applicable, replayed callbacks, refresh failure and restart with a deterministic fake identity provider. An unsupported subscription integration is displayed unavailable. |
| **PV7: Neovim release journey** | Runtime #891 + `phenix-ai.nvim` N4 + `phenix-nvim` D4 | Use runtime-owned provider/auth/model discovery in `:Phenix auth` and `:Phenix model`. With zero/one/two supported methods, show no chooser/direct action/choice respectively. Prove one actual authorized packaged Basic and Advanced turn after selection, plus reconnect, denied/stale choice and no secret leakage. Client/distribution roadmaps own UI and pinning details. |

For every packet: first add a failing unit/native or packaged fixture, then
implement the minimum selected-provider path and include a denial/failure
counterfactual. Keep the existing `Model` contract, graph admission and
libcurl cancellation behavior; treat internal DTOs, helper methods, individual
provider plugin grouping and file organization as implementation choices.
Stop if a slice requires a new authoritative session store, independent
provider registry, another HTTP stack or invented ACP method. Record the
specific missing contract in #891 for review rather than building around it.

**Product acceptance (RED before implementation):** verify zero/one/two supported auth-method flow branches; both methods valid, only one valid, revocation, expiry and reconnection; distinct API vs subscription connection catalogs and billing identity; dynamic `/models` discovery, static-only providers, mixed catalogs, stale/retired IDs and unsupported inference models; authorized revision-checked model switching while another turn runs; masked secret input/no secrets in Nix store, journal, transcript or logs; and packaged Neovim `:Phenix auth` -> `:Phenix model` -> real Basic and Advanced turn. Use deterministic provider/OAuth fixtures in CI and separately label any manually tested live-provider/partner authorization. This is a **top-priority cross-product gate** before P2 is declared usable as an interactive product; issue #891 remains the owning implementation tracker.


**Durable input and approval.** A session runs at most one agent turn at once. Accepted follow-ups have durable IDs, author, sequence and cancellation state in the existing Session/Journal/Storage providers. Admission is ordered and idempotent across replay; uncertain external effects require reconciliation before the next dependent turn. Ordinary input queues behind the active turn. An explicit interrupt-and-submit cancels and drains active work before admitting its replacement. Multiple authorized clients may observe, submit or cancel through the shared controller; their view is not an additional session store.

Permission policy is provided through typed plugins; kernel grants remain the maximum authority. A permission request has a durable ID, scope, expiry and revision. The first authorized compare-and-swap response settles it; subsequent responses fail as stale and observers see the committed result. Pending requests survive client disconnect and restart, expire to denial, and become invalid when their operation is cancelled. Without an authorized approver, required approval denies under the declared headless policy. One-time operation approvals are the default. Optional standing rules require explicit authorization, bounded operation/resource/session/project scope and expiry; project-wide rule management requires project-level authority separate from the ability to approve one operation. A standing rule cannot widen kernel grants.

**Recovery and verification.** Reuse the existing session journal, CAS transactions, fenced owner markers, pending tickets and cancellation. An acknowledged queued turn or approval must not disappear on restart. Admission is exactly once by durable identity; external effects can remain indeterminate after a crash and must never be blindly replayed. Independent fixtures must cover two concurrent client responses, approval deadline/restart/denial, client disconnect, persistent follow-up ordering, interrupt-and-submit, changed configuration during a running turn, explicit model incompatibility, unauthorized cross-project/session access, and replay without duplicate tool effects. These are observable product tests, not a second architecture CI model.

#### Client-contributed MCP capabilities (post-P1)

**Accepted client-capabilities Grill, 2026-10-16, Q1-Q36.** This section owns
the behavior, implementation slices and acceptance tests. The
[product decision](agent-product-decision.md) owns only the kernel/plugin
boundary; [runtime management #891](https://github.com/matthis-k/phenix-ai/issues/891)
owns authenticated clients, grants, session admission and permission
responses. [P2 Advanced](p2-advanced-agent-roadmap.md) consumes the same
ordinary providers and does not gain a second tool engine or release gate.
This is post-P1 interoperability work shared by Basic and Advanced, adjacent
to P2-IO1 through P2-IO4, not a dependency on the Advanced agent loop.

**Protocols and providers (Q1-Q7, Q29-Q36).** An ACP client MAY offer
application-local MCP servers at session setup. Prefer the existing ACP
`mcpServers` declarations and official SDK. Stable ACP v1 and independently
negotiated draft ACP v2 must retain their own verified protocol paths. For
compatible peers, opt in to the official *unstable* native MCP-over-ACP binding:
`mcp/message` addresses a declared `serverId` and a fresh logical
`requestId`, with MCP 2026-07-28 metadata on each request. This binding
has no MCP initialization, connect/disconnect exchange, or reverse request
channel. Reuse the official ACP and MCP SDKs; use compatible standard MCP
stdio/Streamable HTTP transports when native binding is unavailable. Do not
invent ACP methods, translate incompatible MCP revisions by hand, or assume
a server can be added mid-session. One stable application server can add,
replace or remove its tools, resources and skills during a session; separately
authorized server declarations remain possible. The frontend's own Lua/Rust/
TypeScript bindings are presentation and application adapters, not Phenix
plugins or privileged kernel operations. ACP remains frontend control and
permission transport; MCP is the client-to-agent capability interface.

**Origin, discovery and authorization (Q1, Q3-Q13, Q16-Q17, Q26-Q31).**
An MCP contribution is bound to the authenticated client/connection,
host-assigned MCP server identity and original tool name or skill URI.
Names are display labels, not security identities. Preserve the original
MCP names, disambiguate collisions in the model projection and forbid
silent skill shadowing. Client registration makes an item *discoverable*,
not executable. Filter it through existing ToolCatalog/ToolExecute,
Skills/Context and kernel authority before model inclusion and each effect.
The agent can discover all advertised, authorized capabilities; include only
selected schemas and loaded skill instructions in a model request. For
partially enumerated skills support origin-bound `skills/get` by known URI.
Only a caller/session explicitly granted the client tool can invoke it.
Cross-client and project-wide access requires separately authenticated
authorization; sub-agents receive only attenuated, origin-bound grants.
Retain origin and effective grant in read-only client projections.
Approval defaults to a single call; explicit standing rules use the
existing scoped, revisioned and revocable runtime grant system. A persistent
grant is bound to trusted logical client identity, server, operation identity,
scope and validated capability fingerprint, never to a display name or a
reconnected socket alone. Changed privilege/semantics needs fresh approval.
Unrestricted application scripting such as Neovim `nvim_exec_lua` is an
optional high-privilege tool requiring per-call approval by default.
ACP `session/request_permission` may collect approval; the owner application
may also refuse. Timeouts, headless operation and unavailable approvers deny.

**Reactive state and lifecycle (Q2, Q8, Q11, Q14-Q15, Q18-Q25).**
Watch declared tool and skill catalogs without model calls. For MCP
2026-07-28 consume `subscriptions/listen` change notifications where
advertised, then relist or refresh; use its `ttlMs`/`cacheScope` and
selective `skills/get` for eventual freshness where push is absent. Do
not claim immediate notification for skills without a suitable upstream
signal. Subscribe to application resources only for authorized consumers;
`resources/updated` invalidates their snapshot and existing M1
`ValueRef` watches/graph triggers observe committed revisions. An
invalidation does not start a model turn: only explicit user input or
authorized contributed triggers do so. Coalesce state invalidations,
not distinct durable operations; bound per-client queues and reconcile
authoritative state after overflow or reconnection. MCP subscriptions do
not supply durable replay; use existing EventJournal cursors for durable
events. Hold each model step's admitted tool/skill definitions and source
revisions for historical accuracy. Apply new catalogs to later model
requests, but revalidate authority and compatibility before every new
tool effect; revocation denies even an older pinned graph generation.
Changed or removed definitions fail closed rather than silently switching.
On disconnect, remove the live endpoint and cancel affected calls. Ordinary
calls fail; eligible durable jobs may wait with explicit deadline and
revalidate on authenticated reconnect. Cancellation is best effort.
A tool effect interrupted after dispatch may be indeterminate: record that
outcome, reconcile with its owner and never blindly retry it. MCP metadata
does not establish idempotency; unknown tools must not be assumed safe to
replay. Keep skill content-bound approvals and held entries per the MCP
Skills Extension; refresh a changed manifest, verify digest/size and YAML
frontmatter on read, require required approvals before code execution,
isolate resource reads to their source server and require per-call approval
for cross-origin reads. A skill with `resources: "dynamic"` cannot receive
persistent content-bound approval. Skill discovery does not automatically
activate skill instructions or grant tools.

**Contract fit and migration.** Existing Basic
`ToolCatalog`/`ToolExecute`, `Skills` and `Context` are the only
agent-facing interfaces. MCP discovery, requests and client transport
belong to replaceable plugins/adapters. The current `ToolInfo` advertises
an `idempotent` flag without MCP evidence; treat unspecified external
operations as non-idempotent until a provider supplies verifiable semantics.
The current `Skill` value and session `Skills::set` do not encode a
complete MCP skill manifest or lazy content verification. Enforce those
rules inside a Skills adapter, and extend the existing typed contract only
when an independent acceptance case demonstrates missing public semantics.
Do not add a second catalog, session store, grant registry, graph runner
or application-specific kernel logic. The current Basic ACP controller
rejects nonempty `request.mcp_servers` in `new_session`; remove that
limitation only alongside a verified, authorized provider path. Continue
rejecting unsupported session options such as additional directories
independently.

| Slice | Required implementation and independent acceptance |
| --- | --- |
| IO-MCP1: standard MCP | Official SDK-compatible ACP v1 session-supplied stdio/HTTP MCP tools through current Tool contracts; supported ACP v2 tested separately; empty-server P1 behavior unchanged. Reject forged client/server IDs and denied effects. |
| IO-MCP2: native MCP-over-ACP | Negotiated SDK-supported draft binding at MCP 2026-07-28; validate `serverId`, fresh request ID, per-request metadata, outer/inner error distinction, cancellation and downgrade to supported *standard transport*, not hidden protocol translation. |
| IO-MCP3: skills and live subscriptions | Skills Extension `server/discover`, `skills/list` pagination, partial listing and `skills/get`, manifest approval and resource verification; reactive catalog/resource invalidation, TTL fallback, bounded coalescing and graph state update with zero discovery model calls. |
| IO-MCP4: lifecycle and cross-client product | Two independent official protocol peers and deterministic fake application implementations in existing Basic verification composition. Concurrent client visibility and scoped call denial; grant revisions, reconnect, schema/revocation races, stale skills, multi-client target selection, job wait/cancel, unknown external effects and no duplicate execution. Neovim Lua registration is one reusable client example, not a required Phenix-only bridge. |

Tests are functional and authorization gates on the real kernel/SDK, not a
second architecture CI target. Required failures return typed denial,
unavailable, stale or indeterminate outcomes. No slice claims product
acceptance before its exact-head tests and authoritative CI pass.

**P2a: execution driver.** Extract duplicated ACP, Terminal and JSONL graph
polling, cancellation and draining into one host-side controller. Expose
scoped handles with proposed `session.submit(...)`, `session.cancel(...)`
and `session.attach(...)`. The controller retains `GraphExecution` on its
kernel-owning thread and drives readiness, pending tickets and physical
lease draining. Reuse existing authority, generation, root, dispatch and
wake mechanisms; do not create another scheduler, dispatcher or agent loop.
Prefer a wake-driven driver over each frontend's fixed-interval polling.

**P2b: projections.** Provide versioned typed contracts for `SessionView`,
`SessionChange`, `Subscription`, `Projector` and `Cursor`. Names remain
proposals until SDK authoring review. Projectors map one canonical session
into protocol-specific updates without copying the session or knowing what
other frontends received. Reuse `ValueRef` watches for state and B2
EventJournal cursors for durable ordered events. Attach using a consistent
snapshot/replay boundary, including when changes commit during attachment.
Each client owns a bounded queue, cursor, backpressure and disconnect policy.
Filter by authorized resources before serialization. Slow or disconnected
observers cannot stall execution or advance another observer's cursor.

**P2b: A2UI capability selection.** Add optional Agent-to-UI support through a
frontend plugin, using the upstream [A2UI v0.9.1 production
specification](https://github.com/a2ui-project/a2ui/blob/main/specification/v0_9_1/docs/a2ui_protocol.md),
its published JSON Schemas, component catalogs and capability metadata.
Do not define Phenix UI components, a parallel JSON dialect or a second
catalog format. A2UI v1.0 remains a candidate and requires a separate
compatibility review; do not silently interpret v1.0 messages as v0.9.1.
Frontend connections announce supported catalog IDs and optional inline
catalogs using the metadata/initialization mechanism of a supported transport.
The selected A2UI provider advertises only versions and catalogs it can
validate. An inline catalog is untrusted until checked against the upstream
catalog-definition rules and deployment policy; its prose instructions are not
authority. The provider resolves an explicit target client's offered catalogs
against its own supported set. Keep the selected catalog ID, definition/version
and target client attached to the UI delivery. Per-client differences must not
change canonical session history or widen any client's grants.

An ordinary agent-facing plugin import presents the **selected upstream
catalog** and output constraints to the Model. Model compliance is not a
security guarantee. Validate each generated upstream A2UI envelope against
the selected catalog and upstream schemas, then check the surface lifecycle,
component references, resource budgets and authorization before sending it to
the target. A surface keeps its catalog until deletion, as required by A2UI.
Validate any updates against the pinned catalog, even across reconnect and
plugin generations. Reject malformed or unsupported messages without
forwarding partial unvalidated content. Non-A2UI clients receive ordinary
text/progress; for a required interactive exchange, report unsupported
capability instead of fabricating a form. An SVG renderer is supported only
when a trusted catalog advertises that component and the frontend enforces a
safe SVG profile. Schema validation alone does not sanitize active SVG content.

**P2c: I/O routing.** Plugins declare any number of distinct typed input and
output ports, including names, payload schemas and authority requirements.
Composition configures their destinations and one-to-many forwarding to
other plugin ports, pipes, sockets and frontend sinks. Each sink has its
own queue, error and close behavior. Authenticate source/destination grants
when binding; handle generation changes, scope revocation and disconnect.
Semantic state-mutating commands use kernel admission. Authorized raw-byte
streams use maintained I/O crates rather than graph invocation per chunk.
Reuse protocol/transport libraries, notably the official ACP SDK.
Forward upstream A2UI messages without rewriting their protocol structure,
using an existing supported transport binding. A2UI itself is transport
agnostic. ACP remains an ACP protocol adapter: use only capabilities, content
or extension facilities actually supported by the chosen official SDK and
peer; do not invent ACP method names, envelopes or an A2UI-over-ACP protocol.
If an ACP client cannot carry A2UI through a verified compatible mechanism,
retain text/ACP elicitation or use a separately supported standard A2UI
transport. A2UI renderer actions enter through the supported return channel,
are validated against the live surface/catalog, authenticated to the client
and session, and admitted through the existing kernel command/approval path.
A visible button cannot confer permission to execute its action.

**P2d: integration gate.** Start ACP v1 and WebUI clients on the same
session, plus ACP v2 through its supported official protocol adapter.
All receive the same permitted underlying conversation, tool lifecycle and
run result through their own protocol projections. Extension clients may
subscribe to additional fields without exposing them to basic clients.
Permit authorized commands from either client without duplicate tool
effects or bypassing session ownership. Reconnect only one client; verify
consistent snapshot plus ordered replay, separate cursors and no missed
commits. Exercise denied reads/routes, redaction, bounded slow consumers,
multi-destination pipes/sockets, disconnect, cancellation and physical
draining. A client disconnect does not cancel a shared run by default;
explicit authorized lifetime policy may do so. Use independent SDK clients
rather than only self-to-self mocks.
Add independent A2UI v0.9.1 schema/catalog fixtures and an actual renderer
or protocol client, not only hand-authored Phenix JSON. Verify that a client
advertising only Text/Button cannot receive a form or SVG, while another
authorized client with a trusted SVG/form catalog can. Test unknown catalog,
bad component properties, malformed envelopes, invalid surface ordering,
stale catalog after reconnect, forged action names or component refs, cross-session/cross-
client replies, concurrent responders, size/depth limits and disconnect.
Prove that unsupported clients retain text behavior, a valid response is
admitted once, and no invalid UI frame reaches a renderer.

**P2-IO coding handoff.** Implement the existing P2a–P2d track with the
smallest independently provable sequence, not a new frontend framework:

| Packet | Concrete implementation path | Observable gate |
| --- | --- | --- |
| IO1 driver | Extract common graph polling/wakeup/physical drain from `crates/phenix-basic-runtime/src/main.rs` and `acp_sdk.rs` into one host controller; preserve the existing non-`Send` graph-owner thread and pending tickets. | Simultaneous ACP/JSONL/Terminal inputs use the same execution semantics. Cancel or EOF drains exactly once. Existing P1 history and independent ACP-SDK fixtures remain green. |
| IO2 projections | Add typed session snapshot/change/cursor contracts on top of existing Session + EventJournal + ValueRef subscriptions. Introduce per-client bounded queues in the host adapter only after a journal/snapshot atomic-attachment test. | Attach a second client while an event commits; neither missing nor duplicate event appears. Disconnect/reconnect one cursor without advancing the other; deny unauthorized fields before serialization. |
| IO3 typed ports | Define bounded named ports only for concrete producer/consumer needs. Bind each to selected imports and current grants. Preserve raw-stream handling through ordinary maintained I/O; semantic mutations still pass existing kernel root admission. | Fan out one real progress stream to two consumers. Closing a slow consumer does not stall a Model/tool effect. Removed or revoked route denies further delivery. |
| IO4 A2UI negotiation | Add the upstream versioned catalog validation and per-target selection at the frontend adapter. Pass catalog constraints to the selected Model-facing plugin and validate output before delivery. Use the official ACP SDK only where its actual negotiated wire facility supports the payload; otherwise use text/elicitation or a compatible separate transport. | Independent upstream-schema fixture rejects unsupported form/SVG, stale catalog and forged action; a text-only peer still receives normal progress. A valid action goes through current session/client authority once. |

Shared commands after each applicable change: `cargo fmt --all --check`,
focused native tests, official ACP SDK interoperability, packaged Nix product
tests and finally `nix flake check -L` on the exact head. Do not mark the
whole P2-IO track accepted from a pure encoder test. Typed contract names
marked "proposed" above remain editable until a first tested public SDK
consumer depends on them.

**Non-goals.** No second session registry, event journal, agent executor
or kernel-native network/protocol stack. No assumption that clients share
wire bytes or that ACP v2 unsupported operations already work. No new
Phenix-specific UI protocol, catalog format, ACP extension wire contract,
kernel renderer or alternative A2UI parser. Select a maintained validator
and existing renderer/transport libraries after the R1 fit audit; Phenix
owns only its bindings, authorization, selection and delivery policies.

### Library and protocol reuse policy (R1)

This rule applies across Phenix, including future plugins and frontends. Prefer a
well-maintained, widely used Rust library for standard protocols, database
drivers, transports, serialization, async I/O and common utilities. Optimize
for less maintained code and fewer custom failure modes, not the fewest crates.
Use `std` where it already solves the problem cleanly.

**Required for existing and future PRs.** Before implementation or acceptance,
inventory the relevant protocol, parser, transport, persistence and I/O work.
Check established crates first and name the selected crate (with version and
features) or the matching `std` API. Prefer thin typed plugins wrapping those
libraries. Evaluate license, release health, dependency and Nix build cost,
size limits, cancellation/async behavior, failure handling and whether the
crate can preserve Phenix authority and lifecycle semantics. A passing
compatibility test is not a substitute for replacing duplicate protocol
code. Remove the old production implementation after migration rather than
shipping two independently maintained versions of the same standard.
Apply the audit to current open P1 PRs as well as new ones.

A PR that adds custom code for a standard mechanism must first compare
established libraries. Record a short exception when none fits, covering the
missing capability, safety and maintenance costs, and compatibility evidence.
Keep domain-specific contracts, resource checks, journal ordering, graph
semantics, and the frozen Phenix value/ABI encoding in Phenix. Existing custom
ABI code still requires independent conformance tests. A standard library does
not replace Phenix's security checks.

| Area | Default choice to evaluate | Phenix ownership |
| --- | --- | --- |
| HTTP model transport, TLS and connection pooling | Established clients such as `curl` or `reqwest`; an async runtime such as `tokio` only when needed | Model request/response mapping, limits, authorization, cancellation and usage |
| Streaming HTTP/SSE | A maintained event-stream parser integrated with the HTTP client | Typed model progress, ticket lifecycle and journal events |
| SQLite bindings and transactions | `rusqlite` instead of hand-written `sqlite3_*` FFI | Storage keys, revision-checked CAS and session isolation |
| ACP and JSON-RPC transport | Official `agent-client-protocol` Rust SDK if compatible with the required ACP version | ACP-to-typed-event mapping, session grants and cancellation admission |
| JSON and shared encoding utilities | Existing `serde` / `serde_json`; `std` for ordinary conversions | Canonical Phenix wire bytes and declared contract schemas |
| IDs, time, URI handling, retries and other common functions | Established, scoped crates where they remove meaningful code and edge cases | Domain policy and explicit caller-selected behavior |

[The concrete R1 library audit](m1-r1-library-audit.md) records the first migration
and open transport/protocol reviews. The table above remains the stage tracker.

R1 work, before P1 acceptance:

1. Inventory hand-written generic mechanisms in the current kernel, SDK,
   plugins and frontend host. Record the chosen dependency or justified
   exception for each relevant P1 implementation; audit new features during PR
   review so the list does not grow stale.
2. Replace the SQLite plugin's manual C binding layer with a maintained wrapper,
   unless a measured, documented constraint rules it out. Preserve the current
   inter-process CAS, rollback, restart, denial and corruption tests.
3. Review the HTTP Model provider in #803, which already uses the `curl` crate
   for HTTP/TLS. Evaluate its custom SSE framing against maintained parsers;
   retain custom code only with a documented compatibility reason. Verify
   fragmented streams, malformed replies, cancellation, timeouts and errors.
4. Evaluate the official ACP Rust SDK for the frontend and host. Use it for
   supported protocol framing, types and transport. Document any compatibility
   exception, then test against an independent ACP client.
5. Keep external I/O and protocol dependencies in the owning plugins or hosts,
   not in the generic kernel. Check lockfile, license, security, Nix build and
   independent ABI/security tests before closing R1.

R1 is a P1 acceptance dependency and a review rule for later work. It is not
a new runtime layer or a reason to replace proven custom Phenix semantics.

#### R1-ACP: replace the Basic host's custom ACP protocol stack

One production ACP codec/transport is the target, not two implementations
with matching test results. The official Rust `agent-client-protocol` 3.3.0
and `agent-client-protocol-schema` 1.11.0 stable-v1 APIs are the selected crates.
Phenix retains session/resource authorization, per-session ownership,
GraphExecution on its controller thread and durable event journal semantics.

- [x] **R1-ACP1: typed frontend protocol payloads and updates.** Replace
  manually extracted `session/prompt` and `session/cancel` fields with
  official v1 `PromptRequest`, `ContentBlock`, `CancelNotification`, and
  render journal events as typed `SessionNotification` / `SessionUpdate`,
  including `ToolCall`, `ToolCallUpdate` and `ContentChunk`. Keep the ACP
  dependency feature-gated in the ACP frontend artifact. Preserve resource
  denials, tool identities, error state and text joining. The official schema
  may omit `pending` tool status because it is the protocol default.
  PR #841 commit `23f7c408`: official SDK client, canonical conformance,
  flake matrix and generated DAG all passed. This accepts the typed frontend
  slice, not the host cutover.
- [x] **R1-ACP2: SDK-backed host on one graph owner.** Replace
  `run_acp`'s handwritten JSON-RPC reader/router/writer with the official
  SDK's `Agent.builder()` handlers and `Lines` transport backed by Phenix's existing bounded 1 MiB stdin reader. SDK callbacks
  enqueue typed v1 requests on a bounded queue and receive typed responses
  from exactly one Phenix controller. Cancellation/EOF signals reach the live
  graph root without moving Kernel or GraphExecution into protocol tasks.
  PR #842 commit `4474df3a` passed all four exact-head workflows with
  the SDK-backed v1 host selected for independent SDK interoperability.
  The SDK-only production cutover is separately gated in R1-ACP3.
  The stacked `impl/m1-r1-acp-single-sdk-host` branch removes the old host
  and uses the SDK alone. Exact-head legacy-fixture parity remains pending. Keep session/resource grants, durable owner markers and
  journal offsets Phenix-owned. The SDK lockfile resolves 200 packages,
  and SDK `Lines` uses the existing bounded 1 MiB stdin reader and a
  64-entry backpressured frame channel. Exact-head product and EOF parity
  remain an R1-ACP3/R1-ACP4 gate.
- [x] **R1-ACP3: delete old ACP protocol code.** Remove
  `parse_acp_frame`, custom JSON-RPC reply/error framing and the remaining
  notification envelope encoding after the SDK-backed host consumes the
  typed frontend updates. The ACP frontend maps typed journal events to
  SDK v1 updates in R1-ACP1; the host must send them through the SDK transport
  instead of writing JSON-RPC lines itself. Replace, never retain, the old
  host as a production fallback. Keep only Phenix policy and backpressure.
  The `impl/m1-r1-acp-single-sdk-host` slice deletes `run_acp`,
  `drive_acp_prompt`, `parse_acp_frame`, their custom error/reply code and
  old envelope tests. The same slice now removes the Frontend's outgoing JSON-RPC envelope
  and incoming JSON-RPC parser. Only official SDK `PromptRequest`,
  `CancelNotification`, and `SessionNotification` payloads cross the
  native Frontend ABI. The host delegates prompt-to-Submission mapping
  to that plugin; the SDK alone owns wire framing. #843 commit `fb1ba478`
  passed canonical conformance, the flake matrix, generated DAG and the
  independent official SDK client on the same head. R1-ACP4 still owns
  final negative-product parity and integrated P1 release acceptance.
  The official SDK intentionally returns JSON-RPC `-32601` (method not
  found) for `session/cancel` sent as a request, preserving its ID and
  leaving the running root untouched; the prior custom router used
  `-32600`. Test the official SDK's well-defined behavior instead of
  reinstating another ACP protocol handler.
- [ ] **R1-ACP4: prove parity and release.** Run independent official SDK
  stable-v1 client tests, two-process `session/load` replay, ordered tool
  updates, malformed envelopes, unsupported/invalid payloads, bounded input,
  queue saturation, notification flood, active cancellation, request-id
  rejection, disconnected client, session denial and crash recovery against
  the **new SDK host**, not just the legacy host. Resolve supported
  `ResourceLink` content-block behavior explicitly. The Basic Frontend
  now preserves each required v1 resource link as an inert text reference
  (name and URI), never opening it or granting filesystem access. Native
  conformance and process-level SDK fixtures cover this mapping. #853
  adds an independent official SDK client test that passes a typed
  `ResourceLink` to the Basic agent and checks exact journal replay after
  `session/load` and process restart. Its exact-head CI is pending.
  Optional image/audio/embedded-content blocks remain unsupported unless advertised. Pass canonical
  conformance, Nix flake matrix, generated DAG and negative ABI/security
  checks on the exact final head. Then mark R1 and integrated P1 accepted.

The schema module itself adds substantial dependencies: at the current
implementation lock it expands the Cargo workspace from 93 to 169 packages,
while the feature-gated dependency compiles only for the ACP adapter. Review
feature flags and cache cost after R1-ACP2; do not create a second JSON-RPC
transport to save compile time.

- [ ] **R1-ACP5: draft ACP v2 on the same official SDK.** Enable
  `agent-client-protocol` 3.3.0's `unstable_protocol_v2` feature only for
  a separately tested v2 adapter. Use the SDK's v1/v2 protocol routing when
  supporting both on the same endpoint, never a second JSON-RPC stack.
  Adapt v2's prompt-acceptance and session-running/idle semantics rather
  than treating them as v1 `PromptResponse` equivalents. Preserve graph
  controller ownership, authority, cancellation and durable replay. Test
  negotiated v1 and v2 separately, including version mismatch, reconnection,
  v2 session updates, cancellation and the new request lifecycle. Do not
  advertise v2 until those checks pass. Stable v1 P1 acceptance does not
  imply draft v2 acceptance.


  - [ ] **R1-ACP5a — SDK v2 typed Frontend bridge.** Branch
    `impl/m1-r1-acp-v2-typed-frontend` adds optional `acp_v2` to the
    existing native ACP Frontend. It consumes the official SDK schema's
    `v2::PromptRequest`, maps mandatory Text/ResourceLink to the same
    Basic `Submission` as stable v1, and preserves session denial.
    Unknown extension content is ignored by this non-proxy adapter;
    unsupported optional media never gains automatic I/O authority.
    The existing v1 tag remains supported when the feature is enabled.
    This is implementation-in-progress, **not** v2 wire/runtime support.
    The follow-up `impl/m1-r1-acp-v2-cancel-frontend` decodes the official
    `v2::CancelSessionNotification` with the same session authorization and
    typed Basic cancellation action. It tests authorized and cross-session
    cancellation, invalid payloads and stable-v1 coexistence.
    `.github/workflows/r1-acp-v2-frontend.yml` keeps both feature modes
    under `cargo test --locked` and `cargo clippy --locked --all-targets
    -- -D warnings`, then checks the host compile. The official SDK
    interoperability workflow no longer sets the obsolete
    `PHENIX_BASIC_USE_ACP_SDK` environment switch. The sole host remains
    selected unconditionally on R1-ACP3; neither workflow enables v2
    protocol routing. This tests the
    native Frontend contract only; no v2 session controller or wire endpoint
    is advertised. Accept only after feature-gated unit, native ABI and
    exact-head checks.
    **Integrated 2026-10-17:** [#846](https://github.com/matthis-k/phenix-ai/pull/846)
    merged at `ab724c79` and [#847](https://github.com/matthis-k/phenix-ai/pull/847)
    at `d6c042d9` after exact-head checks; the `acp_v2` feature and native
    frontend workflow are on `main`.
    **Integrated 2026-10-18:** [#848](https://github.com/matthis-k/phenix-ai/pull/848)
    merged at `bad8d680`, [#849](https://github.com/matthis-k/phenix-ai/pull/849)
    at `1d092daa`, [#850](https://github.com/matthis-k/phenix-ai/pull/850)
    at `e0c66702`, and [#851](https://github.com/matthis-k/phenix-ai/pull/851)
    at `e722ec29`. Each passed its exact-head CI before merge.
    The earlier workflow-permission blocker is resolved. Default Basic
    remains ACP v1. The v2 SDK router and independent initialize tests
    are accepted as **disabled-by-default preview** only. R1-ACP5b and
    the full session-lifecycle and replay acceptance in R1-ACP5c remain open.
  - [ ] **R1-ACP5b — v2 SDK host lifecycle.** Use `Agent.v2()` handlers,
    typed `v2::Session*Request` and the existing Phenix single-thread
    graph controller. Return `session/prompt` acceptance immediately;
    publish `running` before model progress and `idle` with a stop
    reason after durable graph completion. Implement new/list/resume/
    close/cancel; do not pretend v1's final `PromptResponse` represents
    the v2 lifecycle. Preserve persistence, authority and bounded input.
    The first v2 output slice in `impl/m1-r1-acp-v2-journal-updates` maps
    typed journal text and tool events directly to official
    `v2::UpdateSessionNotification` values. It shares tool-completion
    decoding with stable v1 and keeps SDK framing out of the native plugin.
    It does not yet wire v2 output into the runtime, report running/idle,
    or enable v2 negotiation. Require exact-head v2 native tests, Rustfmt,
    Clippy, canonical conformance and SDK v1 regression before acceptance.
  - [ ] **R1-ACP5c — version routing and independent acceptance.** Route
    negotiated v1/v2 using the *same official crate*, without a second
    JSON-RPC parser.
    The `impl/m1-r1-acp-v2-sdk-router-preview` slice introduces a
    disabled-by-default `acp_v2_preview` Cargo feature. When explicitly
    enabled, the official SDK `Agent.protocol_router()` handles v1 and
    v2 initialize requests, with v2 session capabilities unadvertised.
    The default production build retains the tested SDK v1 host and its
    version rejection behavior. The preview has no v2 session handlers and
    is not release support. Native `cargo check --locked` is required for
    the opt-in feature. The stacked `test/m1-r1-acp-v2-router-sdk-client` PR adds independent official SDK v1/v2 initialize checks, without advertising unfinished v2 session capabilities. Before release, run independent v2 SDK client tests for interleaved
    updates, replay cursors, cancellation, session close/reopen and
    v1/v2 version mismatches. Only then advertise draft v2 support.


### CI performance and flake-aware planning

**15 October CI quality update — part of AI-CI1 through AI-CI4, FC1 through FC4; no new parallel milestone track.** [The current CI quality assessment and historical matrix evidence](m1-ci-matrix.md) specify measured baselines, execution surfaces and detailed acceptance. This roadmap owns the implementation priority and exit criteria below. PR [#900](https://github.com/matthis-k/phenix-ai/pull/900) introduces this guidance only; it does not claim a build-time speedup or change executable CI.

**Single source of truth and target:** `flake.nix` defines executable checks, and the compiled SDK, resolver, graph compiler and kernel enforce their actual contracts and authority. CI verifies **functionality**—including failures, denial, restart, and real plugin compositions—not a separately maintained notion of good architecture. Never introduce an architecture CI rule registry, model-based CI design reviewer, shadow graph/permission validator or duplicate YAML check catalog. A negative test calls the real implementation and expects its rejection. Architecture guidance and ADRs inform development outside CI.

| Priority / existing stage | Action | Measurable completion criterion |
| --- | --- | --- |
| **1. Diagnose (AI-CI1 / FC4)** | Capture the current five-shard PR matrix's critical path and time to first *actionable functional* result. Attribute Nix build, reused versus rebuilt derivations, cache transfer, setup, queue, tests and artifact upload. Measure cold and warm on comparable exact-head inputs. | Publish a reproducible bottleneck ranking with wall time, billed runner-minutes, p50/p95 feedback latency where sufficient runs exist, cache bytes/hits and full check inventory; no speculative speedup claim. |
| **2. Reuse builds (AI-CI1)** | Reduce repeated compatible Cargo compilation and overly broad Nix source inputs; keep independently built ABI, linker, security-negative and external-SDK evidence. | Same-source before/after builds preserve all required checks and real product outcomes while reducing repeated compilation or elapsed time. |
| **3. Surface failures early (AI-CI2)** | Keep cheap formatting/policy integrity checks independent. Add or prioritize inexpensive deterministic *functional* tests when they catch an existing expensive integration failure sooner; reuse owning Rust/Nix checks rather than copy test logic. | Earlier meaningful failures with unchanged required end-to-end behavior and complete PR coverage; no unbounded model inference or flaky network dependency. |
| **4. Tune orchestration (FC1 / FC3 / FC4 / AI-CI3)** | Use the flake-discovered inventory, verified aggregate, measured shared prebuild hotset, cache and shard hints; adjust the existing five-shard strategy or 4×2 remote builder settings only if comparable metrics warrant it. | Every exact-head check accounted for; canceled, skipped, failed or missing required checks never pass. Better critical path or cost demonstrated without reducing test/security coverage. |
| **5. Select impact conservatively (FC2 / AI-CI4)** | Only after real Cargo/Nix reverse-dependency and configuration-input proofs, consider running proven affected PR checks. Unknown/shared/toolchain changes fall back to full. | Adversarial skip-proof tests pass; complete inventory still required on integrated main, release and manual full verification. An unrun check is never labeled passed. |
| **6. Maintain reliability (all stages)** | Classify flakes, eliminate nondeterministic tests, retain actionable logs, pin inputs, cancel superseded PR work, and keep untrusted PR execution credential-free. | No silent retry turning a regression green; each reported flake has a reproducer/owner. Privileged OIDC/cache publication stays on trusted main/release triggers. |

**Feedback cadence:** cheap gates should return useful failures in minutes (the DORA fast-feedback guideline is a target, not a guaranteed runtime); the full PR suite stays comprehensive even if it takes longer while optimizations are in progress. Trusted main/release runs validate the *actual integrated SHA*. Manual benchmarks examine cold/warm caches and worker counts but are not additional definitions of correctness. Preserve functional, security-negative, plugin-composition, ACP interoperability and shipping evidence throughout the CI changes.

CI0 is the measured parallel matrix baseline, not completion of CI optimization.
At the 12 October exact-head comparison, the matrix took 11m10s versus
25m18s for the serial job. Later successful matrices took 9m33s to 14m03s.
The prior old-main selective CI took about 5-6 minutes with different coverage;
do not use it as a full-suite speedup claim. Logs show repeated independent
Cargo compilation of shared contract, SDK and macro crates.

CI0.1: PR #863 replaces the overlapping PR #845 with GitHub Actions
concurrency keyed by workflow and PR number. Canonical conformance, flake
matrix, generated DAG and official ACP SDK interoperability each keep their
own exact-head run. Only a newer commit to the same PR cancels its previous
runs; push and manual dispatch runs use independent run IDs. The change does
not remove Nix checks, weaken product acceptance, or alter cache trust.
Acceptance requires observed cancellation of superseded PR runs, isolation
between different PRs, preserved manual runs and full checks on the final
head. Treat these as CI orchestration tests, not evidence that G4 or P1 is
accepted.

[The M1 matrix rollout](m1-ci-matrix.md) records the current benchmark and
shipping gate. [The proposed generic flake-aware design](https://github.com/matthis-k/phenix-flake-ci/pull/17)
records FC1-FC4, owned by `phenix-flake-ci`. AI-CI1-AI-CI4 are consumer
integration milestones in this roadmap. The library schedules declared Nix
checks and proves selections; Phenix AI owns the derivations, compiler profiles,
fixture packaging and release semantics. Maintain one Nix check definition.

Prioritize AI-CI1 before adding further matrix shards. First remove redundant
Cargo builds and broad source invalidations. Next prove fast check profiles,
then use FC1-FC3 to generate a complete job plan. Keep the original shipping
job required until the generated aggregate verifies every check and exact-head
release report. FC2 and FC4 add selective PR execution and shared caches after
those guarantees hold. Only validated main or trusted revisions may publish
cache outputs; PRs consume them read-only.

Collect same-commit cold and warm runs, per-crate compilation counts, Nix
built/substituted derivations, cache bytes, wall time and billed runner-minutes.
Initial performance targets, not acceptance claims, are 2-4 minutes for a
small warm plugin PR and 5-8 minutes for warm full validation. A faster run
must preserve the same semantic, negative, ABI and shipping evidence.


G1/G2: [PR #782](https://github.com/matthis-k/phenix-ai/pull/782) owns typed graph declarations, ELF verification and the prepared multi-root DAG. Full exact-head shipping CI 38220130161 passed at `1615ed52`, but definition-only and negative semantic evidence remain before acceptance. [Its contract](m1-graph-declarations.md) owns the gates.

G3: [PR #783](https://github.com/matthis-k/phenix-ai/pull/783) introduced
scoped action admission and a state-backed join. CI 38223182503 failed because
the fixture omitted the property resource grants. #785 inherited that failure.
The `impl/m1-serial-graph-semantics` follow-up fixes the fixture and implements
[the frozen serial execution contract](m1-serial-graph-contract.md). Skip
suppresses active dependents transitively while independent nodes continue.
FailRoot stops the remaining action. Kernel failures always stop execution.
A trusted host observes admission, results, suppressed nodes and completion
under one correlated root. Root requirement resource grants bound delegation;
resource denial preserves cell epochs and revisions. The SDK-only native gate
checks four nodes from three independently built plugins. Local workspace tests,
Clippy and native execution pass with Rust 1.96. Full exact-head shipping CI 38228548634 passed at `672b7f09`. G3 serial semantics are proven; full stage acceptance awaits the G1/G2
definition-only follow-up; streaming, cancellation and state triggers remain G4.

### G4f native pending-hook cancellation

The independently compiled graph-C provider now has a pending state-hook
operation. Graph-A commits a watched `pending_signal`; G4 admits the hook
under the original action root. The hook owns a worker file descriptor and
supports both immediate cancellation and a separate physical drain.

The native conformance test verifies that cancellation blocks new state
writes, leaves committed values unchanged, emits one terminal event, rejects
shutdown until the retained worker drains, and releases the worker file
descriptor. PR #862 supplies the fixture. Its exact-head CI is pending.
G4 remains open until these tests and integrated product checks pass.

### G4d bounded synchronous reactive execution

The synchronous `submit_action` helper has no caller cancellation point while
a hook repeatedly writes its own trigger. Limit this helper to 1,024 pump
turns. Exceeding that bound cancels the existing root and returns a typed
`InfrastructureFailure`. The asynchronous `begin_action_scoped` and
`pump` path remains host-driven and cancellable without this bound. This
is a turn limit, not a timer or a second scheduling mechanism.

The native graph fixture includes an opt-in self-triggering receipt hook. It
requires the bounded failure, zero remaining physical leases, and successful
admission of a new root afterward. Exact-head CI must pass before accepting
this test. This does not replace G4 pending-hook cancellation and draining
evidence.

### G4c same-root hook execution

`GraphExecution` captures the live-state epoch at action admission. When
the active event wave finishes, it reads the committed watchable properties,
uses the already prepared state-trigger DAG, and invokes affected hooks in
the same root. Each hook receives its matching changed property identities
once per observed epoch. Its operation, imports, resources and cancellation
use the existing dispatcher and native ticket lifecycle.

Hooks that commit another watched property produce a later epoch. The graph
pump admits the next wave after the current one settles. A domain failure
remains visible across waves; a FailRoot failure or cancellation prevents new
admission. The terminal graph event remains single-shot, and the synchronous
entry point drains ready hook waves before returning.

The existing three-plugin native graph fixture now declares a watchable
signal in graph-A and a typed state hook in graph-C. It checks that the hook
receives only its changed signal and retains the original root correlation.
The negative fixture removes the watch grant and requires offline rejection.
This evidence is not accepted until exact-head native conformance passes.

Remaining acceptance: pending hook cancellation and physical draining;
multiple epochs and repeating state changes; denied resource scopes; integrated
P1 product checks. G4 and P1 stay unchecked.

### G4b state-trigger preparation

State change hooks use the canonical graph declaration's existing `triggers`
field with a `state:<plugin>/<property>` identity. The compiler resolves each
property against prepared package metadata, requires `watch = true`, and
verifies the node owner's existing `property.<name>@1/watch` grant. Hook
operations accept `{ epoch: U64, properties: List<String> }`. They read values
through their selected typed property import, so no untyped cell contents or
implicit capability are passed in the trigger.

`CompiledGraph::state_change_activation_order` uses the same DAG projection
as regular action events. It activates only declared watchers. `after` still
orders active predecessors without triggering dormant ones. This phase
prepares and validates hooks but does not call them. G4c must admit these
nodes with the existing root, handle multiple committed epochs, and prove
cancellation and physical lease draining. Do not mark G4 or P1 accepted on
the compiler tests alone.

### G4 committed-epoch projection

`Cells::watched_changes_since(epoch)` computes the latest committed epoch and
sorted identities of watchable properties changed after the cursor. It reads
canonical cell snapshots, not a second event queue. Multiple writes to one
property between calls coalesce to one identity. Failed commits add no change.
Unwatched fields are excluded. The trusted `GraphExecution` exposes this
projection without changing the native ABI or introducing a second root.
The existing `phenix-m1-graph` fixture now marks graph A and B results
watchable and checks two real native commits across a pending node. Its
`graph.json` report records the observed before/after epoch cases. The
exact-head native shipping gate must pass before accepting this slice.

This slice does not schedule hooks. Remaining G4 work must bind each property
trigger to a statically verified graph event, use the same admitted root and
authorized dispatcher, and admit only affected nodes after the commit. A
dependent hook that writes a property must create a new epoch. Do not mark G4
accepted until native tests prove active joins, dormant predecessors, denial,
pending cancellation and lease draining against actual changed cells.

### Event admission and graph rules

The typed envelope identifies the event, contract/schema, payload, resource,
causation/correlation and admitted generation. Authority derives from the
host/root admission, never from an event's self-claimed origin. Discovery exposes
the prepared graph's action IDs, schemas, descriptions and authorization needs.
Freeze this contract before adding G1 code or changing the native wire format.

An `after` edge constrains ordering; it does not trigger its successor.
If A and B activate, C waits for both active predecessors. If only A activates,
inactive B cannot block C. Disconnected roots remain valid. Follow-up events
advance model turns in new epochs, so the static graph stays acyclic.

The journal retains every accepted semantic event in order. Value watches can
coalesce and cannot replace journal cursors. During pending I/O, one same-root,
non-reentrant control pump admits cancellation and publishes progress.

### F3 post-P1 reactive plugin migration and frontend proof

F3 is a **migration milestone**, not only a new SDK example. Use S5's
macro-generated typed `on_change` and ordered `on_event` handlers to remove
the manual subscriptions, journal polling and event routing already present
in M1 plugins. Migrate existing providers and consumers where they benefit;
do not introduce an ACP-specific abstraction or a second dispatcher. Keep
P1's previously accepted behavior unchanged during this post-P1 migration.
The target annotation spellings are proposed until S5 freezes them.

#### Required migration inventory

These are actual packages in the active minimal M1 tree. Each row is an F3
completion requirement, with a concrete behavior to preserve.

| Status | Package or component | Reactive migration and acceptance |
| --- | --- | --- |
| [ ] | `phenix-plugin-basic-frontends` (ACP, Terminal, JSONL variants) | Replace explicit decode/render/cancel event plumbing with typed input-event and output-event declarations. Stream text, tool start/finish, failures and cancellation in journal order; use selected session scopes and independent delivery cursors. ACP and Terminal/JSONL must both pass product fixtures. |
| [ ] | `phenix-plugin-event-journal` | Publish an authorized ordered-event notification only **after** the durable append commits. Generate subscriptions and observer dispatch instead of requiring frontend loops to discover new pages. Retain monotonically ordered sequences, independent per-observer checkpoint/replay and at-least-once delivery. A wake notification never substitutes for persisted events. |
| [ ] | `phenix-plugin-basic-session` | Expose committed session-status and checkpoint changes as typed, scoped reactions. Ensure run state, cancellation and explicit interrupted-tool reconciliation remain durable. Never infer process death from plugin-local state or retry unknown external effects. |
| [ ] | `phenix-plugin-basic-agent` | Move progress, model/tool continuation and completion event wiring to declared typed reactions where applicable. Keep the existing agent graph, pending-import/ticket lifecycle, call-ID correlation, tool-error recovery and exactly-once checkpoint/effect rules. Do not implement a second agent loop through callbacks. |
| [ ] | `phenix-plugin-model-http` | Use generated output-event publication and completion/cancellation bindings instead of bespoke reactive notification wiring. Retain the established `curl`/SSE transport, worker I/O, typed model stream, pending ticket draining and journal consistency; no duplicate text deltas after resume. |
| [ ] | `phenix-plugin-basic-memory` (Skills and Context providers) | Make committed `Skills::set` and `Context::set` revisions observable through typed `on_change` hooks, so selected consumers refresh or invalidate derived context without polling. Keep Storage CAS, session authorization and catalog/query behavior. |
| [ ] | `phenix-basic-runtime` (host executable, **not** a plugin) | Replace the ACP-specific `drive_acp_prompt`/journal `render_new` coordination and equivalent Terminal/JSONL polling with a shared event-driven frontend controller. The host retains stdio/network I/O, protocol framing, bounded queues and flushing. The kernel retains the single generic graph wake/pump mechanism. |

The migration cannot close by modifying only the ACP adapter: verify the
event producer, durable journal, session owner, agent, and frontend consumer
as one end-to-end composition. Retain the separate independently packaged
providers and public typed contracts.

#### Conditional and future plugin adoption

| Status | Package or planned plugin | Decision |
| --- | --- | --- |
| [ ] Review | `phenix-plugin-basic-tools` | Migrate **only** tool-catalog mutations or lifecycle subscriptions that otherwise need polling. Its current deterministic synchronous `ToolCatalog`/`ToolExecute` functions stay ordinary services; do not duplicate the agent's existing ToolStarted/ToolFinished journal events. Record the decision in F3 evidence. |
| [ ] Review | `phenix-plugin-model-scripted` | Keep the synchronous model implementation simple. Extend scripted fixtures to prove reactive agent/frontend consumers and correct ordered events; migrate only if real subscription boilerplate is added. |
| [ ] Adopt on implementation | Planned `phenix-plugin-logging` and `phenix-plugin-introspection` (L1/I1 after K3) | Use generated subscriptions from the outset for authenticated kernel diagnostics and authorized state/contract changes. Keep logging sinks and inspection projections in their plugins, not the kernel. Diagnostic events remain distinct from the durable semantic journal. |
| [ ] Adopt when applicable | Future separately packaged Skills, Context, Workspace or graph-decorator plugins | Use the same typed change/event declarations whenever they maintain derived state or observe tool/runtime events; no adapter-specific watcher registry. Do not claim these packages exist in the current M1 tree. |
| N/A | `phenix-plugin-storage-sqlite` and `phenix-plugin-storage-fixture` | No compulsory rewrite of transactional CRUD providers. They stay synchronous unless an actual consumer needs post-commit notifications; preserve CAS and rollback semantics. |

#### F3 completion and migration order

1. Accept S5's independent, SDK-only `on_change` and `on_event` conformance,
   including generated subscription lifetime, authority, and ordered-event
   cursor support.
2. Migrate EventJournal and Session publication first, then agent/Model
   producers and Skills/Context state-change consumers. Prove no lost wake,
   unauthorized callback, duplicate journal append or implicit tool replay.
3. Migrate all three Frontend variants and the common host controller.
   Remove superseded per-frontend polling, registrations and dispatch glue;
   keep transport-specific ACP encoding in the owning adapter and adopt the
   official ACP SDK where R1 permits.
4. Run the same Terminal, JSONL and independent official ACP SDK product
   fixtures before and after. Require streaming text/tool updates in order,
   cancellation, bounded slow observers, restart and session continuation,
   cross-session denial, owner retirement and observer replay. Cursor
   acknowledgements follow successful output delivery. Count removed
   boilerplate and reject any second scheduler, registry or root-admission
   bypass. Check and record the conditional decisions above.

State-watch notifications may coalesce revisions; ordered journal events
must not coalesce or disappear. Generated callbacks cannot bypass scoped
admission. S6 gates independently concurrent ACP sessions, not initial F3
migration. F3 remains post-P1 and cannot retroactively block P1 shipping.


### Basic product boundaries

The agent imports prepared selected providers. Model requests contain previous
assistant calls and every tool result. Session/storage plugins retain history,
checkpoints and run status. A crash during an external tool effect preserves an
indeterminate result rather than blindly repeating the call.

A native shell plugin needs a selected workspace execution provider. Kernel
import mediation alone does not implement an OS sandbox. The product reports
only the isolation its selected provider actually supplies. Model endpoint,
model identity and credentials are configured outside CI. Deterministic scripted
and local HTTP fixtures exercise the same contracts used by a live provider.

The final product gate covers start, progress, finish, failure and cancellation;
multi-turn history; multiple tools; typed tool-error recovery; loaded skills and
context; replaceable providers; usage; streaming; durable session continuation;
caller-selected limits; and denied imports with zero tool effects. Planning,
compaction and MCP remain later plugin work.


### B2 SQLite restart slice

`impl/m1-basic-storage-sqlite` is stacked on the Session provider. The new
`phenix-plugin-storage-sqlite` implements the existing public Storage contract.
It uses SQLite transactions and a revision check under `BEGIN IMMEDIATE`.
The provider keeps no process-local record map. The runtime resolves
`PHENIX_BASIC_SQLITE_PATH` from the environment, defaulting to
`$XDG_STATE_HOME/phenix/basic.sqlite` (`$HOME/.local/state/phenix/basic.sqlite`)
and creating its parent directory; it never creates an implicit in-memory store.
An explicit value still wins, and a file-backed path remains required. This is a
file-backed provider, not an OS sandbox or a kernel persistence mechanism.

The `phenix-b2-sqlite` conformance gate executes two separate processes against
the same database. The first checkpoints a Running session with two pending
call IDs, appends journal events and tests a failed multi-key CAS rollback.
The second loads the persisted session, checks the Indeterminate transition,
reads the journal with two independent cursors and explicitly reconciles both
call IDs. The conformance and shipping checks include the resulting report.
The original in-memory fixture remains separate.

The `faults` phase extends the SQLite process-boundary gate. The Nix check
installs a temporary SQLite trigger that aborts the second update in a
two-record CAS transaction. The conformance binary requires the resulting
StorageFailed error and verifies that neither the first record's data nor
either revision changed. It also rejects duplicate edit keys and revision
overflow before writing. The driver removes the trigger before resuming
the original session in another process. The new report is
`sqlite-faults.json`. Exact-head CI must pass before this case is accepted;
database version migrations are tracked by the separate B2 schema slice.

SQLite Storage now proposes a single schema version, v1. Existing files with
`PRAGMA user_version = 0` retain their records and move to v1 within
an immediate transaction. Concurrent first opens recheck the version under
the write lock. Every open validates the four required columns, types,
nullability and primary-key positions. Future versions, malformed legacy
tables and missing v1 tables return StorageFailed before any record operation.
The Nix B2 gate uses separate processes to verify legacy restart, schema
rejection and continued restart on the original v1 file. The report is
`sqlite-schema-migration.json`. This does not claim future schema upgrades;
the exact-head full conformance gate is pending.

The observer slice below adds per-observer persistence and startup replay.
B2 remains unchecked until exact-head acceptance, migration/versioning checks
and the remaining transaction fault cases pass.

### B2 durable observer checkpoint slice

`impl/m1-observer-checkpoints` extends EventJournal and BasicOutput with typed
observer load and checkpoint operations. The Journal plugin stores one
revision-checked record per session and observer through its selected Storage
provider. BasicOutput forwards through the selected Journal import. The
[observer contract](basic-observer-contract.md) owns record validation, offset
bounds, CAS conflicts and crash semantics.

Terminal and JSONL restore separate named cursors at startup. The host writes
and flushes a rendered page before persisting its last sequence. Failure leaves
the page available for replay. A crash between delivery and commit can repeat
the page; delivery is at least once. ACP session/load retains its explicit
history replay and connection-local offsets.

The existing `b2-sqlite` gate now includes a five-process observer fixture with
independent cursors, abrupt exits before/after acknowledgement, stale revisions,
backward/future offset rejection, session denial and corrupt-record rejection.
The product gate includes restart and failed-pipe replay checks. The observer
slice passed [full conformance](https://github.com/matthis-k/phenix-ai/actions/runs/38321349932)
and [matrix CI](https://github.com/matthis-k/phenix-ai/actions/runs/38321349890)
at `f235d68f6b5d236328971b31654e19db69dec1e5`. Full B2 remains open for migrations
and the remaining transaction fault cases.

### B2 session/checkpoint slice

`impl/m1-basic-session` stacks on the journal branch and provides the public
`Session` contract from a separate native plugin. Snapshot open, revisioned
checkpoint and pending tool reconciliation all delegate to an explicitly
selected Storage implementation. A restarted provider's first open of a
previously Running snapshot with unresolved tool calls marks it Indeterminate
through a CAS transaction. It never silently repeats an indeterminate tool;
the caller must reconcile its recorded call ID. A native fixture seeds a
checkpoint in independently packaged Storage to verify the transition,
stale revisions and cross-session denial.

This is not process-restart or SQLite proof: the fixture is volatile and the
simulated reload seeds the stored checkpoint explicitly. SQLite durability,
journal-backed replay integration, true restart/resume, observer cursor storage
and fault-injection remain the B2 product acceptance gate.

### B2 semantic event journal slice

`impl/m1-basic-journal` is stacked on `impl/m1-basic-tools` and
implements an optional `EventJournal` provider using the existing typed
`Storage` import. The provider assigns contiguous sequence numbers only
after reading the revisioned persisted snapshot, and writes through one
Storage compare-and-swap transaction. Concurrent revision conflicts stay
visible to callers instead of being silently replayed. Read offsets are
independent and read-only. It refuses cross-session and malformed persisted
records.

A separately packaged in-memory Storage plugin is used **only as a native
conformance fixture**. It does not prove durability, process restart, SQLite
storage, independent observer checkpoint persistence, or session replay.
Those are still B2 acceptance requirements. No persistence or journal
logic enters the kernel.

### F2 ACP v1 adapter slice

`impl/m1-basic-acp` builds the same Frontend source crate with an exclusive
`acp` feature to yield an independent native package. It accepts ACP v1
JSON-RPC `session/prompt` text blocks and `session/cancel` notifications,
rejects mismatched session IDs and unsupported content, and emits
`session/update` notifications from journal events. The native F2 check loads
the actual selected plugin and verifies this codec.

The ACP connection host now handles initialize, scoped session/new,
session/load, session/prompt, journal-backed notifications and end_turn replies.
Each session selects the same artifacts with distinct kernel-authorized resource
grants. A Python ACP stdio fixture creates a session, sends a prompt, restarts
the host process, loads the persisted SQLite session, continues it and creates
a second isolated session. This still uses a synchronous scripted model, so it
cannot interrupt active model I/O. Streaming cancellation, richer ACP client
conformance and exact-head shipping remain before F2/P1 acceptance.

The branch includes the B5 nested Storage ceiling fix and removes direct agent
Storage grants from the product composition. Runtime and frontend formatting
pass locally. The native ACP codec and two-process stdio/reload fixture pass
with independently packaged native providers. Exact-head Nix CI remains pending.

### P1 terminal and JSONL host smoke

`impl/m1-basic-runtime` packages `phenix-basic-runtime` outside the kernel.
It loads one selected M1 composition with terminal and JSONL adapters,
independent model/tool/skill/context/session/journal/storage providers, and the
definition-only Basic contracts. The host asks the frontend plugin to decode
a prompt, submits `phenix.basic.submit` through the kernel graph and obtains
output by replaying the typed journal through BasicOutput and the selected
frontend renderer. Terminal and JSONL use the same kernel and Basic agent.

The `basic-host-smoke` Nix check submits a prompt and a multi-tool turn in
separate product processes against the same SQLite file. It verifies visible
terminal and JSONL answers, rather than proving only isolated plugin ABIs.
This initial launcher uses one preselected session resource, `s1`; it is not
yet a full multi-session host. The synchronous agent cannot accept live
cancellation while model I/O blocks. The full P1 gate remains unchecked until
pending/streaming, ACP, session/resource selection and all product acceptance
cases pass on the exact integrated head.

The product composition now follows the B5 nested Storage ceiling declaration.
It grants Storage operations only to the owning Session, Journal and memory
providers. The runtime passes Rust formatting. Local native Terminal and JSONL
smoke tests pass in separate processes against one SQLite database. These
checks cover the synchronous slice; canonical Nix CI is still required.

### P1 live terminal and JSONL controller

`impl/m1-p1-live-jsonl-controller` gives the host a dedicated stdin reader
while the controller thread owns the kernel and pending graph. It continues
calling `GraphExecution::pump` while handling follow-up and cancellation
packets. The host reads journal deltas through explicit, session-scoped grants
on the independently selected EventJournal provider; rendering and observer
checkpoints use the same existing Frontend and Storage-backed Journal plugins.
Cancellation waits for graph ticket draining, then checkpoints a cancelled
Session and appends a typed `Cancelled` journal event. The next prompt is
allowed to continue in that session. Prompts queue with a 32-entry limit.

The `basic-pending-http-product` fixture extends its real fragmented HTTP
test with live text, cancellation and same-session continuation. No new kernel
dispatcher, agent entry point or scheduler is introduced. These changes remain
unaccepted until the exact-head Nix shipping gate passes. The terminal and
JSONL launcher still uses a single selected session, and ACP cancellation
still requires its own nonblocking protocol controller. Model-only interrupted
runs also need ownership fencing for safe recovery: a fresh Session instance
cannot infer that another process's live model request has ended. An automatic
recovery attempt in closed PR #817 was rejected for this reason.

### R1 standard SSE framing

`impl/m1-r1-standard-sse-parser` replaces custom line/data accumulation
in the pending HTTP Model plugin with `sse-parse` 0.1.0. Existing `curl`
callbacks and the typed Model delta/usage decoder remain. The parser enforces
the canonical portable size bound; malformed UTF-8 remains a Model error.
The unit suite adds bytewise BOM/CRLF fragmentation, invalid UTF-8 across
chunks, and oversize rejection. The real pending-HTTP Basic and ACP fixtures
remain part of the shipping gate. The 66-package workspace Cargo.lock now pins the parser and its
dependencies. Exact-head shipping CI remains pending.

### P1 owner-marked recovery

`impl/m1-p1-fenced-session-recovery` extends the cooperative OS lock
with a durable run-specific marker in the same lock file. After graph
admission, the host writes `phenix.basic.owner.v1` and the checkpointed
run ID and synchronizes the descriptor. The active root and its Model
worker remain owned by that host. A later process can recover a
`Running` snapshot only while holding the exclusive session lock and
only if its marker matches the exact persisted run ID. Unmarked legacy
runs, truncated markers and mismatched IDs remain `Running` and reject
new prompts.

For a verified interrupted Model turn, the Session plugin checkpoints
`Failed`. If external tool calls remain unresolved, the run moves to
`Indeterminate` and still requires explicit call-ID reconciliation.
Both updates use existing revision-checked Storage and append a typed
`Failure` journal event. On clean graph completion, the host removes
the marker before releasing the lock. A model process-kill fixture
checks restart, an exact matching marker, no automatic retry and
subsequent prompt completion.

The marker is an admission and recovery protocol for cooperating local
hosts using the file-backed SQLite product. It is not a distributed lease.
A crash between the first `Running` checkpoint and marker synchronization
remains fail-closed. Exact-head product and conformance checks are pending.

### P1 cooperative local session locking

`impl/m1-p1-session-advisory-ownership` holds a per-session exclusive
file lock while Terminal, JSONL or ACP drives an admitted graph. The
runtime derives the lock path from the canonical file-backed SQLite path
and a hex-encoded session identity. It uses Rust's standard `File::try_lock`
and retains the descriptor until graph drainage and cancellation
checkpointing finish. Other cooperating local hosts fail admission before
issuing a Model or Tool effect. Locks for distinct sessions are independent,
and the operating system releases a lock when its process exits.

This guards active local execution only. It does not prove that historical
`Running` records were created by a lock-aware host, so the Session
plugin still does not automatically recover abandoned runs. A durable
owner marker and explicit recovery transition remain P1 work. Exact-head
CI and the two-process pending-HTTP product fixture must pass.

### P1 cross-process Session ownership safety

`impl/m1-p1-session-crossprocess-safety` removes implicit recovery from
`Session::open`. A newly loaded Session plugin can read a durable
`Running` snapshot, but its local state cannot prove the prior process
has exited. Changing a live run with outstanding tools to
`Indeterminate` would corrupt another owner's session. The plugin now
returns the stored snapshot without a write. The Basic agent's existing
`Running` and pending-tool admission checks reject conflicting prompts.

The pending-HTTP fixture holds a real Model stream open, launches a second
process against the same SQLite session and requires a `Conflict` before
allowing cancellation and normal continuation. Tool reconciliation remains
explicit and never replays an external effect.

This closes the unsafe inference, not crash recovery. P1 still requires a
provable exclusive session owner, fencing or an equivalent mechanism before
a new host may mark an orphaned `Running` snapshot `Failed` or a run
with pending tools `Indeterminate`. Exact-head CI is pending.

### P1 typed tool completion status

`impl/m1-p1-typed-tool-outcome` adds a Portable `ToolCompletion { failed }`
payload to the existing `ToolFinished` journal event. The Basic agent
encodes the outcome once after the tool result has been checkpointed;
the ACP Frontend adapter decodes it and selects `completed` or `failed`
for `tool_call_update`. Payload-free historical events retain their old
successful interpretation, while malformed typed payloads are rejected
instead of misreported. The frontend unit gate checks success, failure,
legacy empty payloads, corrupt data and call-ID preservation.

This is a semantic-event plugin change, not a new kernel event type or
ACP-specific graph scheduler. Full shipping conformance and official SDK
interop must pass before acceptance.

### P1 ACP tool event delivery

`impl/m1-p1-acp-tool-events` extends the real ACP stdio product fixture
with an entire scripted two-tool turn. It checks typed `ToolStarted` and
`ToolFinished` journal events become `tool_call` and
`tool_call_update` notifications with the same IDs, ordered starts before
completions and an exact final text chunk. The independent official ACP SDK
client now executes that tool turn and requires at least six successfully
decoded typed notifications across its initial prompt and tool continuation.

These tests exercise the existing `Frontend::render` contract and the
shared host journal-to-stdout path. No kernel event subscriptions, duplicated
ACP scheduler or plugin ABI changes are introduced. Acceptance requires the
exact-head canonical matrix and official SDK compatibility workflow.

### P1 independent ACP SDK integration

`impl/m1-p1-acp-sdk-interop` adds a separate official Rust ACP SDK 3.3.0
client. It launches the packaged `phenix-basic-acp` executable over stdio,
negotiates stable v1, creates a session, sends a prompt and counts typed
session updates. The dedicated CI job is distinct from the canonical
Phenix-wire tests. The test crate has its own Cargo workspace; neither the
kernel nor Basic runtime imports the SDK. The initial SDK-client job passed on `9534a35d`, and its transitive
dependencies are now locked. The updated exact-head `--locked` run and the
canonical shipping checks must still pass before P1 acceptance.

### P1 ACP initialization and JSON-RPC errors

`impl/m1-p1-acp-protocol-boundary` keeps malformed idle JSON-RPC from
terminating the connection. It returns the standard parse/invalid-request
errors. It rejects missing or invalid protocol versions without consuming
the initialization slot. The original handwritten host permitted a higher
client version to negotiate down to v1. The official SDK 3.3.0 v1-only
`Agent.builder()` instead rejects version 0 and draft v2 at its protocol
compatibility boundary with JSON-RPC invalid request `-32600`;
after a rejected version the client may retry stable v1. Dual-version
negotiation requires the separate SDK router (R1-ACP5c). The stdio product
fixture proves rejection and subsequent stable-v1 initialization.

### P1 live ACP controller

`impl/m1-p1-live-acp-controller` replaces the blocking ACP prompt path
with a reader thread and one kernel-owning control loop. A pending prompt
drives the existing `GraphExecution::pump`. Journal reads and Frontend
rendering reuse `GraphExecution::invoke_selected` while the root is live.
Cancellation is decoded by the ACP Frontend plugin and cancels the same
graph root. The host waits for physical draining, persists the cancelled
Session and emits a journal event before answering the prompt with
`stopReason: cancelled`. It then admits the next turn on the same session.

The connection retains other ACP requests in a bounded 64-entry FIFO until
the current prompt completes. This avoids a second kernel thread, an
additional scheduler and modification of the graph execution ABI. The
`acp-pending-http-product` fixture uses an independent chunked HTTP
server to test a streamed update, live cancellation and same-session
continuation. All acceptance remains pending exact-head shipping CI and an
independent ACP client compatibility check.

### P1 JSONL session selection

`impl/m1-p1-jsonl-multisession` routes each JSONL packet by its declared
session using the existing ACP `scoped_kernel` composition. Each session
gets independent resource authority and its own Storage-backed journal
observer checkpoint. The controller runs one graph at a time and queues
other sessions' prompts until the current root completes. A cancellation
packet can affect only its matching active session. Starting JSONL without
input still replays undelivered events for the default `s1` session.

The `jsonl-multisession-product` check interleaves two sessions, restarts
the host, checks independent output and journal offsets and verifies that
an idle cancellation cannot change another session. Concurrent execution
and multi-session Terminal support remain outside this slice. The check
has not yet passed canonical CI.

### P1 Terminal session selection

`impl/m1-p1-terminal-session-selection` adds an optional
`--session SESSION` argument to the Terminal runtime. Its default remains
`s1`. The launcher reuses ACP's `scoped_kernel` admission to bind the
selected session resource and applies the existing journal observer checkpoint
for that session. Terminal retains the same serial graph controller and the
same frontend plugin. No extra scheduler or ABI is introduced.

The `terminal-sessions-product` fixture switches between two sessions,
restarts the launcher, checks independent observer cursors and rejects an
invalid session identifier. Full P1 integration and external ACP compatibility
still require exact-head CI and independent verification.

### F1 terminal and JSONL frontend codecs

`impl/m1-basic-frontends` builds two standalone Frontend plugins from
one source crate with exclusive Cargo features. Both decode input into the
same typed `phenix.basic.submit` action payload and render journal events.
JSONL enforces its declared session against the authenticated client session.
The host controller owns active graph roots. A frontend's cancellation packet
signals that controller, while the direct `Frontend::cancel` method refuses to
report success without an active root to cancel.

The `phenix-f1-frontends` native check loads both artifacts separately and
checks typed prompt/cancel packets, event output and cross-session refusal.
The terminal event loop, JSONL host event loop, streaming/cancel dispatch and
ACP integration are still required for F1/F2/P1 acceptance.

### B5 graph-driven Basic agent slice

`impl/m1-basic-agent` declares a graph node triggered by the existing
`phenix.basic.prompt` event and implements `BasicInput` and `BasicOutput`
using public SDK calls. The frontend invokes the discovered
`phenix.basic.submit` action through the kernel graph. The plugin imports the
selected Model, ToolCatalog, ToolExecute, Skills, Context, Session and
EventJournal providers. The kernel contains no Basic agent handlers.

Before executing tool effects, the agent checkpoints assistant call IDs as
pending. It appends each tool result and clears its pending ID only after the
effect returns. A restart with unresolved IDs relies on the Session plugin's
Indeterminate state and explicit reconciliation. Journal records cover accepted
prompts, model calls, text updates, tool starts/results and terminal answers.
History, resource admission and caller limits travel through the typed imports.

The `phenix-b5-agent` native gate composes seven independently packaged
plugins with the definition package and submits a real graph action. This is a
scripted synchronous slice, not yet P1. It still needs pending streaming,
cancellation while model/tool I/O is in progress, HTTP-backed model tests,
reactive value triggers and shared terminal/JSONL/ACP product integration.
Exact-head CI acceptance remains required.

The B5 grant fix declares an optional Storage requirement to retain the
operation ceiling through nested Session, Journal and memory calls. The agent
has no selected Storage slot or direct Storage grants. Storage-owning providers
retain their explicit grants. Resolver and effect-admission rules are unchanged.
The native gate also rejects duplicate and unknown grants, then narrows root
Storage authority and verifies denial leaves the session idle with no history.
Local workspace tests, formatting and the packaged debug native gate pass.
Canonical Nix acceptance must still pass on the published commit.

### B3 persisted skills and context slice

`impl/m1-basic-skills-context` is stacked on the SQLite Storage slice.
`phenix-plugin-basic-memory` provides the existing typed Skills and Context
contracts and requires the selected Storage provider. Both contracts can be
selected independently. The plugin stores declarations under separate
session-scoped keys, rejects empty or duplicate IDs before writing, verifies
persisted session identity and propagates CAS conflicts. It does not scan the
filesystem or assemble model prompts.

The `phenix-b3-memory` gate executes two independent processes against
one SQLite database. It proves a new session starts empty, skills and context
survive process restart, unauthorized cross-session changes reject and invalid
updates leave the stored records unchanged. This is implementation evidence
until exact-head Nix CI, shipping and negative gates pass.

B3 still needs Basic-agent integration for selected skill/context use, tools
with durable counts and other Basic parity cases. Optional shell execution is
a separate plugin and requires a real selected workspace execution provider.

### B3 native tools slice

`impl/m1-basic-tools` is stacked on `impl/m1-basic-providers`. It
implements the independently selectable `ToolCatalog` and `ToolExecute`
providers with the existing public Basic contract and the current M1 native
dispatcher. The fixture checks query-bound cursor revisions, UTF-8 character
counts, JSON string arguments, independent session counts, denied executions
and no-effect rejection. The scripted B4 provider emits JSON arguments to
match the tool contract. This partial B3 slice intentionally does not claim
durable skills/context or a completed B2 dependency; storage-backed providers,
session replay and the triggered B5 agent remain separate stages.

### Development branch after the CI matrix

`impl/m1-basic-providers` branches from `impl/m1-ci-matrix` (PR #790)
without changing the CI branch. The matrix remains a cross-cutting
development accelerator, not an agent execution layer or a replacement for
the shipping acceptance gate. CI0's measured comparison is between the
same PR's head and generated merge commit; full shipping parity is open.

The first executable B4 slice provides a stateless scripted Model contract
implementation using the public Basic types. It validates transcript tool
call IDs, returns deterministic multiple calls and usage, honors caller
token limits, and checks the authenticated session resource at the native
ABI boundary. An independent package and native black-box gate exercise
these contracts. The HTTP provider below adds a separate pending implementation;
full B4 acceptance remains unchecked. B2 durable sessions, B3 tools/skills/context, G4 committed
state-change triggers, B5 graph-composed agent and F1/F2 remain in their
existing roadmap order. This slice does not add a new kernel mechanism.

### B4 pending HTTP model slice

`impl/m1-model-http` provides the existing Model contract as an independent
native package. It imports the selected Journal and declares an optional Storage
ceiling for nested journal writes. A libcurl worker owns HTTP/HTTPS transport,
certificate verification, complete-history requests and incremental SSE decoding.
It holds only owned data, a cancellation flag and an existing wake permit.
Authenticated poll callbacks append text deltas through the Journal import.

The [HTTP model contract](basic-http-model-contract.md) owns request mapping,
caller limits, reply validation and cancellation. Function arguments use an
API object envelope that unwraps into the unchanged ToolCall JSON arguments.
The worker retains its pending native lease until transport ends and it joins.
Cancelled cleanup discards progress and makes no journal writes.

The independent local HTTP/HTTPS fixture checks live progress before completion,
split UTF-8/SSE, multiple calls, complete history, skills/context, typed usage,
errors, truncated/malformed streams, caller limits, session denial, redirects,
CA trust and cancellation/draining. The new `b4-http` check participates in the
matrix and shipping evidence. Exact-head Nix CI is still required. The current
Basic agent remains synchronous; pending import integration and full frontend
streaming/cancellation remain B5/P1 requirements. Kernel and SDK code are unchanged.

## 9. Kernel operations and independent plugins

The product boundary and preset rationale are recorded in [the agent product decision](agent-product-decision.md). These stages add kernel management and observation without turning the kernel
into a logger, inspection contract provider, storage backend or agent runtime.
Their only acceptance checklist is the stage table above.

### K1-K3: kernel-owned mechanisms

**K1: dynamic plugin management.** Reuse `Prepared`, `Generations`,
the verifier, root admission and retirement. The kernel accepts authorized
composition changes and verified artifacts, stages candidates, trials, promotes,
rolls back and drains removed instances. Plugin compilation, artifact fetching,
CLI/UI policy and build tools remain external. Plugin callers cannot grant
their own management authority.

**K2: desired-state reconciliation.** Diff the desired composition against
the active prepared generation. Preview without effects. Apply through the same
resolver, graph compiler, authority checks and generation lifecycle. Retain
already admitted roots and physical leases during replacement. No separate
plugin registry, resolver, reconciliation executor or task manager.

**Live authority revocation.** A pinned generation preserves code, bindings and its configured grant ceiling, not a permanent entitlement to use that ceiling. Kernel-mediated effects must check currently valid authenticated resource grants at the effect boundary. Revoking session/project access denies subsequent effects and requests cancellation of affected roots across all resident generations. A revoked callback cannot regain authority by using an old generation or ticket. Existing cancellation and physical leases retain native code until it drains; a noncooperative trusted native plugin may remain active and must be reported, not forcibly unloaded. Test revocation after admission but before an effect, during a pending callback, across old/new generations, and under reconnect. Do not add a separate policy evaluator or grant registry.

**K3: diagnostic events and raw inspection.** The kernel emits versioned
structured facts about dispatch, lifecycle, state, scheduling, authorization
and generations. Include stable correlation, root, call, plugin and generation
identities where available. These are *diagnostic* events, separate from the
triggered application events in G1-G4 and from B2's durable semantic event
journal. Emission uses bounded, non-reentrant delivery; absent, slow or
failing consumers do not affect execution. Report overflow and gaps rather
than promising lossless diagnostic history.

The kernel owns a narrow, authenticated host query API. It returns consistent
snapshots of prepared bindings, selected graphs, live values, calls, tickets,
generations and authority-filtered metadata. Read access does not mutate state.
Callers receive only information authorized by their root/resource scopes.
The API is an internal kernel boundary, not a public unchecked ABI.

### L1/I1: independent optional plugins

**L1: logging plugin.** `phenix-plugin-logging` consumes diagnostic events.
It owns filtering, formatting, sinks, retention and optional file or telemetry
export. The kernel holds no log store, formatter, sink configuration or tracing
backend. The plugin can be absent or replaced without changing kernel outcomes.

**I1: introspection plugin.** `phenix-plugin-introspection` calls authorized
kernel query APIs, then projects selected results through ordinary versioned,
typed Phenix contracts. It owns contract schemas, pagination, presentation and
consumer-specific redaction. Agents and frontends receive only separately
granted read operations, not a raw kernel handle. It queries the current kernel
state rather than reconstructing it from logs.

L1 and I1 each depend on K3, never on one another. Introspection is not a
logging plugin; logging is not a contract API. B2's event journal remains the
durable semantic replay mechanism for sessions, not a diagnostic sink.

### Deferred plugin-owned work

Durable storage and migrations belong to storage plugins, building on B2
where appropriate. Remote or external language runtimes and OS isolation
providers belong to plugins. Kernel authority must apply to all their effects;
none may expand its own grants. None introduces a second kernel executor.

### Acceptance evidence

K1/K2 require denied-management, unchanged-root, rejected-candidate,
no-effect-preview, rollback and pending-retirement fixtures. K3 requires
no-consumer and slow-consumer equivalence, explicit gap reporting, consistent
snapshots and denied-read tests. L1 and I1 must build independently against
the public SDK. Removing either leaves the other and all kernel tests
unchanged. Restricted introspection grants must not expose secret state
or plugin-management authority.

## 10. Evidence, closure, and PR discipline

Each stage updates only its own source files, fixtures, the relevant RFC addendum, and this matrix. The owning PR body records exact head, targeted acceptance tests, unresolved blockers, and relation to the previous stage. Mark tasks `PROVEN` only with executable evidence from the exact head; use `NOT STARTED`, `IMPLEMENTED`, `TESTED`, and `PROVEN` consistently. A stage cannot claim acceptance based on green CI alone or unit tests that mirror declarations.

Run focused tests while implementing, full `nix flake check -L` and release evidence before marking a stage accepted. Verify M0 tests, external independent artifacts, denied-effect counters, negative ABI cases, state/ref lifecycle, and canonical identity vectors. Nix builds remain declarative and CI read-only.

Rules for restructuring stages: record a concrete dependency and file ownership before creating another PR. Do not reintroduce the old kernel, a second scheduler, temporary compatibility paths, or agent code into the kernel. Remove an obsolete code path only after its replacement has independent executable proof.


S3b host support is scoped by `m1-contract-authoring-contract.md`: the existing
artifact extractor owns the bounded M1 section; normalization owns declared
maximum delegation; pending owns versioned command envelopes; runtime owns
cell-only control dispatch. These are generated-authoring prerequisites and
use existing boundaries, not new managers.

### Definition-only graph follow-up

`impl/m1-graph-definition-packages` uses the existing contract package format
with an optional checksummed graph file. Generated definition macros expose its
canonical bytes without a native entry point. The existing resolver composes
its events and actions with native node declarations. The external fixture now
uses this independently compiled package, rejects forged digests, package-owner
mismatches and executable nodes before constructors, and proves that dormant B
does not block C and that an ordering edge alone does not activate D. Schema and
write-grant negatives remain in the same gate. The
[frozen definition contract](m1-definition-graph-contract.md) owns this scope.
Local native execution, Rust workspace tests, Clippy and formatting pass.
Full follow-up shipping CI 38231650549 passed at `fbe55d3d`; the downloaded artifact binds 30 native cases to that checkout. G1/G2 are accepted. G3 serial semantics are proven; admitted generation context remains before its full stage closure.

### Pending graph coordination follow-up

`impl/m1-pending-graph` replaces the serial graph loop with one readiness pump
around the existing Root and Ticket handles. Synchronous action submission uses
that same pump with pending admission disabled. Ready independent providers can
run while another source is pending; active joins wait for successful outcomes.
Cancellation and failures stop future node admission and cancel existing ticket
trees. Logical completion is distinct from retained physical draining.

The independent native fixture holds a worker file descriptor while A is
pending, completes B, and delays C. It proves immediate and delayed cancellation,
rejected cleanup writes, shutdown rejection during a retained lease, one terminal
update and ticket release. Domain and kernel failures in B cancel pending A.
The [frozen pending contract](m1-pending-graph-contract.md) owns this scope. Local
workspace tests, Clippy and the SDK-only native gate pass. Full follow-up shipping
CI and live committed-state triggers remain before G4 acceptance.

### Basic contracts

`phenix-basic-contracts` defines eleven provider/frontend contracts and the
`phenix.basic.submit` prompt action in one definition-only package. All schemas
derive from Rust types. Model requests carry full messages, assistant call IDs,
tool results, tools, skills, context and caller-selected limits. Storage uses
revision-checked transactions; journal/query, session replay and frontend types
remain plugin-owned. The shared Blob SDK type uses the existing byte value.

The Nix definition package builds from public SDK and Basic contracts only. The
existing verifier checks it without loading code. Definition packaging now uses
one shared Nix helper for the authored, graph and Basic packages. Shipping's
required evidence set includes the Basic contract gate. The
[Basic product contract](basic-agent-contract.md) owns provider semantics. Runtime
providers and the composed agent are the next implementation stages.
