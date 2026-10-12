# G4 pending graph coordination

Base: definition-only graph follow-up, PR #787.
Owners: `graph.rs`, existing `runtime.rs`/`pending.rs` and external graph fixtures.

An admitted action owns one existing Root. Its graph execution retains waiting
node IDs, settled outcomes and opaque existing Ticket handles. The dispatcher
still owns calls, authority, native scopes, cancellation and physical leases.
Graph bookkeeping adds neither ticket records nor an executor.

The host pumps the same root without blocking. Each pump polls existing work
once, invokes ready nodes serially and returns pending, draining or finished.
Independent ready nodes can start while another provider is pending. A join
requires every active predecessor to succeed. A physically busy native instance
waits for its existing lease; it is never reentered. Synchronous submission uses
this same graph coordination with pending admission disabled.

Cancellation marks the execution terminal before admitting another node. It
cancels existing parent tickets through Root, which owns child cancellation.
Logical completion is distinct from physical draining. The host can continue
pumping cleanup, and the existing root/leases block retirement until quiescent.
Dropping the execution uses existing root abandonment and safe draining.
Revoked work gains no state or tool authority during cleanup.

The trusted host can invoke explicitly selected control operations through the
same Root between pumps. These calls retain its exact scope and existing grants.
This lets a frontend query an independently provided output contract while a
model child is pending. Native reentry and resource checks remain unchanged.

Host updates retain the action's root correlation and add pending-node and
cancelled completion facts. A terminal update occurs once. Kernel failures stop
the graph, cancel other live branches and remain observable while leases drain.
Typed domain failures retain the G3 Skip/FailRoot semantics.

Native acceptance must prove a slow A, independently completed B, delayed C,
same-root cancellation before C effects, real retained worker leases, rejected
cleanup writes and safe release. This focused change does not claim live
state-trigger propagation or full G4 acceptance; that follows separately.
