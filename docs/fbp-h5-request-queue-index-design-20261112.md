# H5 — bounded per-queue request membership index

Date: 2026-11-12. Parent: #1275, exact `9911b42e3c0a3d84c7c60658090a51d426190dd1` (verified six-job CI #43281700444).

## Phase 0 — problem, invariants, no-change baseline

`NetworkSession::request_input_closed` repeatedly scans every packet in each upstream queue to test membership in a specific request, once for each candidate closure/readiness evaluation. `request_live` and `request_work` perform the same scan across all physical queues. Under bounded but long/shared queues and many requests this is O(buffered packets) *per query*, and closure traversals may repeat the scan.

The canonical FIFO packets remain the sole payload/metadata/lease authority. Add no new scheduler, extra buffer or derived request ownership that could alter delivery, cancellation, EOS, capture selection, metadata authentication or atomic fanout. Empty/foreign/unscoped packets must not become phantom request work. Preserve original transaction rollback/Full/closed precedence. The index is internal to the queue, never surfaced as an authority.

## Phase 1 — design comparison

| Option | Advantage | Rejection/acceptance |
| --- | --- | --- |
| No change | No invariant to maintain | Repeated O(queue length) membership scans during closure and status |
| Rebuild map on every query | Easy consistency | Same O(buffered packets) cost; no benefit |
| Session-wide map of requests to queue indexes | Fast request overview | Additional cross-queue state, duplication, coupling to fanout and disposal |
| Precompute transitive upstream packet sets | Avoid traversal | Dynamic queue state cannot be statically cached; O(V²) potential memory |
| **Per-queue multiset request-count index** | O(log distinct request IDs) membership and cardinality; one owner with all queue mutations | Small bounded update cost/extra memory; **choose** |

Add private `BTreeMap<RequestId, usize>` to `ConnectionQueue`. Only committed queue packets are counted. Include `Packet` entries with authenticated `CaptureMetadata.request = Some(id)`; ignore packets with no request. Count multiplicity, not just presence (multiple captures/input packets for same request are legal). Index entries are bounded above by occupied packets and removed at zero.

Update the index *inside queue-owned mutations*: accepted direct `push`, `pop`, correlated `pop_at`, `close_reader`, and request-scoped `discard_request`. Preserve `ConnectionSet::commit` atomic preflight and enqueue through a private queue append helper only after **every** target passes validation. On `Full`/type/closed/foreign/cancelled/replay error, do not alter FIFO or index. `cancel()` calls `close_reader` and clears it. Queue writer closure does not discard packets or counts. `stamp_capture` runs pre-commit and does not change request identity.

Expose one `pub(crate) fn request_count(&self, id: RequestId) -> usize` (private to crate). Replace only membership scans in:
- `request_input_closed` (upstream queue checks), preserving its DFS and dynamic closure obligations;
- `request_live` for passive settlement and release; and
- `request_work.queued` cardinality.

Do not replace readiness `packet_for` / correlated capture selection: those still need exact FIFO positions, not merely membership.

## Acceptance and limitations

- Exact cardinality after direct push/pop, middle pop, duplicates, request disposal, close-reader/cancel; no phantom entries.
- Atomic multi-target fanout Full retry and type/closed failure leave index unchanged; successful retry indexes all copies exactly once.
- Shared-session cancellation and correlated capture regression suites keep packet order, provenance, leases and first-terminal results.
- No new public API, no legacy shim, no background work, no new scheduler. Only storage overhead proportional to occupied distinct request IDs **per queue**.
- This makes **request membership** O(log distinct IDs) rather than O(packet count). Full `request_input_closed` still traverses upstream processes/queues, and `request_work` still scans processes and queues. **H5 remains Partial** until representative benchmarks show actual wall-time/allocation improvement and full closure complexity is understood.
