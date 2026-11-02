# Phenix — Trigger, Dataflow and Execution Algebra

**Status:** 2 November 2026 **proposed engine-independent contract** for
[Graph V2.4](graph-architecture-v2.4.md) and the Timely/DFIR experiment.
This is a normative *target for evaluation*, not current implementation or
permission to change ABI 3, default product behavior or existing PR ancestry.

## A. Minimal concepts

A trigger **starts dataflow** by delivering typed outputs directly to named
node input ports. Normal graph edges propagate node outputs. Resource reads
and writes never implicitly activate execution.

| Entity | Definition | Ownership |
|---|---|---|
| Source | Produces occurrences from state, I/O, network, timer or input | Selected adapter/plugin |
| Trigger | Named typed output interface and graph ingress bindings | Authored graph; trusted selection |
| Occurrence | One authenticated firing with output values and causal provenance | Source requests; kernel admits |
| Node | Operation with `In<T>`, `Out<T>`, `Read<T>` and `Write<T>` | Plugin behavior; kernel invocation |
| Resource | Owner-held scoped state or authorized snapshot/transaction | Domain owner |
| Wave | Causally correlated graph deliveries | Kernel |
| Ticket | Pending asynchronous native/remote work | Kernel |
| Placement | Selected local/remote executor and transport | Trusted composition |

Source listeners and trigger declarations are inert with respect to
root-completion accounting until an occurrence has been admitted.

## B. Static graph model

For one immutable, verified compiled generation, define:

    G = (N, Pin, Pout, Eflow, Edeferred, Eorder, T, B, R, A)

- `N`: node identities, selected operations, and execution policy.
- `Pin` / `Pout`: typed labelled input/output ports.
- `Eflow`: same-wave output-to-input delivery.
- `Edeferred`: explicit same-root next-wave delivery.
- `Eorder`: nonactivating order constraints.
- `T`: typed trigger output interfaces; `B`: bindings from those outputs to InPorts.
- `R`: scoped resource access declarations; `A`: trusted grants and security policy.

One trigger may target multiple nodes or several ports of the same node.
Its outputs must be checked and delivered **atomically**. Type/grant failure
rejects the whole occurrence without admitting partial work. A trigger does
not directly call the node. A node executes only when its port readiness,
ordering, resource and authority requirements are satisfied.

**Authoring illustration; NOT compiling API:**

```rust
#[node]
fn prepare(
    message: In<Message>,
    session: Read<Session>,
    context: Write<PreparedContext>,
) -> Result<Prompt> { /* owner-defined behavior */ }

trigger("message.received")
    .out::<Message>("message")
    .to(prepare::message);
```

`In<T>` provides graph data, `Read/Write` provide state access. Subscriptions
are graph-level `T/B` declarations, not a second `On<T>` node scheduler.
Macros may derive metadata, but normal authoring without macros must work.
Initially lower virtual trigger ingress to existing `SourceDecl`,
`NodeKind::Source` and `Flow` without altering Graph V2 runtime semantics.
Adapters for HTTP, terminals, clocks, files, network messages and committed
state are selected plugins rather than intrinsic kernel node kinds.

## C. Runtime identity and transitions

Abstract runtime state:

    Σ = (Generation, Roots, Waves, Mailboxes, Invocations,
         Resources, Tickets, Capabilities, Ingress, Trace)

An occurrence carries a source identity, trigger interface, source-local
occurrence/dedup ID, finite map of named output values and kernel-verified
causal origin. The kernel determines authority and correlation; payload
fields cannot grant either.

Invocation correlation conceptually includes:

    (GenerationId, RootId, WaveId, ActivationId, NodeId, OccurrenceId)

Independent external occurrences create separate roots and starting waves.
An authorized internal state-watch/event or Deferred continuation creates a
new wave in the same root. Ordinary Flow remains within the current wave.
Separate external roots must not satisfy one another's input readiness.
Overlapping views of the same physical invocation do not create extra work.

| Transition | Condition | Effect |
|---|---|---|
| `AdmitExternal` | Authentic source, bound typed outputs, full grants | Create root/wave; deliver all ingress outputs atomically |
| `AdmitInternal` | Verified live causal parent and authorized event | Queue new same-root wave |
| `Deliver` | Typed/authorized edge and matching wave | Admit input, transfer capability before release |
| `Start` | Input readiness, Order and resource rules met | Freeze inputs, acquire resource view, start invocation |
| `Yield` | Selected native call pending | Retain ticket/leases; no output yet |
| `Wake` | Matching live ticket completed | Resume that invocation, not a new trigger |
| `Commit` | Owner-validated write/revision and publication policy | Commit owned state and recoverable event intent |
| `Settle` | Invocation terminal, outputs validated | Publish selected outputs; release only its own obligations |
| `Cancel` | Authorized lifecycle command for existing scope | Fence effects, request interruption; retain drain obligations |
| `Complete` | Sealed ingress, exhausted possible producers and physical drain | Report root/region terminal state |

**Distinct actions, one kernel lifecycle:** Admit starts dataflow; Wake
continues pending work; Cancel interrupts existing work. A deadline timer
may request Cancel, not a graph node that must wait behind the work it is
canceling. Native I/O completion wakes the pending ticket rather than
creating an unrelated root.

## D. Dynamic input policies and progress

For port `p` under correlation `k` distinguish admitted deliveries `D(p,k)`
from all still-live potential causal producer capabilities `C(p,k)`.
A node's ability to run depends on its explicit policy:

- `Each`: each qualifying correlated delivery creates its own activation.
- `Once(group)`: at most one activation, with explicit winner rule.
- `All(group)`: require correlated members or terminally resolve their absence.
- `ExactlyOne` / finite `Optional` / `StaticMany`: retain terminal cardinality,
  failure precedence and closure semantics; not aliases for eager `Each`.

These repeated-delivery policies are **target extensions**; currently
implemented `InputReadiness` is not automatically reinterpreted.

For a producer invocation, a selected output creates child obligations
**before** that producer is retired. An unselected output creates no
synthetic unused-edge delivery. Its absence does not seal that output
for another invocation or alternative producer. Frontier exhaustion is
derived from live causal capabilities plus conservative graph reachability.

A region gate may succeed only when:

    IngressSealed(region, scope)
      AND no live capability can reach relevant region inputs
      AND physical leases in that region are drained
      AND no admitted relevant deferred descendants remain

It must not wait for future unrelated external triggers or outside-region
work. A region cannot depend on its own completion to seal its ingress.
Quiescence (no runnable work) is weaker than completion (no relevant
possible future work). Cancelled does not imply drained.

## E. Read/Write and state-change publication

`Read<T>` resolves an authorized, version-consistent owner-scoped snapshot.
`Write<T>` resolves an owner-authorized transaction; other plugins mutate
foreign private state only through declared operations. These parameters
are not activation sources or universally shared ECS cells.

- Mutability access does not itself imply a semantically committed change.
- Writes validate ownership, revision/CAS and any publication policy.
- Failed or ambiguous commit cannot publish an acknowledged success.
- Async/remote nodes use snapshots or transaction handles rather than
  holding ordinary Rust borrows across process/await boundaries.
- Successful committed changes may publish a typed `Changed<T>` occurrence
  with revision/provenance; internal-origin changes preserve causal root.
- Durable state+notification requires atomic recoverable intent (e.g.
  transactional outbox) or equivalent reconciliation; replay needs stable
  IDs, deduplication and policy-defined idempotent effects.

State storage and durability remain domain concerns, not an excuse to move
every plugin field into a new global graph store.

## F. Distribution is an implementation choice

The compiled topology, type/grant model, root/wave/activation semantics,
and exposed plugin contract remain unchanged when an operation is placed
remotely. A remote transport must preserve typed envelopes and capability
handoff, including:

1. Generation/root/wave/activation and occurrence identity.
2. Source and delivery authorization with scope attenuation.
3. Register downstream or in-flight obligations before releasing upstream.
4. Replay/deduplication and external effect idempotency policy.
5. Pending/cancel/late-result fencing and physical drain.
6. Explicit failure on partition/unknown remote outcome, never false success.

The first experiment may use one authoritative coordinator and remote
workers. Coordinator failover and concurrent roots are **not** implied by
type-safe message transport or the initial M1 implementation.

## G. Formal invariants

- **I1 Type preservation:** every admitted value satisfies its port schema.
- **I2 Authority monotonicity:** no trigger/edge/resource use widens grants.
- **I3 Correlation:** deliveries from independent roots never implicitly join.
- **I4 Nonactivation:** `Read`, `Write` and `Order` do not initiate execution.
- **I5 Atomic ingress:** one firing either admits all authorized bound outputs
  or no work at all.
- **I6 Per-invocation settlement:** one skipped branch cannot close another
  producer's future valid delivery.
- **I7 Completion safety:** no success while any relevant future producer,
  queued delivery, pending ticket or physical lease exists.
- **I8 Cancellation safety:** fenced operations cannot publish unauthorized
  late success; actual worker drain is observed.
- **I9 State coherence:** no successful write acknowledgment without the
  required committed state/publication.
- **I10 Placement independence:** local and remote implementations preserve
  authorized observable results under the same documented failure policy.
- **L1 Conditional liveness:** bounded admitted work eventually terminates
  if its selected operations resolve and scheduling/transport are fair.
  Unbounded cycles and permanently pending I/O are not promised termination.

## H. Falsifiable engine experiment

Use **one engine-neutral logical fixture**:

    Trigger<Message> → Prepare → Switch → (Model | Tool) → Join → Commit
                           + Read/Write<Session>

Run it with current Phenix, Timely and DFIR. Avoid compensating for a
candidate by rebuilding an entire second scheduler around its API.

| Test | Assertion |
|---|---|
| T01 | Trigger multiport ingress is atomic and typed |
| T02 | Independent external roots never cross-join |
| T03 | `Read/Write` alone does not execute |
| T04 | Committed watch generates causal wave, failed write none |
| T05 | Conditional unused branch exhausts with alternate producer live |
| T06 | Two same-node invocations select opposite output ports |
| T07 | `Each` is eager but `ExactlyOne` remains terminal |
| T08 | Pending operation cannot complete before Wake and real drain |
| T09 | Deadline Cancel fences late completion and drains |
| T10 | Deferred Next preserves root, changes wave |
| T11 | Forged/unauthorized emission is rejected before execution |
| T12 | Long-lived listener does not hold root open |
| T13 | Duplicate/lost/delayed remote messages do not cause false success |
| T14 | Overlapping views do not duplicate a physical ticket |
| T15 | Region gate observes sealed ingress and relevant frontier only |
| T16 | Crash between durable commit and notification is recoverable |
| T17 | Node-level named-port injections use real readiness and authority |
| T18 | Graph-level trigger sequences yield correct causal traces |

Each backend must report **pass / fail / not implemented / unsupported**
for each case. A reference model pass is not a production runtime pass.
Compare **observable partial-order traces**, not incidental scheduling
order. Record source/ABI fidelity, test evidence, exact commit, custom
scheduler/progress code, dependency/build cost and performance.

**Decision policy:** keep Graph V2 if external crates merely wrap custom
scheduling; adopt focused library mechanisms if they remove proven work;
replace substantial execution machinery only after behavioral/native
acceptance and a rollback-capable migration path.

## I. Current versus target and unsettled points

**Existing groundwork:** Graph V2 SourceDecl/SourceKind, source nodes,
Flow/Order/Deferred, typed ports, finite-wave readiness, selected operation
grants and native tickets. #1220 adds recursive placement; #1238 is a
bounded progress-model test branch; #1239 is a draft production settlement
guard. Current exact-head status must be checked separately.

**Not implemented by this specification:** direct typed ingress IR,
function-signature Read/Write injection, generalized repeated-delivery
readiness, capability-derived region gates, durable trigger adapters,
distributed remote execution and all 18 shared acceptance scenarios.

**Open design decisions:** (1) retained dedup ledger and replay policy per
durable source; (2) default `In<T>` readiness under repeated delivery;
(3) cross-owner state-change transaction boundaries; (4) region ingress-cut
representation; (5) whether resource descriptors require new contract
metadata versus generated adapters. Keep them explicit rather than
inventing backwards-compatible shims.

## References

- Timely progress and capabilities: https://timelydataflow.github.io/timely-dataflow/chapter_5/chapter_5_2.html
- DFIR architecture: https://hydro.run/docs/dfir/architecture/
- Bevy ECS observers/parameters: https://docs.rs/bevy/latest/bevy/ecs/
- Tokio TaskTracker: https://docs.rs/tokio-util/latest/tokio_util/task/struct.TaskTracker.html
- Termination detection: https://research.tue.nl/en/publications/derivation-of-a-termination-detection-algorithm-for-distributed-c/
- Transactional outbox: https://docs.aws.amazon.com/prescriptive-guidance/latest/cloud-design-patterns/transactional-outbox.html
