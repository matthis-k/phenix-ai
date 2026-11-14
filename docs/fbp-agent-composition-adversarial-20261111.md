# Adversarial critique — agent composition slice

Status: Phase 2, review of [the design](fbp-agent-composition-design-20261111.md)
before implementation.

1. The test must start from `NetworkBuilder::new().build()` and install the
   preset as an ordinary `Registration`. Otherwise it cannot prove the preset
   itself is unprivileged.
2. An independent replacement may only preserve an exact public interface; it
   cannot silently change a schema, namespace or semantic claim. Ensure
   `ComponentDef::map` interfaces really match.
3. Every splice requires authorization for the anchor and all generated edges,
   not just the new process. Grant those explicitly; reject the usual shortcut
   of auto-authorizing arbitrary plugin-declared writes in a production host.
4. No special first-party priority, registration order or kernel branch.
   Declare dependencies where ownership/order requires them. Revoke the base
   preset and check transitive effects.
5. String-based stand-ins are not actual tool execution, durable memory,
   compaction logic or multiple reasoning iterations. Document these limits
   prominently and do not inflate P1/P2 completeness.
6. Crossed-request FIFO joins and private nested editing are still unsolved:
   a pleasant single-request pipeline cannot be used as evidence they are done.

Conditional acceptance: test implementation uses only existing FBP contracts,
asserts execution before/after edits and release behavior, and exact-head CI
passes. Otherwise return to Phase 1.
