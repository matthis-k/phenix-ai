# Native FBP foundation

This is the central design and requirements document for the native graph foundation
and its integration. API contracts, examples and implementation invariants belong in
Rustdoc on their owning types and modules; executable acceptance belongs in tests and
CI. This document describes architecture and durable requirements, not a completion
scorecard. The current source and tests are authoritative for implemented behavior.

## Purpose and boundaries

Build a small, independently usable set of Rust libraries that compose well together.
FBP models flows of work and data, not every operation in the application. Ordinary
library calls are appropriate for storage, authorization, configuration, and other
non-flow work. Provide a minimal standard set of components and adapters rather than
product-specific scheduler features. Maintain one current API and implementation;
remove superseded paths instead of adding compatibility layers.

| Library | Owns | Does not own |
| --- | --- | --- |
| `phenix-graph` | Component definitions, typed ports, network authoring, composition, compilation, bounded execution and inspection | Plugin inventory, concrete storage, resource permissions, application policy, second executor |
| `phenix-plugin` | Plugin definitions, contributions, registration, snapshots and code/revision ownership | Graph scheduling, concrete application behavior |
| `phenix-kernel` | Host-selected plugin composition, grants, contract resolution, generation publication, producer/driver lifecycle | Graph scheduling, concrete storage, product-specific flows |
| `phenix-schema` | Canonical portable schema identity | Graph claims, storage ownership or authorization |
| `phenix-storage` | Scoped resources, persistence, revisions, atomic operations, transactions and reactive invalidation | Graph execution or plugin discovery |
| `phenix-storage-plugin` / `phenix-storage-graph` | Explicit bridges for storage contributions and graph-based derivation/queries | A second storage implementation or scheduler |

The Graph, Plugin, Storage and Schema libraries remain usable without Kernel.
Dependencies point toward the libraries, not toward the product harness. Graph
does not depend on Storage or Kernel. Resource capabilities are opaque, typed,
explicitly granted values; the underlying resource library enforces its own authority.

## Network model

- A primitive and a composite have the same named public input/output boundary.
  A primitive owns a process factory; a composite contains a network. A composite
  is flattened by the compiler, not executed as a second runtime.
- Named typed authoring and dynamic authoring use the same canonical declarations.
  Local Rust type identity, portable schema identity, semantic contracts, origin,
  and authority are distinct. Shape equality alone is not semantic membership.
- A process is stateful and cooperatively steps. Factories create fresh instance
  state for a session. Inputs, outputs, sources, explicit closure and bounded FIFO
  edges suffice for branching, joining, feedback and module composition. Loops
  are permitted; recursive definitions are rejected.
- A compiled network is an immutable generation. The compiler validates and lowers
  deterministic physical identities and projections under finite limits. Handles
  authenticate their generation and exact declaration/endpoint; request IDs
  authenticate their owning session. Dynamic lookup must still validate authority.
- Runtime owns queues, readiness, work admission, staged continuations,
  publication, capture, closure and cancellation. There is one scheduling path.
  Readiness and diagnostic queries must be passive and not execute application
  callbacks. A process instance is single-flight, even across requests.

## Composition and plugin integration

A host chooses plugin revisions and grants. Contributions declare owned patches,
dependencies, priorities and offers/imports; the host supplies exact provenance
and authority separately. The Composer validates a candidate from its frozen
base and selected registrations, then publishes atomically. Registration order
must be deterministic; missing dependencies, ambiguous conflicts, invalid
origins or grants and nonunique selection fail closed. No factory runs during
patch replay. Failed updates preserve the previous generation and registry.

Private nested replacement and additive extension are different permissions.
Add/Connect/export requires author-opted seams, exact host grants at each
applicable boundary and checked lowering. No descendant or incident-edge
authority is inferred. Existing claims and frozen requirements survive edits.
Replacement preserves exact public boundaries and inherited semantic obligations.
A provider with a different interface may be installed inside an explicit
ordinary composite facade that exactly implements the old boundary; conversions,
extra outputs and extra inputs must be handled explicitly. Implicit width
subtyping and silent schema conversion are not requirements.

The exact selector has a finite per-attempt work allowance. Composition planning
has a separate finite logical work/storage allowance, shared across a Kernel
build's main and self-import preview composers. Neither limit promises a
wall-clock, payload-byte, allocator-byte or total host-build bound. Compiler
limits and semantic validation remain separate. Incremental compilation must
match canonical full compilation, preserve all authority/projection invariants,
and produce a fresh generation; unsupported deltas fall back to the compiler.

Kernel resolves and prepares selected contributions against approved capabilities.
Preview bindings grant no final-generation authority. Only a successfully
validated candidate is published; selected executable values, jobs and failures
pin exact revision, code and known resource owners for their lifetimes. Do not
hold inventory/ownership locks across plugin callbacks. Failed preparation or
publication does not promise rollback of arbitrary external application effects.

## Execution and lifecycle requirements

- One pending emission per process; validate and prepare selected fanout once
  before atomic commit. Backpressure does not replay application effects.
  Accepted packets drain before EOS; disposal is accounted, not success.
- Owned work handoff authenticates tickets and returns refused jobs/completions
  intact. Accepted failure/cleanup preserves the primary typed cause and ordered
  secondary failures. No unbounded tracing, callback preemption or hidden
  execution path. Worker guards preserve ownership context across delegation.
- Request admission, close, settlement, capture and release are explicit.
  Shared FIFO remains the default and ordinary readiness is head-only.
  Opt-in authenticated `CorrelatedAll` preserves FIFO within each request
  while matching across foreign heads; unscoped packets remain head-only.
  Opt-in bounded lanes isolate queues and capture credit, not mutable process
  state, external effects, or execution durations. A full queue is backpressure,
  not permission to exceed a bound. No universal independent progress or
  arbitrary-arrival fairness is promised.
- Closure requires actual EOS or a sound request-local absence witness, not
  global idleness. Diagnostics distinguish waiting, runnable, quiescent and
  terminal states without speculative liveness claims.
- Session contention and callback/worker reentry fail fast. Whole producer
  drains and blocking-driver ownership are exclusive; host publication
  coordinates retirement without holding application locks through callbacks.
  Application-created lock cycles, threads and non-yielding callbacks are not
  made safe by the host.

### Approved lifecycle decisions

| Policy | Requirement |
| --- | --- |
| LeasedArchival (C3) | Retirement stops host-owned polling; retained explicit generation/session/job capabilities remain valid under their leases. No strict revocation or forced unloading. |
| Cooperative (C5) | Optional deferred-job deadlines are supervised while driven; no autonomous abandoned-host watchdog, preemption, effect rollback or forced worker reclamation. |
| Restart And Persist (C6) | Successors start with fresh process-private state. Continuity is explicit through compatible durable grants. Reject incompatible schema; no automatic live migration, fencing or exclusive successor writer. |
| TrustedNativeScope (C7) | Plugins run as trusted native Rust code. Native execution is not a sandbox or dynamic-library ABI. Untrusted isolation is outside the current foundation scope. |

## Product composition direction

Prove the libraries with real, executable mock components before coupling them
to production providers or tools.

- Basic harness: request/session input, model invocation and streaming,
  tool dispatch, feedback/termination and observation. The agent loop is a
  composed network, not one opaque agent process.
- Advanced harness: extend the same contracts and flow with tool/environment
  selection, richer state/context, additional observers or subflows. Do not
  make basic a separate incompatible runtime or make a preset privileged.
- ACP is an optional plugin for either harness: inbound protocol events trigger
  approved graph ingress; outbound graph events are observed and translated.
  It does not own the scheduler or require the advanced harness.
- Keep application-owned durable state, model/provider selection and credentials
  outside Graph. Use regular library operations where a flow is not beneficial.
- Mock acceptance must exercise actual contribution selection, replacement,
  nested extension, event attachment, failure, cancellation, feedback and
  generation ownership. Production provider/tool integration is separate work.

## Remaining foundation acceptance

These are the outstanding design/verification areas from the previous 33-finding
campaign, not numerical progress estimates. Do not reopen accepted bounded
contracts merely because broader guarantees are intentionally excluded. Do not
mark remaining work complete without source and exact-head evidence.

| Finding | Requirement / unresolved decision |
| --- | --- |
| H4 | Measure representative scheduling work and latency. Address remaining mandatory source/Custom/staged/global scans only with sound invalidation. |
| H5 | Measure closure scaling including repeated upstream proofs and full global EOS. Preserve exact closure/witness semantics. |
| H6 | Establish representative latency, throughput, allocation/live-memory and concurrency workloads and acceptable budgets. Existing bounded measurements are not a portable product SLO. |
| H7 | Decide shared-state single-flight vs independent sessions or explicitly request-local process instances. Define effect/state continuity, progress assumptions, capture backpressure and resource bounds before promising isolation. |
| Q2 | Establish general dependency-aware invalidation/recomposition scaling; preserve canonical fallback and fresh generation authority. No speculative persistent reload cache. |
| Q3 | Verify whole planning limits and host prefix/preview path on exact published source; measure successful dense priorities, defeat/recovery, skew, diamond, hierarchical edits and ordered splices. Local evidence alone does not establish hosted acceptance or a universal performance budget. |
| Q4 | Specify an analyzable structural subset and sound reject/unknown/progress outcomes with explicit environment assumptions; arbitrary native callbacks and cycles cannot receive a universal static liveness guarantee. |

## Verification and maintenance

Use named unit/integration tests, actual mock networks, protocol/interleaving
tests, compiler/selector oracles and paired benchmarks as acceptance evidence.
Test negative paths, stale authority, backpressure, cancellation, unwind/cleanup,
generation retention and default/no-default configurations. Record exact source
and CI results in pull requests or issues, with raw samples in workflow artifacts.
Do not commit rolling review diaries, duplicated issue ledgers, subjective
completion percentages, temporary benchmark dumps or exact-head CI transcripts.

Code documentation belongs on the owning Rust API, including its invariants,
limits and examples. Add to this document only when requirements, library
boundaries or product-level architecture actually change. Historical design and
acceptance records are available in [the pre-consolidation tree](https://github.com/matthis-k/phenix-ai/tree/1d4f85713b984c7c269e0cabaff37479e74a7947/docs).

Source entry points: [Graph](../packages/phenix-graphs/src/lib.rs),
[Kernel](../packages/phenix-kernel/src/lib.rs),
[Plugin](../packages/phenix-plugins/src/lib.rs),
[Storage](../packages/phenix-storage/src/lib.rs).
