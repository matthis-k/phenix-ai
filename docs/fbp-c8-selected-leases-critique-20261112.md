# C8 — adversarial critique

1. Selected code must outlive every compiled factory, active process,
   packet, queued capture and returned callback cause. Attaching only to the
   Kernel object is insufficient.
2. Error objects arising before selection must still lease all candidate
   revisions, not just winners or no revisions.
3. Do not assume a plugin can be partially unloaded at contribution
   granularity. Multiple selected contributions of one owner require the
   same owner snapshot.
4. A winner's resulting patch may contain dynamically captured factories.
   Its snapshot remains in `code_lease` even if a later session has no
   pending producer or packet.
5. Escaped preview graphs are intentionally different owners. Their explicit
   preparation lease is not an unintended leak in a post-selection graph.
6. Do not remove `_snapshots` from a live Kernel; its full inventory and
   destruction ordering remain independent of detached runtime leases.
7. Verify a **real composition defeat**, not merely an unused snapshot
   without a contribution; otherwise the test misses the issue.
8. Preserve caller-supplied execution leases both during preparation and
   runtime; avoid taking and discarding the only owning `Arc`.

Verdict: accepted if exact owner-selection regression proves drop ordering,
and all existing detached factory and callback-cause lease tests pass.
