# H1 — shared-session crossed-head correlation

Status: Phase 0/1 design, 2026-11-12. Baseline `cea0465c68cb15f34385f887ee83cd6259881301`.
This document does **not** authorize a silent transport semantics change.

## Phase 0: minimal requirement and impossibility boundary

One `NetworkSession` contains a persistent process instance and multiple
bounded FIFO `ConnectionQueue`s. `Readiness::All` reserves head packets with
equal authenticated `RequestId`. Two requests A and B can form:

```text
  input a FIFO: [A, B]       input b FIFO: [B, A]
  All join:     requires (A,A) or (B,B)
```

Neither request is selectable while preserving *both* complete connection FIFO
orders. No policy over only the existing heads can make progress, even with
unbounded execution steps: the admissible head pairs are only (A,B).
Scanning further breaks existing FIFO guarantees; postponing A/B requires a
second packet owner and at least one additional bounded capacity rule.
This is a semantic conflict, not a scheduling bug.

Goals: retain bounded memory and atomic fanout; preserve session and request
identity, cancellation/disposal, typed payload/code leases, no privileged agent
path, and explicit host-granted plugin composition. Shared process state is
meaningfully distinct from the existing bounded `NetworkSession`-per-request
M8 proof with explicitly shared application state.

## Phase 1: baseline / extension / alternative

| Alternative | Benefit | Unavoidable cost | Decision |
| --- | --- | --- | --- |
| Baseline: strict per-edge FIFO, no additional API | Minimal invariant-preserving execution | Crossed requests cannot complete on one shared `All` join | Preserve as default |
| Independent per-request sessions | Already tested, bounded by host; per-session FIFO and request isolation | Process instance state is not intrinsically shared | Supported workaround, not H1 resolution |
| Scan past FIFO heads | Small apparent patch, bypasses crossed heads | Violates global FIFO; changes output/effect order and backpressure; no capacity for skipped packets | Reject |
| Implicit request demultiplexer | Can make crossed joins selectable | Hidden second buffering authority, request closure/retirement ambiguity, capacity and fairness hazards | Reject |
| Explicit **correlated lanes** on selected boundaries | Per-request FIFO and a deterministic per-lane execution order, with bounded global admission | Deliberately changes cross-request FIFO semantics on those boundaries; needs transaction, cancellation, source/closure, capture and fairness proofs | Separate Phase 1 proposal required before code |
| Diagnostic-only head mismatch reporting | Explains why no progress without changing authority, buffering or ordering | Does not unblock work; not a proof that the graph can never progress | Selected small addition now |

### Selected bounded diagnostic

Add a passive `NetworkSession::correlation_blocks()` view for idle
`Readiness::All` processes with at least two input ports. Report a process
only when every required port currently has one or more **authenticated scoped
request heads** and the intersection of their request-ID sets is empty.
Include the exact port names and head request identities, not packet payloads.

It MUST not scan behind heads, move packets, evaluate custom readiness,
refresh the session, spawn a process, change capacity, or declare global
deadlock. This is a snapshot: an operator may cancel/release work or supply
future topology events; the method cannot prove permanent nonprogress.
Mixed unscoped heads and processes not using `All` are outside this
conservative diagnostic. There is no new scheduler or payload owner.

A real correlated-lane implementation would need explicit opt-in in
`SessionOptions` or input-port layout, global bound on total buffered packets,
request-local FIFO, deterministic fairness, exactly-once fanout, no older
packet silently stranded, cancellation attribution, EOS semantics and agent
effect ordering. Until that design is accepted, do not mark H1 resolved.

## Phase 4 gates

A native regression must demonstrate exact [A,B]/[B,A] head IDs,
non-mutating repeated inspection, no diagnosis on aligned requests and on
unscoped/global traffic, and continued `Waiting` status under the existing
strict-FIFO execution. Existing graph/kernel and request tests must stay green.
No performance or full deadlock-detection claim. Verify exact-head CI.
