# H5 — precompute static reachability edges

Phase 0/1, baseline `8c8d8b365f00c5fca0aa8d3f51d2e62daa95e854`.

`NetworkSession::note_request_root` performs a DFS for each admitted
request. Every visited process previously scanned **every compiled
connection** and filtered by its source node, causing O(V*E) work per
request on large connected graphs, before closure checks could even run.

Compare:
- No change: repeated global edge scans.
- Cache full transitive reachability: fast per-request set unions but
  O(V^2) memory and topology-dependent invalidation; reject.
- **One immutable outgoing adjacency list** built at session start:
  O(E) extra topology entries; reachability DFS O(V+E). Choose.
- Work-stealing/scheduler-ready index: unrelated to request closure,
  introduces more state; defer to H4.

No new public API. Edge order follows compiler connection order. Duplicate
parallel process edges are allowed; existing per-request reachable BTreeSet
suppresses repeated node work. All packet, EOS, request and lease semantics
remain unchanged. This is a *partial* fix: upstream input-closure checks
still scan live queue packets and can be expensive under many pending
requests. H4 and full H5 require measured workloads before further change.

Phase 4: compare existing request closure/cancellation tests, exact-head
CI, strict formatting, clippy and no-default features. Add repeatable
scaling benchmarks before claiming full closure performance resolved.
