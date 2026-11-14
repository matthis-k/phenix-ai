# H5 — adversarial review of static reachability index

1. Must create the index from the exact `CompiledNetwork::connections()`
   used to allocate physical queues. Never cache authoring-level paths.
2. Node IDs are compiler indices; no graph mutation occurs within a
   `NetworkSession`. On a new generation a fresh session builds a new index.
3. Avoid transitive closure caches and hidden duplicate buffers. An outgoing
   adjacency list is linear in edge count and owned by the session.
4. Preserve duplicate edges; the reachable set must prevent repeated visits.
5. The modification is in request-root registration, not the readiness or
   publication scheduler. It must not change FIFO, cancellation or EOS.
6. `request_input_closed` can still have O(V * occupancy) behavior and
   is not optimized by this patch. Do not mark entire H5 resolved.
7. The index's owner must be the session, not a clone that pins an extra
   plugin revision or physical compiler generation.

Verdict: accept as a behavior-preserving subproblem reduction; full H5
requires measured stress and diff evidence.
