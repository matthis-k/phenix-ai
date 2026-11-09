# Contract Composition Iteration

## Scope And Status

R01 supersedes this report's schema-interning limitation: canonical identities now
use reclaimable `Arc<str>` ownership throughout Graph, with no global strong cache
or static-string compatibility API. Manual wire values require complete descriptors,
including codec. See the [Schema contract](../packages/phenix-schema/README.md) for
ownership regressions and exact before/after release harness measurements. The
older lifecycle rows and measurements below remain historical evidence.

Evidence snapshot: 2026-11-09, branch `impl/resolved-contract-imports-20261108`,
exact starting commit `e91fdd3e096ba40f6adb0b4775ae83734a8ebcc8`.
This integrated report accompanies implementation and regression tests. The final
commit SHA and exact-head hosted CI URL are recorded in the draft PR and final
delivery report, avoiding a self-referential commit identity inside its own tree.

The task handoff records that baseline `cargo test --workspace` passed. That is
reported baseline evidence, not a command rerun by this documentation pass.
`git branch --show-current` and `git log` locally confirmed the branch and base.
`gh pr list --state open --json number,title,headRefName,headRefOid,baseRefName,url`
verified the following baseline stack is still OPEN, with these advertised heads:

| PR | Head Branch | Verified Head |
|---|---|---|
| [#1242](https://github.com/matthis-k/phenix-ai/pull/1242) | `refactor/kernel-plugin-boundary-20261107` | `2b70703cd5a5589d936593a4bf6c86d200605195` |
| [#1243](https://github.com/matthis-k/phenix-ai/pull/1243) | `feat/rust-plugin-catalog-prototype-20261107` | `147e8c2f9524af18f450206a11b550075f3e220a` |
| [#1244](https://github.com/matthis-k/phenix-ai/pull/1244) | `feat/kernel-generation-staging-20261107` | `8dff84abe17b1a9c9ad1d14877cec16b5233d40c` |
| [#1245](https://github.com/matthis-k/phenix-ai/pull/1245) | `feat/plugin-graph-generation-bridge-20261107` | `eab06d3dd4ad0eb52688a794817520d8a5696e8c` |
| [#1246](https://github.com/matthis-k/phenix-ai/pull/1246) | `feat/graph-interface-surfaces-20261107` | `a831ba31ee116939e66deefaf5f779ddc7361fdd` |
| [#1247](https://github.com/matthis-k/phenix-ai/pull/1247) | `impl/scoped-storage-resources-20261108` | `86c29792f4f33eea58736a9c19d5f0a299fcc128` |
| [#1248](https://github.com/matthis-k/phenix-ai/pull/1248) | `design/graph-native-derived-values-20261108` | `0bb81da48f204db822afa304d2a929e1cf50b6e6` |
| [#1249](https://github.com/matthis-k/phenix-ai/pull/1249) | `impl/atomic-storage-cells-20261108` | `44f66c1d155eedd7a1ca374df0f00e6127be65b7` |
| [#1250](https://github.com/matthis-k/phenix-ai/pull/1250) | `impl/graph-native-derived-values-20261108` | `ee2abaa2f0b39e09ae32db5b16d6618b18fc388a` |
| [#1251](https://github.com/matthis-k/phenix-ai/pull/1251) | `impl/graph-interface-contracts-20261108` | `40f368bb783a26d085d058b1e90bb46dd987e94b` |
| [#1252](https://github.com/matthis-k/phenix-ai/pull/1252) | `fix/contract-catalog-consistency-20261108` | `e91fdd3e096ba40f6adb0b4775ae83734a8ebcc8` |

The earlier plugin/boundary PRs are included for prerequisite context; their
presence in this table does not assert that every sibling PR is a direct ancestor.
The adjacent CI branches also remain OPEN: #1253 at
`0f369980d230a367ebd096c48afe7b138e397503` and #1254 at
`8262d9bffa328f5016377e2b7d0493e398774824`. Open status/head identity does not
establish passing CI, merge approval, or verification of this dirty worktree.

Iteration 1 and Iteration 2 describe logical implementation stages, not separately
published revisions. Final evidence is recorded against the integrated commit;
unimplemented candidate designs never receive an implemented PASS.

## Implementation

Candidate A extends the existing Graph-owned shape-plus-explicit-claim design
with resolved imports and coherent host publication. It does not replace Graph,
introduce a scheduler, grant storage access, or make native Rust plugins a sandbox.

1. Collect leased snapshots; reject duplicate owners, malformed ownership, missing
   definitions, contradictory definitions, dependencies, and unknown selections.
2. Match requirement definitions to advertised offer definitions using Graph's
   matcher. Apply trusted allow/deny policy (deny wins); accept a unique eligible
   offer or require a host selection. Missing/ambiguous/ineligible imports fail closed.
3. Add provider-contribution dependencies, reject cycles, and order ready composers
   lexically. Each composer receives only its declared imports through
   `ComposeContext::import`, plus the existing low-level builder.
4. Bind the resolved actual provider through `ResolvedImport::bind`; project only
   contract endpoints and explicitly connect them. No automatic edge insertion is
   claimed. The consumer need not capture the provider's path or implementation type.
5. Register activation and typed exports in candidate-local artifacts while composing.
   Validate all offers, compile once, validate the full contract catalog and exact
   producer origins, preflight routes, and construct the deployment without starting it.
6. `KernelHost::prepare` builds the exact staged inventory, runs the nonadmitting
   `Binding` hook, then starts producers without polling them. It returns an opaque
   host/epoch-bound candidate, not an admission handle.
7. `publish` rejects stale/foreign candidates, locks the old generation, atomically
   commits the validated plugin batch, retires old producers and replaces the active
   pointer. Old explicit engine/query handles remain pinned and usable. Old producer
   drains are disabled. Producer destructors run with old code leases still alive.

Preparation may acquire subscriptions/resources before publication. Composers,
binding hooks and activation factories are trusted native callbacks. They must not
admit waves or leak admission handles during preparation. Owned-resource cleanup
is RAII, **not guaranteed rollback of arbitrary external effects**. Panic/process
failure atomicity, distributed transactions and live revocation are not promised.

## Evidence Index

All test references below were source-reviewed, **not run in this documentation
pass**, except the explicitly recorded inspection benchmark. Historical checks in
[the original report](graph-interface-contracts.md) apply to their named commits only.

| Key | Source / Existing Test Evidence |
|---|---|
| G | [interface.rs](../packages/phenix-graphs/src/interface.rs): sealed unit views, exact inputs, output inclusion, semantic claims, compiled binding and catalog validation. |
| A | [authoring/interfaces.rs](../packages/phenix-graphs/src/authoring/interfaces.rs): effective layouts, full-shape revalidation on each bound endpoint projection, claim ownership and replacement obligations. |
| A-test | [canonical_interfaces.rs](../packages/phenix-graphs/tests/canonical_interfaces.rs): `retained_authoring_binding_rechecks_full_contract_after_layout_change`; [authoring.rs](../packages/phenix-graphs/src/authoring.rs) unit test `local_identity_ignores_diagnostic_labels_but_not_type_or_mode`. |
| L | [authoring.rs](../packages/phenix-graphs/src/authoring.rs), [provenance.rs](../packages/phenix-graphs/src/provenance.rs): declaration tokens, local `TypeId`, origin ancestry, replacement and assembly transactions. |
| K | [Kernel lib.rs](../packages/phenix-kernel/src/lib.rs): resolver, imports, candidate artifacts, final validation, routes, startup, polling and wake driver. |
| H | [host.rs](../packages/phenix-kernel/src/host.rs): candidate preparation/publication, serialized generation access and retirement. |
| P | [Plugin manager.rs](../packages/phenix-plugins/src/manager.rs): `validate_batch`, `candidate_snapshots`, `commit_batch`, revision ancestry binding. Test `batch_validation_is_atomic_and_candidate_inventory_is_exact`. |
| D | [derived/query.rs](../packages/phenix-storage-graph/src/derived/query.rs): `QuerySpec::bind_engine`, exact source/tap and route checks, opt-in guarded executor. |
| R | [declarative_imports.rs](../packages/phenix-kernel/tests/declarative_imports.rs): `node_and_module_providers_switch_and_reload_without_consumer_paths`, `eligibility_semantics_cycles_and_exact_ownership_fail_closed`, `duplicate_owner_is_rejected_even_for_distinct_revisions_before_composition`. |
| O | [plugin_binding.rs](../packages/phenix-kernel/tests/plugin_binding.rs): `kernel_offer_binds_only_to_its_plugin_revision_and_exact_producer`; [contract_offers.rs](../packages/phenix-kernel/tests/contract_offers.rs) and [contract_catalog_consistency.rs](../packages/phenix-kernel/tests/contract_catalog_consistency.rs): unused offers/catalog conflicts and foreign claim laundering. |
| T | [generations.rs](../packages/phenix-kernel/tests/generations.rs): `publication_pins_old_engines_but_stops_old_producers_and_batches_unload`, `resolution_compile_binding_and_nth_start_failure_leave_active_unchanged`, `interleaved_candidates_foreign_hosts_and_empty_batch_epochs`, `publication_waits_for_old_in_flight_wave_without_invalidating_its_handle`, `replacement_without_readers_drops_producers_before_plugin_leases`. |
| X | [activation_artifacts.rs](../packages/phenix-kernel/tests/activation_artifacts.rs): `artifacts_and_engine_drop_before_plugin_leases_on_success_and_build_failure`, `nth_activation_failure_retries_the_exact_composed_state_without_recomposition`. |
| W | [host_portable_contracts.rs](../packages/phenix-kernel/tests/host_portable_contracts.rs): `host_selects_portable_node_and_nested_module_and_preserves_active_on_bad_replacements`, `independently_compiled_consumer_executes_through_host_publication`; includes separately compiled consumer fixture. |
| Q | [host_derived.rs](../packages/phenix-storage-graph/tests/host_derived.rs): `host_exports_bind_before_start_and_pin_exact_old_and_new_snapshots`, `host_dispatch_rejects_cross_runtime_queries_on_local_and_worker_threads`. |
| E | [derived_plugins.rs](../packages/phenix-storage-graph/tests/derived_plugins.rs): `reader_snapshots_rebuild_and_failed_candidate_does_not_poison_exports`, `starting_a_after_b_uses_as_exact_observation`, `duplicate_candidate_factories_are_rejected_not_silently_overridden`. |
| S | [Schema lib.rs](../packages/phenix-schema/src/lib.rs): canonical framing and unchanged process-lifetime `Box::leak` interning. |

## Candidate Matrix

Candidate A was chosen after source investigation of authoring endpoints,
module endpoint enums and expansion. B would require unresolved-reference scope,
alias identity and a new linking phase. C can wrap explicit public ports today,
but cannot capture outer-scope endpoints inside a module and adds topology without
benefit for descriptor-compatible values. Neither B nor C was implemented as an
alternative resolver. Their PARTIAL cells identify reusable existing machinery,
not a planned-design PASS. No unsafe casts, placeholders or adapter nodes were added.

| Acceptance criterion | A: Composition context | B: Symbolic links | C: Adapter modules |
|---|---|---|---|
| Consumer independent of provider path | PASS: private locator in resolved import; R/W. | UNKNOWN: prototype scope/link identity needed. | PARTIAL: public module paths exist; selection adapter absent. |
| Consumer independent of provider implementation crate | PASS: separately compiled C in W. | UNKNOWN: compile independent fixture with symbolic linker. | PARTIAL: public boundary hides types; resolver absent. |
| Works with ordinary nodes | PASS: R/W execute node offers. | PARTIAL: existing symbolic node connections; external links absent. | PARTIAL: nodes can be packaged; adapter not implemented. |
| Works with nested subgraphs | PASS: two-level nested provider in W. | PARTIAL: existing expansion; no external reference variant. | PARTIAL: nested modules already supported; adapter resolution absent. |
| Supports named multi-slot ports | PASS: W noncommutative left/right execution. | PARTIAL: typed slots exist; unresolved selectors need design. | PARTIAL: module slots exist; wrapper construction untested. |
| Supports independent portable Rust types | PASS: W independently compiled C codecs. | PARTIAL: connect_wire reusable after linking; no linker test. | PARTIAL: wire module ports reusable; no adapter test. |
| Preserves strict local TypeId safety | PASS: G/L and local diagnostic-label regression. | PARTIAL: existing local identity reusable; unresolved identity untested. | PARTIAL: existing module TypeId validation reusable. |
| No additional Graph executor semantics | PASS: K inserts ordinary connections, H only gates admission. | PARTIAL: linking could be compile-only; no implementation. | PARTIAL: existing module executor sufficient, wrappers unimplemented. |
| No duplicated compatibility checker | PASS: K delegates to G. | PARTIAL: G can be reused; linker absent. | PARTIAL: G can validate adapters; adapter absent. |
| No fabricated implementation claims | PASS: actual producer claim required; O/W. | UNKNOWN: test linker claim provenance. | UNKNOWN: test wrapper versus underlying semantic claims. |
| Inferred composition dependency ordering | PASS: producer Key resolves order; R. | UNKNOWN: linker dependency extraction prototype needed. | UNKNOWN: adapter ownership resolver needed. |
| Supports deterministic host selection | PASS: ImportPolicy and R/W switches. | UNKNOWN: selection/link integration needed. | UNKNOWN: selection/instance integration needed. |
| Fails on ambiguity without host policy | PASS: structured Ambiguous; R before callbacks. | UNKNOWN: no candidate resolver. | UNKNOWN: no candidate resolver. |
| Does not expose unpromised outputs | PASS: bound projection hides diagnostic output; W. | UNKNOWN: symbolic visibility implementation needed. | PARTIAL: explicit exports restrict shape; adapter absent. |
| Detects missing providers early | PASS: Missing before composition; R. | UNKNOWN: missing-link diagnostic prototype. | UNKNOWN: missing-adapter diagnostic prototype. |
| Detects dependency cycles | PASS: R includes inferred import cycle. | UNKNOWN: link-cycle experiment needed. | UNKNOWN: adapter dependency-cycle experiment needed. |
| Supports provider replacement | PASS: W path-changing node/module reload. | PARTIAL: existing replacement engine reusable; links untested. | PARTIAL: exact module replacement exists; host adapter untested. |
| Supports staged plugin generations | PASS: P/H/T/W. | UNKNOWN: bind symbolic resolution to candidate generation. | UNKNOWN: bind adapter instances to candidate generation. |
| No stale cross-generation handles | PASS: G compiled pointer/lineage checks, Q old/new rejection. | UNKNOWN: symbolic alias lineage model needed. | PARTIAL: module lineage reusable; adapter handles untested. |
| No authorization inference | PASS: host allow/deny independent; Store grants unchanged, Q/E. | PARTIAL: feasible separate policy; not implemented. | PARTIAL: module interfaces grant nothing; resolver absent. |
| Macro-free low-level API remains viable | PASS: W manually authors NodeTemplate/ports. | PARTIAL: existing symbolic strings macro-free; linking absent. | PARTIAL: GraphBuilder modules macro-free; adapter absent. |
| Smallest additional public API | PASS: context/import/policy reuse bound endpoints; K. | PARTIAL: requires new unresolved endpoint/link API per module enums. | PARTIAL: module API reused but adapter/selection API still needed. |
| Smallest additional runtime state | PASS: no execution import state; candidate owns artifacts, K/H. | UNKNOWN: measure linker tables and retained aliases. | PARTIAL: additional topology per wrapper; no measured prototype. |
| Best Rust borrowing/type inference | PASS: owned metadata then owned endpoints release borrow; W. | UNKNOWN: prototype typed symbolic endpoint projection. | PARTIAL: existing module handles work, wrappers repeat port declarations. |
| Lowest migration complexity | PASS: context.graph plus declarative requirement migration in examples. | PARTIAL: compiler/link identities require wider migration. | PARTIAL: manual wrapper exports introduce authoring boilerplate. |
| Preserves derived V1 | PASS: E/Q plus unchanged derived/snapshot regressions. | UNKNOWN: run derived suite against a linker prototype. | UNKNOWN: run derived suite against adapter resolution. |

## Acceptance Matrix

PASS refers to implemented behavior backed by the cited source/tests. The final
verification ledger distinguishes local execution from hosted CI. Iteration cells
record logical progress rather than independently published snapshots.

| Acceptance criterion | Starting state | Iteration 1 | Iteration 2 | Final implementation |
|---|---|---|---|---|
| GraphUnit shared by nodes/modules | PASS: G baseline. | PASS: G retained. | PASS: G retained. | PASS: G/W node/module execution. |
| Canonical Graph contract validation | PASS: G baseline. | PASS: K delegates G. | PASS: K final catalog. | PASS: G/O/W. |
| Explicit semantic claims | PASS: G baseline. | PASS: O negative tests. | PASS: W replacement rejection. | PASS: O/W. |
| Offer/requirement consistency | PASS (#1252): O. | PASS: advertised definition matching. | PASS: W actual final claim. | PASS: O/R/W. |
| Global contract catalog consistency | PASS (#1252): O. | PASS: K preflight definitions. | PASS: G final reconciliation. | PASS: O. |
| Scoped claim ownership | PASS (#1252): A. | PASS: A retained. | PASS: exact producer checks added. | PASS: A/O. |
| Independent provider discovery | FAIL: selected offer required. | PASS: K/R discovery. | PASS: H candidate resolution. | PASS: R/W. |
| Authorized host selection | PARTIAL: manual host keys. | PASS: allow/deny/selections R. | PASS: H uses policy. | PASS: R; no Store grant inference. |
| Ambiguous provider rejection | FAIL: no discovery. | PASS: R before callbacks. | PASS: H rejects candidate. | PASS: R/W. |
| Provider-independent consumer composition | FAIL: captured path. | PASS: context import R. | PASS: independent C fixture W. | PASS: W has no B dependency. |
| Composition ordering from requirements | FAIL: manual requires. | PASS: producer Key R. | PASS: H rebuilds order. | PASS: R cycles and self-import tests. |
| Changing selection changes actual wiring | FAIL: independent configuration. | PASS: R output 11 versus 21. | PASS: W host publication switches. | PASS: R/W actual execution. |
| Node provider support | PASS: G baseline. | PASS: R. | PASS: W. | PASS: R/W. |
| Nested module provider support | PASS: G baseline. | PASS: R. | PASS: W two nested levels. | PASS: W. |
| Named multi-slot input support | PASS: module_slots baseline. | PASS: same endpoint API. | PASS: W left/right arithmetic. | PASS: W. |
| Independent portable Rust types | PASS: portable_wire baseline. | PASS: graph_contracts C. | PASS: Cargo-isolated C W. | PASS: W. |
| Contract-restricted endpoint views | PASS: G/A baseline. | PASS: R private output rejection. | PASS: W harmless extra output hidden. | PASS: A/W. |
| Local Rust type safety preserved | PASS: L baseline. | PASS: local R execution. | PASS: label/TypeId regression. | PASS: L/A-test. |
| Generation-safe provider imports | PARTIAL: borrowed compiled bindings. | PASS: final producer origin + lineage. | PASS: H old/new graphs pin leases. | PASS: O/Q/W; raw addresses remain low-level. |
| Atomic visible generation publication | FAIL: separate commit/start/swap. | FAIL: no coordinator yet. | PASS: H/P batch and pointer transition. | PASS: T/Q/W under serialized host ownership. |
| Failed candidate preserves active generation | PARTIAL: individual stages. | PARTIAL: build does not commit. | PASS: T failures and Q Store failure. | PASS: T/Q/W. |
| Producer admission gated during activation | FAIL: manual orchestration. | FAIL: no host gate. | PASS: opaque candidate never polled. | PASS: T no poll/fire before publish. |
| Old in-flight operations remain valid | PARTIAL: leases only. | PARTIAL: standalone old kernels. | PASS: T barriers and Q old queries. | PASS: T/Q; publication waits scoped work. |
| Plugin leases retained correctly | PARTIAL: baseline handle lifetimes. | PASS: K keeps snapshots. | PASS: X artifacts and T producer teardown. | PASS: O/T/X/Q. |
| No schema/authorization conflation | PASS: separate Store grants. | PASS: R eligibility distinct. | PASS: Q/E actual grants. | PASS: R/Q/E. |
| No new contract scheduler | PASS: Graph executor. | PASS: composition-only resolver. | PASS: H admission gate only. | PASS: K/H source and execution. |
| No Store/Graph dependency cycle | PASS: Cargo manifests. | PASS: Kernel no Store dependency. | PASS: adapter remains storage-graph. | PASS: manifests/workspace compile. |
| Derived V1 behavior preserved | PASS: derived suites baseline. | PASS: endpoint model unchanged. | PASS: E/Q candidate-owned state. | PASS: derived and derived_plugins plus Q. |
| Structured diagnostics | PARTIAL: InterfaceError only. | PARTIAL: Missing/Ambiguous/Ineligible/Cycle. | PARTIAL: lifecycle phase errors. | PARTIAL: typed compile/interface/origin/composition; callback strings remain. |
| Bounded schema lifecycle | FAIL: S leaks unique descriptors. | FAIL: representation unchanged. | FAIL: intentionally separate foundation change. | FAIL: deferred; static schema strings still retained forever. |
| Authoring performance measured | UNKNOWN: no focused measurement. | UNKNOWN: investigated scans. | PASS: 10/100/1000 and nested probe. | PASS: measurement below; allocations not profiled. |
| Exact-final-head tests and CI | PASS: baseline run 42507202762. | UNKNOWN: logical stage not published. | UNKNOWN: logical stage not published. | PARTIAL: all local gates pass below; exact final commit and hosted CI tracked in PR, never inherited from baseline. |

## Ownership Invariants

This inventory covers new/changed public abstractions and the existing foundations
they rely on; it does not create new authorities alongside Graph or Storage.

| Public Surface | Owner / Lifetime | Enforced Invariant | Escape / Limit |
|---|---|---|---|
| Schema, ContractValue, TypeIdentity | Schema/Graph; wire identities currently static | Full descriptor equality for portable connect; local equality is TypeId | Schema interning is process-lifetime; codec honesty unproved. |
| InterfaceShape, GraphContract | Graph immutable metadata | Structure differs from explicit semantic membership | Neither grants permission nor proves behavior/readiness. |
| GraphUnit, AuthoringUnit, UnitSnapshot | Graph; borrowed authoritative view versus owned historical data | Sealed views inspect effective declarations | Snapshot is not a validation token. |
| BoundAuthoringUnit and typed endpoints | Exact authoring graph/declaration | Full contract shape rechecked per projection, promised-only names and types, declaration lineage | Interior pending layouts can change; no cache attestation. |
| BoundUnit, BoundInput, BoundOutput | Borrowed immutable compiled graph | Resolve rejects another graph pointer | Extracted raw addresses are not provenance-carrying. |
| GraphOrigin::child/belongs_to | Graph opaque allocation ancestry | Distinct composer identity within exact loaded plugin ancestry | Public trusted-host provenance, not a capability or code lease. |
| PluginSnapshot, ContributionHandle | Plugin Arc-backed loaded revision | Code/implementation lifetime outlives consumers | Plugin-level bind checks ancestry, not exact producer ownership. |
| PluginSet batch methods, StagedPlugin | Plugin inventory identity/epoch | Validate whole batch before mutation; candidate inventory is exact | Raw PluginSet users must coordinate kernels themselves. |
| Key, Contribution, Compose | Kernel leased contribution | Qualified IDs; dependency order; compose gets context | `context.graph` intentionally retains low-level native authoring. |
| ContractDefinition, GraphOffer, ContractRequirement | Kernel leased catalog | Offer names actual unit and producer; requirement names consumer and definition | No duplicated purported implementation shape or plugin-chosen provider policy. |
| ImportPolicy, ResolutionError | Trusted host / one build | Deny wins, unique or explicitly selected eligible provider, structured main resolution failures | Some invalid/build errors are strings; no policy sandbox. |
| ComposeContext, ResolvedImport | One composition / owned import metadata | Only declared local imports; bind delegates to Graph | Native closure can ignore import; final checks do not prove chosen wiring. |
| ComposeContext::activate/export, Activate | One build's candidate artifacts | One factory and one typed export per contribution; captures exact composed state | Bundle exports in a struct; factories own partial-acquisition cleanup. |
| Kernel::export, Kernel | Deployment | Artifacts/actions/producers retain code leases; export type checked by downcast | No cross-generation export certification by type alone. |
| Producer, Notice, ProducerEvent, ProducerError | Started kernel / one poll | Nonblocking producer contract; declared routes; poll/admission/wave outcomes distinct | Poll bound is not a wall-clock bound. |
| WakeHandle, run_until | Optional host driver | Notification/shutdown with race-aware registration | Producer must support wake; no preemption or new scheduler. |
| KernelHost, Candidate, LifecycleError | Host inventory and publication epoch | Opaque prepared state; no mutable inventory getter; coherent active generation | Trusted binding hook can cause external effects; publication is not panic-proof transactionality. |
| Generation::with_kernel/with_engine/fire/drain | Arc generation with serialized access | Old explicit engines survive; retired producer drains return empty | Callbacks must not reenter same generation; old explicit fire is intentionally allowed. |
| QuerySpec::bind_engine, QueryHandle | Derived adapter / borrowed exact engine | Exact source/tap uniqueness and refresh target validation; binding fires no wave | Low-level engine path bypasses startup guard; query only after publication. |
| DerivedHost / DerivedGeneration::query | Store-owned host adapter and pinned published generation | Always guard dispatch and execution threads against nested queries; validate via nonadmitting Binding | Detached threads/unrelated unguarded engines and arbitrary external effects excluded. |

## Mutation Audit

All public authoring operation families relevant to contract integrity are covered
below. Audit means source review, not exhaustive fuzzing or proof of every raw
declaration combination. `GraphBuilder` remains intentionally mutable and trusted.

| Operation | Validation / Identity Consequence | Evidence / Remaining Boundary |
|---|---|---|
| node/add; low-level NodeTemplate and ModuleDef authoring | New declaration tokens; current scope origin attributed; final compiler validates actual declarations/claims | L/G; raw metadata is not itself certification. |
| Pending NodeHandle input/layout edits | Effective pending overlay; layout changes revoke stale endpoint lineage, preserve handler origin | A/L; projection now rechecks the full contract, not just selected endpoint. |
| instance/import/package/module/compile | Dependency identities and origins propagate; module public boundary validated; final claims/requests checked | L/G; packaging consumes pending state; public module origin identifies instantiator, not every private handler. |
| clone/replay | Pending state isolated; inherited handles intentional, divergent declarations distinct | L; existing clone_lineage and contract_dependency_identity tests source-referenced by original report. |
| implements/implements_public | Scoped direct-node ownership checked; scoped root/module/nested shared claims denied; unscoped module definition claims supported | A; repeated identical claims no-op; alias edits refresh dependency ancestry. |
| require_contract | Stores final obligation, not early certificate; malformed path rejects | A/G; compiler validates after all edits, including late edits. |
| replace node | Complete numeric/named signature exact; old claims explicitly retained, new claims validated; token/origin replaced only on success | A/L; failed replacement retains pending state/handles. |
| replace module instance | Complete public shape/grouping exact; claims and requests retained; dependency conflicts reject before mutation; new instance token/origin | A/L; public equivalence does not prove private behavior. |
| connect/connect_wire | Scope, declaration tokens, actual endpoint identity and duplicate edge checks before insertion; wire additionally requires full descriptors | L; no Local/Wire coercion, no ABI cast. |
| disconnect | Scope/tokens/type/actual endpoints validated; missing edge returns error | L; topology mutation does not acquire semantic membership. |
| expose_input/expose_input_slot/expose_output; symbolic/low-level exports | Token and actual identity checks; distinct public pairs and routing keys; expander validates fanout/source consistency | L/G; literal dots are not implicit grouping. |
| trigger/public-input lookup/directed destination | Current public input/declaration identity and actual destinations resolved | L/G; route names preflighted again by K before startup. |
| patch/splice/assembly transaction | Clone-then-apply; declared write footprints include claims/requests/replacements; error discards candidate edits | L and authoring/patching.rs; existing patch_lineage tests; callback external effects not rolled back. |
| with_origin | Restores previous scope on normal return/error/unwind; no relabel of existing units | authoring/provenance.rs; trusted host may explicitly create scopes. |
| Context activate/export | Duplicate registration rejects, artifacts local to build; build error drops artifacts before leases | K/X/E; no mutable snapshot slot for candidate products. |
| Plugin load/reload/unload/commit/commit_batch | Stages tied to set identity/epoch; duplicate names and stale revisions reject whole batch; nonempty batch advances epoch once | P; empty batch changes no plugin epoch, host publication still advances host epoch. |
| Host prepare/publish/drop | Nonadmitting bind/start before publish; stale candidate rejected; old drain serialized/retired; producers dropped before old leases | H/T; no arbitrary external-effect rollback. |
| Kernel start/fire/drain/run_until | Successful start only once; failed start retries same artifacts; declared routes only; poll and wave errors preserved | K; `engine()` deliberately bypasses start guard. |
| Generation scoped access / query bind | Shared read-only Kernel, no mutable producer escape; exact engine adapter; old explicit queries supported | H/D/Q; DerivedHost always installs the store-owned executor guard. |

## F08-F16 Investigation

| Finding | Investigation And Disposition |
|---|---|
| F08 Binding and lease coupling | H scoped generation retains engine/artifacts/plugin leases together; O checks ancestry plus exact producer. Raw PluginSnapshot.bind deliberately returns a lease, not a graph certificate. G compiled bindings still borrow exact graph. No lease cycles added. |
| F09 Expensive authoring inspection | Measured 10/100/1000 and repeated nested instances below. Removed duplicate bind/implements scans. No stale cache; final compile authoritative. |
| F10 Schema interning lifecycle | S uses Box::leak with process-lifetime static identities. Deferred foundational Arc/owned identity migration: &'static str and Copy assumptions permeate Graph schemas/layouts. No memory bound claimed and no fake eviction of live static references. Distinct-descriptor reload probe below confirms linear retained string bytes; full plugin reload RSS profiling remains deferred. |
| F11 Structured diagnostics | ResolutionError now preserves CompileDiagnostic, InterfaceError, contribution/offer/unit/origin context; lifecycle has Plugins/Build/Binding/Start/StaleCandidate phases. Callback/plugin String causes remain partial. diagnostics.rs tests machine-readable fields. |
| F12 ContractValue naming / portability | Source review: trait concerns serialization, not semantic membership. Kept name to avoid rename-only churn. Explicit descriptors required by connect_wire; nominal fallback does not certify portable cross-type interoperability. Local endpoints require no codec. No codecs rewritten. |
| F13 Inspection allocations | UnitSnapshot remains owned; measured inspect versus inspect+snapshot separately. Full bound projection revalidation repairs live-layout correctness. Borrowed compiled view hierarchy deferred: no allocation profile justifies complexity yet. |
| F14 Replacement policy | Retained exact complete boundary by default. Contract output inclusion is intentionally weaker than replacement compatibility. Existing edges/exports/claims/requests/lineage tests justify no usage-aware replacement mechanism. Provider switching uses a fresh candidate graph. |
| F15 Provenance versus authorization | Origins are trusted attribution, including revision ancestry; no sandbox/capability claim. Host allow/deny and Store grants are independent and tested in R/Q/E. Explicit trusted patching remains possible; no blanket foreign-mutation prohibition. |
| F16 Derived-value integration | Preserved atomic snapshots, cells, source groups/chains/diamonds, deferred observation and no nested query execution. E fixes shared composition state; Q integrates exact-engine host queries. Host uses explicit guarded executor; low-level engine bypasses remain documented. |

## Performance Evidence

Locally executed successfully during this documentation pass:

```sh
cargo test --locked -p phenix-graph --release --test inspection_cost -- --ignored --nocapture
```

One test passed. `inspection_vs_compile` reports median of 11 batches of 16
operations, microseconds/operation, after warmup; construction and explicit cloning
are excluded. This is a manual cost probe with no timing assertions, not a stable
benchmark or an allocation/retained-memory test. Concurrent machine load can matter.

| Shape | Nodes | Inspect Root | Inspect Then Snapshot | Bind Root | Implements Root | Compile |
|---|---:|---:|---:|---:|---:|---:|
| named | 10 | 43.6 | 46.9 | 45.7 | 22.8 | 27.1 |
| named | 100 | 436.8 | 777.4 | 457.9 | 380.5 | 427.0 |
| named | 1000 | 5072.9 | 9982.1 | 4941.2 | 4812.8 | 8925.9 |
| nested | 10 | 30.6 | 71.8 | 30.8 | 38.1 | 58.2 |
| nested | 100 | 502.8 | 887.9 | 482.4 | 419.2 | 701.5 |
| nested | 1000 | 4946.6 | 10443.2 | 4973.9 | 4890.0 | 7681.8 |

The handoff's prior approximate named-node figures were inspect ~24/300/3500 us
versus compile ~29/530/5500 us at 10/100/1000 nodes; those were not reproduced
exactly here and are not substituted for the fresh output. Module/root inspection
is substantial work, sometimes comparable to compile. `inspect(...).inspect()`
intentionally repeats inspection; direct bind/implements no longer perform that
duplicate scan. **No inspection cache was added and no optimization beyond removing
duplicate scans is claimed.** Full endpoint projection revalidation is deliberate
correctness work, including when only another endpoint's pending layout changed.

The ignored `phenix-schema` probe `distinct_descriptor_reload_retention` was also
executed in release mode. Each descriptor is dropped after interning. Retained
canonical-string bytes were 670 / 6,790 / 68,890 / 698,890 at 10 / 100 / 1,000 /
10,000 distinct descriptors, excluding BTreeSet/allocation overhead. Batch times
were approximately 27 us / 141 us / 2.1 ms / 26.8 ms. This measures the interning
primitive used on reload, not end-to-end plugin reload or allocator RSS.

## Findings And Limits

Significant issues addressed in the inspected implementation, ranked by impact:

1. Publication/inventory mismatch and partial batch commits: batch validation plus
   opaque host candidates couple the exact inventory to the prepared deployment (P/H/T).
2. Cross-candidate observation/export contamination: candidate-local context-only
   activation/export artifacts capture exact composed state; A can start after B
   without subscribing to B's resources (K/X/E).
3. Foreign or sibling contribution offer laundering: exact producer child origin
   is checked, while plugin-level bind accepts only the exact revision's ancestry (L/K/P/O).
4. Consumer wiring hard-coded to provider path: local resolved imports and inferred
   provider ordering permit node/module substitution without editing the consumer (K/R/W).
5. Stale bound authoring shape: projection now revalidates complete effective shape
   and membership, not merely the requested endpoint; diagnostic names no longer
   participate in module local type identity (A/L).
6. Unusable host derived adapter: scoped Kernel access, candidate exports and exact
   engine query binding integrate storage without adding a Kernel storage dependency (H/D/Q).

Remaining limitations, ranked by acceptance impact:

1. Schema interning remains unbounded for arbitrary distinct descriptors. Foundational
   static-string identity migration is deferred; do not claim bounded reload memory.
2. Native callbacks can have external effects or leak handles. Binding hooks and
   factory cleanup are trusted contracts; arbitrary external effects cannot be rolled back.
3. Structured diagnostics are partial. CompileDiagnostic and InterfaceError are now
   preserved, with typed composition/origin/unit failures and error sources. Generic
   invalid metadata and native binding/start callbacks still contain String causes.
4. `DerivedHost` enforces host-only derived dispatch protection. Explicit low-level
   `bind_engine`/`engine()` bypass startup guards; arbitrary engines may be unguarded. Old
   explicit handles remain executable by design; retirement stops producers, not queries.
5. Inspection has no cache and can approach compilation cost. Publication waits for
   old in-flight scoped work; callbacks must not reenter the same generation mutex.
6. Imports do not sandbox native composers or prove that arbitrary closure wiring
   follows the selected import. Semantic claims prove declared membership, not
   behavior, authorization, scheduling, emission or ABI safety.
7. The existing blocking driver treats poll errors as activity. A permanently
   closed producer can repeatedly report errors; host shutdown/backoff or a future
   terminal-source protocol is still needed. Bounded drain remains bounded.
8. String-based symbolic module selectors intentionally remain shorthand, not a
   grammar for arbitrary grouped slots. R12/R14 provide complete typed field-path
   export/module/contract projection and exact public-slot trigger lookup using
   the existing endpoint identities. See [before/after authoring examples](grouped-authoring.md).

## Weighted Assessment

Scores are explicit engineering judgments of this composition task, not measured
test pass rates or replacements for hard gates. Baseline means the exact e91 base,
not the earlier report's differently scoped 8.3 assessment. Iteration scores are
logical-stage estimates; B/C have no defensible implemented weighted score.

| Dimension | Weight | Baseline | Iteration1 | Iteration2 |
|---|---:|---:|---:|---:|
| Correctness and invariants | 30% | 8.0 | 8.8 | 9.4 |
| Authoring/API ergonomics | 20% | 6.0 | 8.5 | 9.2 |
| Ownership and responsibility boundaries | 20% | 9.0 | 9.0 | 9.2 |
| Evolution, replacements and generation lifecycle | 15% | 5.0 | 7.0 | 8.7 |
| Simplicity and migration complexity | 15% | 8.0 | 8.0 | 7.0 |
| **Weighted total** | **100%** | **7.35** | **8.40** | **8.835** |

There is no artificial 9+ overall rating: the schema lifecycle leak is unchanged,
structured errors are partial, and host guard setup is opt-in. Higher correctness
scores do not assert arbitrary effect rollback.

## API Changes

| Before At Base | Current Worktree |
|---|---|
| Compose callback receives `&mut GraphBuilder` | Receives `&mut ComposeContext`; existing builder is `context.graph`. |
| Requirement names a selected `offer` | Requirement names `consumer` and contract; ImportPolicy controls provider choice. |
| GraphOffer names unit and contract | Also names exact `producer: Key`. |
| Consumer captures provider path | `context.import(local_id)?.bind(context.graph)?` resolves selected actual unit. |
| Contribution has static optional `activate` | Removed; register once with `context.activate(factory)` during composition. |
| Composed outputs passed via shared mutable slots | `context.export(value)` and `kernel.export::<T>(&key)` are candidate-owned. |
| PluginSnapshot.bind requires equal origin | Accepts exact revision ancestry; Kernel still checks exact composer origin. |
| PluginSet commits one stage | Batch validation, exact candidate snapshots and atomic batch commit; single commit delegates. |
| Host manually commits/starts/swaps | KernelHost prepare/publish with nonadmitting Binding, opaque Candidate and pinned Generation. |
| Generation engine-only access | `with_kernel` also exposes read-only generation-owned exports; `with_engine` remains a convenience. |
| Derived query bound through DerivedRuntime | DerivedHost always guards host preparation; validate_binding before start, DerivedGeneration::query after publication; bind_engine remains explicitly low-level. |
| Authoring bound projection checks promised endpoint | Also revalidates entire effective shape/claim on each projection. |

## Grouped File Changes

This is a worktree inventory, not a claim that this documentation pass authored the code.

| Group | Files And Purpose |
|---|---|
| Graph integrity/cost | `packages/phenix-graphs/src/{authoring.rs,authoring/interfaces.rs,provenance.rs}`; `tests/{canonical_interfaces.rs,inspection_cost.rs}`: TypeId correction, projection revalidation, ancestry and manual cost probe. |
| Plugin inventory | `packages/phenix-plugins/src/manager.rs`: batch staging/commit and ancestry-aware leased binding. |
| Kernel resolver/lifecycle | `packages/phenix-kernel/src/{lib.rs,host.rs}`: imports/policy/errors, candidate artifacts, driver and publication coordinator. |
| Kernel integration | `packages/phenix-kernel/tests/{declarative_imports.rs,plugin_binding.rs,generations.rs,activation_artifacts.rs,host_portable_contracts.rs}` plus portable fixture; existing contract catalog/offers/graph/kernel/storage tests migrated to context API. |
| Kernel examples | `packages/phenix-kernel/examples/{basic.rs,storage.rs,scoped_storage.rs}`: context authoring and activation migration. |
| Derived adapters | `packages/phenix-storage-graph/src/derived/{mod.rs,query.rs}`, `examples/derived.rs`: engine adapter, guarded executor, candidate export/activation migration. |
| Derived regressions | `packages/phenix-storage-graph/tests/{binding.rs,derived.rs,derived_plugins.rs,host_derived.rs}`: migrated APIs and candidate isolation/host query coverage. |
| Documentation | This new report; `docs/kernel.md`, `docs/graph-interface-contracts.md`, `docs/derived-values.md`: current API/lifecycle corrections and historical-evidence boundaries. |

## Verification Ledger

Cargo commands below run through `nix develop --command`. All listed successful
checks exercised the integrated source; exact committed-head reruns and hosted CI
are recorded in the PR. The initially dirty worktree was clean before this task.

| Command | Exit | Evidence |
|---|---:|---|
| `cargo fmt --all --check` | 0 | Formatting; an initial new probe formatting failure was fixed with rustfmt. |
| `cargo test --workspace --locked` | 0 | All workspace tests/doctests, including independent Cargo fixture and derived host integration. |
| `cargo test --workspace --no-default-features --locked` | 0 | Same regression surface without workspace default features. |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | All targets; initial large-error/test-layout lints fixed without blanket lint suppression. |
| `cargo test --locked -p phenix-graph` | 0 | Conformance, contracts, replacement, lineage, portable codecs and compile-fail tests. |
| `cargo test --locked -p phenix-plugin` | 0 | Inventory, staging, provenance, binding and leases; exhaustion covered in subsequent workspace run. |
| `cargo test --locked -p phenix-kernel` | 0 | Resolution, admission, publication, independent provider/consumer and lifetime suites; rerun after error boxing. |
| `cargo test --locked -p phenix-storage` | 0 | Grants, atomic snapshots, revisions, SQLite authority and subscriptions. |
| `cargo test --locked -p phenix-storage-graph --test derived --test derived_plugins` | 0 | 29 derived regressions and 8 plugin integration regressions. |
| `cargo run --locked -p phenix-storage-graph --example derived` | 0 | Real arithmetic, independent subscriptions, project isolation, persistence and denied grants. |
| `nix flake check --no-update-lock-file --print-build-logs --max-jobs 2` | 0 | All checks, including derived/no-default/storage/kernel/integration, format, clippy and smoke. |
| Release inspection and descriptor-retention probes | 0 | Explicitly run ignored measurement tests; no timing assertions or ignored correctness tests added. |
| `git diff --check` | 0 | Whitespace checks. |

The first unrestricted `nix flake check --no-update-lock-file --print-build-logs`
exceeded the tool's 200-second limit while compiling independent cold outputs.
It was rerun with a 1,200-second tool allowance and `--max-jobs 2`; all checks
passed. This is local resource control, not a changed test/check definition.

The final source review found no additional actionable high-value correctness
defect in the modified paths. It requested deterministic producer-drain retirement,
activated-candidate abandonment/stale cleanup, and epoch-exhaustion coverage;
these were added in `generation_retirement.rs` and host/plugin unit tests.
The known limitations above remain explicit rather than being scored away.
