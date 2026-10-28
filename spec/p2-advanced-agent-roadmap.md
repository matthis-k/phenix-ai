# P2 advanced agent: memory, compaction, workspace, sub-agents and inspection

> **Status note (26 October 2026):** This document records intended P2 work and dated milestone evidence, not the current implementation. P1 stabilization precedes a graph-v2 P1-parity migration; new graph design follows [v2.4](graph-architecture-v2.4.md), not the older reactive DAG RFC. Current source and exact-head tests determine actual behavior. Consult [the spec index](README.md) for precedence.

Status: IN PROGRESS. P2-0, P2-1 and P2-2a are integrated; P2-2b and P2-3 remain active. P2-3 slices through P2-3al and typed preparation receipt #1017 have merged, but verified changed-candidate compaction admission, crash-atomic journal/session reconciliation and production tokenizer parity are unaccepted. Provider catalog and authorized API-key import/revoke/query **slices** are integrated (#1006, #1009–#1011), not provider-verified readiness, operational Model rebinding, revision-CAS model selection or the full Neovim journey. P2 release gates remain open.
Updated: 2026-10-20
Parent: [M1 roadmap](m1-roadmap.md)
Historical reference: archived oldmain, especially rust/crates/phenix-agent-configurations and rust/crates/phenix-plugin-memory.
P1 evidence and CI gate: [P1 release acceptance](p1-release-acceptance.md) and [post-P1 CI cutover PR #879](https://github.com/matthis-k/phenix-ai/pull/879).

## Goal and release boundary

P2 is the Advanced agent product. Its kernel is the same M1 kernel as P1. Phenix's goal is a fully customizable coding agent: plugins define what it does, and the kernel defines how contributions execute. Generic kernel behavior follows from that boundary, not an independent infrastructure product goal. The [accepted product decision](agent-product-decision.md) records the rationale. Advanced features live in independently replaceable plugins; the kernel retains authenticated dispatch, generation identity, pending tickets, graph scheduling, authority and cancellation. A product composition selects plugins and their declared bindings. It never becomes a second agent executor. Basic and Advanced remain working examples that users can extend or replace with entirely custom agent workflows.

The P2 release target is a usable agent with P1 parity, reference-aware long-term memory, reversible compaction, code intelligence, safe plugin hot reload, workspace management, durable jobs, sub-agents with session trees, architecture decision review and debugging/efficiency evaluation. **Provider onboarding and usable model selection are also P2 product-release requirements**, with priority ahead of optional frontend embellishments: a user must be able to authenticate by an available API-key or approved OAuth method, discover/select a usable model and send an actual Advanced turn through the supported Neovim frontend without manually editing environment variables. Restore useful oldmain behavior through the M1 contracts and graph, not by importing the old kernel or its embedded application framework. General-purpose planning, repository workers, user-configurable hook rules, the standalone artifact catalog, a broad options service, command-toolbelt discovery and interactive UI remain deferred. These may be added later as ordinary plugins; any small functionality strictly required by the P2 acceptance tests is part of its owning P2 stage.

**Sequence:** preserve the accepted P1 baseline and single-authority CI. PRs #897, #898, #899, #901 and #902 merged on 15 October 2026 after their PR-head checks passed. The resulting `main` commit is `aa48137bc2fb`. Its production CI is separate from PR-head verification. Tests are written RED before implementation for every feature slice. An exact-head full CI pass is required for release acceptance.

### Post-P1 integration correction (20 October 2026)

This section records additional merged implementation evidence after the 17 October milestone tables; it does not change accepted P2 architecture or assert complete P2 stages. Runtime `main` checked at `e6228d51` (#1022 merged). The P1 CI cutover #890 and the P2-3 slice ledger #931–#976 remain **historical accepted evidence**. Use current GitHub PR state over provisional `draft` notes within older task descriptions.

- **Provider (cross-product gate #891):** #1006 read-only native catalog; #1009 exact-resource API-key import/revoke; #1010 authorized read-only auth/model operations; #1011 launcher-granted profile capabilities; #1012 plugin-owned offline/local connections; #1019 typed admitted model choice, #1026 strict explicit HTTP credentials, #1027 admitted credential identity, and #1031 built-in inference endpoint registrations are merged; checked endpoint bridge #1030 remains draft. Verified provider authentication, provider-bound execution, persisted revision-checked model switching, and approved OAuth remain open. Avoid implementing duplicate credential ACP extensions.
- **Advanced model preparation:** #1017 merged validated, source-epoch/digest preparation receipts. This does not prove source authorization or changed-compaction-candidate admission. Closed transcript migration #1023 remains an active draft; code consuming Model messages/compaction should integrate against its final canonical shape rather than create parallel history.
- **Workspace:** #1013/#1014 integrated Environment/XDG resolution, and #1015 added revisioned per-session cwd-selection metadata. That cwd is not workspace/project membership authority or a filesystem grant; P2-6 authorized file/shell/Git effects remain open.
- **Provider-native reasoning/compaction:** #982 tracks durable private thinking/reasoning continuation, and #611 tracks later provider-native compaction. These are distinct from canonical reversible Phenix checkpoints; neither is complete. Reasoning continuation should follow #1023's typed transcript interface at integration.
- **Frontend:** client #69's editable queue remains presentation-only; #78–#90 and distribution #147/#148 already shipped substantial model/auth handling and pinning. P2 Advanced Neovim acceptance still requires the runtime-backed authorized round trip.

## Five immutable package compositions

Build one kernel and compose increasingly capable products. A preset is a packaged, versioned artifact list with selected contract bindings, launch entry and declared grants. It is not another runtime implementation. An installed user-facing preset must start without a separate composition file.

| Flake package / executable | Derives from | Plugins and purpose | Release exposure |
| --- | --- | --- | --- |
| phenix-kernel | M1 kernel | Kernel-only CLI, offline contract/package inspection, no agent graph or model | Public |
| phenix-basic | Kernel + P1 composition | Model, tools, Skills/Context, Session/Journal/Storage, Basic graph, Terminal/JSONL/ACP | Public, already packaged |
| phenix-basic-verification | Basic composition + test provider override | Deterministic history-aware Model and existing acceptance fixtures | CI only, already packaged; retain |
| phenix-advanced | Basic composition + P2 plugins | Advanced memory, compaction, code intelligence, workspace tools, jobs, sub-agents/session trees, inspection and efficiency evaluation, hot reload | Public after P2 |
| phenix-advanced-verification | Advanced composition + isolated test providers | Typed scenario Model, sandboxed workspace/process/child-agent fixtures, controllable failures, clock/revision and reload fixtures | CI only |

Do not silently replace the default Basic Model, weaken Basic grants or add verification-only providers to either user-facing closure. The scripted P1 model may remain the offline reference default; production Model bindings stay replaceable (HTTP or another provider). The verification binary must exercise the real kernel, real session/journal/storage, real graph and real plugin entry points. Only the model, clock and external environment inputs are controlled.

Refactor the P1 flake's basicCompositionSpec, historyVerificationComposition and basicPreset implementation into ordinary overridable Nix package functions, using `callPackage` or `lib.makeOverridable` where appropriate. A consumer should derive `phenix-advanced.override (old: { ... })` with changed plugin sets, bindings and grants; the unmodified preset remains intact. Reuse the same package artifacts, canonical contract metadata and composition compiler, without an extra Phenix-specific override DSL, handwritten JSON patches or a second resolver. Plugin availability from Nix never implies execution grants. Build an external flake fixture proving that an override adds and replaces plugins while an invalid binding or excessive grant is rejected by the real kernel. Produce manifests/digests for all five presets. The verification product must have exactly the same Advanced bindings except for named fixture providers. Check no fixture artifact or test-only capability is reachable from phenix-advanced's Nix runtime closure.

## Scope and implementation order

| Stage | Dependency | Owned change | RED test / completion gate |
| --- | --- | --- | --- |
| P2-0: composition and parity | P1 + CI authority | Freeze oldmain versus P1/P2 behavior inventory, five preset derivations, contracts, separate fixture package | Launch/inspect all presets; prove Basic binary/behavior unchanged; verification plugins excluded from public closure |
| P2-1: evidence model | P2-0 | Typed references, memory scopes, temporal/freshness metadata and durable evidence links | Stable references survive restart; stale/forged/out-of-scope references reject |
| P2-2: shared memory | P2-1 | P2-2a stores project records under authenticated project roots. P2-2b must bind Basic sessions to stable project identities through authorized membership, then retrieve source-checked evidence across those sessions. Optional curation follows. Keep the existing EvidenceStore contract. | Cross-session project recall, CAS, restart, revocation, scope denial, temporal correction and no invented provenance |
| P2-3: reversible compaction | P2-1; may build alongside P2-2 | Policy, summaries, snapshot lineage, deterministic expansion and source hydration | Tight-window continuation, exact replay/rehydration, crash safety, no duplicated tool effects |
| P2-4: deterministic code intelligence | P2-1; integrate with P2-6 workspace edits | Revisioned code-entity graph, lineage, edit validation, provider evidence, incremental reconciliation and memory links | Rename/move/split/delete, provider disagreement, failed edit, external change and crash/rebuild integrity |
| P2-5: graph contributions and hot reload | P2-0; P2-1 for persistent plugin state | Recompose immutable candidate graph from versioned plugin node/edge/trigger contributions on replacement or authorized runtime selection | Updated plugin inserts workflow steps; new roots use new topology, old roots finish old graph; invalid candidates roll back |
| P2-6: workspace | P2-0, P2-4 for code integration | Scoped file operations, shell/process execution, Git integration and revision-checked atomic writes through plugins | Real workspace editing, denied traversal, shell streaming/cancellation, conflicts, rollback and tool-visible results |
| P2-6A: architecture and cross-plugin memory | P2-2 + P2-4 + P2-6 | Architecture guidance plugin, Git-backed decisions, source-linked context and optional design-review graph nodes | Relevant decisions survive sessions, stale evidence is flagged, suggestions are source-backed and ordinary fixes need no extra model step |
| P2-7: durable jobs | P2-6 | Persistent job identity, lifecycle, owner grants, log cursors, recovery and bounded detached work | Parent disconnect, process restart, attach, cancellation, duplicate admission, lease cleanup and fault recovery |
| P2-8: sub-agents and session tree | P2-7, P2-1 | Session parent/child links, delegated scoped budgets, independent child contexts, aggregation and propagation policies | Scripted parent delegates to two real child sessions; restart, authority/limit enforcement, failure and cancellation |
| P2-9: inspection and efficiency | P2-0; instrument P2-2 through P2-8 | Separate logging, introspection/debug and efficiency-evaluation plugins consuming permitted kernel events and projections | Durable/replayable traces, redacted inspection, cost/latency/usage metrics and repeatable comparisons |
| P2-10: advanced verification and release | P2-2 through P2-9, including P2-6A | All scenario fixtures, production transport parity, oldmain capability matrix and benchmark/regression report | Full reproducible exact-head CI, P1 regression, five-package closure audit and final-main check |

### Integrated P2-3 implementation evidence (2026-10-17)

These slices are merged on `main`. They are implementation evidence, not P2-3
release acceptance.

| Slice | PR | Merge commit | Integrated behavior |
| --- | --- | --- | --- |
| P2-3s | [#931](https://github.com/matthis-k/phenix-ai/pull/931) | `47b05ea0` | Bounded, paginated, source-verified live journal reads |
| P2-3t | [#932](https://github.com/matthis-k/phenix-ai/pull/932) | `08411a67` | Deterministic checkpoint proposal over completed turns |
| P2-3u | [#934](https://github.com/matthis-k/phenix-ai/pull/934) | `cd6676ee` | Selected-meter context-pressure classification |
| P2-3v | [#935](https://github.com/matthis-k/phenix-ai/pull/935) | `f27d80ce` | Over-window admission requires a verified checkpoint candidate |
| P2-3w | [#936](https://github.com/matthis-k/phenix-ai/pull/936) | `07764087` | Compacted request remeasurement before admission |
| P2-3x | [#937](https://github.com/matthis-k/phenix-ai/pull/937) | `555a3bad` | Dual live original/compacted measurement |
| P2-3y | [#938](https://github.com/matthis-k/phenix-ai/pull/938) | `5f316e68` | Source-authenticated checkpoint publication through CAS |
| P2-3z | [#939](https://github.com/matthis-k/phenix-ai/pull/939) | `d6f39f74` | One verified atomic compaction admission transaction |
| P2-3aa | [#940](https://github.com/matthis-k/phenix-ai/pull/940) | `b6a62b38` | Opt-in live automatic checkpoint at measured pressure |
| P2-3ab | [#941](https://github.com/matthis-k/phenix-ai/pull/941) | `21766101` | Incremental recompaction through linear CAS revisions |
| P2-3ac | [#942](https://github.com/matthis-k/phenix-ai/pull/942) | `3480035a` | Selected Model-backed reversible summarizer plugin |
| P2-3ad | [#943](https://github.com/matthis-k/phenix-ai/pull/943) | `39f534af` | Pending summarizer continuation before CAS/admission |
| P2-3ae | [#944](https://github.com/matthis-k/phenix-ai/pull/944) | `a378f849` | Summary confined to verified older completed turns |
| P2-3af | [#945](https://github.com/matthis-k/phenix-ai/pull/945) | `1486820e` | Native cancellation gate for transitive summary Model |
| P2-3ag | [#946](https://github.com/matthis-k/phenix-ai/pull/946) | `f72ccf60` | Exact typed Basic-to-Model request projection |
| P2-3ah | [#951](https://github.com/matthis-k/phenix-ai/pull/951) | `633bfdfb` | Source-bound Advanced preparation contract and denial gates |
| P2-3ai | [#952](https://github.com/matthis-k/phenix-ai/pull/952) | `21c480be` | Opt-in Basic prepare node publishing a watched request |
| P2-3aj | [#956](https://github.com/matthis-k/phenix-ai/pull/956) | `6f7f81a5` | Basic Model-advance node gates on the watched prepared request |
| P2-3ak | [#957](https://github.com/matthis-k/phenix-ai/pull/957) | `f919c35f` | Selected Advanced preparer writes the watched Basic request; packaged JSONL/SQLite prepare→advance→answer proof |
| P2-3al | [#976](https://github.com/matthis-k/phenix-ai/pull/976) | `439c4c2d6` | Staged tool re-entry through the watched preparation graph; packaged JSONL/SQLite multi-turn tool proof |

P2-3 is **not accepted**: the staged graph now re-enters preparation after
each tool result and completes identity multi-turn workflows, but changed
candidates still require verified compaction admission, the Advanced
preparer still forwards only the exact original request, and crash-atomic
journal/session reconciliation plus production tokenizer parity remain
open.

P2-2b has three integrated slices: [#906](https://github.com/matthis-k/phenix-ai/pull/906) authenticates durable project membership, [#907](https://github.com/matthis-k/phenix-ai/pull/907) revalidates project-record sources, and [#908](https://github.com/matthis-k/phenix-ai/pull/908) provides packaged cross-session EvidenceStore retrieval (merged at `340b2ac2`). [#910](https://github.com/matthis-k/phenix-ai/pull/910) verifies denial after revocation of an already-admitted root (merged at `8ea04017`). Trusted runtime session/project admission and cancellation of active work remain tracked in [#891](https://github.com/matthis-k/phenix-ai/issues/891); P2-2b is not accepted yet.

P2-3a [#909](https://github.com/matthis-k/phenix-ai/pull/909) merged at `ede59abc`, providing typed checkpoint partitions. P2-3b [#911](https://github.com/matthis-k/phenix-ai/pull/911) merged at `e63633cb`, verifying byte-exact original journal events and preventing fabricated expansion. P2-3c [#913](https://github.com/matthis-k/phenix-ai/pull/913) merged at `5849b45a`, providing a typed revisioned checkpoint provider with Basic Storage CAS, journal revalidation and native multi-process SQLite restart testing. P2-3d [#914](https://github.com/matthis-k/phenix-ai/pull/914) merged at `8625a28b` after exact-head full CI, providing bounded, cited derived context projections, split/unresolved tool-call refusal and real SQLite-backed native restart projection verification. P2-3e [#915](https://github.com/matthis-k/phenix-ai/pull/915) merged at `f6df679e`, introducing an additive, pure model-budgeted view-selection stage with tests-first acceptance: candidate checkpoints are source-verified newest-first; a selected Model adapter must assemble the entire request from authoritative session history and count that exact request, including tool schemas and context, before admission. Invalid source or failed measurement denies continuation instead of silently falling back; capacity-only misses may try another verified checkpoint. It does not claim the Basic event journal contains complete Model messages or perform model execution. P2-3f [#916](https://github.com/matthis-k/phenix-ai/pull/916) merged at `92367e23`, adding a deliberately text-only assembler which aligns authoritative SessionSnapshot messages with exact journal PromptAccepted/Answer events before eliding completed older turns into a cited context item. Tool-bearing messages are rejected: the Basic journal does not retain their complete arguments, so reconstructing tool conversation state would fabricate source data. P2-3g [#918](https://github.com/matthis-k/phenix-ai/pull/918) merged at `6d8cd0f2`, adding tests-first verification of append-only EventJournal tails beyond a stored checkpoint's immutable input span: source replay is revalidated, successive events retain exact bytes and digest citations, tool pairs must be complete and the combined view obeys a total byte cap. This is still a pure projection, not a Model message serializer or a grant to observe a source. P2-3h adds a real, selected Advanced Model adapter, interposed only in the Advanced and Advanced-verification presets. It delegates through the existing typed pending Model import and preserves runtime session scope, streaming cancellation and the exact request/response; the Basic graph remains the only agent loop. An independent packaged product test verifies selected upstream binding, persisted tool and session history, multi-session isolation and restart. The existing pending-HTTP acceptance fixture is reused with the Advanced composition to verify real streaming, cancellation and crash recovery across the extra provider boundary. Until source-checked policy admission, selected-model token metering and full tool-message lineage are available, it deliberately forwards prompts unchanged rather than performing unsafe compaction. P2-3i extends the existing typed `CompactionStore` with a session-scoped `latest` operation for selecting the highest durable linear checkpoint revision. The provider validates ledger lineage and rechecks that checkpoint's authoritative Basic journal on every lookup, so missing or erased source fails closed rather than selecting an older checkpoint. Native SQLite restart and cross-session/erasure verification remain the acceptance gate. P2-3j [#921](https://github.com/matthis-k/phenix-ai/pull/921) makes checkpoint `put`, `get` and `latest` recheck each ancestor's journal source in revision order. A valid descendant digest no longer hides a missing ancestor. Tests and exact-head CI still gate integration. Live Model adapter token counting, full tool-message provenance, Advanced graph prompt wiring for actual compaction, older-candidate selection, permanent-erasure authorization, crash-mid-commit fault injection and work continuation remain open; P2-3 is not accepted.

P2-3k [#922](https://github.com/matthis-k/phenix-ai/pull/922) defines a selected-model token meter over the complete typed `ModelRequest`. The result binds the model and tokenizer revision to a digest of the exact request, including history, tool schemas, skills, context and limits. Checked budget admission includes reserved output. A verification-only Model provider defines an exact synthetic byte-token tokenizer, and its reported usage matches the measured input. The fixture has no production token semantics. Until a selected production Model provider supplies its actual tokenizer and the Advanced adapter binds the meter to that same provider, compaction stays disabled and the existing request forwards unchanged. Exact-head tests and Nix CI still gate this slice.


P2-3m extends M1's existing authenticated host slot introspection with a
typed selected-provider query; it is observational, never an authority grant.
The Advanced Model adapter uses it to refuse a metering provider distinct
from its actually selected upstream Model, even when both slots have grants
and the measurement digest is valid. The default Advanced preset remains
unmetered. The independent JSONL product fixture uses a deliberately
cross-wired scripted Model and byte-token meter to require denial before
Model execution, in addition to unchanged missing-grant/stale-digest and
recovery coverage. This does not itself provide an HTTP tokenizer or permit
compacted request submission. Exact-head CI and P1 parity remain mandatory.

P2-3n (tool-history retention) extends the existing pure, source-verified
ModelRequest assembler. It may summarize older text-only turns while keeping
complete newer tool-bearing turns byte-for-byte from the authoritative
SessionSnapshot. Journal ToolStarted/ToolFinished records must match call IDs,
tool names, output text and failure status in order. A pending Model
continuation after tool results remains supported. Legacy journal records
lack tool-call arguments and the assistant's tool-call text, so any attempt
to summarize or partly elide a tool-bearing turn fails closed. Neither
Basic's journal schema nor its loop changes. This is a nonexecuting
verification stage; actual Advanced graph compaction and independent full
tool-message source provenance remain separate release gates.

P2-3o adds an opt-in typed full Model tool-reply receipt through the
existing `EventJournal` contract and `EventKind::AssistantReply`. Before
returning a tool-calling Model reply to the Basic loop, the selected
Advanced Model adapter commits the exact assistant Message, including
arguments and call IDs, to its explicitly authorized session journal.
This is a normal effect through a selected optional `Transcript` import;
absence preserves the default Advanced behavior. Missing grants deny
before tool execution. A pending Model reply preserves its original
run identity. The source-checked assembler accepts tool-turn elision
only when the corresponding journal receipt exactly matches the separately
persisted SessionSnapshot and tool completion records. It still rejects
unattested legacy tool turns, partial spans and conflicting data. The
verification fixture checks journal append before tools, provider authority
denial and SQLite restart. Live compaction and crash-mid-reply checkpoint
reconciliation remain unaccepted.

P2-3l [#923](https://github.com/matthis-k/phenix-ai/pull/923) introduces an optional selected-meter preflight in the existing Advanced Model adapter. An explicitly metered verification composition binds `Upstream` and `Meter` to the same model provider, reserves the caller's full remaining output-token cap, verifies the original typed request digest and rejects invalid measurements before starting Model execution. A test-only stale digest causes a real JSONL Model turn to fail; later turns still run. The public Basic and default Advanced presets remain unmetered pass-through. Model name strings are not proof of same-provider selection: this slice verifies co-binding in the packaged composition, while general user-authored composition enforcement, real HTTP provider tokenization, tool-message lineage, source-backed view admission and complete P2-3 acceptance remain open.

P2-3p [#928](https://github.com/matthis-k/phenix-ai/pull/928) joins a source-verified durable checkpoint to its contiguous, authorized Basic journal tail before assembling a typed ModelRequest. The pure verifier checks both the saved source bytes and newer events, then matches the entire result against the authoritative SessionSnapshot. It retains new tool turns exactly, permits summarizing a fully attested older tool turn, and denies missing receipts, incomplete effects, forged arguments and foreign sessions. The derived full-span checkpoint exists only in memory. This slice does not select journal or checkpoint providers, grant source reads, execute the Model, or enable Advanced compaction. Live source admission, ancestor revalidation, output-reserved selected-Model token accounting and crash-safe recovery remain release gates.

P2-3q is a verification-only opt-in for the existing Advanced Model
adapter. The `Checkpoints` binding selects the canonical `CompactionStore::latest`,
which rechecks source lineage. The adapter reads a bounded, complete journal
suffix through its separately granted `Transcript` import, matches the
checkpoint and live events against the authoritative ModelRequest using
P2-3p, then meters the exact selected request before upstream Model
execution. An absent checkpoint forwards the original request; missing
grants, source changes, missing meter, incomplete pages and misaligned
history deny before Model execution. The Basic preset is unchanged. The
existing independent JSONL verifier uses real SQLite and a native checkpoint
writer to require the selected Model to receive the cited, compacted history.
This does not enable automatic checkpoint creation, dynamic model policy,
crash-atomic journal/session reconciliation or production HTTP tokenization.
P2-3 remains unaccepted.

P2-3r [#930](https://github.com/matthis-k/phenix-ai/pull/930) adds native
SQLite restart controls for missing checkpoint or journal grants, a missing
selected token meter and a corrupt checkpoint ledger. Failures must not
silently forward unverified original requests or modify the good session.

P2-3s adds bounded paginated source reads to the already opted-in Advanced
Model adapter using only the selected EventJournal interface. Each returned
page must contain strictly contiguous events for the selected session, an
exact cursor and a bounded count. Read through the first empty page; neither
a full nor a short nonempty page proves the end of the journal. Deny before
Model execution on missing data, invalid cursors, foreign sessions or total
capacity overflow. A packaged native SQLite test seeds and checkpoints more
than one 128-event page, then verifies the exact compacted Model request.
Automatic checkpoint policy, production tokenizer parity and crash-atomic
checkpoint/session reconciliation remain unaccepted.

P2-3t adds a **pure proposal** for source-verified checkpoint boundaries.
The chosen policy retains the last N completed turns and the active Model
continuation, and summarizes only whole older turns. It first verifies
the complete authoritative SessionSnapshot against original Basic journal
bytes with all events retained, then validates any proposed elision against
the same historical evidence and typed AssistantReply receipts. Gaps,
foreign sessions, forged history, incomplete/failed turns, unsupported
tool-call elision, stale parent ancestry and invalid checkpoint lineage
reject. Not enough completed history returns no proposal without writes.
A separately selected summarizer must provide factual summary content;
the structural planner cannot attest semantic faithfulness. The Advanced
graph still owns token thresholds, authorized journal reads, selected
summarization, checkpoint-store CAS and publication. This slice neither
performs a Model request nor creates durable checkpoints on its own.

P2-3u reuses the canonical ModelTokenMeasurement verifier to classify
context pressure using a caller-selected high-water percentage and the
output reserve in the *exact* authoritative ModelRequest. The selected
Model/ModelTokenMeter provider co-binding remains an authenticated host
selection check, not a model-name claim. Valid measurements below the
threshold take no action; approaching the window triggers graph-owned
checkpoint policy, and a validated over-context-window request requires
compaction or explicit denial before Model execution. Changed request
digests, missing output reserve and mismatched selected model identities
deny; u128 arithmetic prevents token/percentage overflow. This policy
only classifies and does not automatically summarize, commit, grant
journal reads or execute a Model. The Advanced graph must integrate it.

P2-3v joins the selected Model's measured context pressure to the existing
source-verified completed-turn checkpoint planner. Below the configured
high-water mark, the original budget-admissible request continues without
requiring journal reads. At the high-water mark, a complete and verified
candidate is proposed if one exists; otherwise the original request may
continue while it still fits. An over-context-window request must produce a
verified candidate or be denied before Model admission. Invalid measurements,
changed source evidence and incomplete turns fail closed. This is a pure
decision, not an automatic checkpoint write. The Advanced graph must select
the summarizer, authorize journal and store access, commit with CAS and
remeasure the rebuilt request through the same selected Model before execution.
It cannot fall back to an over-window original after a failed compaction.

P2-3w provides an exact-request admission check after a checkpoint
candidate is selected. The pure verifier recreates the complete ModelRequest
from the authoritative Basic SessionSnapshot and source-checked journal,
retains the original nonhistory fields and context, then invokes the selected
Model's meter on that exact rebuilt request. A candidate is accepted only if
its measured input plus the original full output reserve fits the selected
context window. Changed request digest, missing journal bytes, changed Model
identity, tokenizer revision or context window, and provider failure reject
before any upstream Model call. The original can be over budget only while
proposing compaction; a derived over-budget request never reaches the Model.
This helper neither commits a checkpoint nor authenticates provider co-binding.
The Advanced graph still owns selected journal access, summarizer, CAS commit,
provider generation and Model execution. P2-3 release remains unaccepted.

P2-3x applies the same selected-model measurement rule to the actual
opt-in Advanced Model adapter after a durable checkpoint. The adapter
continues to read checkpoint ancestry through CompactionStore and exact
journal bytes through its selected Transcript import. When that read
changes the ModelRequest, it measures the original authoritative request
and then the source-rebuilt request through the same authenticated pinned
Model/ModelTokenMeter provider. A valid original over the context window
may enter this compaction path, but only a rebuilt request that fits the
window may reach Model execution. Changed original digest, tokenizer
revision, model context window, and malformed rebuilt measurements reject.
The tests use separate SQLite restart forks and fault-injected selected
fixture measurements. Public Basic and unmetered Advanced presets stay
unchanged. This is admission of already committed checkpoints, not
automatic graph-triggered summarization or checkpoint writes.

P2-3y adds source-authenticated checkpoint publication through the existing
CompactionStore revision CAS and selected latest-read contract. A candidate
must be structurally valid, match the current checkpoint head and expected
parent revision, receive an exact CAS acknowledgement and survive a fresh
authenticated source/lineage readback before its compacted Model view can be
considered. A restarted authorized root reuses an exactly matching durable
checkpoint without repeating a write; an ambiguous failed write never retries
inside the same request. A competing head, forged acknowledgement, stale
source, or failed readback denies admission. Native SQLite acceptance includes
independent restart reuse and the unchanged Basic journal. The selected
summarizer, pressure trigger, source reads, commit and downstream Model call
remain graph responsibilities; this reusable publication guard is not itself
the automatic graph contribution.

P2-3z composes the accepted selected-pressure decision, structurally
verified journal checkpoint planner, source-validated CompactionStore CAS
publication and exact compacted Model-request remeasurement into one
graph-callable pre-Model transaction. When the original fits comfortably,
it makes no store write or second meter call. At a threshold with no
eligible completed prefix, it keeps the original. An over-window original
without a safe prefix is denied. A candidate can reach Model admission
only after a selected-store CAS/readback and exact remeasurement under
the same pinned Model configuration. The transaction performs no Model or
tool execution. A committed checkpoint remains durable if later metering
fails, but no prohibited fallback to over-window original occurs.
Deterministic tests cover all paths. The real graph contribution, selected
summary quality, provider wiring and native live acceptance for automatic
checkpoint creation remain open.

P2-3aa introduces an opt-in selected CompactionSummarizer typed contract
and connects the complete pressure→journal→summary→verified checkpoint plan
→CompactionStore CAS/readback→selected Model remeasurement transaction to
the existing Advanced Model adapter. The Basic agent still owns the Model
request, graph turn, session state, tools and streaming. When the optional
Summarizer is unbound, the public Advanced composition behaves as before.
The selected summary must cite the exact bounded journal source span; its
contents are subject to the existing independently verified Basic snapshot
alignment and checkpoint source hashing. Automatic compaction currently
targets a first checkpoint with one retained completed turn and no
existing checkpoint; continuation through a persisted checkpoint remains
source verified. This is a stage-one selected Model-boundary trigger, not
yet a separate additive graph node or dynamic checkpoint replanning.
Native verification creates a checkpoint on pressure, checks the compacted
Model request, and proves restart does not create duplicate CAS revisions.
Further graph composition and configurable summarization policy remain
required before P2-3 acceptance.

P2-3ab extends the opt-in Advanced adapter to recognize pressure on an
already selected, source-verified compacted request. If the compacted request
fits comfortably, existing checkpoint reuse continues unchanged. When it no
longer fits, the adapter authenticates the full Basic session, supplies the
previous source-validated checkpoint and next linear revision to the existing
P2-3z transaction, and CAS-publishes a fresh candidate before independently
metering the new compacted request. No checkpoint or source journal is erased
or rewritten, and a failed replan cannot fall back to a rejected, over-window
Model input. Native SQLite acceptance verifies successive authorized
turns advancing revisions 1→2→3 under renewed measured pressure. A newly
submitted turn is not an exact CAS retry: even when the prompt text repeats,
the authoritative session has grown. The same-candidate ambiguous-write
recovery test independently verifies restart idempotence without an
extra revision. The selected
Summarizer remains opt-in and first-stage policy is a fixed retained-turn
count; independent graph contribution and production policy controls remain
open.

P2-3ac introduces a separately packageable, opt-in Model-backed
CompactionSummarizer plugin. It accepts the existing typed source-cited
summary request, verifies the exact journal digest, encodes bounded event
data as an untrusted input to a separately selected Model, forbids tool
calls, constrains output budget and response size, and returns only a
proposed source-bound summary. No checkpoint write, Basic journal edit,
session authority transfer or agent loop is introduced. The plugin
supports pending selected Model imports and cancellation. Its pure prompt
and reply controls are tested independently in the Nix matrix. Completing
real live integration requires making the Advanced adapter's selected
Summarizer import pending-aware: its current synchronous import can only
consume immediately ready summary implementations, such as the existing
deterministic verification fixture. Model providers that stream journal
deltas into the active Basic source ledger must not be bound to the
summarizer without a separate authorized scratch/isolated context.
Until that coordination and native end-to-end gate exist, this production
provider is opt-in and not part of the default Advanced composition.

P2-3ad makes the existing Advanced Model provider's pending lifecycle
aware of a separately selected pending CompactionSummarizer. Preparation
produces either a ready ModelRequest or a source-bound summary proposal.
The same M1 pending ticket awaits the selected summary child, with no
checkpoint write and no primary Model call while it is unresolved. The
completed summary is independently checked against exact source bytes,
submitted through the existing checkpoint CAS/readback transaction and
remeasured before starting the original Upstream Model. Cancellation
releases either pending child and clears source state. A dedicated native
composition binds the actual Model-backed summary plugin to a fixture
Model that deliberately replies only after poll; it also verifies denial
of the nested summarizer->Model grant before any checkpoint CAS.
Default Advanced remains unchanged and the Basic graph still owns turns,
tool execution and authoritative state. A future additive graph
contribution and failure/restart acceptance remain required.

P2-3ae confines the nested summary Model's visible source to the exact
older completed-turn prefix that the existing canonical checkpoint planner
would elide with the configured one-turn retention. The full pending
authoritative journal remains source-authenticated by its original SHA-256
envelope, but the nested Model prompt receives only the separate
source-verified elided prefix and its digest. The newest retained completed
turn, pending user text, tools and independent context do not enter that
prompt. If the planner finds no eligible prefix or history/source verification
fails, deny before invoking any summarization Model. The source-cited
CompactionSummary response retains the original full journal citation so
CAS publication and selected Model remeasurement still bind the complete
authoritative request. Native pure unit tests guard the exclusion and
no-eligible-prefix cases; no duplicate history parser or new storage is added.

P2-3af adds native cancellation acceptance for the selected nested
pending summary Model. The test seeds completed Basic turns, starts an
authorized pending summary whose deterministic Model fixture records a
test-only witness, cancels the actual running JSONL graph root after that
witness, then verifies the durable Cancelled event, absence of a new CAS
checkpoint, and absence of downstream Answer or ToolStarted events.
The existing kernel owns cancellation propagation and pending-ticket
draining; the Advanced adapter and summarizer clear pending state without
introducing their own scheduler, autonomous worker or storage write. The
test does not assert safe recovery from an external process kill, which
remains a separate acceptance requirement.

Each stage must use existing M1 typed values and native descriptor validation. New plugin contracts are versioned where required. Search for maintained crates and evaluate fit, license, maintenance, dependency weight and Nix/ABI support before writing a parser, vector index, database or protocol. Keep the [R1 library audit](m1-r1-library-audit.md) current when a library is selected.

## Implementation handoff: code-sized P2 slices

The stage table above owns scope and release status. Use this section when assigning
implementation to a fresh-session coding agent. Each packet is a **starting path**,
not a mandatory internal Rust design. The actual checked-in contract types, package
descriptors and tests take precedence over file names or examples written here.
Finish one observable vertical slice per PR; do not implement an entire P2 stage
from a single prompt.

### Working rule for a coding agent

1. Read this stage's acceptance row, the linked contract/ADR, the actual selected
   provider descriptor and the current implementation. Identify the exact upstream
   commit and overlapping open PRs. Prefer extending one accepted implementation.
2. Add a RED test through the relevant existing contract or packaged product.
   Include a counterfactual that denies a grant, changes a revision, removes a
   required provider, or interrupts a live operation.
3. Implement the smallest slice on the current M1 SDK, provider imports and graph.
   Make local choices about data layouts, helper functions, internal modules and
   maintained third-party crates. Record an unusual trade-off in the PR, not a
   new architecture document by default.
4. Run focused Rust tests, `cargo fmt --all --check`, applicable real native/JSONL/
   ACP/SQLite product tests, then the exact-head canonical Nix checks. Link the
   commands, SHA and passing evidence. A unit-only result is not stage completion.
5. Stop and raise a specific blocker when the slice would require changing the
   authority owner, inventing a transport/protocol, introducing a competing
   persistent registry or agent loop, widening grants, or resolving conflicting
   published contract identities. Do not make those choices implicitly.

Local implementation is flexible; the accepted user-facing semantics, contract
ownership, immutable graph generations and denial behavior are the constraints.
A path listed below is an inspection entry point. Add a new crate only when a
separate provider or public contract is necessary; the exact private module
layout is a coding choice. Keep Basic product tests green throughout.

### Packets with entry points and first executable proof

| Stage / first PR | Inspect and change | Implement first | RED proof before expansion |
| --- | --- | --- | --- |
| **P2-2b: runtime project admission** | Existing `EvidenceStore`, project membership provider, `phenix-basic-runtime` session/control admission; #891 | At authenticated new/resume, resolve stable project identity and existing membership. Propagate the selected, scoped project grant to retrieval. Deny revoked access on an *already executing* root, then cancel it through the existing lifecycle. | Packaged two-session SQLite scenario: allowed cross-session recall; forged project/session selector and revoked in-flight root yield zero project reads/writes. Repeat after restart. Do not count #906–#910 provider tests alone as runtime admission. |
| **P2-3: selected prompt preparation** | `crates/phenix-plugin-basic-agent/src/lib.rs`, `crates/phenix-plugin-advanced-model-adapter`, `phenix-advanced-contracts`, `tests/p2-model-adapter-product.py`, pending P2-3aj PRs | Select **one** compatible graph-preparation contract/contribution from the overlapping #959/#960/#961 work before changing code. Wire Basic's watched pending Model request to selected Advanced preparation and independently checked Basic Model admission. Keep the Basic graph as the only turn executor. Start with byte-exact no-compaction forwarding, then a verified compacted candidate. | Opt-in packaged JSONL/SQLite sequence `Basic prepare -> Advanced proposal -> Basic advance -> Model -> optional tool continuation`. Remove the proposal-read grant or alter its digest: no Model/tool effect. Verify cancellation and restart. Do not combine alternate P2-3aj plugin IDs or claim that a preparation-only witness executes the agent. |
| **P2-3: durable compaction gate** | Existing `CompactionStore`, `EventJournal`, `SessionSnapshot`, selected `ModelRequest` meter/assembler and `tests/p2-model-adapter-product.py` | From authorized current source, validate complete older tool-call receipts and checkpoint ancestors; assemble the **exact** candidate request, meter with the selected Model provider and reserve output before calling that provider. Commit a checkpoint only under existing CAS and journal rules. | Force a small selected context budget: the actual Model input contains bounded, cited old context and exact recent turns. Corrupt an ancestor, change Model identity, inject an incomplete tool call or crash before checkpoint commit: deny/reconcile without duplicate execution. A scripted byte-token meter proves only fixture semantics. |
| **P2-4: code observations** | `phenix-sdk` contracts, existing Storage/Workspace source revisions and oldmain code-entity semantics; use maintained Tree-sitter and LSP libraries | Add a selectable Rust-language observation provider producing bounded definitions/references/diagnostics tied to exact content hashes and provider epochs. Store evidence plus unresolved/stale state before exposing current relations. Add lineage only after the revision check works. | Native isolated Rust fixture: current definition/references match source; edit, rename, missed watcher change or provider restart invalidates prior-current output. Two same-named symbols must not be merged by heuristic. |
| **P2-4 + P2-6: edit reconciliation** | Code observation provider, proposed Workspace read/write operations, Session/Storage revision/CAS mechanisms | Validate expected bytes/revisions before writes. After a successful OS write, re-read content and publish verified observations. Persist `needs-validation` when index publishing fails; repair on restart. | Write conflict produces no overwrite. Inject failure between successful file write and index publish: on restart the file stays changed, but queries cannot claim the index is current until revalidation. |
| **P2-5: contribution replacement** | `phenix-kernel` resolver/graph generation and leases, `phenix-sdk` graph contribution descriptors, current native reload tests | Load a versioned plugin candidate and compile its **complete** graph with existing validation; atomically select it only after validation and state migration. Keep old roots pinned to old generation/leases. | Version A executes 3 nodes; B executes 5 for new roots. An old root completes A. Denied schema/grant or failed migration preserves A, without constructing denied plugin code. |
| **P2-6: session cwd selection** | Optional `phenix.workspace@1`, selected Basic Storage and session authority | Select or clear an absolute session cwd through one revision-checked Storage record. The returned cwd is **only a location choice**, not project identity or a filesystem grant. Treat the launch cwd from Environment as an optional hint, not as this selection. Use the selected revision for future turns and preserve the admitted turn's existing execution context. | Native SQLite restart, stale CAS, foreign-session denial, traversal rejection and reset. Later filesystem/process providers must authorize every effect and verify the workspace root against project grants. |
| **P2-6: Workspace I/O** | Existing M1 Tool/Workspace candidate imports, root resource admission, `phenix-plugin-basic-tools` as a contract example, packaged Advanced composition | Start with authorized read and one revision-checked write in a disposable Git worktree. Then add diff, move and bounded process execution with cancellation using maintained OS/Git libraries. Return typed receipts rather than implicit mutation. | Independent Model makes a real read/edit/diff call. A symlink escape, stale hash or denied execute grant changes no forbidden file or process. Streaming/cancel tests check final process state. |
| **P2-6A: architecture memory** | P2-2b evidence, P2-4 source refs, P2-6 Git/Workspace, existing Skills/Context and ADR conventions | Persist a reviewed Git decision with exact project/commit/source identity; retrieve it in another authorized session. Add optional proposal/review hooks only after source-backed recall works. | Restart and recall exact accepted decision; outdated source marks it for review. A model-generated conflicting proposal cannot promote itself over an accepted ADR; a denied project cannot read it. |
| **P2-7: durable job** | Existing Session/Journal/Storage CAS, Workspace/process provider and selected generic kernel cancellation | Store job ID, owner, operation key, generation, cursor and recovery state before starting one effect. Add attach/detach/cancel and reconcile unknown external effects on restart. Keep process execution in the process provider. | Detach, reconnect and restart an actual packaged job. Repeated admission gives one job/effect; crash after admission but before observable completion reports `indeterminate` unless verified, never blind rerun. |
| **P2-8: child session** | Existing Basic Session/Journal and root admission, P2-7 lifetime/leases, `phenix-advanced-verification` composition | Spawn one real child through typed Session admission with an attenuated grant and separate journal; then add second child, bounded budgets, await/aggregate and restart reconciliation. Parent stores child IDs/evidence refs rather than flattened transcript. | Scripted parent gets two distinct child Model requests; a child with denied Tool scope cannot call it, and cancelling one child preserves sibling policy. Restart retains child identities. |
| **P2-9: inspect/evaluate** | M1 generic event stream, selected Journal/Storage and proposed optional observer plugins | Start with authorized inspect-by-root and trace correlation across model, tool and child events; then add bounded cursors, redaction and measured usage. Control mutations use separate authorized operations. | Independent observer reconstructs transitions across restart. Denied cross-session query returns no data; blocked/slow observer cannot block an agent root. |
| **P2-10: product closure** | `flake.nix`, `tests/p2-model-adapter-product.py`, AV-01–AV-45 and P1 tests | Wire each accepted plugin into `phenix-advanced`, fixture variants only into verification; keep external `.override` working. Use the real selected provider/graph, not a direct unit-test entry point. | All five packages inspect/launch, actual JSONL and independent ACP tests pass, no fixture plugin enters public closure, P1 behavior unchanged, exact-head Nix CI and final integrated-main SHA are green. |

### Preferred slice size and handoff record

A useful PR changes one owner or an interface plus its immediate consumer and proves
one complete behavior. Wide changes are appropriate only when they cannot leave
an independently valid composition; use expand/migrate/contract commits with a
single end-to-end proof. Every PR body should give:

```text
Stage and owning roadmap row:
Base SHA / selected upstream PR:
Contract imports and existing provider reused:
Files actually changed (the table above is only a starting point):
RED command and counterfactual:
Implementation choice left local and why:
Packaged command, results and exact SHA:
Remaining blocker or next independently testable slice:
```

If code differs from a suggested path while preserving the selected contract,
update the handoff pointer in the same PR. Do not fossilize a temporary helper
name as a new architectural rule. Implementation agents must not reinterpret
a stage's existing acceptance tests as evidence that all its earlier slices
are merged or complete.

## Advanced graph topology and contract ownership

**P2-3ag: typed Basic request projection (preparatory, not a graph hook).**
Extract the exact request snapshot from the existing Basic agent's pre-Model
boundary into one pure, typed and independently tested
`project_model_request` function. Include the complete journal-derived
history (tool call arguments/results), selected tools and skills, context,
run/session identity and caller limits. Both immediate and pending Model
paths must consume the same value without state mutation. This refactor
does **not** yet claim to move compaction out of the Model adapter.

**P2-3ah: source-bound preparation contract (non-authorizing).** A new
versioned `phenix.advanced.model-preparation@1` contract accepts the exact
Basic ModelRequest and its canonical domain-separated digest. Selected
preparers return a proposed candidate with the same source digest, session,
run, tools, skills and limits. Any changed history/context requires a
structurally valid checkpoint citation, but a citation is never sufficient
to execute Model or grant a tool effect: the actual caller must independently
source-verify complete EventJournal bytes, CAS/readback the checkpoint,
remeasure the final selected Model request, and require effective graph/node
authority. Pure native denial probes cover stale digests, foreign sessions,
changed run identities, changed tool/skill permissions, output budgets and
unproven history/context edits. This remains a **contract boundary**; it does
not yet make Basic submit asynchronous across two graph waves.

**P2-3ai: first real Basic graph preparation node (opt-in).**
The same Basic agent is packaged in a separate `staged-graph` feature
variant. Its graph contributes a standalone `prepare` node triggered by
`phenix.basic.prompt`, declared as the writer of
`phenix.basic.agent/pending_model_request`. It shares the existing
session admission, selected tools/skills/context, turn limit and ModelStarted
journal boundary with the production Basic turn logic. It retains the
authoritative in-process Running state and publishes exactly one typed
ModelRequest snapshot through M1 watched state before the graph wave
finishes. The prepare operation does **not** invoke Model and no Advanced
hook is invoked in this slice. A feature-specific graph declaration test
and Nix-built native plugin establish the ABI and metadata; the default
P1 Basic graph and product composition are unchanged.

The staged binary is not user-facing or P2 accepted: its Running session
has intentionally not yet reached a terminal answer. The next selected
Advanced prepare node must read and source-check this watched request,
publish an authorized candidate, and unblock a separately staged Basic
Model advance node, including multi-turn tool continuations. Do not ship
the prepare-only composition as an agent, and do not model a state-hook
as able to interrupt an already-pending Basic submit node.

**P2-3aj: Basic pending-capable Model admission as a separate graph wave.**
The staged-only Basic graph now declares a second node
`model-advance`, subscribed to the watched
`phenix.basic.agent/prepared_model_request` property. It exposes a typed
`phenix.basic.turn-advance@1/advance` operation and a separate
`BasicPreparedModelRequest` property. This second operation resumes the
same authoritative in-process Running/session/run value, not a new agent
loop. It checks the observed committed state epoch, exact source request,
selected candidate equality, active run identity and the absence of an
already pending Model; then uses the normal selected Model pending
begin/poll/cancel lifecycle. For this slice changed requests are denied
even with an untrusted checkpoint citation; only the original exact
request can enter Model. Tool-bearing Model replies fail closed before
any tool effect until the next graph-iteration handoff is implemented.
The staged BasicInput direct submit provider is denied so callers cannot
bypass the mandatory preparation wave by invoking it directly.

The advanced graph preparer that watches Basic's pending request and
publishes the independently selected prepared candidate is the **next**
slice: this second Basic graph node alone cannot activate without an
explicit prepared-request write and correct M1 property grants. Neither
the plain Basic preset nor the existing Advanced adapter composition
changes. Native tests validate graph declaration, stale/cross-run
candidate denial and a prepare-only black-box behavior. Full prepared
node activation, pending Model and tool-iteration acceptance remain
unaccepted pending a selected Advanced graph preparer.

**P2-3ak: selected Advanced prepare graph handoff (identity-only).**
A native `phenix.advanced.graph-preparer` plugin contributes one M1
state-change node watching
`phenix.basic.agent/pending_model_request`. It imports the selected
Basic pending getter and prepared setter through existing property
contracts and invokes the externally selected
`phenix.advanced.model-preparation@1/prepare` provider. A separate
`phenix.advanced.preparer-noop` plugin implements the initial strict
identity proposal. The graph hook rechecks the current session resource,
canonical original request digest, selected proposal source binding and
candidate byte equality, and only then commits the prepared-request
property. The resulting watched epoch activates Basic's distinct
pending-capable `model-advance` node; the kernel owns the entire
two-wave ordering and cancellation. No new scheduling protocol exists.

A test-only native composition selects all three plugins (staged Basic,
graph preparer, no-op preparer), defines their precise M1 property
getter/watch/setter bindings and effective scoped grants, and verifies
one JSONL/SQLite prompt produces an ordinary scripted-model answer,
without speculative tools. The negative composition removes the
graph-preparer's prepared-state write grant and must reject the graph
before journaling any turn. The existing Basic and Advanced presets
remain unchanged. Real pending summarizer selection, checkpoint CAS
proof, tool-call re-entry through every graph wave, restart, cross-root
concurrency and compaction migration are still required for P2 acceptance.

**P2-3al: staged tool re-entry through the watched preparation graph.**
The staged Basic `model-advance` node no longer fails closed when the
selected Model returns tool calls. It executes the tool effects exactly as
the ordinary Basic loop does, then publishes the next authoritative
`ModelRequest` (with the updated history including tool results) as a fresh
watched `phenix.basic.agent/pending_model_request` cell and ends its graph
wave. The kernel re-admits the selected Advanced graph preparer, which
re-invokes the identity `model-preparation@1/prepare` provider and commits
the next watched `prepared_model_request`, activating the same
`model-advance` node for the following Model call. The `model-advance` node
now declares its `pending_model_request` write and an `after` edge to
Basic's `prepare` node so the graph compiler admits the two ordered
writers. The default Basic (non-staged) path is unchanged; the no-op
preparer remains the default Advanced preparer. A packaged JSONL/SQLite
fixture runs `tools:hello` through the selected three-plugin composition
and asserts two `ModelStarted`/`ToolStarted`/`ToolFinished` events and the
exact final `mock:call-1:HELLO|call-2:5` answer, proving the second Model
request was re-prepared with correct tool results rather than denied.
The internal `BasicTurnAdvance::advance` contract now returns
`TurnAdvanceOutcome { completed: None }` after committing the next
watched request, or `completed: Some(RunResult)` only for a terminal Model answer. Reprepare retains
the Running session and does not pretend to have produced an empty user-visible
answer. It reuses the same M1 graph waves, dispatch, pending tickets and
journal; there is no additional scheduler or client-visible result.
Graph-failure reconciliation still needs independent cancellation,
preparation-denial and restart-between-waves tests before P2 release acceptance.

Pending summarizer selection, verified compaction admission, crash-atomic
journal/session reconciliation and production tokenizer parity remain
release gates.

**Mandatory graph split for P2-3ah onward.** The current Basic submit graph
node remains pending while it invokes and polls the Model. M1 state-triggered
hooks are admitted after a graph wave settles, not inside this pending
node. A merely parallel Advanced node cannot see or gate the final
authoritative ModelRequest. Implement the next slice as separate
**Basic prepare → selected Advanced prepare → Basic Model advance** graph
operations: Basic prepares the exact typed request and ends its graph
wave; a watched, scoped kernel-owned property causes the selected
Advanced prepare node to compute a verified, budgeted replacement; then
a Basic advance node resumes the same durable session/run from an
authorized prepared-request property. Subsequent Model/tool iterations
must preserve this handoff, including their pending tickets and failure
semantics. Use the existing M1 state-change triggers, selected imports,
typed values, grants and graph scheduler. Do not add an Agent::run entry,
manual event bus, protocol or a second scheduling engine.

**Required native acceptance before switching defaults:** the Advanced
node is actually activated and blocks Model admission; a denied or
tampered prepared-request write never invokes Model; a no-compaction
composition yields identical Basic multi-turn/tool results; a pending
summarizer blocks the Model advance graph node; cancellation, restart,
generation replacement and concurrent clients retain authoritative
session identity and physical leases. During migration, the current
Advanced Model adapter remains the working opt-in fallback. Only remove
its compaction-preparation path after these checks pass on the integrated
exact head.


Provide an additive phenix-plugin-advanced-agent plugin for the Advanced composition. It contributes typed graph nodes and event handlers around the selected Basic agent; it does not fork the Basic model/tool loop or implement a second scheduler.

- On prompt admission: resolve authorized memory/context anchors, check code/source revisions, retrieve budgeted evidence and choose a checkpoint/current-view policy before the Model request is prepared.
- On committed conversation/tool events: enqueue idempotent extraction, association updates and optional memory maintenance. Effects carry session, source event and generation identity. A failed background maintenance step cannot duplicate or reorder the active Model/tool turn.
- On context threshold: commit compaction checkpoint and publish the selected compacted view through typed Context state before the next Model call. Refresh only impacted projections through generated on-change hooks.
- On code revision or source removal: invalidate dependent memory references, index entries and derived context. The next lookup checks freshness before using the entry.
- On plugin replacement or authorized contribution selection: compose the updated plugin's declarations with other contributions, validate a candidate graph and atomically promote a new generation. Existing roots remain pinned to their original graph and provider bindings.

Define any needed Memory, ContextView, Compaction, CodeIndex and ReloadControl contracts in a standalone public contract crate. A production implementation remains a selectable provider. Model-facing answers expose the source reference IDs used by retrieval and expansion, so tests and users can inspect evidence. The Basic topology, plugins and binary remain byte-for-byte independent of this additive composition.

## Reference-aware memory

Retain the capabilities evident in oldmain's phenix-plugin-memory: canonical source references, scoped records, context anchors and associations, ranked retrieval, freshness/dependency revalidation, supersession, consolidation, compaction and expansion. Do not copy its old runtime API. The new memory plugin uses M1 public contracts and the selected Storage provider.

### Evidence and identity

Define typed records with: stable memory ID; kind (fact, event, preference, task or derived observation); authorized scope (session, project, user or explicit shared scope); subject/predicate/object or typed payload; source references; source revision and byte/line/span or event offsets when known; observation and validity times; confidence; evidence-versus-inference flag; supersedes links; status (current, historical, uncertain or needs validation); version; and durable creation/update cause. Every answerable factual memory must link to material that can be fetched and checked. References must resolve through authorized providers, not by exposing raw paths or assuming the reference grants access.

Store an immutable source/event record and mutable versioned projections separately. Make source deletion, redaction and consent limits invalidate derived facts and indexes. Avoid cross-session contamination and inferred facts masquerading as user statements. Preserve history when a fact is corrected: current queries select the latest valid supported version; as-of queries retrieve the historical version. Duplicate event ingestion must be idempotent.

### Permanent erasure and retention

Normal factual correction creates a superseding observation and keeps cited history. An authorized **permanent-delete** request is different: immediately deny access to the targeted source and dependent current facts, then purge underlying source records and eligible journal bytes, evidence links, index entries, caches, compacted summaries and other derived content through their owning storage plugins. Redact or invalidate remaining references, including historical and as-of retrieval. Keep only non-sensitive deletion metadata needed to prevent resurrection by replay or later reindexing. A tombstone by itself does not satisfy permanent erasure.

Providers must declare their retention, purge and verification capabilities. Report deletion as complete only when required owned copies are verified erased or made irrecoverable under the declared storage policy. Report inaccessible backups and external copies as explicit limitations; a provider cannot claim completed erasure without evidence. A pending purge must remain inaccessible to retrieval, compaction, observer replay and context hydration. Restart, failed partial purge, and repeated deletion requests must be idempotent, recoverable and visible for authorized inspection. This exception to immutable observations is intentional and does not change ordinary history-preserving corrections.

### Hybrid retrieval: default candidate

Start with three complementary methods:

1. Exact/structured lookup for references, IDs, scopes, time bounds and key/value relations. SQLite tables/indexes and FTS5 are candidates using the existing rusqlite provider.
2. Lexical retrieval with source-term expansion for code identifiers, names, paths and exact wording.
3. A bounded relation/temporal graph walk over entities, associations, supersession and evidence edges. Embeddings and semantic rerankers are optional selected providers, not required for offline correctness.

Deduplicate results by source identity and current validity. Rank with explicit relevance, freshness, confidence, scope and source coverage. Apply retrieval token budgets and expose the scored evidence list plus rejected/stale reasons for debugging. A compact entity profile may help query expansion but cannot replace source evidence. The agent should abstain when evidence is missing or contradictory.

Do not turn on every retrieval backend by default. Measure lexical-only, structured+lexical, structured+graph and optional dense/hybrid profiles against the same tests and token budget. Keep the simplest configuration that demonstrably improves recall, answer accuracy or cost. All modes implement one Memory contract and can be replaced independently.

### Research to reproduce, not performance claims

- [LongMemEval](https://arxiv.org/abs/2410.10813) tests extraction, multi-session reasoning, time, updates and abstention. Its official repository also published a [2026 v2 update](https://github.com/xiaowu0162/LongMemEval).
- [Mem0](https://arxiv.org/abs/2504.19413) motivates selective fact extraction and optional relational memory; compare graph benefit against its cost.
- [Zep/Graphiti](https://arxiv.org/abs/2501.13956) motivates temporal entity/relationship revisions and hybrid graph+text retrieval.
- [Hindsight](https://arxiv.org/abs/2512.12818) separates facts, experiences, entity summaries and evolving beliefs; adopt that distinction where traceable.
- [Redis's 2026 hybrid memory evaluation](https://redis.github.io/redis-ai-research-public/longmemeval-agent-memory/) compares raw retrieval, extracted memories and a hybrid on the same question set. Use this as evidence that retaining raw references alongside extracted facts merits testing.

Published model-dependent scores cannot certify Phenix. Record dataset/version, ingestion budget, context tokens, retrieval precision/recall, final answer correctness, abstention, latency and cost. The offline scripted Model is the gating oracle; external model benchmarks are secondary non-gating reports unless a reproducible model/version is pinned.

## Cross-plugin memory and architecture decisions (P2-2 and P2-6A)

Memory is a shared service, not an agent implementation. It stores evidence-backed knowledge across sessions; consumers decide what that knowledge means. P2-1's `EvidenceRecord`, `SourceRef`, `SourceEvidence` and `EvidenceStore` are canonical. Extend those contracts deliberately instead of adding a second evidence store or a competing `Memory::put/get` API. The P2-1 source-evidence PR stack #896-#898 is prerequisite work, not completed long-term memory.

### Ownership and package boundaries

| Package / owner | Owns | Imports / boundary |
| --- | --- | --- |
| `phenix-advanced-contracts` | Typed evidence, source, scope and future memory/decision contracts | Definition-only package; no execution or database |
| `phenix-plugin-memory` (P2-2) | EvidenceStore persistence, scope-filtered current/as-of retrieval, relationships, supersession and retrieval budgets | Uses one authorized storage provider; no Model, scheduler or domain-specific approval |
| Optional memory curation provider (P2-2 follow-up) | Proposed extraction, deduplication, consolidation and contradiction reports | Reads cited evidence; model outputs remain proposals until validated |
| `phenix-plugin-architecture` (P2-6A) | Design alternatives, contextual ADR guidance and optional implementation review | Consumes Memory, CodeIndex, Workspace/Git, observed test results and optional Model; contributes ordinary nodes to the existing Advanced graph |
| Code intelligence (P2-4) | Current code-entity identity, lineage, source revisions, dependents and freshness | Deterministic Tree-sitter/LSP observations; emits impacted reference IDs, not memory writes by fiat |
| Workspace/Git (P2-6) | Authorized project/worktree identity, commit history and accepted ADR file changes | Git-backed accepted decisions are authoritative; index points to exact file/commit |
| Basic Session/Journal and SQLite Storage | Conversation history, journal events, transactions and persistence mechanics | Do not copy entire chats into architecture memory or turn a session ID into a project identity |

P2-2 must define **authorized project scope** separately from `phenix.session.<id>`. A project has a stable logical identity, independent of filesystem paths and Git remotes. Map repositories, directories, branches and worktrees to that identity only through authorized project membership; matching Git remotes or paths may suggest membership but do not grant it. Sessions in the same authorized project can share project-wide memory and accepted architecture decisions. Branch- or revision-dependent code facts carry applicability and source revisions and must be revalidated before current use. Unauthorized or revoked members cannot retrieve project data even if they know the project ID or a source reference.

The existing Basic Storage interface checks session resource identity. It must remain unchanged for P1. Extend the selected SQLite storage provider with a separate typed, resource-authorized project-record contract or select an equivalent storage provider. Runtime grants, authenticated workspace identity and provider checks decide who may read/write a project; a user-supplied project ID or `SourceRef` grants nothing. Shared/user scopes require separately declared grants and must fail closed where unsupported. A composition may resume an authorized session or project only when its selected persistent data contracts are compatible; incompatible state requires a declared provider-owned migration or a new namespace, not a kernel-invented conversion.

Start with revision-checked records and the existing SQLite transaction implementation, structured filters, lexical FTS5 and bounded relation traversal. Keep source/evidence revisions and immutable observations separate from mutable current projections. An embedding/reranker provider is optional and must outperform the simpler profile on frozen retrieval tests before becoming a default dependency.

### Durable knowledge model and validity

Keep distinct record kinds: verified fact, task/working note, observed failure, verified debugging result, suggested procedure and architecture decision. Store typed payloads or links to source-backed evidence rather than creating unrelated free-text schemas for each producer. Common metadata: stable ID, kind, scope, creator/cause, source IDs, revision, observed and valid times, status, supersedes/dependency edges, affected code entities and exact source/provider revisions.

A decision has `Proposed`, `Accepted` or `Superseded` lifecycle states; `NeedsReview` is a separate validity finding. Accepted project-wide architecture decisions live in reviewed, versioned Git ADR files. Memory indexes those files with their exact commit/digest and may retain proposals, evaluations and rejected alternatives. A model, web page, failed test or subordinate agent cannot publish an accepted ADR without its authorized Git workflow. Contradictory decisions are surfaced as guidance during development, never independently enforced by a second CI architecture-policy engine.

Code lineage and change events trigger targeted validation of affected decision references. A moved symbol may keep a verified identity; deleted or ambiguous entities make dependent conclusions `NeedsReview`. The recorded basis commit proves what was inspected, not that a decision remains current. For current claims, re-check source digests and relevant assumptions before Model injection. Historical/as-of queries retain superseded decisions with provenance. Deletion or redaction invalidates derived projections and retrieval indexes according to retention policy.

### Cross-plugin interactions and review points

| Integration | Input / trigger | Result | Release gate |
| --- | --- | --- | --- |
| Memory + Context/Compaction | Prompt admitted or compacted context requested | Retrieve only authorized, budgeted, current evidence; compaction stores references, never replaces original Journal bytes | P2-2/P2-3 |
| Memory + CodeIndex | Committed workspace edit, external edit, provider epoch change | Revalidate affected code refs, set dependent memories to `NeedsReview` until verified | P2-4/P2-6A |
| Memory + Workspace/Git | Project selection, branch switch, accepted ADR change | Scope decisions to repository/worktree/branch where applicable; reindex exact ADR revision | P2-6A |
| Architecture + code/research | Contract, owner, public API or cross-crate change proposed | Discover existing mechanisms/crates, compare reuse, extension and replacement before implementation | P2-6A |
| Architecture + test results | Functional or conformance tests finish during development | Use real failures and passing evidence to inform optional design review; never add a duplicate architecture gate | P2-6A |
| Debugging + CI + Memory | Failure reproduced, fix independently verified | Store source-linked root cause and passing regression evidence; hypothesis stays provisional | P2-6A/P2-9 |
| Subagents + Memory | Child delegated with bounded parent grants | Child reads permitted project/task evidence, publishes provisional findings; no scope or policy promotion escalation | P2-8 |
| Skills + Memory | Repeated independently verified procedure identified | Propose a versioned skill with examples/evidence for approval; no automatic skill overwrite | Optional after P2 |
| Evaluation + Routing | Repeatable memory/review benchmarks | Compare retrieval quality, architecture regressions, tokens and time; routing changes need measured benefit | P2-9 / later |

Broader planning and generalized skill discovery remain deferred. P2-6A may use a short design proposal for architecture-sensitive edits without implementing a full planner.

### Graph integration and adaptation

The Architecture plugin contributes optional typed operations and graph nodes when the coding agent is designing or reviewing a change. It may read existing functional test results as evidence. Graph preparation validates its declared imports, triggers, predecessors and grants. It uses the existing M1 scheduler, cancellation, generation selection and pending tickets. No additional agent loop, hidden workflow engine, model-driven graph rewriting or kernel-specific architecture branch. Normal CI does not invoke the Architecture plugin's Model.

Classify edits using observable impact. A local implementation-only fix skips model-driven design analysis unless requested. A public contract, new authoritative state owner, scheduler, resource authority or cross-crate dependency change can trigger development-time alternatives analysis. The agent cites code and accepted ADRs when suggesting a design; its advice is not a runtime policy or an extra CI validator. What can be expressed through the actual SDK, resolver, graph compiler and authority model is enforced there. Functionality, including contract rejection and denied effects, is exercised by existing product/conformance tests. No separate rule registry or source-level architecture score is maintained.

The curation plugin may propose memories after committed, uniquely identified journal/test/Git events. Ingestion is idempotent across replay and process restart, revision conflicts reject, and failed background curation cannot alter a running user turn. Curation does not accept architectural decisions or mutate durable job/Session state. Introspection may expose which references and decisions influenced an answer, with authorized redaction.

### Ordered test-first implementation slices

| Slice | Implementation after RED tests | Exit evidence |
| --- | --- | --- |
| P2-2a: project-scoped evidence | Authorized scope identity and CAS provider on existing SQLite mechanics | Independent project/session isolation, forged scope denial, concurrent writes, restart and immutable Basic behavior |
| P2-2b: retrieve/reconcile | EvidenceStore projections, lexical retrieval, relationships and historical/as-of selection | Source revision change, correction, supersession, contradiction, exact citation and abstention |
| P2-2c: optional curator | Source-linked extraction proposals and idempotent event ingestion | Replay/crash repeats do not duplicate records; unsupported model claims cannot be promoted |
| P2-4 integration | Deterministic code reference and dependency invalidation | Rename/move/delete, provider epoch, stale cache and changed worktree tested without Model |
| P2-6A-a: durable architecture decisions | Typed decision records, reviewed Git ADR authority and project/branch applicability | Separate-session recall, exact Git commit provenance, unauthorized acceptance rejection |
| P2-6A-b: contextual design guidance | Optional alternatives and review using current code and accepted ADRs; no CI policy enforcement | Functional test proves correct retrieval, citation, scoping and optional invocation; design quality evaluated outside required CI |
| P2-8/9 integration | Delegated scopes, provisional child contributions, retrieval/quality inspection | Parent/child denial, concurrent proposal collision, redacted retrieval trace and restart |
| P2-10 | Assemble the Advanced verification preset and rerun all cross-plugin cases | Exact-head full Nix matrix, P1 parity and benchmark report on integrated SHA |

Each slice owns a focused implementation PR with functional tests authored first. The Advanced user-facing package must exclude verification-only models and intentionally unaccepted curator behaviors. Do not label P2-2 or P2-6A accepted from unit tests alone; run the existing independent packaged product and negative authorization gates. Do not introduce an extra architecture-only CI pipeline.

### Single source of truth: functionality is the CI target

The implementation itself defines and enforces Phenix's actual constraints: typed SDK contracts, selected bindings, graph preparation, kernel authority, cancellation and lifecycle. **Do not duplicate those constraints in a second architecture schema, policy registry, source-code linter or CI architecture reviewer.** CI launches the real compiled compositions and runs functional, security and failure-path tests against them. A kernel rejection is tested as product behavior, not by independently maintaining a copy of the graph or permission rules. Reuse current conformance cases; only add tests when new observable behavior is introduced.

Accepted ADRs describe engineering rationale for agents and humans; they are not executable policy. They may influence design suggestions but do not create a second runtime authority. Optional manual, model-based design evaluations may compare alternative implementations and long-term maintainability to improve the Architecture plugin. They do not block normal PRs or P2 release, and CI consumes zero design-review Model tokens.

## Development-time design skills, glossary and work slicing

Study the [Matt Pocock skills](https://github.com/mattpocock/skills) patterns as reusable **skills**, not additional Phenix executors, registries or CI gates. Skills supply instructions through the existing selected Skills/Context contracts. Architecture uses authorized CodeIndex, Memory, Git/Workspace and Model providers where available. Reference the upstream skills as prior art; implement Phenix-specific workflows with its typed contracts, not an embedded copy of another agent harness.

| Inspiration | Phenix workflow | Persistence / authority |
| --- | --- | --- |
| [`grilling`](https://github.com/mattpocock/skills/blob/main/skills/productivity/grilling/SKILL.md) | When important design questions remain, build a dependency-ordered decision tree; ask the currently independent frontier with suggested options. Answer code-discoverable facts using tools, not user interrogation. Let unresolved prerequisites defer only dependent questions. | Working session/task context; a model's suggestion cannot mark a question decided for the user |
| [`grill-me`](https://github.com/mattpocock/skills/blob/main/skills/productivity/grill-me/SKILL.md) | Opt-in interview without a repository or writes; same grilling primitive. | Conversation only |
| [`grill-with-docs`](https://github.com/mattpocock/skills/blob/main/skills/engineering/grill-with-docs/SKILL.md) | Opt-in repository-aware grilling; combine interview with domain-modeling, progressively capturing agreed terms and consequential decisions. | Authenticated Git/Workspace writes; no hidden automatic commit |
| [`domain-modeling`](https://github.com/mattpocock/skills/blob/main/skills/engineering/domain-modeling/SKILL.md) | Challenge ambiguous terminology, test edge-case scenarios and compare statements with actual code. | `GLOSSARY.md` for vocabulary only; ADR for consequential decisions |
| [`to-spec`](https://github.com/mattpocock/skills/blob/main/skills/engineering/to-spec/SKILL.md) | Optionally synthesize a large, settled multi-session design without reopening decisions. Small tasks skip this step. | Reviewed spec/issue references; do not make a second implementation authority |
| [`to-tickets`](https://github.com/mattpocock/skills/blob/main/skills/engineering/to-tickets/SKILL.md) | On request, turn accepted plans/specs into fresh-session-sized **vertical, demonstrable functional slices** with only genuine blocking edges and explicit acceptance outcomes. | Issue tracker or Git-tracked local task files via authorized existing tools; review breakdown before publishing |
| [`codebase-design`](https://github.com/mattpocock/skills/blob/main/docs/engineering/codebase-design.md) | Use precise interface/seam/depth/locality vocabulary and test alternative design placements during development. | Reference guidance only; not a new validator or mandatory agent loop |

### Capture at the right granularity

- **Glossary:** use one project `GLOSSARY.md` (or explicitly mapped bounded-context glossaries when needed) to define domain words briefly, with no runtime mirror, policy schema, implementation plan or source-code duplication. Add terms when genuinely settled. Code/contract definitions remain authoritative for operational behavior.
- **ADRs:** create under the repository's established design-document convention only when a decision is hard to reverse, surprising without its rationale **and** reflects a material trade-off. Review and accept through normal Git workflow. Existing accepted decisions stay the memory retrieval source; reject invented ADRs and unnecessary documents.
- **Working memory:** capture provisional alternatives, rejected ideas, open questions, supporting source refs and explicitly resolved answers while grilling. Keep these separate from the glossary and accepted ADRs; pass settled context directly to a spec or implementation without lossy re-synthesis.
- **Work items:** issue descriptions express behavior and evidence, not a mandated file-by-file implementation. Prefer vertical end-to-end slices; a coordinated wide refactor may require expand–migrate–contract or a single integration branch when no individually green slice exists. The task DAG is planning metadata, **not** an alternate runtime workflow graph or a second CI test framework.
- **User agency and cost:** grilling and ticket publication are opt-in or triggered by genuine unresolved decisions. Do not interrogate for code facts, repeatedly grill settled decisions, invoke expensive models for trivial changes or publish issues without approval. Curation can suggest documentation updates but must not silently accept them.

The existing Advanced agent's tests should demonstrate **functional** outcomes: a glossary update persists correctly and remains scoped; a previously accepted ADR is retrieved in a new session; a task handoff retains relevant evidence and blocker identity; unauthorized project/issue writes fail without effects. Use scripted models and existing product checks. There is **no** separate model-based architecture CI gate and no requirement to mechanically enforce the advice in these skills.

## Reversible compaction

A compacted prompt is a derived view, never the sole durable record. The Session/EventJournal append-only ordered messages, complete tool requests/results and source references remain authoritative. A compaction checkpoint records its input event range and hashes, parent checkpoint, selection policy/version, model/version if a summarizer was used, generated summary, retained recent messages, excluded ranges and hydration references.

At a configurable context threshold, preserve system/tool/authority context, unresolved tool calls, important recent exchanges and budgeted recalled evidence. Summarize older completed spans; store the summary with cited immutable ranges. Permit nested checkpoints and repeated compaction without losing the oldest evidence. Expansion can restore an original range byte-for-byte by replaying the journal, or selectively hydrate cited source ranges into a new prompt. Do not claim that a summary can recreate exact wording without the original records.

**Required properties:** transactional checkpoint commit, restart recovery, stable message IDs, deduplication of injected evidence, bounded token count, source visibility, optional human inspection, undo/switch-back to an earlier checkpoint view, and explicit retention policy for intentionally deleted data. Compaction never executes a tool, marks a tool as completed, changes the authoritative session timeline or grants access to hidden sources. If original bytes were intentionally erased, report that expansion is unavailable rather than silently inventing text. Permanent deletion must invalidate compacted summaries, source hydration references and derived context even if earlier checkpoints were otherwise immutable.

Keep compaction, memory retrieval and journal persistence separate contracts. The advanced graph chooses when to compact, what to retrieve and which selected Context view the Model receives. The kernel does not know about token windows or summaries.

## Code intelligence

P2 requires a **deterministically maintained code-entity graph**. Restore oldmain's logical code entities, provider epochs, facet revisions, provenance, relation facts, lineage, identity-continuity states, rebuild checkpoints and edit-validation evidence. This code graph is distinct from the kernel's executable workflow DAG.

### Entity identity and lineage

- Identify entities by repository/worktree, logical ID, language and provider, source revision and supporting symbol facts. Track source file hashes, byte/UTF positions, provider epoch and exact definition/reference spans. Names and line numbers alone are insufficient identity.
- Record rename, move, changed signature/body, insertion, deletion, split and merge as versioned lineage edges, with supporting evidence and explicit confidence/continuity state. Prefer versioned workspace/Git changes, parser AST identity and fingerprints, scoped reference/definition results and validated provider facts. Similarity heuristics alone cannot certify an identity.
- Handle ambiguous successors, reused names, provider disagreement and missing evidence as **unresolved or unknown**. Never silently merge two entities, redirect a stale reference or present an unconfirmed relationship as fact.
- Attach every node/relation and reverse dependency to the exact source/provider revision that supports it. Revise or invalidate affected relations, cache entries, compaction references and memory facts when source or provider epochs change.

### Deterministic validation and reconciliation

- Use maintained Tree-sitter parsers for incremental structure and LSP providers for language-specific semantics, references, rename and diagnostics. These standard providers deliver observations; Phenix's code-intelligence plugin enforces revision, evidence and identity semantics.
- For an edit, verify grants, expected file and entity revisions, range consistency, overlapping writes, parser results and applicable semantic facts. The workspace plugin performs revision-checked writes. After the real filesystem commit, re-read bytes and reconcile parser/LSP observations against the committed revision before publishing a new valid code-graph projection.
- Filesystem writes and index writes do not share an automatic transaction. On indexing failure after a successful file write, mark affected entities and relations **rebuilding/needs-validation**. Queries requiring current facts must fail closed until repair. Offer inverse edits only as separately verified revision-checked operations; never claim an unverified filesystem rollback.
- Handle workspace writes, external editor updates, Git checkouts, missed watcher events, language-server restarts, provider-epoch shifts and process crashes. Persist rebuild checkpoints and an observation trail. Verify source hashes when serving current claims, then reindex the affected neighborhood or rebuild as needed.
- Return source/provenance IDs, exact revision, relation coverage and freshness with every result. When support cannot be verified, return partial/stale/unknown explicitly. The memory plugin and Model-facing Context accept only validated current code references for current factual claims.
- Keep symbol identity, lineage validation, graph repair and edit safety **model-independent**. A model may propose edits or interpretations, but cannot invent confirmed lineage, suppress failed diagnostics or mark an index current without deterministic evidence.

Public tools cover file/symbol search, outline, definitions/references, diagnostics, changed-neighborhood queries, lineage/history, source-backed graph relations and edit preview/validation. Rust is the first acceptance language. Language-provider adapters stay replaceable.

Accept only with direct fixture checks of code-graph integrity independent of the scripted Model, plus product tests that prove the Advanced agent receives verified results or explicit unknown/stale outcomes. Include rename/move/split/merge ambiguity, same-name different-entity, edit conflicts, failed post-write validation, external file changes, provider epoch changes and crash/restart repair.

## Plugin-authored runtime graph evolution and safe hot reload

The active workflow graph is an immutable composition of **plugin-owned** node, edge and trigger contributions. P2 must support changes to that composition **while Phenix runs**. When a process plugin is replaced by a richer version, its new contribution may add a validation step, replace an executor node or insert verification before completion. This is a plugin contract change, not an agent rewriting the kernel graph.

### Runtime contribution changes

- The SDK/macros publish inspectable typed contribution descriptors with stable node/action IDs, triggers, predecessor edges, join/failure policies, resource grants and read/write effects. Updated plugin artifacts may publish different contributions. Authorized configuration can also select among published contribution sets from installed providers.
- All executable node/action implementations and maximum grants must be declared in validated package metadata. If runtime proposals are needed, they only reference already declared callable IDs, typed operations and bounded resources. No unvalidated plugin code, new permissions or arbitrary mutable graph patching.
- On install, replace, remove or authorized contribution selection, the kernel resolves a **complete candidate graph** from selected plugin declarations. Validate package identity, imports, schema and resource grants, ownership, anchors, trigger contracts, cycles, write conflicts, fan-out, effects and dependency order using the existing M1 compiler and resolver before activation.
- Record the candidate graph digest, contribution owners and structural diff. Stage required plugin state migration separately, then atomically promote the new immutable generation. **New roots use the new graph. In-flight roots stay pinned to the old graph and provider leases** until completion/cancellation and physical drain. Do not splice new nodes into running executions.
- If work must continue under a new topology, submit an explicitly authorized new root with correlation and idempotency data. Failed compilation, denied grants, missing callbacks or migration failure keep the last active generation untouched.
- Provide typed control/inspection operations to stage, diff, validate, promote, roll back and inspect graph generations. The kernel remains the single graph authority. Logging and introspection consume its generic lifecycle events.

Example: plugin version A contributes `start -> execute -> complete`. Its replacement B contributes `start -> validate -> execute -> verify -> complete`. Upon authorized reload, Phenix validates B's declared effects and promotes generation B. A new run follows all five steps; an existing run completes using A's three steps. An invalid version C cannot change either run.

### Native lifecycle and state

Plugin reload is a generic kernel generation transition requested through an authorized control plugin or host. Validate ABI, descriptor, package hash, contracts and maximum authority before candidate execution where offline checks suffice. Stateful plugins declare snapshot/export/import migrations or an explicit restart-required policy. Migration and generation publication must be rollback-safe; a graph change does not imply a state migration. State compatible with the declared persistent contract may be reused by another provider; incompatible versions require explicit provider-owned migration or a fresh state namespace, with failure-atomic rollback.

Pinning a graph or provider never preserves revoked live authorization. Kernel-mediated effects recheck current grants before use, even for roots admitted on a previous generation. See the [M1 runtime authority rules](m1-roadmap.md) for the one authority boundary and negative revocation fixtures.

Support native plugin add/replace/remove with content-addressed loaded artifacts and deferred retirement. No library unload while an old callback, observer, ticket or leased state can still use it. Recover the last committed generation after a staging crash and never execute constructors of denied candidates. Reuse kernel resolver, graph compiler, dispatcher, generation manager, leases and cancellation; do not add a second scheduler or mutable graph registry.

## Workspace management (P2-6)

Restore the useful behaviors of oldmain's `phenix.workspace` and local environment plugins. Define a Workspace contract for root-scoped file listing, read, search, create, move, delete, versioned writes and Git status/diff operations. Keep filesystem and process I/O in selected workspace/environment plugins backed by maintained libraries. The kernel owns only generic effect admission and authority. P2's Basic user-facing product must not acquire these grants.

- Use an explicit workspace root and per-operation grants: read, write, execute, git. Enforce canonical filesystem containment at the actual OS operation, including symlink and race protections.
- Implement revision-checked edits and atomic multi-file batches where the filesystem permits. Return receipts with before/after revision hashes and conflict/error states. Never silently overwrite changed content.
- Expose a typed terminal/process tool that streams bounded stdout/stderr, reports exit/interrupt state, supports cancellation and does not block kernel dispatch. Capture process IDs and recovery handles only when supported.
- Git status, diff and permitted repository operations should use established Git libraries or Git subprocesses with explicit argument arrays and policy checks. Do not invent a new Git protocol or shell escape grammar.
- Integrate code intelligence invalidation with actual workspace revision events. A file write triggers reindexing only for affected documents.

Acceptance: an independent scripted Model calls read, edit, diff and command tools through the composed Advanced executable; the fixture asserts actual bytes, command output, revision rejection, traversal denial and correct cancellation. Inspect grants for zero denied effects. Use a temporary isolated repository, not the host's working tree.

## Durable jobs (P2-7)

Add a selectable Jobs plugin for work that outlives a single tool call or frontend attachment. A job is a persistent, authorized record and state machine linked to its owning session or project. Record job ID, operation identity, parent/root, granted resources, selected provider/generation, status, progress cursor, output refs, started/updated times and recovery policy. Reuse Session/Journal/Storage for durable writes and observable changes.

Support create, list/get, attach/detach, progress/log retrieval, cancel, complete, error and garbage collection. Job creation and transitions must use idempotency keys and compare-and-swap; repeated requests or journal replay cannot run a side effect twice. A job may be promoted to project lifetime only under explicit grants and a recorded lease change.

**Durable record is not a promise of OS process survival.** After a host restart, a resumable task with a reliable checkpoint can continue; an unmanaged physical process must be reconciled or marked interrupted/unknown. Never report success or rerun a non-idempotent external command without verified outcome. Keep long-running execution in dedicated environment/process plugins, not inside the kernel or a duplicate scheduler.

Acceptance: submit, detach, reconnect, inspect partial output, cancel and restart a real packaged job. Simulate a crash at enqueue, after effect admission and during cleanup. Assert one job identity, monotonic cursor, correct owner authorization, no double execution and no leaked leases.

## Sub-agents and session trees (P2-8)

A sub-agent is a child session with an independently recorded message/journal history. The parent delegates a task through typed agent/Session operations, not by calling the Basic run loop directly or creating a second executor. Each child has a stable parent ID, task/goal, selected composition/profile, inherited-or-reduced tool/model grants, explicit memory visibility, budget limits and cancellation scope. Support multiple concurrent children through the kernel's ordinary graph scheduling and pending tickets.

The SessionTree plugin maintains parent/child links, cycle and depth validation, status, ordered child results, durable references and a parent-visible summary of the child's work. Children retain their own conversation and checkpoints. Parent history includes delegation and result references; do not flatten child messages into the parent transcript. Parent cancellation has an explicit policy for active children, and deliberate detachment requires a separately authorized durable job. Delegation cannot elevate parent capabilities, model budgets, token limits, workspace roots or memory scopes.

Provide an API for spawn, inspect, list, communicate, await, cancel and resume child sessions. The parent may delegate to multiple children, combine their results and revisit cited child evidence. Restart recovers links and unresolved requests. Child failures are explicit outcomes, not missing completion events.

Acceptance: the deterministic Model directs one parent to delegate two different subtasks. The real graph creates distinct child sessions and model turns, returns evidence-bearing results and resumes correctly after restart. Test delegated grant denial, recursion depth, total budget, one-child failure, independent cancellation and a stale child-generation/provider binding. A canned response that bypasses child sessions fails verification.

## Inspection, debugging and efficiency evaluation (P2-9)

Preserve the agreed separation: **kernel emits generic events**, a logging plugin consumes and persists authorized events, an introspection/debug plugin exposes a typed inspect/query interface, and an efficiency-evaluation plugin computes reports from events plus Model/Tool usage records. This is not an agent-specific kernel debug API or a second runtime controller.

The inspect interface must report selected graph and generation IDs, plugins/providers, contract bindings, in-flight roots, pending tickets, state revisions, session trees, job states, journal cursors, memory retrieval provenance, compaction checkpoints, tool errors and recent lifecycle transitions. Expose safe filtered snapshots; only authorized callers may inspect other sessions or private tool arguments. Bound expensive traversals, pagination and snapshot size. Sensitive model/tool data must be redacted by default and never leaked to lower-authority observers.

The efficiency plugin measures model calls and tokens, cost when a provider supplies valid pricing, wall and model/tool latency, tool success/failure, compaction savings versus rehydration cost, memory retrieval hit/quality, child/job fan-out, and trace-level resource use. Separate actual measured usage from estimates. Use stable correlation IDs across root/child/job/model/tool events and version all event/replay schemas. Prefer OpenTelemetry-compatible tracing/metrics libraries if their dependency cost fits; avoid implementing a custom observability transport.

Provide inspect-by-session and inspect-by-root paths, replayable event offsets and exportable benchmark reports. A failed observer must not block a live root. Inspection cannot mutate runtime state; any control action requires its own authorized contract.

Acceptance: on a scripted multi-agent, multi-job session, inspect exact graph/provider bindings and correlate all child/model/tool transitions with one root. Verify replay across process restart, redaction, denied cross-session reads, stalled observer backpressure and metrics recomputed consistently from the same recorded events.

## Code intelligence and hook scope

**Code intelligence in P2:** entity lineage, edit validation, provider-epoch tracking, revision-grounded relations, deterministic rebuild and fail-closed stale-source handling are required. They work without a Model. Deep optional query syntaxes may remain deferred, but correctness of the code graph and source references is not optional.

**Hooks in P2:** generated typed `on_change`/`on_event` triggers remain the plugin-authoring mechanism. **Runtime graph evolution is required** through new or selected plugin-owned contributions, recomposed and published as new validated immutable generations. The old general-purpose user-written hook-rule language remains deferred. It is not needed to replace a running process plugin with one that contributes extra steps.

## Deferred after P2

Full objective/plan/exploration management, repository-worker task scheduling, generic artifact catalogs beyond the existing source/output references, user-configurable runtime hook rules, a broad options/preferences service, command-toolbelt discovery, rich interactive UI and non-Nix installation tooling. Keep existing P1 functionality and permitted public plugin extensions. These deferred items do not block P2 acceptance.

## Advanced verification plugins and acceptance

Create a dedicated phenix-plugin-advanced-test-model (or similarly named fixture crate), packaged only with phenix-advanced-verification. It implements the same real typed Model contract. For each scenario it consumes the complete Model request, including actual ordered messages, tool-call IDs/results, contextual evidence and selected revisions. Its deterministic state machine refuses a golden answer when required evidence is absent or stale. It must not read the test answer directly from test harness state, fixture filesystem or private Storage records. Provide a second controllable fixture for source revisions, clock, provider failures and hot-reload lifecycle as needed.

Every test runs the packaged executable, not a function that bypasses the graph. The suite records rendered output, actual model inputs, tool effects, journal offsets, state revisions, artifacts and provenance. Use an isolated SQLite database and real restart between phases. At least one independent ACP client and one Terminal or JSONL frontend drive the same scenarios.

| ID | Independent scenario | Required result / negative control |
| --- | --- | --- |
| AV-01 | P1 Basic multi-turn, tools, streaming, cancellation and replay on Advanced preset | Identical observable P1 semantics; no hidden fixture provider |
| AV-02 | Two sessions; fact introduced in A and asked in B with authorized shared scope | Correct recall with source citation; denied scope cannot retrieve it |
| AV-03 | Later correction to earlier fact; current and as-of questions | New answer for current query, old answer for historical query, both with provenance |
| AV-04 | Multi-hop association, partial evidence, contradictory evidence and no supporting fact | Only supported answer; abstain when evidence insufficient; no fabricated citation |
| AV-05 | Source revision changes or memory explicitly retracted | Invalidate stale fact and related code references; no stale answer |
| AV-06 | Real input exceeds configured context threshold; run compaction | Model receives bounded compacted view; old facts stay retrievable through references |
| AV-07 | Ask verbatim question about old compacted text; expand by journal range | Original text byte-for-byte; no summary hallucination; undo selects old view |
| AV-08 | Crash during summary checkpoint, during tool call and after committed checkpoint | Restart restores consistent state; no duplicated side effects or orphaned tool-call completion |
| AV-09 | Parse Rust repository with symbol relations, references, diagnostics and provider epochs | Exact revision-bound source evidence; no invented or stale current relation |
| AV-10 | Rename, move or edit an entity between requests | Proven lineage with source revisions; dependent references and memory invalidated as required |
| AV-11 | Replace plugin whose new contribution inserts validation and verification steps | New roots use expanded graph; old roots finish old graph with leases safely drained |
| AV-12 | Reload stateful memory provider with schema migration | State survives across generation and process restart; invalid migration rolls back |
| AV-13 | Denied grants, forged package, incompatible ABI and corrupt graph on reload | Candidate rejected before effects; active generation unchanged |
| AV-14 | Remove/re-add plugin with pending roots, cancellation and multiple graph generations | No use-after-unload, leaked tickets, doubled observer delivery or partial graph mutation |
| AV-15 | Build all five presets and inspect closures and contract bindings | Fixture plugins only in verification products; Basic unchanged |
| AV-16 | Compare lexical, hybrid and optional dense memory using frozen corpus | Report answer quality, retrieval completeness, p95 latency, token use and DB size; no ungrounded benchmark claim |
| AV-17 | Read, edit and diff an isolated Git worktree through Advanced tools | Real file bytes and Git diff match expectation; symlink escape and unauthorized write have zero effects |
| AV-18 | Concurrent workspace edit and running shell command | Revision conflict rejects overwrite; bounded stdout/stderr and cancelled process report correct terminal state |
| AV-19 | Launch, detach and reattach a durable job across a frontend disconnect | Stable ID, monotonic journal/log cursor, no duplicate execution and authorized job ownership |
| AV-20 | Crash during job admission, effect and finalization; restart | Resume checkpointed work or report indeterminate state; no silent command rerun or leaked lease |
| AV-21 | Parent session delegates to two separate scripted-model child sessions | Independent child journals, actual model requests/tool results and parent aggregation with referenced evidence |
| AV-22 | Delegate with narrower grants, limit or depth; cancel one child | Cannot escalate permissions/budgets; failure/cancel propagation and unaffected sibling behavior match policy |
| AV-23 | Restart with an incomplete child and detached job | Parent-child lineage and job ownership survive, and reconciliation is explicit rather than a fabricated success |
| AV-24 | Inspect a running graph, generation, session tree, job, memory and compaction state | Typed inspect snapshots match verified events and references, including pending/terminal transitions |
| AV-25 | Request debug data across sessions; replay after restart with slow observer | Sensitive fields redacted, denied data inaccessible, independent observer cursors and bounded backpressure |
| AV-26 | Measure scripted multi-tool and child-agent scenario under two memory/compaction policies | Counts, usage, latency and source citations are reproducible; estimates distinguished from measured costs |
| AV-27 | Disable generated on-change hook or deny a job/child import | Required state refresh or delegation test fails deterministically; no alternate private control path |
| AV-28 | Rename/move symbols with duplicates or ambiguous split/merge lineage | Deterministic evidence only; ambiguous candidates remain unresolved instead of falsely unified |
| AV-29 | Propose code edit with stale revision, conflicting spans, invalid syntax or changed references | No incorrect valid code-graph publish; failed post-write validation explicitly marks affected entities stale |
| AV-30 | External edit, missed watcher event and provider epoch change | Source digest revalidation and targeted rebuild eliminate stale current relations without Model intervention |
| AV-31 | Crash between durable filesystem edit and code-index publish | Persistent rebuild state; no false current claim before verified reindex and restart reconciliation |
| AV-32 | Hot-replace a process with a plugin contributing additional graph nodes/edges | Candidate graph diff validated, new root executes extra steps, old root remains on old topology |
| AV-33 | Switch selected published plugin contributions during runtime | Authorized candidate generation publishes atomically; invalid import/anchor/cycle keeps existing generation |
| AV-34 | Submit undeclared callable, forbidden graph write or incompatible schema in candidate | Reject before effects or constructor; no scope escalation, graph corruption or lost session |
| AV-35 | Write project decision in session A, restart, retrieve in B | Same accepted decision and exact Git source, with no chat replay required |
| AV-36 | Denied project or sibling worktree attempts read/write using a known memory ID | No data disclosure or write; untrusted ID/reference grants no authority |
| AV-37 | Competing memory writers and journal replay repeat a proposal | CAS conflict or idempotent no-op; no lost update or duplicate item |
| AV-38 | Proposed decision contradicts accepted ADR; model tries to approve it | Accepted policy unchanged, conflict reported with both source IDs |
| AV-39 | Rename, delete or change source supporting an accepted design | Deterministic lineage preserves proven links or marks decision NeedsReview; no false-current claim |
| AV-40 | Two child agents contribute findings; one lacks project grant | Authorized proposals visible with provenance, denied child cannot read or promote a project-wide accepted decision |
| AV-41 | Verified regression fix generates memory; source is later retracted | Debugging insight retrieved with test evidence, then withheld as current when source withdrawn |
| AV-42 | Two authorized worktrees and branches of the same stable project, followed by project-access revocation | Share only project-applicable evidence; branch-sensitive claims revalidate; removed member cannot retrieve or write |
| AV-43 | Permanently delete a source referenced by journal, memory, index and compaction | Invalidate access immediately, verify owned-source erasure, reject stale replay/rehydration and report external-copy limits accurately |
| AV-44 | External Nix flake overrides a Basic or Advanced preset | Uses ordinary `.override`, preserves original preset, validates bindings/grants through real kernel; no private manifest language |
| AV-45 | Compatible composition resumes authorized durable session; incompatible provider cannot | Preserves original history and selection, or fails explicitly without silent substitution or data loss |

For each test include a relevant counterfactual: remove or corrupt retrieved source, change only the revision, disable the expected graph edge, reorder independent events, deny access, or revoke a child/job grant. The test must then fail or abstain. Assert a known exact answer only after proving the model actually consumed the cited evidence. Validate that the fixture model can also perform a tool-call/continuation sequence, instead of acting as a static canned-answer lookup.

Add independent contract/ABI vectors, compiled external plugin fixtures, malicious negative cases, native sanitizer or equivalent lifetime tests where feasible, and repeated process restarts. Run Rust unit/integration and Nix package checks first, then the full canonical flake CI release inventory. Require exact-head artifacts and a green result again on the integrated main SHA.

## Compatibility inventory from oldmain

The original archived advanced profile included memory, planning, language, repository workers, jobs, session trees, workspace, hooks, debugging, command toolbelt, options, execution and environment adapters. Before P2-0 closes, classify each as implemented by P1, a required P2 replacement, or deferred. **P2 deliberately targets selected advanced capabilities, not full oldmain feature parity.** Record user-observable scenarios rather than simply restoring crate names. Planning, stand-alone artifact catalogs, generic repository workers, dynamic user-configurable hooks, broad configuration and interactive UI are deferred. The selected P2 features must meet their oldmain behavioral baseline where applicable.

Retain oldmain's tested memory-specific semantics where they remain useful: immutable records, scope, freshness, canonical code references, source validation, embeddings/reranking as optional providers, source supersession and confirmation of contextual associations. Port contracts and acceptance, not oldmain's implementation-bound service calls.

## Naming and adjacent post-P1 work

PR #852 used P2a–P2d for multi-client state projection and I/O routing before the Advanced release label was assigned. Keep its work and dependency, but name its track P2-IO1 through P2-IO4 when integrating its roadmap. Advanced P2-0 through P2-10 is the product track. The multi-client I/O track can proceed in parallel after the CI gate; P2 does not depend on synchronized multiple frontends unless a verification scenario explicitly requires them.


Standard [client-contributed MCP tools, resources and skills](m1-roadmap.md#client-contributed-mcp-capabilities-post-p1) belong to the adjacent post-P1 I/O and client-management track. Basic and Advanced may compose the same MCP adapter; Advanced adds no distinct tool registry, permission engine or release dependency. Client-provided tools remain scoped to their authenticated origin and must pass the same kernel authority checks as other tools.

Standard [A2UI interactive UI support](m1-roadmap.md#post-p1-p2-shared-session-projections-and-typed-io-routing) belongs to the adjacent P2-IO2 through P2-IO4 frontend track. It uses upstream A2UI contracts, not a Phenix-specific UI protocol. It does not make rich interactive UI an Advanced P2-0 through P2-10 release gate; keep this requirement and its tests in the M1 roadmap.

## Release decision

P2 is ACCEPTED only when the packaged Advanced product also passes the interactive provider-authentication and model-selection gate owned by [M1 runtime management #891](https://github.com/matthis-k/phenix-ai/issues/891) and the [M1 roadmap](m1-roadmap.md): one- and two-method onboarding, connection-specific model catalogs, secure credential handling, dynamic/static discovery, revision-checked selection and an actual authorized Neovim Advanced turn. OAuth subscription integrations must be officially supported for this application; otherwise they report unavailable rather than spoofing a login. General A2UI and visual customization remain outside this release gate. In addition, all P2-0 through P2-10 gates must pass together, including P2-6A and functional cases AV-35 through AV-45, each preset starts as a packaged executable, memory/compaction can produce verifiable evidence, code intelligence handles revision changes, hot reload is generation-safe, workspace and durable jobs pass their fault/authority tests, child sessions survive restart with bounded grants, inspection/efficiency evidence is available, and Basic tests still pass. Publish a comparison against the oldmain capability inventory and actual benchmark measurements. Keep the milestone PLANNED until executable checks exist.
