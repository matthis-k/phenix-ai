# C8 — separate preparation and selected-execution code leases

Phase 0/1, baseline `6c4f87b32e890031d975fcc15facf62cbcbed133`.

## Phase 0
`Kernel::build_with_options` previously constructed one `code_lease`
from **all** candidate `PluginSnapshot` revisions before composition.
It attached that lease to every compiled graph clone, session, packet and
returned callback error. As a result, a detached compiled graph or packet
could pin defeated plugins that did not contribute executable code.

Correctness invariants: no selected executable owner can unload before all
jobs, packets, compiled definitions, processes and errors using it are
destroyed. Failed/temporary preparation can reference **any** candidate
snapshot and must retain them until those references disappear.

## Phase 1 comparison

| Option | Strength | Weakness | Choice |
| --- | --- | --- | --- |
| Keep all snapshots in all leases | Simple, safe | Unbounded retention of defeated revisions across detached runtime owners | Reject |
| Lease only selected owners *before composition* | Minimal retained owners | Selection not known; unsafe for preparation errors | Reject |
| **Split temporary preparation and selected execution leases** | Exact selected owners for active graph, all candidates safely pinned during preparation | Requires two lease references; escaped preparation exports still own their own candidate lease | **Select** |
| Rewrite plugin registry to track per-node exact leases | Finer granularity | Much larger cross-crate redesign with many new ownership seams | Defer |

At build start, clone the caller-supplied execution lease into a transient
`preparation_lease` alongside all candidate snapshots. Use it for
`CandidateContext` previews and leased preparation failures.
After `Composer::current().active()` resolves the selected contributions,
retain only snapshots whose **owner** has at least one selected contribution
in the final runtime `code_lease`, alongside the caller-supplied lease.
Attach this to the final compiled graph, session, producer errors and packets.
The kernel still owns its complete input snapshots while it exists.

A callback that intentionally clones a preparation preview into a durable
export can retain the preparation lease; that is an explicit escaped owner,
not implicit retention by every runtime output. Selection is at plugin-owner
granularity; no per-contribution code-unload claim.

## Phase 4 acceptance
A fixture supplies two mutually conflicting contributions, selects one by
host-granted priority, detaches the compiled graph then drops the Kernel.
The defeated plugin must drop immediately; the selected plugin must remain
alive until the detached graph drops. Existing plugin/process/lease tests
must continue passing under exact-head CI.
