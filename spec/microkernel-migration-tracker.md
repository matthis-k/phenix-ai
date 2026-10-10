# Microkernel migration work tracker

Status: active, documentation only. Updated: 2026-10-10. Coordination PR: this branch. Do not merge this PR as a feature. It tracks ownership, proof and retirement across implementation PRs.

## Read first

Normative architecture: [kernel RFC](kernel-runtime-rfc.md), [microkernel composition roadmap](microkernel-composition-roadmap.md), [native ABI design](native-plugin-abi-and-guest-runtimes.md), [selectable generations](selectable-generations.md). Current migration detail: [#726 topology](https://github.com/matthis-k/phenix-ai/pull/726), [full repository disposition audit](post-redesign-file-dispositions.md).

The RFC, not any interim implementation, decides architecture. The merged disposition audit is a *provisional* classification, not a deletion authorization. Keep it unchanged as the baseline. This tracker records execution and evidence.

Working tables:
- [Entire main-tree file checklist](microkernel-tracker/repository-file-matrix.md): every tracked file, one check per exact path, reconciled with the merged disposition audit.
- [#726 changed-file checklist](microkernel-tracker/pr726-file-matrix.md): every added/modified path, explicit decision, replacement and checks.
- [Per-PR execution matrices](microkernel-tracker/pr-task-matrices.md): atomic tasks grouped by owner PR. Copy or link each group in that PR. Update the owner matrix first, then the row here.

## Updating state across sessions

1. Start with this document; inspect the owning PR HEAD, base, CI and the latest owner-task matrix. Do not reconstruct status from conversation history or rely on a past green CI.
2. Work on one owned task ID. Use `TODO`, `IN PROGRESS`, `BLOCKED`, `PROVEN` or `REMOVED`. A checked box means the acceptance proof is linked at the same task, not simply that code was written.
3. Update the owner PR's own task matrix and its implementation. Update this tracker only when the owner gate changes, the file disposition is decided, or a dependency changes. Record PR and commit/test links beside the completed item.
4. If an action spans owners, split it into ordered tasks or move it to a new PR. Stop adding unrelated work to #726. A task with a new mechanism requires a corresponding RFC invariant, an owning PR and a replacement/removal decision before implementation.
5. Before deleting a file, check Rust imports, direct consumers, feature flags, tests, Cargo/workspace/Nix/CI packaging, external protocol identity and replacement behavior. Migrate semantic tests first. `Retire` means deleted and verified; `Keep` means justified and verified.
6. Each session leaves a short dated handoff in the owner PR: completed IDs with commit SHA, next unblocked ID, unresolved failures, exact CI run URLs and paths changed. Keep the matrix as the enduring record rather than multiplying progress comments.
7. Refresh this inventory after each merge: compare `git ls-files` with the tracked main-tree checklist; add new paths and retain removed paths as `REMOVED` with the deleting PR. Never silently drop a row.

Owners are non-overlapping. Status is conservative: a partial implementation or green unit suite remains `TODO` until the stated end-to-end acceptance holds.

## Order and dependencies

`S0` architecture and file ownership -> `S1` slim canonical executor (#726) and typed descriptors (#728) -> `S2` selected contributions (#760), native loader extraction, entry/IR completion -> `S3` patch composition (#729) and kind lowering (#730) -> `S4` real consumers (#731), Basic/Full parity, durability (#725) -> `S5` legacy retirement and repository cleanup -> `S6` integration and closeout.

#729 implementation waits for stable #726 IR and #728 envelope. #730 kind declarations can proceed once #728 stabilizes. #731 waits for both #729 and #730. #725 depends on #760; #763 is integration verification, not a product implementation. Native loading must have its own implementation PR and must use canonical Core dispatch; its design contract is already merged as #734.

## Central dependency and proof matrix

| ID | Status | Owning PR | Depends on | Task and completion proof |
| --- | --- | --- | --- | --- |
| S0-01 | PROVEN | #736 | none | Kernel RFC merged; use `spec/kernel-runtime-rfc.md` for decision boundaries. |
| S0-02 | PROVEN | #727 | none | Ordered microkernel composition roadmap merged. |
| S0-03 | PROVEN | #733 | none | 592-path provisional audit merged; **not** proof of file liveness. |
| S0-04 | PROVEN | #734 | none | Native/plugin guest runtime design merged; runtime implementation remains open. |
| S0-05 | TODO | tracker | #726 audit | Freeze exact diff baseline, assign every #726 path once and record keep/move/simplify/retire with evidence in [file matrix](microkernel-tracker/pr726-file-matrix.md). |
| S0-06 | TODO | tracker | #733 | Verify each current main file independently in [main-tree checklist](microkernel-tracker/repository-file-matrix.md); classify replacements and consumers before marking done. |
| S0-07 | TODO | tracker | S0-05 | Track net production lines, removed paths, obsolete APIs, and CI/parity before/after. Count tests separately. |
| S1-01 | TODO | #726 | S0-05 | Canonical four-step Invoke/Fork/Join/Exit IR with cycles, finite map fan-out, all closed joins, stable IDs and bounded inline expansion; non-agent conformance. |
| S1-02 | TODO | #726 | S1-01 | One executor entry internally; consolidate sync/pending and framed/unframed request, projection, rollback and error handling. |
| S1-03 | TODO | #726 | S1-01 | One root/call scope, resolver, authority, dispatch/Layers and pinned generation; proof with substituted providers and stale bindings. |
| S1-04 | TODO | #726 | S1-02 | Review `workflow_tasks.rs` against `tasks.rs`; retain only workflow ticket/scope state that general TaskRuntime cannot represent. |
| S1-05 | TODO | #726 | S1-01 | Typed frame private/published slots, mapped includes, frame/alias and cancellation semantics; negative and positive integration tests. |
| S1-06 | TODO | #726 | S1-02 | Native provider callbacks resume same scheduler; cancellation and lease retention until actual settlement, including failing and abandoned callbacks. |
| S1-07 | TODO | #726 | S1-01 | Eliminate provisional test-only compiler paths and duplicate intermediate execution models after behavior parity; document remaining compatibility entry points. |
| S1-08 | TODO | #728 | S0-01 | SDK authoring: ordinary service `interface + implementation + provide` needs no workflow or manual registration; contract IDs stable. |
| S1-09 | TODO | #728 | S1-08 | Typed static `Declare/Require/Provide/Modify/Observe` and owned portable descriptors; duplicate-key and schema rejection. |
| S1-10 | TODO | #728 | S1-09 | Selected plan definitions and EntryBindings accepted from artifacts; no builder-only registration and no second resolver. |
| S2-01 | TODO | #760 | #728 | Freeze selected portable contributions, owner provenance and identity in candidate generation; deterministic enumeration, reject forged/stale artifacts. |
| S2-02 | TODO | native implementation PR to create | #734; S1-03 | Extract ABI/loader/bridge changes from #726 into independently reviewable owner; retain Core only minimal generic invocation contract. |
| S2-03 | TODO | native implementation PR to create | S2-02 | ABI table, buffer lifecycle, begin/poll/wake/cancel, callback ownership and native cross-import canary; no plugin-specific Core branch. |
| S2-04 | TODO | native implementation PR to create | S2-03; #760 | Versioned side-by-side library residency, trial/promotion/retirement and failure isolation; canonical selected graph and physical lease proof. |
| S2-05 | TODO | future guest adapter PR | S2-03 | Lua guest adapter as normal native plugin; `require("phenix")` guest vs Neovim client isolation and Rust <-> Lua imports. |
| S3-01 | TODO | #729 | S1-01; S1-10 | Closed IR edit vocabulary, versioned slots, deterministic patch composition and provenance/conflict rejection. |
| S3-02 | TODO | #729 | S3-01 | Validate patch owner-only metadata, frame extension and attenuation before activating candidate. |
| S3-03 | TODO | #730 | S1-09 | Typed contribution kind provider schemas; bounded pure template lowering and explicit bootstrapping/cycles. |
| S3-04 | TODO | #730 | S3-03 | Exceptional procedural preparation frozen *before* resolution; no callbacks in canonical resolver. |
| S4-01 | TODO | #731 | S3-01; S3-03 | Migrate tools and skills from hand wiring to declared contributions. Test third-party replacement and non-agent kernel-only flow. |
| S4-02 | TODO | #726 and #731 | S1-05; S1-10 | Real Basic/Full turn-tool-turn equivalence: outputs, typed continuations, IDs, budgets, events, streaming, progress, cancellation, error and side effects. |
| S4-03 | TODO | #725 | #760 | Durable idempotent admission and writer claims, restart/orphan recovery, fencing, journal CAS and no replay of ambiguous side effects. |
| S4-04 | TODO | #763 | S4-02; S4-03 | Full pinned-head integration CI on actual combined stack; include Nix product, SDK docs, Core, harness, native ABI and client checks. |
| S4-05 | TODO | owner to assign | S2-04; S4-02 | Selected plugin upgrades preserve old roots, generation provenance, Environment pins, journal events and durable compatibility. |
| S5-01 | TODO | cleanup implementation PR to create | S4-02 | Remove legacy `phenix-plugin-basic-agent/src/agent_loop.rs` execution path and route-selection duplication; rehome behavior tests first. |
| S5-02 | TODO | cleanup implementation PR to create | S4-01 | Retire `phenix-plugin-hooks` after Layers/Listener parity; remove Advanced default and Cargo/Nix references. |
| S5-03 | TODO | cleanup implementation PR to create | S4-01 | Replace hardcoded `phenix-plugin-catalog` factories/manifest lists, harness builder and default Basic profiles with portable discovery. |
| S5-04 | TODO | cleanup implementation PR to create | S4-01 | Move AI-specific contracts from Core/SDK to plugin-owned interfaces, keeping wire IDs and compatibility. |
| S5-05 | TODO | cleanup implementation PR to create | S4-02 | Audit `phenix-plugin-step-runner` and `phenix-plugin-execution/src/tool_schedule.rs`; keep tool semantics, remove duplicated orchestration. |
| S5-06 | TODO | cleanup implementation PR to create | S5-01; S5-03 | Move deterministic `phenix-plugin-basic-model` into test fixtures; stop selecting echo as production default. |
| S5-07 | TODO | cleanup implementation PR to create | S5-01 through S5-06 | Update `rust/Cargo.toml`, `rust/Cargo.lock`, Nix packages, product configs, CI, clients and docs; delete verified stale crates/files. |
| S5-08 | TODO | tracker | S5-07 | Reconcile all rows in both file matrices with changed tree. Every Retire/Move path has replacement PR + test evidence or documented `Keep` decision. |
| S6-01 | TODO | #763 or new final integration PR | S2-S5 | Final focused and full checks on exact merge candidates, including non-agent service-only and product parity. |
| S6-02 | TODO | tracker | S6-01 | Closure report: paths/classes removed, net production LOC, total tests, mechanisms before/after, open exceptions and owning follow-ups. |
| S6-03 | TODO | tracker | S6-02 | Close tracker after all required matrices are complete and remaining scope is explicitly assigned to new PRs. |

## Current PR owners

| PR | Actual state at 2026-10-10 | Scope rule |
| --- | --- | --- |
| [#726](https://github.com/matthis-k/phenix-ai/pull/726) | 19,611 added, 266 removed, 58 files; partially implemented, **not accepted** | Generic plan compiler/executor and bounded agent migration. Native loading to be extracted. |
| [#728](https://github.com/matthis-k/phenix-ai/pull/728) | partial implementation; stacked on #726 | SDK + authoring envelopes and portable plan/entry metadata. |
| [#760](https://github.com/matthis-k/phenix-ai/pull/760) | partial implementation; stacked on #728 | Selected contributions frozen into generation. |
| [#725](https://github.com/matthis-k/phenix-ai/pull/725) | partial implementation; stacked on #760 | Durable prompt admission and claims. |
| [#729](https://github.com/matthis-k/phenix-ai/pull/729) | design only | Generic IR patch composition. |
| [#730](https://github.com/matthis-k/phenix-ai/pull/730) | design only | Contribution-kind templates and preparation. |
| [#731](https://github.com/matthis-k/phenix-ai/pull/731) | design only | Tool/skill consumers and non-agent proof. |
| [#763](https://github.com/matthis-k/phenix-ai/pull/763) | integration branch, not a product PR | Full combined-stack validation only. |

The [#733 audit](post-redesign-file-dispositions.md) already lists concrete downstream consumers, including `phenix-plugin-hooks`, `phenix-plugin-basic-model`, `phenix-plugin-catalog` and `phenix-plugin-step-runner`. Reuse that list, then test the exact implementation before deletion.

## Non-negotiable acceptance

- Core remains domain-neutral. Ordinary services do not require plan, agent, profile or graph-patch machinery.
- Exactly one canonical resolver, permission lineage, service invocation, root task lifecycle and execution scheduler. Sync/pending are modes of one executor.
- Each new mechanism has a named RFC requirement, owning PR, file set, replacement target and positive/negative conformance tests.
- Every checked implementation item cites a **current-head** commit and test run; CI success never substitutes for a missing semantic proof.
- Verify complete file coverage on `main` and changed PR branches before final cleanup. The tracker is a coordination record; implementation stays in the owning PR.
