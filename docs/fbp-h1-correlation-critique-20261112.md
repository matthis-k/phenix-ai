# H1 adversarial challenge — correlation diagnostic

Status: Phase 2, separately written against the H1 Phase 1 design.

1. **Does the diagnostic solve request correlation?** No. Explicitly label it
   observation-only. H1 remains open for a single shared process.
2. **Could it mark a satisfiable `All` join as blocked?** A port may have
   multiple physical producers, each with its own head. Intersect **sets of
   requests from every queue head on each port**; do not compare only the
   first queue or port cursor. A common request anywhere among each port's
   *heads* means the join can be selectable.
3. **Could it classify global packets as scoped deadlock?** Skip a process if
   any observed port head lacks a real `RequestId`, or a required port has no
   queue head at all. No inferred IDs.
4. **What if the process already owns an activation?** Report only idle
   processes. A staged WaitInput has a distinct reservation and scope.
5. **Does observation accidentally drive readiness?** Do not call
   `NetworkSession::state`, `refresh` or user-defined `ReadinessPolicy`
   inside the diagnostic. Only read existing transport heads and metadata.
6. **Does the report imply a permanent deadlock?** No. Other processes may
   consume/close queues, or the host may cancel requests. Describe a current
   head correlation mismatch, not graph deadlock.
7. **Could a public diagnostic leak plugin-private payloads?** Only return
   process path/index and authenticated request IDs. Never return packet
   contents or forged caller-supplied identifiers.
8. **Does this justify a second queue?** No. A future correlated-lane design
   must separately prove boundedness and acknowledge cross-request FIFO
   ordering changes; do not sneak it into this patch.

Verdict: conditionally accepted for the diagnostic only, subject to exact
regressions and final-head CI. The fundamental H1 runtime limitation is
unresolved and must remain in the issue ledger.
