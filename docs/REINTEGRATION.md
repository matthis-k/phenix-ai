# Deferred integration gate

The previous branch head is `d8b26b43614c5f6b4b7b53cfd7d4480998e7c6e2`. All removed domain sources are in Git
history (`git show d8b26b43614c5f6b4b7b53cfd7d4480998e7c6e2:<path>`). Restore only necessary responsibilities
and port each graph-facing call to the sole prototype runtime.

1. **Freeze graph semantics.** Compile and pass all policy, completion,
   module, replacement, splicing, cancellation and adversarial suites.
2. **Close prototype holes.** Add output routing that returns a validated
   subset of existing outgoing edges, verify negative tests and precedence.
3. **Portable boundary.** Lower graph contributions through one ABI-safe
   representation; do not restore old `phenix-contract::graph::Patch`.
4. **Execution adapter.** Bind selected native operations to graph actions.
   Kernel owns grants, authorization, physical tickets, and native drain;
   graph owns waves, readiness, causal admission, topology and completion.
5. **Domain nodes.** Restore Basic model, tools, session, memory, environment
   providers as ordinary actions or modules, replacing imperative continuation.
6. **Frontends.** Restore Terminal/JSONL/ACP only as trigger publishers,
   observers and cancel sources. No frontend-specific graph scheduling.
7. **Composition/generation.** Wire module contribution, patch resolution,
   deterministic compiled identity and selected provider binding exactly once.
8. **Acceptance.** Restore all meaningful former native, SDK, security,
   cancellation, P1 and ACP parity tests against the new engine. Do not claim
   a product test pass before it executes over this architecture.

No old call sites may be reintroduced while performing these steps. If a
former behavior cannot be represented correctly, fail explicitly instead of
adding a second scheduler or compatibility fallback.
