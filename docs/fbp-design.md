# Foundation specification

Build independently usable Rust libraries that compose into an agent application. Use flow-based programming (FBP) for work and data flows. Use ordinary library calls for storage, authorization, configuration, and other work that does not need a flow. Maintain one current API.

Read the [glossary](../GLOSSARY.md) for domain terms. The sections below specify required behavior, not implementation status.

## Library boundaries

| Library | Responsibility |
| --- | --- |
| `phenix-graph` | Author, compose, compile, execute, and inspect bounded networks. |
| `phenix-plugin` | Register contributions and retain plugin revisions and code. |
| `phenix-kernel` | Resolve host-approved contributions, prepare and publish generations, and manage host producers. |
| `phenix-schema` | Identify portable schemas shared by Graph and Storage. |
| `phenix-storage` | Authorize resource access, persist values, and publish invalidations. |
| `phenix-storage-plugin` | Bind plugin resource requests to host-approved storage. |
| `phenix-storage-graph` | Connect storage observations and derived values to networks. |

Graph, Plugin, Storage, and Schema work without Kernel. Graph has no dependency on Storage or Kernel. Bridges depend on the libraries they connect. The agent application owns product policy. Graph accepts opaque capabilities; the resource library enforces their authority.

## Extension parity

- Represent intentionally open domains with a common type or behavioral contract. Named constants, constructors, and standard implementations are ordinary members of that domain. Adding an implementation within the contract must not require changing its consumer.
- Defaults supply ordinary values; presets assemble ordinary configuration or contributions through public APIs. Make precedence, selection, and required capabilities explicit. Equivalent configuration must not behave differently because of its construction path or origin.
- Standard and external implementations use the same registration, resolution, validation, execution, and lifecycle contracts. Expose the operations needed to reproduce standard behavior. Keep authority, provenance, and code ownership explicit and independent of implementation origin.
- Use enums for intentionally closed alternatives, structural forms, protocol outcomes, and state machines. Keep mandatory invariants in the enforcing library. Do not introduce extension points for behavior that is intentionally closed. Internal specialization must preserve the contract and be justified by measurements.

## Network semantics

- Primitives and composites expose the same interface. The compiler flattens composites. One runtime executes the resulting processes.
- Typed and dynamic authoring share one declaration model. Local type, portable schema, contract, provenance, and authority remain distinct. Validate dynamic lookups.
- Processes run cooperatively and hold state within a session. Factories create fresh process state. Support bounded FIFO connections, branches, joins, feedback, and explicit sources and closure. Permit network cycles and reject recursive definitions.
- Compile deterministically under finite limits. Authenticate each bound handle against its exact declaration and compiled network. Authenticate each request against its network session.
- Inspection is passive. Process readiness callbacks run through the runtime, not through inspection. A process instance runs one activation at a time.
- Keep the common component library small. Express application-specific behavior through composition.

## Composition and authority

- The host selects plugin revisions and issues exact grants, origins, and priorities. Contributions declare patches, dependencies, offers, and imports. Shape equality establishes neither a contract claim nor authority.
- Compose from a frozen base. Require one consistent selection. Reject missing dependencies, ambiguous conflicts, invalid grants, and invalid origins. Replay without executing process factories. Preserve the published composition on failure.
- Treat private replacement and additive extension as separate permissions. Require author opt-in and exact grants at each applicable boundary for nested additions, connections, and exports. Preserve existing claims and inherited obligations. Grant neither descendant nor incident-edge authority implicitly.
- Replace a component only through its exact public interface. A different provider may use an explicit composite with the old interface. The composite handles conversions, supplies additional inputs, and consumes additional outputs. Implicit width subtyping is out of scope.
- Bound selection work per attempt and logical planning work across the entire Kernel build, including self-import previews. The limits cover neither total build work nor wall-clock time or allocated bytes. Incremental compilation must match full compilation, retain projections, and produce fresh compiled authority. Fall back to full compilation for unsupported edits.
- Prepare and validate candidates before publishing. Preview bindings cannot authorize final execution. Hold code and resource leases through the destruction of every independently retained executable value, job, and failure. Release inventory and ownership locks before invoking plugin code. Candidate rejection does not undo external effects.

## Execution and lifecycle

- Admit one pending emission per process. Prepare and commit selected fanout atomically. Retry blocked publication without replaying process effects.
- Authenticate owned work tickets. Return refused jobs and completions intact. Preserve the primary typed failure and ordered cleanup failures. Carry execution context across worker handoff.
- Admit, close, settle, capture, and release request scopes explicitly. Default to shared FIFO and head-only readiness. Opt-in `CorrelatedAll` matches request packets across foreign heads while preserving their per-request order. Unscoped packets remain head-only. Bounded lanes isolate queue and capture capacity, not process state or effects.
- Respect queue capacity. Drain accepted packets before end of stream (EOS). Account for disposal separately from success. Prove request-local absence from upstream work rather than from global idleness. Distinguish runnable, waiting, quiescent, and terminal states. Current inspection is not a proof of future progress.
- Fail fast on session contention and callback or worker reentry. Make whole producer drains and blocking-driver ownership exclusive. Publication coordinates retirement with admitted drains. The host cannot prevent application-owned lock cycles or non-yielding callbacks.

### Lifecycle decisions

| Policy | Requirement |
| --- | --- |
| Leased archival | Stop host-owned producer polling at retirement. Retained generation, session, and job capabilities remain valid under their leases. |
| Cooperative supervision | Supervise optional deferred-job deadlines while the session is driven. Do not promise an autonomous watchdog, worker preemption, effect rollback, or forced reclamation. |
| Restart and persist | Start a successor with fresh process state. Preserve continuity through compatible durable grants. Reject incompatible schemas. Live migration, fencing, and exclusive successor writers are out of scope. |
| Trusted native code | Run trusted Rust plugins in-process. Neither untrusted-code isolation nor a dynamic-library ABI is in scope. |

## Storage

Issue stable logical identities independent of physical paths. Resolve them to authorized resources explicitly. Support versioned reads, atomic compare-and-exchange, and revisions that cannot be reused after deletion and recreation within a backend lifetime. Validate durable schema compatibility. Publish invalidations only after successful writes or commits. Bound observation queues and report overflow explicitly. Let consumers decide how to refresh. A resource request does not grant access.

## Agent composition

Start with executable mocks. Exercise real contribution selection, nested extension, replacement, event attachment, cancellation, failure, feedback, and generation ownership before integrating production tools or models.

- Basic harness: accept requests, invoke a model, stream output, dispatch tools, return tool results to the model, and observe completion. Compose the agent loop as a network rather than an opaque process.
- Advanced harness: extend the same contracts with tool environment selection, state, context, observers, and additional flows. Neither preset has privileged runtime access.
- Agent Client Protocol (ACP): make it an optional plugin for both harnesses. Translate incoming protocol events into authorized graph inputs and observe outgoing graph events. Keep protocol handling outside the scheduler.

Keep provider selection, credentials, and durable application state outside Graph.

## Performance and progress requirements

These requirements retain the scope of the original acceptance findings. Track implementation status in issues and PRs, not here.

| Finding | Acceptance requirement |
| --- | --- |
| H4 | Measure scheduling work and latency on representative workloads. Change mandatory source, custom, staged, or global scans only with sound invalidation. |
| H5 | Measure repeated upstream closure proofs and global EOS. Preserve exact closure results and witnesses. |
| H6 | Establish representative latency, throughput, allocation, live-memory, and concurrency budgets. Local measurements alone do not establish a portable service-level objective. |
| H7 | Define supported independent progress, state and effect continuity, capture backpressure, and resource bounds before choosing shared state, independent sessions, or request-local process instances. |
| Q2 | Measure dependency-aware recompilation and recomposition. Preserve canonical fallback and fresh authority. |
| Q3 | Verify planning limits and Kernel prefixes and previews. Measure dense priorities, defeat and recovery, skew, diamonds, hierarchical edits, and ordered splices. |
| Q4 | Define a structural subset and sound reject, unknown, and progress outcomes under explicit environment assumptions. Arbitrary native callbacks and cycles have no universal static liveness guarantee. |

## Acceptance

Use tests for public contracts, actual mock networks, negative paths, stale authority, backpressure, cancellation, cleanup, and retained generations. Use oracles for compiler and selector equivalence, bounded interleaving models for concurrency, and paired benchmarks for performance claims. Test open extension contracts with independently authored implementations using only public APIs. Compare equivalent default, preset, and explicit configurations, including resolution, authorization, failure, and cleanup. Exercise default and no-default feature builds. Do not claim more than the tests and exact-head CI establish.
