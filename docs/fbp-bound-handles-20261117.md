# A5 Bound Handles: Implementation And Acceptance Design

## Final Acceptance: A5, 2026-11-20

**Original A5 is scoped verified at
`45c2dfb6f9089fb4b2f25bd102a37d2181e44eaf`: 24.0% -> 100.0%, +76.0 pp.**
The independent final whole-scope Graph/Kernel/StorageGraph review reports no
remaining findings after all review corrections. Full local default/no-default
workspace and isolated Graph test gates, explicit doctests, all-target compilation,
strict Clippy and formatting passed. All ten checks on the untracked-inclusive Nix
snapshot passed, and all fifteen hosted workflows completed **SUCCESS** on that exact
SHA, confirmed through `gh`; the complete run URL table appears below.

Acceptance covers real compiled-generation-owned handles through Graph basic and
internal declaration/endpoint resolution, ordinary/request execution,
configuration, wake/notifier, source and closure operations; checked
composition/compilation bridges without transferring intermediate-preview authority;
Kernel/Generation forwarding and final `Binding`; and the actual production
StorageGraph query. Stale/foreign rejection preserves canonical execution rather
than adding another scheduler or request store. Known-owner, per-handle,
nonretroactive leases, hostile-drop cleanup and Q7 typed primary/cleanup behavior
are preserved. Necessary validated dynamic paths remain; physical coordinates are
no longer application correctness proof.

Only A5 changes in the authoritative scorecard. Counts are **20 scoped verified /
6 partial / 6 open / 1 approved exclusion**, leaving **12 in-scope findings**. The
score sum moves 2489 + 76 = **2565**, and 2565 / 33 = 77.727...%, rounded to
**77.7%**. This subjective unweighted issue mean is not product completion. Every
other score is unchanged, including H7 at 56.0% and excluded C7 at 12.0%. H7's
older-request scoped-capture retention and shared-process occupancy residual stays
partial and unresolved: A5 completes its original binding-authority scope, not
universal request progress or fairness. Green benchmark workflows are execution
evidence, not performance acceptance; M7/M8 remain incomplete.

This final status and the exact-head evidence below supersede the earlier pending
checkpoint and historical gate language. The design, baseline and acceptance matrix
are retained as history. This evidence-only documentation revision requires its own
checks; the accepted implementation's checks do not certify a later documentation
revision. No commit or push is part of this reconciliation.

## Current Implementation

API names below are the implemented names, not speculative names from the design.
The existing compiled-generation `Arc<()>` token authenticates immutable owned
handles by pointer identity. Clones/leased clones retain that owner; recompilation
and recomposition do not. Explicit `with_lease` copies known compiler/code owners
at issuance; the token is not a codec lease, and later clone enrichment is not
retroactive. Old handles retain C3 execution on their old owner; successors reject
them, without retirement revocation. `RequestId` still authenticates its session.

- `BoundDeclaration`, owned `BoundInput<T>`/`BoundOutput<T>`,
  `IngressHandle<T>`, `CaptureHandle`, and `SourceHandle` are defined in
  `network/resolution.rs` and re-exported. `CompiledNetwork` supplies
  `declaration` (including root), unique `bind_definition`, explicit repeated
  `declarations`, `bind_ingress`, typed/erased `bind_capture`/`capture`, and
  `source`. Composite/root declarations enumerate only designated physical
  descendants through `sources()`; they are not process instances.
- Endpoints retain private projections and exact cached local/wire identity.
  `matches_input`/`matches_output` compare complete authenticated projections;
  public ingress/capture types cannot be forged from internal endpoints.
  `IngressHandle::value` converts with known leases and `retain_value` retains
  without encoding or granting authority. Captures/sources can retain known
  owners in returned packets and source jobs.
- `runtime/bound.rs` implements `BoundSessionOptions` and Graph front doors:
  ordinary/scoped `inject_bound[_value]`, `begin_bound_request`,
  `take_bound_output`, `take_bound_request_output`, `validate_bound_capture`,
  `bound_output_closed`, `close_bound_input`, `start_bound_request_source`,
  `bound_wake_handle`, and `bound_process_notifier`. They authenticate before
  canonical admission/request/source/closure/observation operations; there is no
  second scheduler, request store, transport, cache, or H7 queue policy.
- `SessionOptions::bound` supplies checked readiness/resources/discard selection;
  `network.rs`, `runtime/mod.rs`, `process.rs`, and `runtime/request.rs` retain
  configured/source owners and share request prechecks. `compose/edit.rs`
  supplies checked symbolic connect/expose bridges. These are authoring data,
  not authority over the resulting compilation.
- `kernel/src/bound.rs` mirrors those operations on `Kernel`; `host.rs` mirrors
  them on `Generation` and exposes final-composition `Binding` resolution.
  Preparation returns owned immutable handles without admission/start/session
  access; failed candidates cannot donate authority. Intermediate
  `CandidateContext` imports preview endpoints only: its checked `Edit` bridge
  and legitimate symbolic producer routes/grants are not cross-final or execution
  authority. Fresh final `Binding` handles are required.
- `QuerySpec::bind_preparation` resolves final tap/root/refresh/capture/alias
  routes into owned `QueryBinding<T>`; `bind_kernel` authenticates that exact
  generation. Production `query_with_budget` uses those handles for admission,
  Full retry, all-output draining, validation, and existing cleanup/failure
  ownership, rather than physical tuple comparisons or a query-only replacement.

## Operation → Code → Tests Inventory

Paths abbreviate `packages/phenix-graphs` as Graph, `packages/phenix-kernel` as
Kernel, and `packages/phenix-storage-graph` as StorageGraph. Policy loops are not
counted as additional tests.

| Operation / obligation | Implementation | Actual focused evidence |
| --- | --- | --- |
| Owned root/composite/primitive/duplicate/missing/same-shaped declarations; complete projections and local/wire matching | Graph `src/network/resolution.rs` | `fbp_bound_handles.rs::{owned_move_only_endpoints_and_handles_accept_clones_not_recompilation, composite_root_occurrences_and_complete_fanout_are_not_guessed, full_schema_mode_direction_and_raw_retry_without_reencoding, pure_authentication_uses_cached_identity_not_schema_providers}` |
| Ordinary/scoped typed admission, exact Value retry, atomic fanout, returned rejection and codec owners | Graph `src/runtime/bound.rs`; canonical admission | `full_schema_mode_direction_and_raw_retry_without_reencoding`, `request_full_retry_and_closure_before_later_lane_construction`, `bound_fanout_full_is_atomic_for_slow_branch_and_raw_retry`, `repeated_raw_retention_deduplicates_issuing_leases_without_encoding_or_validation`, `every_raw_rejection_keeps_handle_and_explicit_codec_owners` |
| Request/session ownership, aliases, exact counts, scoped reads, stale/sealed/lane reuse and capture validation | Graph `src/runtime/bound.rs`, `request.rs` | `bound_requests_keep_session_ids_aliases_exact_counts_and_lane_staleness`, `scoped_reads_skip_unrelated_heads_and_exact_single_capture_settles`, `foreign_and_stale_front_doors_never_supervise_overdue_jobs` |
| Source selection/handoff, input/output closure, readiness/resources/discard and wakes/notifiers | Graph `src/network/resolution.rs`, `src/runtime/bound.rs`, `src/runtime/mod.rs`, `src/process.rs` | `only_designated_physical_sources_start_and_composites_enumerate_them`, `checked_configuration_and_symbolic_composition_bridge_keep_owner_proofs`, `source_handoff_keeps_issuance_and_extra_leases_after_handle_and_session_drop`, `foreign_configuration_is_rejected_before_factories_and_pure_validation_has_no_codec_callbacks` |
| Configuration overwrite/abandonment, hostile Drop and aggregate/base cleanup | Graph `src/runtime/bound.rs`, `src/network/resolution.rs`, `src/network.rs` | All 10 `fbp_bound_cleanup.rs` tests: abandoned/cloned handles, successful/foreign start, actual override cleanup and first failure, whole-inventory refusal, detached jobs, shared replacements, source job/completion ordering, configuration/startup failures, and abandoned options |
| Checked composition and intermediate preview isolation | Graph `src/compose/edit.rs`; Kernel `src/lib.rs` `CandidateContext`; migrated declarative/portable consumers | Kernel `bound_handles.rs::preview_endpoints_never_become_final_authority`; `declarative_imports.rs`, `graph_contracts.rs`, `composer_declarations.rs`, `composer_nested_extension.rs`, and `fixtures/host_portable_consumer.rs` migrations |
| Kernel/Generation mirrors, callback ordering, Full/busy/reentrant retention and stable repeated raw retry | Kernel `src/bound.rs`, `src/host.rs` | `typed_admission.rs` four new bound tests for conversion ordering, hostile conversion, Full retry and stable repeated raw retry; its six retained typed-admission regressions remain in force |
| Final host Binding, candidate failure/panic, C3 archive/successor, source and cross-session request authority | Kernel `src/host.rs` | `bound_handles.rs::{host_final_binding_failure_archival_successor_and_alias_authentication, preview_endpoints_never_become_final_authority, final_bound_source_and_wakes_keep_actual_session_request_authority}`; `native_sessions.rs::prepared_binding_uses_declaration_keys_without_admission` |
| Preparation-owned QueryBinding, exact owner adoption, root/tap/error redirects and real data/failure execution | StorageGraph `src/derived/query.rs`, `mod.rs` | `query_bindings.rs` six tests for prepared/archive/rejected/successor owners, heterogeneous aliases/Full retry, alias/stale capture identity, both resolvers' same-shape rejection, independent leases, and both explicit failure policies |
| Zero-step capture relief, bounded cancellation/jobs and no cross-lane theft | StorageGraph `src/derived/query.rs` | `query_progress.rs` four tests for capture-relief progress, exhausted budgets, no-progress cancellation, and retained older-lane authority; retained `query_cleanup.rs` three tests |
| Compile-time payload/direction/capability/privacy enforcement | Private fields and typed signatures in resolution/runtime | `fbp_bound_handle_diagnostics.rs`: one native harness, eight negative fixtures asserting E0308/E0451 plus a positive control; no derive/import/missing-macro oracle |

## Final Review Corrections And Evidence

The final whole-scope independent review reports **no remaining findings**.
Graph review confirms all cleanup/aggregate findings fixed in the current source;
Kernel review confirms stable retry retention; query review confirms the initial
relief fix and an explicit H7 residual outside A5. The current implementation
corrects all seven detailed issues:

1. `BoundConfiguration` replaces configuration tuple/drop-order reliance with
   explicit value cleanup before declaration leases; abandoned, cloned and
   overwritten options preserve this order.
2. `HandleLeases` catches every hostile final handle/lease release and its panic
   payload instead of aborting the remaining cleanup.
3. Bound override registration wraps custom readiness to retain code through
   callbacks and cleanup; actual shadowed/dynamic replacement faults are ordered
   cleanup secondaries without fabricating faults for surviving shared owners.
4. Foreign configuration validates the entire selector inventory before any
   replacement, process factory, notification/readiness or correlation callback.
5. `HandleLeases::append` makes appended owners and payloads release before the
   base execution owner for jobs, completions, sessions and escaped failures.
6. `IngressHandle::retain_value` uses `Value::with_lease` to deduplicate stable
   `Arc` identity: repeated ordinary, scoped, Full, foreign and WouldBlock raw
   retries neither re-encode nor grow a fresh lease wrapper. Source handoff
   attachment also deduplicates weakly.
7. Both initial Full-relief and first post-admission query drain loops track
   actual drained captures: capture removal can make progress after a zero-step
   report, but cannot replenish exhausted budget. They never consume a foreign
   request's scoped BoundedLanes captures. An older lane can still occupy a
   shared process or retain its scoped captures until its owner relieves them;
   this demonstrated **H7 residual remains outside A5**.

Recorded focused evidence, not new runs by this documentation task: **29 Graph
bound tests under both default and isolated no-default features** (18 bound + 10
cleanup + one diagnostic harness with eight negative and one positive fixture);
**13 focused Kernel tests** (three bound + ten typed-admission, of which four are
new bound tests); and **55 StorageGraph tests**. These groups overlap with the full
workspace gates and are not an additive test total. Focused results alone did not
close A5; the complete scope review and full gates below supply that acceptance.

| Gate | Final status |
| --- | --- |
| Independent final whole-scope Graph/Kernel/StorageGraph review | Passed; no findings after all review fixes |
| Focused Graph/Kernel/StorageGraph probes | Passed as recorded above |
| Full default/no-default workspace and isolated Graph no-default tests | Passed |
| All-target compilation, strict workspace Clippy and formatting | Passed |
| Explicit default/no-default workspace and isolated Graph no-default doctests | Passed |
| All ten checks on untracked-inclusive snapshot `/nix/store/ksma6x2zbs6k7v77rxnqax3szd0047hi-source` | Passed |
| Exact-head Loom/native concurrency Q5 gates | Passed in the four hosted Q5 runs below; no exhaustive Q5 closure inferred |
| Exact committed assembled-head hosted CI | Passed; all fifteen workflows SUCCESS on `45c2dfb6f9089fb4b2f25bd102a37d2181e44eaf` |

## Exact-Head Hosted Acceptance

Accepted implementation SHA: **`45c2dfb6f9089fb4b2f25bd102a37d2181e44eaf`**.
`gh api` confirmed that each run below has that exact `head_sha`, status `completed`
and conclusion `success`. No parent-head or different-SHA result is substituted.

| Workflow | Exact-head run URL | Conclusion |
| --- | --- | --- |
| Flake CI | [44667125080](https://github.com/matthis-k/phenix-ai/actions/runs/44667125080) | SUCCESS |
| Q5 session ownership Loom | [44667124389](https://github.com/matthis-k/phenix-ai/actions/runs/44667124389) | SUCCESS |
| Q5 actual wake Loom | [44667124605](https://github.com/matthis-k/phenix-ai/actions/runs/44667124605) | SUCCESS |
| Q5 native notifier retirement concurrency | [44667124524](https://github.com/matthis-k/phenix-ai/actions/runs/44667124524) | SUCCESS |
| Q5 in-flight worker recovery | [44667124303](https://github.com/matthis-k/phenix-ai/actions/runs/44667124303) | SUCCESS |
| Native runtime benchmark | [44667124534](https://github.com/matthis-k/phenix-ai/actions/runs/44667124534) | SUCCESS |
| H4 EOS endpoint paired benchmark | [44667124413](https://github.com/matthis-k/phenix-ai/actions/runs/44667124413) | SUCCESS |
| H4 scoped optional poll paired benchmark | [44667124327](https://github.com/matthis-k/phenix-ai/actions/runs/44667124327) | SUCCESS |
| H5 scoped closure paired benchmark | [44667124296](https://github.com/matthis-k/phenix-ai/actions/runs/44667124296) | SUCCESS |
| H5 request index paired benchmark | [44667124437](https://github.com/matthis-k/phenix-ai/actions/runs/44667124437) | SUCCESS |
| H4 targeted wake paired benchmark | [44667124354](https://github.com/matthis-k/phenix-ai/actions/runs/44667124354) | SUCCESS |
| H4 notified Custom readiness benchmark | [44667124549](https://github.com/matthis-k/phenix-ai/actions/runs/44667124549) | SUCCESS |
| H4 live request sparse paired benchmark | [44667124381](https://github.com/matthis-k/phenix-ai/actions/runs/44667124381) | SUCCESS |
| H6 agent allocation events | [44667124623](https://github.com/matthis-k/phenix-ai/actions/runs/44667124623) | SUCCESS |
| H4 mixed agent acceptance benchmark | [44667124297](https://github.com/matthis-k/phenix-ai/actions/runs/44667124297) | SUCCESS |

All **six Flake jobs** succeeded: exact-head inventory evaluation, shared Nix
build/cache writer, check shard-00, shard-01, shard-02 and the matrix evidence
aggregate. No skipped shard or pending aggregate is counted as a pass.

No performance credit or A1/A2/Q2/Q3/Q4/Q5 closure follows from these runs.
Existing C3 archival, C5 supervision, C6 restart/persistence and C7 trusted-native
scope decisions are retained; this change does not broaden or close them. The H7
limitation is not relabeled solved. The sections below preserve the original design
and pre-implementation baseline as history; the implementation/inventory and final
acceptance above are authoritative for current names, status and evidence.

## Original Scope

The original [A5 finding](fbp-issue-ledger-20261111.md) is: "Physical string
paths still escape typed handles in compilation and binding APIs." The original
[remaining-work acceptance](fbp-remaining-work-20261115.md) required coherent bound
handles for required compilation, binding, and execution operations, with
stale/foreign rejection, while retaining necessary validated dynamic paths.
[A6 typed admission](fbp-typed-admission-20261115.md) explicitly leaves generation
authority to A5: `InputKey<T>` is a typed name, not an owner or permission proof.

The full A5 target here is a usable, authenticated route from final compiled
declarations through typed boundary resolution to actual Graph operations,
Kernel/Generation calls, preparation validation, and the real StorageGraph query.
A wrapper that merely returns physical coordinates, a Graph-only demonstration,
or a replacement query fixture leaving production query code unchanged does not
meet that target. At design time, this plan supplied no implementation evidence;
the current implementation and focused evidence are recorded above.

## Baseline Source Audit (Historical)

Paths below describe the audited pre-implementation parent baseline, not current
code. They are retained to explain the design, not as a stale current-state audit.

| Boundary | Baseline behavior and gap |
| --- | --- |
| `packages/phenix-graphs/src/network.rs` | `CompiledNetwork` already owns a private `generation: Arc<()>`, inspection/boundary maps, and declaration leases. `PhysicalDeclaration` exposes public `path` and `index`; compiled input/output maps expose physical tuples. Detached `ComponentSnapshot` is diagnostic data, not membership. |
| `packages/phenix-graphs/src/network/compiler.rs` | Successful compilation allocates a fresh generation token. Inspections include primitive and composite declarations; only primitives receive physical indices. Root inspection has neither a definition key nor a physical index. Flattened boundary maps already contain composite/root projections. |
| `packages/phenix-graphs/src/network/resolution.rs` | `lookup_definition` currently filters out declarations without physical indices and rejects duplicate primitive occurrences. `bind(path, contract)` checks compiled implementation. Borrowed `BoundInput`/`BoundOutput` use `Arc::ptr_eq` but `resolve` returns physical targets/source. They cannot supply the complete owned execution surface. |
| `packages/phenix-graphs/src/port_keys.rs`, `src/value.rs` | Input/output keys encode direction and local/wire mode, not authority. Local identity is `TypeId`; wire identity is the complete `ContractValue::schema()` identity, not a Rust name or schema prefix. Existing `Value`/leased wrappers and typed failures establish ownership rules. |
| `packages/phenix-graphs/src/runtime/admission.rs` | Public ingress, atomic fanout, returned rejected `Value`, input-writer closure, and global capture reads already have canonical implementations. Request lanes are selected from the actual request, not from an endpoint. |
| `packages/phenix-graphs/src/runtime/request.rs` | `RequestId` authenticates session, session generation, and live sequence. Request begin/exact capture, injection, explicit compiled source start, seal/status/release, scoped read, and capture validation currently take names/paths. Capture validation checks session/request, source process/output, alias, origin, and expected output. |
| `packages/phenix-graphs/src/runtime/mod.rs`, `src/runtime/scheduling.rs` | `SessionOptions` uses physical paths for readiness/resources/discard configuration; wake/notifier lookup also takes a physical process path. These need checked declaration/endpoint selection, not a new scheduler or the assumption that a composite is a runtime process. |
| `packages/phenix-kernel/src/lib.rs` | `inject_key` converts before acquiring exclusive session ownership, then delegates raw admission. `with_session` is started-only, fail-fast on contention/reentry, with RAII restoration. A bound convenience layer must not wrap conversion in `with_session`. |
| `packages/phenix-kernel/src/host.rs` | `Generation` delegates explicit session work. Preparation-only `Binding` exposes inspection, definition lookup, `input_targets`, and `output_source`, but no compiled/session getter. Host `prepare` runs the binding callback before `kernel.start`; successful preparation subsequently activates producers without polling them. |
| `packages/phenix-storage-graph/src/derived/query.rs` | `QuerySpec` validates final tap/root definitions, exact refresh targets, selected/root/error captures through physical tuple equality, both against compiled state and `Binding`. `QueryHandle` revalidates, injects by refresh name, retries Full with the returned value, drains all output names, authenticates captures, seals, settles, and finalizes the request. This production path must migrate. |

The existing [approved C3 LeasedArchival contract](fbp-lifecycle-acceptance-20261116.md)
supersedes older pending-policy text: retained old generations, compiled clones,
sessions, requests, and jobs retain their original authority and leases. A5 must
not introduce revocation through a host's active pointer or publication epoch.
The [Q7 review](fbp-q7-final-review-20261117.md) also establishes known-owner,
per-handle, nonretroactive leases and typed primary/cleanup preservation. Q6
observations remain passive host-owned presentation.

## Design Constraints (Historical)

### Identity And Ownership

1. Extend the existing resolution generation token, using private ownership and
   `Arc::ptr_eq`; do not create a second epoch service, global handle registry,
   topology hash authority, or independent session identity scheme.
2. Handles are owned, clonable immutable capabilities, not borrows tied to the
   compiled object's stack lifetime. Private fields retain the token, necessary
   resolved identity/projection data, and known code/resource leases. No public
   constructor, deserializer, integer, path, snapshot, or tuple can manufacture a
   bound capability. Safe-Rust API authority is not protection from malicious
   trusted native code or unsafe memory access.
3. Owner means **compiled generation**, not Kernel host epoch, shape, component
   definition key, or a particular session. `CompiledNetwork::clone` and
   `with_lease` preserve owner identity. Independent compilation and recomposition
   use different tokens even when definitions, names, schemas, and topology agree.
4. A handle may address multiple explicit sessions started from that compiled
   generation. A request remains owned by its actual session: a matching compiled
   token never validates another session's `RequestId`. Numeric session generation
   metadata is not the host publication epoch. Released IDs never regain authority
   when their bounded lane is reused.
5. Retained old handles remain valid against retained old compiled/session owners
   under C3. A successor rejects old handles even for identical shape. Rebinding
   resolves afresh against the successor, not by mutating/adopting an old handle.
   Failed, abandoned, foreign, or stale candidates never donate authority to the
   active/successor generation, and never change already issued active handles.
6. The token alone does not pin plugin code. Keep the exact known declaration,
   converter, payload, failure, and execution leases needed by each escapable
   object. Do not retain an entire Kernel/candidate/session, defeated snapshots,
   or queued payloads merely to establish generation identity. Attaching a lease
   later to a compiled clone does not enrich previously escaped handles.

### Capability Kinds

These are distinct permissions even if their implementations share private data.
Prefer the smallest extension of existing `BoundComponent`/input/output resolution
that expresses them, not an unrelated public endpoint hierarchy.

| Capability | May prove or request | Must not imply |
| --- | --- | --- |
| Bound declaration | Membership of one final primitive/composite/root declaration; checked contract and typed endpoint lookup | Physical process existence for a composite/root; injection, capture, source activation, session access |
| Bound input/output endpoint | Direction, exact identity, and compiled projection at a declaration boundary | Permission to write an internal input or observe an unexported output |
| Bound public ingress | Membership of one actual exported root input alias and its checked input identity | Arbitrary internal injection, request creation, credit, or open-writer state |
| Bound capture | Membership of one actual exported root output alias, exact identity and source projection | Reading any other alias of the same producer, fabricating metadata, or authorizing a request from another session |
| Bound source | Membership of one explicitly compiled zero-input source start | Starting an arbitrary zero-input-looking declaration, spawning a replacement process, or bypassing busy/done/sealed checks |

Name keys remain freely constructible selectors. Resolving a key against an
authenticated owner checks the actual direction, mode, schema, declaration,
contract, and public export as appropriate. Possession of `InputKey<T>`,
`OutputKey<T>`, a `DefinitionKey`, an internal endpoint, or a matching textual name
is not sufficient for admission or capture. Capability upgrade to ingress/capture
requires checking an actual public export, never just endpoint shape equality.

The preparation-only `Binding` may issue and compare immutable bound handles and
retain them in an application binding result. It must still expose no admission,
start, polling, mutable session, factory getter, compiled clone, or accessor from
which execution can be recovered. Returned handles are data plus authority that
requires a separately held executable owner, not an executable owner themselves.
This preserves the boundary, not a sandbox around arbitrary application exports.

### Declaration And Projection Resolution

- Resolve a unique `DefinitionKey` across all final declaration occurrences,
  including composite declarations without a physical process index. Missing is
  an explicit error; two or more matches are ambiguity, never first-match wins.
- Supply explicit duplicate-occurrence selection. A validated dynamic locator can
  select an occurrence only after the resolver checks both the supplied key and
  actual final membership at that locator; alternatively an authenticated
  occurrence from enumeration can select it. An ordinal/index alone is not
  authority. The selected handle owns that generation and occurrence.
- Provide explicit root resolution. The baseline root has no `DefinitionKey`;
  do not invent a primitive index or make an absent key match every root/composite.
  Key lookup covers keyed composites, while root resolution returns the same
  declaration capability kind with the root's actual interface/projections.
- Contract binding still checks actual compiled shape and explicit semantic
  claims. Endpoint lookup cannot use a detached/mutated snapshot as proof.
  Replacement with the same shape but a different final key must not silently
  satisfy the replaced definition. Nested body revision and final replacement
  use final compiler metadata, not authoring-time cached physical paths.
- Public-to-declaration mapping is an authenticated resolver predicate, for
  example exact-ingress-projection and capture-of-endpoint checks. Authenticate
  both owners before comparing private flattened projections. Exact ingress
  matching compares the whole target projection, preserving target/multiplicity
  semantics, not mere containment of a desired target or textual alias equality.
  Capture matching checks the output projection; runtime capture identity still
  distinguishes aliases with the same source. Contract/identity checks are not
  replaced by projection equality.
- Applications must not reconstruct `(process index, port String)` or compare
  interpolated physical-path/index strings for correctness. Opaque handles may
  retain physical tuples privately; immutable diagnostic inspection may still
  show coordinates. Diagnostic data cannot be fed back as a forged capability.

### Types And Conversion

Both local and wire input/output handles must be supported. Local T needs only
the bounds actually required by local transport, not `Clone`, `ContractValue`,
derive macros, serialization, or a wire codec. Wire handles use the full canonical
schema identity, including version/structure, and existing canonical conversion.
Local and wire representations of the same Rust T are not interchangeable.
Direction is enforced by signatures and by checked dynamic resolution.

Binding resolves and validates declared identity; it does not encode/decode a
payload, read Storage, or run a process. Schema-provider/conversion code is not
assumed pure. On typed admission conversion precedes canonical admission, as in
the existing key API; rejection does not promise zero codec callbacks or return
the original unencoded T. Errors retain the constructed `Value`. A retry of that
value uses the value-taking bound admission path without encoding again.

For Kernel/Generation forwarding, run supplied codec/conversion callbacks
**outside the exclusive Kernel session lease**. Do not introduce an outer
`with_session` wrapper or a blocking lock. A converter may reenter or block;
normal session calls inside it see the existing actual ownership state, not an
artificial lease taken by the wrapper. Contention after conversion returns owned
rejection; RAII restoration and fail-fast access behavior remain unchanged.

Retain all known converter/input/declaration leases during conversion, including
while a local payload is consumed/dropped. Accepted packets, returned rejected
values, decoded leased results, and escaping typed failures/cleanup secondaries
must independently retain their needed known owners before the originating
handle/Kernel is dropped. Managed caught cleanup must retain leases through
payload destruction and cleanup/panic reporting; a parent failure's lifetime is
not sufficient for an independently escaped secondary. Do not flatten failures
or replace an initiating error with a destructor error. Preserve existing panic
containment boundaries; no promise of codec purity, external rollback, abort
recovery, or reporting an unwind that an ordinary Drop already swallowed.

Q6 summaries/observations must not retain these executable capabilities or invoke
conversion, schema providers, external `Display`/`source`, readiness, or drop
callbacks. Copy only cached host-owned metadata; no new callbacks in Q6.

## Execution Coverage

Authenticate the handle's owner against the actual receiving compiled/session
owner before delegating. Never reduce a handle to its name and let a foreign
owner accept that name. Rejection must not reserve a request/lane, touch queues,
run deferred supervision, or activate work as a side effect of handle validation.
Typed conversion can already have occurred; that is not session admission.

After authentication, reuse the canonical operations and their existing policy,
validation, failure, and ownership behavior. No second transport, request table,
scheduler, admission algorithm, lane allocator, retry cache, or topology cache.
The implementation must cover all of these, not only ordinary typed injection:

| Surface | Required bound operation and preserved behavior |
| --- | --- |
| Graph ordinary admission | Public-ingress typed and already-constructed-Value admission; exact identity, atomic fanout, Full/closed/terminal rejection and returned ownership |
| Graph request begin | Optional exact bound capture selects the existing expected-output/count semantics; reject foreign capability before reserving request credit |
| Graph request admission | Public ingress plus actual `RequestId`; preserve Foreign/Stale/Sealed, queue policy selection, request metadata, and returned value |
| Graph captures | Ordinary and request-scoped bound reads, explicit packet validation, and output-closed observation; preserve global FIFO versus per-request selection, origin and alias authentication, exact-capture counting and failure-drain rules |
| Graph sources | Start explicit bound source with an actual request, through the existing persistent handoff and request reachability; keep invalid/busy/done/sealed behavior |
| Graph input closure | Close bound public ingress using the existing writer closure across canonical and request-lane queues; request sealing remains distinct and does not close persistent network ingress |
| Graph configuration and wake lookup | Checked declaration/endpoint selection for readiness, resource attachment, discard-output configuration and process wake/notifier lookup; require an actual primitive process where appropriate, authenticate the compiled owner before configuring/starting it, and preserve existing policy/resource validation and session-owned wake behavior |
| Graph request lifecycle | Seal, status, failure/cancellation, disposal/cancelled-job inventory, and release retain actual session ownership; handles do not replace RequestId or StepTicket |
| Kernel/Generation | Usable bound resolution and forwarding for these session operations, including ordinary/scoped admission, begin/read/validate/source/closure through the established session boundary; no conversion inside the exclusive lease and no retirement revocation |
| Preparation `Binding` | Same immutable declaration/endpoint/export/source resolution and authenticated mapping predicates as executable owners, without an execution getter or admission/start |

Existing dynamic names remain supported for symbolic authoring, runtime-discovered
lookup, explicit duplicate selection, and diagnostics. They must still be
validated against the actual owner. A5 removes the need for applications to use
physical coordinates as proof, not every identifier String in the repository.
Migrate in-tree correctness consumers of the superseded coordinate-binding
surface directly. Do not add a speculative legacy alias/conversion shim or keep
an obsolete authority path just to avoid updating callers. Retain raw immutable
inspection only where it has an actual diagnostic/inspection role.

Checked configuration may lower privately to the existing options maps after
owner validation; it must not lose the owner proof before application to the
target compiled generation. Composite/root declarations do not select an
arbitrary descendant process. Returned wake/notifier objects still belong to the
actual session and existing process lifetime; a topology handle is not itself a
session wake capability. Dynamic configuration remains validated, not a bypass.

## StorageGraph Migration

`QuerySpec` remains an authored description of final definition lineage and
symbolic public names. Binding it must resolve a generation-owned immutable query
binding: final tap/root declaration occurrences, typed refresh ingress,
selected/root/error capture capabilities, and the drain set of actual exported
captures. Use authenticated mapping predicates to establish exactly the current
requirements: refresh projects only to root `run`; selected capture is tap
`value`; root data/error captures project to root `value`/`error`. Do not weaken
singleton refresh to containment or allow another same-shaped tap/root.

Both `validate_binding` and executable binding must share canonical resolver
checks, with no application tuple equality or public `index`/port reconstruction.
Preparation validation must not perform a demand read or build a live query
session. Owned bindings may escape preparation, but only execute with a separately
authorized, successfully published/started matching owner. A rejected candidate's
binding cannot be rebound by name into the active generation.

Migrate the actual `QueryHandle::query_with_budget` path to the bound capabilities,
not only its preflight. Preserve nested-query rejection, bounded synchronous
driving, Full retry using the exact returned value, sealing, all-output draining
to relieve old observations/backpressure, authenticated selected/root/error
capture counts, and `QueryValue`'s lease-preserving ownership. The drain set must
include alias and unrelated observation captures; selecting only the requested
capture can deadlock atomic publication.

Preserve request-local recovery and budget cancellation, primary failure identity,
typed ordered cleanup, original non-Clone causes, termination-failure selection,
disposal counts, released-scope staleness, and outermost `RequestInterrupted`
runner-owned job inventory. Revalidation may authenticate the stored binding
against its owner; it must not rebuild authority from formatted coordinates.
Existing query cleanup regressions, including `tests/query_cleanup.rs`, remain
required rather than being replaced by new happy-path tests.

## Alternatives

| Alternative | Benefit | Decision / falsifiable cost |
| --- | --- | --- |
| Keep keys/names and runtime checks only | Smallest implementation; supports dynamic authoring | Rejected as A5 solution: same names in a successor can accept an old selector and callers still prove mappings with physical tuples |
| Add typed wrappers around physical indices/paths | Easy adaptation of current lookup | Rejected: forgeable coordinates and application equality do not authenticate owner, export authority, composite/root projection, or aliases |
| Keep only borrowed bound endpoints | Reuses current API verbatim | Insufficient: cannot supply owned preparation results and complete execution/query capabilities without coordinate escape |
| New globally registered handle IDs or topology hashes | Potential serialized lookup or structural reuse | Rejected: duplicates existing Arc owner, risks same-shape adoption, adds lifecycle/cache authority not required by A5 |
| Session-owned topology handles | Strong single-session restriction | Rejected for topology binding: prevents immutable preparation and reuse across same-compiled sessions; RequestId already supplies actual session ownership |
| Clone/store whole compiled network, Kernel, or candidate per handle | Convenient access to factories and leases | Rejected as default: broad retention and possible preparation execution escape; retain only required immutable resolver data and known leases |
| Extend existing resolution with owned opaque, purpose-checked capabilities | Reuses token, maps, exact type checks, canonical execution and lease rules | Selected plan; more migration than name wrappers, and accepted only after the complete matrix and independent review |
| Remove every String API or add a parallel compatibility engine | Superficially uniform surface | Rejected: validated dynamic lookup is necessary; neither broad deletion nor a speculative shim demonstrates authority correctness |

## Historical Acceptance Matrix

These are the original falsifiable design requirements, not pending-implementation
claims. At the design checkpoint, final acceptance remained subject to assembled
gates and exact-head CI; the final acceptance above supersedes that pending status.
The matrix requires `SharedFifo` and `BoundedLanes`, and both `FailSession` and
`RestartAffectedProcess` failure policies where the operation/failure is applicable.
Justified nonapplicability, such as production DerivedRuntime fixing its recovery
policy, must be explicit; no unexecuted cross-product is claimed.

| Case | Required observation / failure oracle |
| --- | --- |
| Owner identity | Clones and leased clones accept; independently compiled identical graphs and recompositions reject each other's handles for every handle-consuming operation, without queue/request mutation |
| Archived/successor generations | Real host publication leaves retained old handle/session execution valid under C3; successor rejects old handle despite same shape/names; old/new RequestIds cannot cross; retired producers still do not poll |
| Definitions and occurrences | Primitive, nested composite and explicit root resolve; unique key succeeds; reused primitive/composite key is ambiguous; explicit selection checks key at occurrence; missing/wrong key, removed declaration and same-shaped replacement reject; final ancestor revision uses its new identity |
| Projection predicates | Composite/root flattening and multi-target ingress compare exact full projections; extra/missing/replaced target fails; wrong-owner comparisons fail; two capture aliases may share a producer but remain distinct capture authorities |
| Typed local/wire | Move-only non-codec local payload works; canonical encoded bytes interoperate; wrong local type, local/wire mode, full schema/version/structure, and direction reject; malformed bytes follow existing decode failure rules, without implicit local fallback |
| Public authority | Internal endpoint does not inject/capture; nonexported and wrong alias fail; freely constructed same-name keys and modified snapshots cannot forge a handle; non-source or merely zero-input declarations cannot acquire source permission |
| Full retry ownership | Capacity-one ingress/fanout Full returns the original constructed Value with known leases; no partial delivery; retry after relief does not re-encode/duplicate; Kernel contention retains rejection ownership; producer pending events, where used, are not repolled/recreated on Full |
| Requests and lane reuse | Request from another session of the same compiled owner fails; sealed/released/stale IDs reject; lane reuse cannot validate old request, capture, source activation, or close operations; bounded credit and exact-output counts remain canonical |
| Capture authentication | Real ordinary/scoped reads and explicit validation check request/session, origin, actual source/output and exact alias; packet for alias A fails alias B even with same producer; unrelated buffered heads preserve global/per-request ordering; invalid matching packet is not consumed/skipped |
| Sources and closure | Explicit source uses existing handoff; wrong source, busy/done, foreign/sealed/stale request reject; bound input closure handles existing and later lane construction as canonical code does; accepted packets drain, future writes reject, request seal does not close network writers |
| Configuration and wakes | Correct primitive/output handles configure the existing runtime; foreign-generation and composite/root-as-process selections reject before factory activation; invalid readiness/resource/discard configuration retains canonical errors; wake/notifier lookup affects only the receiving session and preserves stale/done/terminal behavior |
| Failures and cleanup | Both relevant policies preserve first primary, exact request attribution, recovery outcome, disposal, cancelled-job inventory and release rules; no returned value/job/failure disappears behind a convenience error |
| Lease lifetime and hostile drops | Drop compiled/Kernel/host/handle in adversarial order; accepted/rejected payloads, converted values, escaped primary/secondary failures retain exact known code through final use and hostile destructor unwinds; managed cleanup reports caught faults without erasing primary; no candidate-wide retention used as substitute |
| Callback ordering | Instrument converter/schema/formatter/source/readiness callbacks; binding does not encode/read/activate; Kernel conversion/reentry occurs outside exclusive session lease; busy/reentrant admission stays fail-fast and restores session after unwind; arbitrary external side effects are not asserted rolled back |
| Q6 passivity | Repeated explanations/observations invoke zero new converter/schema/external formatter/source/readiness/destructor callbacks and retain no executable handle/failure ownership; existing Q6 state and accounting regressions still pass |
| Preparation and candidate failure | During binding resolution/validation, Storage read, producer activation/poll, and session admission counters remain zero; rejection/panic means zero subsequent activation/admission and unchanged active generation. Successful prepare may activate only after binding returns, as today; stale/foreign/abandoned publication cannot adopt candidate handles |
| Real Graph lifecycle | End-to-end bound ordinary and scoped local/wire flows, exact capture request, source start, input close, capture read/validation, settlement/cancel/fail/release under required policies, not resolver-only tests |
| Real Kernel/Generation | Started/nonstarted, contention/reentry, scoped operations, retained archival owner and successor rejection exercised through public forwarding and real host Binding, not just direct Graph calls |
| Real StorageGraph query | Actual SQLite-backed source/derive/query executes through migrated binding/runtime; wrong refresh mapping, same-shaped root/tap substitution, missing/duplicate definitions, aliases, old observations, Full relief, decode/read failure, budget exhaustion, recovery and cleanup preserve current data/error/job/lease behavior |
| Native compiler negatives | Wrong payload/direction and endpoint-versus-ingress/capture yield intended E0308; forged fields/construction and preparation execution access fail intended privacy/unavailable-method checks. Positive controls compile/run with the same imports/features; no missing macro/import/stale-signature failure is accepted as the negative oracle |

For compiler evidence use a harness that checks the intended diagnostic rather
than treating any compile failure as success. Include hand-written, derive-free
fixtures under isolated Graph `--no-default-features`; macro absence cannot satisfy
a type/authority test. Rustdoc examples should complement, not weaken, that oracle.

## Historical Gate Plan And Final Disposition

These were the required commands at the pre-acceptance checkpoint, not a claim
that focused tests alone were sufficient. The accepted implementation passed the
full local gate set under the pinned Nix development environment with an isolated
target directory:

```bash
cargo test --locked --offline --workspace --all-targets --no-fail-fast
cargo test --locked --offline --workspace --no-default-features --all-targets --no-fail-fast
cargo test --locked --offline -p phenix-graph --no-default-features --all-targets --no-fail-fast
cargo test --locked --offline --workspace --doc
cargo test --locked --offline --workspace --no-default-features --doc
cargo test --locked --offline -p phenix-graph --no-default-features --doc
cargo check --locked --offline --workspace --all-targets
cargo clippy --locked --offline --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

The explicit compiler diagnostic harness and all ten configured flake checks also
passed: workflow synchronization, formatting, strict Clippy, storage/schema, Graph/
plugin/derive docs, Kernel, derived, no-default, integration and kernel smoke. The
untracked-inclusive snapshot `/nix/store/ksma6x2zbs6k7v77rxnqax3szd0047hi-source`
included the new files; a Git-filtered local flake that omitted them was not used as
evidence. Exact-head hosted runs preserve actual-source wake Loom, session
ownership, notifier/retirement and in-flight worker recovery gates. Successful
benchmark execution is not performance acceptance or a new budget.

Implementation acceptance combines the operation-to-test inventory and source audit
showing that production StorageGraph/Kernel binding no longer uses physical
coordinates as correctness proof with the complete reviews and gates above.
Intentionally retained raw inspection and validated dynamic selectors are not
authority bypasses. No percentage or finding reclassification follows from a few
focused passing tests alone.

The independent final whole-scope review inspected the assembled production
migration, tests, compiler diagnostics, lease/drop ordering, preparation
nonexecution and canonical delegation, and reports no findings. At the earlier
checkpoint this did not replace the then-pending gates; their final passing results
are now recorded above. Any later implementation change requires renewed review
and affected/full gate evidence. Local passes, earlier reviews and a parent commit's
green CI do not establish acceptance of a different head.

The pre-acceptance plan required all applicable hosted workflows green on the
**exact committed assembled head**, with SHA, run links, matrix coverage and final
review disposition recorded before closure. That requirement is now satisfied at
`45c2dfb6f9089fb4b2f25bd102a37d2181e44eaf`. No commit, push or hosted run is
requested or performed by this documentation-only task. The formerly pending
implementation gates are complete; this evidence revision still requires its own
checks.

## Negative Scope

- No removal of all identifier strings, symbolic authoring, validated dynamic
  lookup, public alias names, or diagnostic physical paths/indices.
- No A1 general private-composite extension, A2 interface subtyping/compatibility
  relaxation, incremental compilation, Q2/Q3 scaling resolution, new
  cache/scheduler, Q4 static-liveness or Q5 exhaustive-concurrency closure, or
  performance claim.
- No new sandbox or protection against untrusted native code; no rollback of
  arbitrary callback effects, forced worker interruption, or abort recovery.
- No C3 archival revocation, automatic migration/adoption, sole-successor writer
  guarantee, different request/lane ownership model, or broadened Q6 callbacks.
- No speculative compatibility shim, duplicate execution engine, unrelated-issue
  closure, product-completion score or universal progress claim; A5's accepted
  scope is the binding-authority contract recorded above.
