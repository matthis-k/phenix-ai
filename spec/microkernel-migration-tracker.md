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
| S0-05 | IN PROGRESS | tracker | #726 audit | Frozen #726 at `5fce639b` against main `d488c405`: all 58 changed paths match W001-W058 exactly; no missing or extra rows. First file ownership triage is recorded in [#726 matrix](microkernel-tracker/pr726-file-matrix.md). Full file-specific keep/move/simplify/retire proof and current-head conformance remain open. |
| S0-06 | IN PROGRESS | tracker | #733 | Verified the nontruncated main Git tree has 596 blobs and the matrix includes all 596 once; four tracker-only paths are separately recorded as F0597-F0600. First source/consumer reviews and static keep decisions are in [main-tree checklist](microkernel-tracker/repository-file-matrix.md); remaining rows require independent proof. |
| S0-07 | IN PROGRESS | tracker | S0-05 | #726 exact diff: +19,611/-266 (net +19,345). W file-matrix now separates 3,271 dedicated-test lines, at least 5,543 additional new-module inline-test lines, 569 docs and 125 build lines. Added production Rust is **at most 10,103 lines** before accounting for inline tests in modified files; native extraction W016/W023/W040-W044 moves 2,153 lines, not deletion. Complete precise inline-test split and removal/CI parity before PROVEN. |
| S1-01 | TODO | #726 | S0-05 | Canonical four-step Invoke/Fork/Join/Exit IR with cycles, finite map fan-out, all closed joins, stable IDs and bounded inline expansion; non-agent conformance. |
| S1-02 | IN PROGRESS | #726 | S1-01 | Source check at `5fce639b`: sync/pending scheduler entries converge on `workflow_scheduler.rs::execute_driven`; four public root entry methods and projection/error adapters still repeat logic. See W022, W031 and W033; full execution-mode parity and simplification remain open. |
| S1-03 | TODO | #726 | S1-01 | One root/call scope, resolver, authority, dispatch/Layers and pinned generation; proof with substituted providers and stale bindings. |
| S1-04 | IN PROGRESS | #726 | S1-02 | Compared exact symbols in `tasks.rs` and `workflow_tasks.rs`: TaskRuntime owns generic scoped spawn, authority attenuation and plugin cancellation; WorkflowPendingTasks owns generation/scope/call correlation, callback cancellation/settlement and root closure. #726 `3dca0cc` consolidates duplicate strict child-scope prefix matching inside workflow task admission/cancellation. [Current #726 CI](https://github.com/matthis-k/phenix-ai/actions/runs/38033327293) passed at `1c032d7`, but W026/W032 still require exact native callback/generation-lease conformance before PROVEN; preserve one generic TaskRuntime. |
| S1-05 | TODO | #726 | S1-01 | Typed frame private/published slots, mapped includes, frame/alias and cancellation semantics; negative and positive integration tests. |
| S1-06 | TODO | #726 | S1-02 | Native provider callbacks resume same scheduler; cancellation and lease retention until actual settlement, including failing and abandoned callbacks. |
| S1-07 | IN PROGRESS | #726 | S1-01 | `workflow.rs::execute_bound` and `execute_bound_framed` use `execute_nodes`, which reaches the same cooperative scheduler. Audit remaining public/compatibility entry wrappers and test-only compiler helpers before deleting anything. Source evidence W033; no retirement claimed. |
| S1-08 | IN PROGRESS | #728 | S0-01 | Source checked at `98d2682f`: `phenix-sdk/tests/service_only_provide_conformance.rs` resolves and invokes a service-only provider without workflow registration. Preserve this canary; prove third-party artifact activation, stable contract IDs and real Basic/Full use. |
| S1-09 | IN PROGRESS | #728 | S1-08 | `phenix-contract/src/contribution.rs:21-40/125-209` defines inert typed contributions with sorted owner-verified artifact decoding, duplicate/forged owner rejection and tests. Full Prepare/Modify/Observe authoring and selected activation remain open. |
| S1-10 | TODO | #728 | S1-09 | Selected plan definitions and EntryBindings accepted from artifacts; no builder-only registration and no second resolver. |
| S2-01 | IN PROGRESS | #760 | #728 | `composition/resolver.rs` on #760 freezes selected portable contribution metadata into generation identity. Current implementation rejects some reselections; prove authenticated artifact replacement, promotion and deterministic identity before PROVEN. |
| S2-02 | TODO | native implementation PR to create | #734; S1-03 | Extract ABI/loader/bridge changes from #726 into independently reviewable owner; retain Core only minimal generic invocation contract. |
| S2-03 | TODO | native implementation PR to create | S2-02 | ABI table, buffer lifecycle, begin/poll/wake/cancel, callback ownership and native cross-import canary; no plugin-specific Core branch. |
| S2-04 | TODO | native implementation PR to create | S2-03; #760 | Versioned side-by-side library residency, trial/promotion/retirement and failure isolation; canonical selected graph and physical lease proof. |
| S2-05 | TODO | future guest adapter PR | S2-03 | Lua guest adapter as normal native plugin; `require("phenix")` guest vs Neovim client isolation and Rust <-> Lua imports. |
| S3-01 | IN PROGRESS | #729 | S1-01; S1-10 | `graph_patch_order.rs:55` resolves slot insertion order with duplicate, cycle, ambiguity and qualified-slot tests. It is a pure precursor, **not** an IR edit compiler, owner authority check or live candidate patch application. |
| S3-02 | TODO | #729 | S3-01 | Validate patch owner-only metadata, frame extension and attenuation before activating candidate. |
| S3-03 | IN PROGRESS | #730 | S1-09 | `phenix-sdk/src/kind_lowering.rs:78` lowers bounded typed kind templates without callbacks. Tests cover deterministic order, malformed fields, cycles and limits. Selected provider/owner verification, authority and canonical generation integration remain open. |
| S3-04 | TODO | #730 | S3-03 | Exceptional procedural preparation frozen *before* resolution; no callbacks in canonical resolver. |
| S4-01 | IN PROGRESS | #731 | S3-01; S3-03 | `plugin_template_consumer_canary.rs:46` proves a non-agent Core graph with resource-only topology and two independently selected providers, including missing-provider rejection. Tool/skill migration to live templates has not begun; full gate remains blocked by #729/#730. |
| S4-02 | TODO | #726 and #731 | S1-05; S1-10 | Real Basic/Full turn-tool-turn equivalence: outputs, typed continuations, IDs, budgets, events, streaming, progress, cancellation, error and side effects. |
| S4-03 | IN PROGRESS | #725 | #760 | PR #725 contains prompt-admission receipt and claim code in `harness/application.rs` and session/ACP/Lua contracts. Host-fenced orphan recovery, replay ambiguity, physical side-effect settlement and independent client reconnect are still merge gates. |
| S4-04 | IN PROGRESS | #763 | S4-02; S4-03 | #763 carries combined CI changes in `.github/workflows/ci.yml` and `modules/development.nix` on #725. It validates the current partial stack, not final portable entry/native/product parity; rerun against exact merge candidates. |
| S4-05 | TODO | owner to assign | S2-04; S4-02 | Selected plugin upgrades preserve old roots, generation provenance, Environment pins, journal events and durable compatibility. |
| S5-01 | IN PROGRESS | #766 | #726 | The imperative `phenix-plugin-basic-agent` crate, its factory registration and old execution tests are deleted on #766, stacked directly on #726. Head `6ca962f` now removes stale deleted-plugin test selection and regenerates impact metadata. [CI 38033473680](https://github.com/matthis-k/phenix-ai/actions/runs/38033473680) passed at `fed0a97`; declarative product/client parity remains unproven. No compatibility runner. |
| S5-02 | IN PROGRESS | #767 | #766 | The `phenix-plugin-hooks` crate, Advanced default, catalog exports, Nix/Cargo registration and legacy hook tests are deleted on #767. Branch was reconstructed on exact #766 head using a force-with-lease; GitHub reports a clean stack. #767 `bbfbe50` deletes the stale hook migration guide, `9a7079a` updates Event coverage, and `68aba30` updates canonical Layer/Event spec. [CI 38033481435](https://github.com/matthis-k/phenix-ai/actions/runs/38033481435) passed at `8f21551`; generic Event/Layer parity still required. |
| S5-03 | TODO | cleanup implementation PR to create | S4-01 | Replace hardcoded `phenix-plugin-catalog` factories/manifest lists, harness builder and default Basic profiles with portable discovery. |
| S5-04 | TODO | cleanup implementation PR to create | S4-01 | Move AI-specific contracts from Core/SDK to plugin-owned interfaces; preserve required selected contract identity while deleting retired APIs and adapter paths. |
| S5-05 | TODO | cleanup implementation PR to create | S4-02 | Audit `phenix-plugin-step-runner` and `phenix-plugin-execution/src/tool_schedule.rs`; keep tool semantics, remove duplicated orchestration. |
| S5-06 | TODO | cleanup implementation PR to create | S5-01; S5-03 | Move deterministic `phenix-plugin-basic-model` into test fixtures; stop selecting echo as production default. |
| S5-07 | IN PROGRESS | #765, #766, #767 | S5-01 through S5-06 | Breaking cleanup underway. #765 removes routing/MCP/macro compatibility; its failing MCP fixture was corrected in `00a62b0` and new-head CI is pending. #766 and #767 delete obsolete agent and hook crates and update Cargo lock, Nix, product selection and tests. Remaining catalog and echo cleanup belongs to other PRs. |
| S5-08 | TODO | tracker | S5-07 | Reconcile all rows in both file matrices with changed tree. Every Retire/Move path has replacement PR + test evidence or documented `Keep` decision. |
| S5-09 | IN PROGRESS | #768 | #766; #767 | Application's alternate `AgentLoopInterface` and `agent_loop_service` dispatch path deleted in `1784d1c`; one selected generation-pinned `agent.turn` workflow is required before prompt admission. Foreign service-only test removed because selected-turn application tests already cover provider cancellation, output and journal replay. Corrected CLI fixtures `ff60387`/`8285aad`/`3ddb504` prove that disabling the application adapter permits a generic Core graph but does not silently select an agent workflow; the application preflight denies admission without one. [CI 38033484106](https://github.com/matthis-k/phenix-ai/actions/runs/38033484106) passed at `d2c1a5b`; client parity remains before PROVEN. [Owner matrix](https://github.com/matthis-k/phenix-ai/pull/768#issuecomment-6094811078). |
| S6-01 | TODO | #763 or new final integration PR | S2-S5 | Final focused and full checks on exact merge candidates, including non-agent service-only and product parity. |
| S6-02 | TODO | tracker | S6-01 | Closure report: paths/classes removed, net production LOC, total tests, mechanisms before/after, open exceptions and owning follow-ups. |
| S6-03 | TODO | tracker | S6-02 | Close tracker after all required matrices are complete and remaining scope is explicitly assigned to new PRs. |

## Baseline audit checkpoint, 2026-10-10

- **Repository tree.** [Main tree at `d488c405`](https://github.com/matthis-k/phenix-ai/tree/d488c4056ee334938c2ca88aec9017379a3ef7c4) returned `truncated=false` with 596 tracked blobs. Exact set comparison against F0001-F0596 found zero omissions, additions or duplicate paths. F0597-F0600 are tracker-only.
- **#726 diff.** [PR head `5fce639b`](https://github.com/matthis-k/phenix-ai/pull/726/commits/5fce639b40c4a0bb6a990363dba667b68a154c66) and base `d488c405`: 58 changed paths match W001-W058 exactly. This is an inventory proof, not an implementation or liveness proof.
- **Growth classification.** +19,611/-266 total changed lines, including documentation, manifests, fixture tests, inline tests and production code. Dedicated test-only paths W027, W042 and W054 account for at least 3,271 added lines. No exact production/test split is available yet because Rust implementation modules also contain tests. Seven candidate native extraction paths W016, W023, W040-W044 add 2,153 lines; extraction relocates code instead of counting as deletion. Patch accounting identifies at least 8,814 added test-only lines, leaving at most 10,103 added production Rust lines. See the category table and classification limits in the W matrix.
- **Initial file checks.** Direct source inspection proved the `LICENSE` and `.gitignore` retain decisions without runtime tests. Traced live Nix consumers for the Git hook, Cargo source closure, commit candidate, flake exports and Stitch. These need targeted Nix CI proof. `scripts/check-rust-safety-policy.sh` is still invoked by `modules/development.nix`; keep it until its Nix replacement is tested.
- **Ownership boundary.** W016, W023 and W040-W044 cannot complete as moves until the separate native implementation PR exists and #726 no longer owns those implementations. W050 and W052 remain blocked on real product migration, not file deletion alone.

Next unblocked tracker action: for W014/W022/W026/W031-W033, use the exact function inventory in the file matrix to trace callsites, identify genuinely duplicated work and run targeted sync/pending/frame/native callback conformance. Preserve the one existing scheduler. Next independent main-tree checks: F0001/F0018 hook-candidate Nix checks, F0017 dependency-closure builds, F0026 Stitch smoke and F0491 safety-rule replacement. Record exact-head test or source proof on each row; do not certify an entire group from one passing check.

## Current owner and CI checkpoint, 2026-10-10

The owner comments linked in [PR task matrices](microkernel-tracker/pr-task-matrices.md) control individual checkboxes. This table records checked head commits and branch CI outcomes. A passing reduced CI does not certify the full semantic gate.

| PR | HEAD checked | CI | Current responsibility and missing proof |
| --- | --- | --- | --- |
| [#726](https://github.com/matthis-k/phenix-ai/pull/726) | `1c032d7` | [passed](https://github.com/matthis-k/phenix-ai/actions/runs/38033327293) | Generic plan executor exists. Deduplicated nested task scopes; still needs frame/native lease and selected artifact parity, and loader extraction. |
| [#728](https://github.com/matthis-k/phenix-ai/pull/728) | `98d2682` | not rechecked here | Typed contribution envelope and service-only canary exist. Selected artifact entry activation incomplete. Branch base is behind the current #726 head. |
| [#760](https://github.com/matthis-k/phenix-ai/pull/760) | `9cad397` | not rechecked here | Generation identity includes selected inert contributions. Owner-authenticated replacements and lifecycle incomplete. |
| [#725](https://github.com/matthis-k/phenix-ai/pull/725) | `35f738c` | not rechecked here | Prompt receipts and claims exist. Host-fenced recovery and client reconnect proof incomplete. |
| [#729](https://github.com/matthis-k/phenix-ai/pull/729) | `c2be8b6` | not rechecked here | Pure deterministic slot ordering. Typed IR edits, owner authority and candidate activation missing. |
| [#730](https://github.com/matthis-k/phenix-ai/pull/730) | `c26b3d6` | not rechecked here | Pure bounded kind-template lowering. Selected kind provider and generation integration missing. |
| [#731](https://github.com/matthis-k/phenix-ai/pull/731) | `7fca307` | not rechecked here | Non-agent selected-provider canary. Real tool/skill migration remains. |
| [#763](https://github.com/matthis-k/phenix-ai/pull/763) | `54c5760` | not rechecked here | Combined-stack CI only, not final native/client/product parity. |
| [#765](https://github.com/matthis-k/phenix-ai/pull/765) | `00a62b0` | [prior head failed](https://github.com/matthis-k/phenix-ai/actions/runs/38034834335) | Removed old routing/MCP/macro branches. The MCP regression first used a private constructor, then an ambiguous `.into()`; `00a62b0` now passes a string literal to `McpConnectionId::new`. New-head Clippy/protocol CI and semantic tests pending. |
| [#766](https://github.com/matthis-k/phenix-ai/pull/766) | `fed0a97` | [passed](https://github.com/matthis-k/phenix-ai/actions/runs/38033473680) | Deleted old imperative agent crate and registrations. Declarative turn/tool side effects and clients still need proof. |
| [#767](https://github.com/matthis-k/phenix-ai/pull/767) | `8f21551` | [passed](https://github.com/matthis-k/phenix-ai/actions/runs/38033481435) | Deleted old hook crate and registration. Layer/Event equivalence and parent-head integration remain. |
| [#768](https://github.com/matthis-k/phenix-ai/pull/768) | `d2c1a5b` | [passed](https://github.com/matthis-k/phenix-ai/actions/runs/38033484106) | Deleted alternate application agent-service executor. Selected workflow and external client parity remain. |
| [#764](https://github.com/matthis-k/phenix-ai/pull/764) | `9a673bd` before this edit | [maintenance and security passed](https://github.com/matthis-k/phenix-ai/actions/runs/38033391834); main CI skipped | Tracker-only docs. Revalidate documentation checks at the new head. |

The #766, #767 and #768 PR base snapshots are behind their respective parent branch tips. They report mergeable, but mergeability does not prove exact-parent integration. Refresh stacked ancestry before combining or merging; rerun tests on the resulting heads.

## File audit and implementation handoff

- **Main inventory:** 596 files at unchanged main `d488c405`, plus four tracker-only files. Exactly 600 stable F IDs: **2 PROVEN, 109 IN PROGRESS, 489 TODO**. The [file matrix](microkernel-tracker/repository-file-matrix.md) holds the sole authoritative path-level classification. This session inspected 31 exact files: 13 repository/build/SDK/skill paths and 18 SDK domain contract paths F0438/F0440-F0456. The contract audit records each service ID, owning plugin boundary and relocation test. Source inspection alone keeps them IN PROGRESS.
- **#726 inventory:** 58 W rows at frozen `5fce639b`: 50 IN PROGRESS, 8 BLOCKED, none PROVEN. The current #726 head is `1c032d7`; reconcile changed-file contents and test results before moving any W row. The [W matrix](microkernel-tracker/pr726-file-matrix.md) holds source roles and growth accounting.
- **Central gates:** 43 S rows: 4 PROVEN, 18 IN PROGRESS, 21 TODO. Architecture baselines are complete, runtime and deletion acceptance are not.
- **#765 failure root cause:** [CI 38033713326](https://github.com/matthis-k/phenix-ai/actions/runs/38033713326) failed E0423 on a private MCP connection ID constructor. [CI 38034834335](https://github.com/matthis-k/phenix-ai/actions/runs/38034834335) then failed E0283 on an unnecessary `.into()` into `McpConnectionId::new`. [`00a62b0`](https://github.com/matthis-k/phenix-ai/commit/00a62b04178a87a255720d3b926812fa9a4084cf) passes the string literal directly. L765-02/L765-05 remain unchecked until the new head passes and protocol behavior is verified.

Next independent work: create the native ABI/loader extraction owner for W016/W023/W040-W044; simplify W014/W022/W026/W031-W033 without duplicating Core task or resolver behavior; verify strict MCP/routing semantics in #765; run exact-head Nix hook, Cargo closure, Lua callback, Stitch, ABI and client checks. Update each owning task with commit and test evidence, then refresh the F/W rows.

The tracker records progress and evidence. It does not implement mechanisms or authorize deletion. No new compatibility layer is planned.

## Non-negotiable acceptance

- Core remains domain-neutral. Ordinary services do not require plan, agent, profile or graph-patch machinery.
- Exactly one canonical resolver, permission lineage, service invocation, root task lifecycle and execution scheduler. Sync/pending are modes of one executor.
- Each new mechanism has a named RFC requirement, owning PR, file set, replacement target and positive/negative conformance tests.
- Every checked implementation item cites a **current-head** commit and test run; CI success never substitutes for a missing semantic proof.
- Verify complete file coverage on `main` and changed PR branches before final cleanup. The tracker is a coordination record; implementation stays in the owning PR.
