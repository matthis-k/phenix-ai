# Composable agent runtime: kernel, Basic, Full and external integrations

status: proposed
audit_revision: 65c0ea6c8805fadb283719ee831096ae6cf27261
scope:
  - contract ownership and substitution
  - first-party reference agent graphs
  - external harness forwarding and capability use
  - memory and context integration

## Executive decision

The kernel runs a graph of contracts and capability providers. **It does not contain an agent.**
`phenix.agent.basic` is the executable first-party example agent graph, **not** a mandated implementation.
`phenix.agent.advanced` inherits Basic, and `phenix.product.full` selects the in-house Full product.
Full may replace Basic's providers or add new capabilities; it must not duplicate an unrelated agent runtime.

An independent harness can:
- provide `AgentLoopInterface` itself, using Phenix memory/context if desired;
- be invoked through a Phenix terminal provider that forwards to the external harness;
- consume authorized Phenix memory/context capabilities through a protocol adapter,
  without adopting the Phenix loop.

**Forwarding is terminal-provider replacement, not layer/decorator interposition.**
A Layer wraps the remaining service chain using the kernel's one-shot continuation.
Neither forwarding nor replacement grants additional authority.

## Audit of main (2026-10-08)

| Scope | Disposition | Evidence and correction |
| --- | --- | --- |
| Kernel provider resolution | clean | `composition/component.rs`, `composition/provider_resolution.rs`: compatible imports/exports, explicit policy, pinned generations |
| Layer and terminal separation | clean | `spec/plugin-service-layering.md` and runtime dispatch: distinct semantics |
| Basic-to-Full ancestry | clean | `phenix-agent-configurations/src/lib.rs` directly includes Basic in Advanced |
| Third-party provider replaceability | clean foundation | `phenix-core/src/third_party_component_regression.rs`, `phenix-plugin-basic-agent/src/component_regression.rs` |
| Agent contract ownership | finding | `AgentLoopInterface` and wire types lived in the Basic implementation crate. Extract into `phenix-sdk` without changing wire identity. |
| Application launch coupling | finding | `phenix-harness/src/application.rs` invokes `phenix.agent-loop@1` using a concrete Basic-era request. Contract types are now SDK-owned, but the application should ultimately use configurable capability bindings rather than hard-coded agent implementations. |
| Harness/plugin wiring | finding | `runtime_builder.rs` conditionally installs application agent-tool adapter and triggers when a specific Basic loop plugin is selected. Derive this from required capabilities, not plugin identity. |
| Composition override surface | finding | Core supports `ProviderCompositionPolicy`, but `PhenixRuntimeBuilder::build_using` does not expose/pass it. Wire the existing resolver policy through the supported product composition boundary. |
| Standalone memory | finding | `phenix.memory` has a required helper-invocation import even for storage/query-only use. Separate the helper-dependent mechanisms. |
| Standalone context | finding | `phenix.context` requires Phenix execution/resource providers, preventing simple external turn-preparation usage without those services. Expose a provider-neutral preparation boundary. |
| Basic context compaction | finding | No independent deterministic Basic compaction provider; Full's compaction contract is exported by `phenix.memory`. |
| External capability gateway | finding | Application SDK/callable APIs and process runtime bridges exist, but a standalone authorized memory/context gateway and independent-harness conformance fixture are not demonstrated. |
| Naming | finding | `PhenixRuntimeBuilder::with_basic_suite` is a minimal harness fixture, not the `phenix.agent.basic` agent graph. Document/rename it without breaking consumers unnecessarily. |

This is a source-level architecture audit, not a claim that all integration tests or supported products have run.
The implementation work remains outstanding except for the contract extraction included in this PR.

## Stable contract template

Put provider-neutral types in `phenix-sdk` (or a dedicated contract crate if it later adds demonstrable value),
never in a Basic or Full implementation crate. Reserve `phenix-core` for kernel-level execution primitives.
Each contract declares:
1. Stable versioned identity and directional request/response schema.
2. Permitted authority and scope semantics.
3. Explicit domain failures versus transport/runtime failures.
4. Cancellation, idempotency and retry expectations.
5. State/identity ownership and references.
6. Provider compatibility and graph-generation pinning.
7. Contract conformance fixtures shared by native, forwarding and external implementations.

Common contracts:
- Agent execution / model invocation / tool execution.
- Session history and exact evidence references.
- Context preparation, compaction and expansion.
- Durable memory store, retrieval and maintenance.
- Optional helper inference, language code facets and evidence resolution.

These are **logical contracts**, not a requirement for one crate or plugin per operation.
Split physical providers only where independent configuration or state ownership warrants it.

## Reference graphs

~~~text
Kernel: graph resolver + authority + persistence + lifecycle + generic dispatch
  (no mandatory agent)

Basic:
  agent.execute -> basic-agent-loop
      -> model.invoke       -> basic/provider-routed model
      -> tool.execute       -> tool executor
      -> context.prepare    -> context provider
          -> context.compact -> deterministic basic compactor
      -> history            -> session event owner

Full = Basic + Extensions + Overrides:
      context.compact       -> memory-backed advanced compactor (replacement)
      memory.store          -> durable memory
      memory.retrieve       -> selective hybrid retrieval
      memory.maintain       -> source-backed extraction/consolidation
      context.expand        -> exact checkpoint expansion
      plus planning, workspace, language, jobs, observability
~~~

A custom graph may omit `agent.execute` and the entire agent loop and expose memory/context alone.
The Basic loop must depend only on contracts. The same consumer code must work when an alternate provider wins.

## Provider composition

Use the existing `ProviderCompositionPolicy` and generation resolver; do not create another resolver.
Support include, add, explicit bind/replace, disable, permitted pre-dispatch fallback, and Layer policy.
Product-specific overrides are explicit; registration order is not semantic.

A required import without an eligible provider fails resolution; an optional import may remain unbound.
Provider fallback is allowed only when both the contract and product policy permit it.
Failure after dispatch or external side effects **does not trigger implicit provider switching**.
In-flight operations remain generation-pinned; new operations resolve through the new graph.

Do not encode dependency on `phenix.agent-loop` as shorthand for availability of tools, session APIs or SDK.
Tool adapter installation should be based on declared capability requirements.

## External harness interoperability

**Outbound forwarding terminal**

~~~text
Phenix AgentExecution importer
  -> selected external-agent-terminal
  -> typed protocol adapter
  -> foreign harness agent loop
~~~

The terminal translates the same contract, forwards correlation/cancellation and reports transport failures
separately. It cannot recursively invoke the same service to find the old provider.
Use a Layer only for actual interception with an explicit continuation.

**Inbound authorized capability gateway**

~~~text
Foreign harness loop (model and tools remain external)
  -> typed Phenix capability client or MCP tools
  -> authorized Phenix gateway
  -> selected memory/context providers in Phenix graph
~~~

A tool-only MCP interface can expose `memory.query`, `memory.record`, `memory.recall`,
`context.expand`. Structured `context.prepare_turn`/`context.commit_turn` calls require
the foreign harness to integrate lifecycle hooks; merely adding a tool cannot automatically
intercept its context assembly. Expose only contract-approved endpoints, never unrestricted
kernel invocation over a network. Authenticate and scope each call.

Process-backed plugin runtime adapters remain transport/packaging boundaries, not alternate graph semantics.

## Memory and context constraints

- Canonical source history is not the mutable context projection.
- Basic compaction removes only eligible prior material and retains exact recovery references.
- Full uses structured, source-backed summaries and checkpoints; budget verification is mandatory.
- The projection must include **both prompt resources and the model's tool-turn continuation**.
- Tool-call/result identity and ordering must survive compaction.
- Session, workspace, global and optional agent memory scopes remain distinct, authorized and relevance filtered.
- Source changes invalidate derived memories by stable evidence references and revisions.
- Automatic retrieval is selective. Keep manual structured/semantic queries and exact expansion accessible.
- Maintenance must be event-driven, restart-safe and idempotent; no LLM on every turn by default.
- Search/indexes are derived state; preserve canonical records independently.

## Acceptance criteria

1. Kernel starts with no agent-loop provider.
2. Basic starts and completes multi-turn model/tool work; Full extends its dependency closure.
3. A third-party AgentLoop provider is selected through an explicit graph binding without changing consumer logic.
4. A standalone memory-store/query graph runs without model helpers or an agent loop.
5. A standalone context preparation graph runs without fake Phenix execution records.
6. Basic/Full compactors satisfy the same contract and preserve exact evidence after repeated compaction/restart.
7. Explicit provider bind/disable and replacement are inspectable in resolved generation provenance.
8. Forwarding terminal invocation preserves correlation, cancellation, failures and authority.
9. An external harness can query and update authorized memory; unauthorized scopes are rejected.
10. Provider changes never silently rebind in-flight calls or replay effects.
11. Static analysis rejects contracts importing implementation crates and generic application logic branching on provider identity.
12. Shared test fixtures exercise native, replaced and forwarded implementations.

## Implementation order

1. **This PR:** move agent-loop wire contract/identity into the provider-neutral SDK, preserve original Basic exports, document audit and target graph.
2. Application entry decoupling: configurable agent-execution binding and capability-driven tool wiring.
3. Expose `ProviderCompositionPolicy` through harness configuration; graph substitution tests.
4. Add genuinely functional deterministic Basic compaction with recoverable tool-history checkpointing.
5. Full compaction substitution, repeated-checkpoint lineage, enforced token targets and continuation integration.
6. Separate memory storage/query from helper-backed maintenance; standalone graph tests.
7. Source-resolution, scope enforcement, maintenance and retrieval improvements.
8. Forwarding terminal + external capability gateway, shared interoperability fixtures.
9. Reproducible cost/quality benchmark variants; do not claim unmeasured superiority.

## Research and interoperability

Memory and context choices draw from:
- [MemGPT](https://arxiv.org/abs/2310.08560): hierarchical virtual context.
- [ACE](https://arxiv.org/abs/2510.04618): incremental context curation and avoiding summary collapse.
- [ACON](https://arxiv.org/abs/2510.00615): compression of long agent observations and interaction history.
- [LongMemEval](https://arxiv.org/abs/2410.10813): extraction, temporal updates and multi-session retrieval.
- [Self-RAG](https://arxiv.org/abs/2310.11511): conditional retrieval.
- [Lost in the Middle](https://arxiv.org/abs/2307.03172): excessive context and position effects.

The specific component graph, per-interface decomposition and forwarding-provider model are
**architectural engineering decisions**, not experimentally established by those papers.
Benchmark the tradeoffs separately. Standards are interoperability boundaries, not the kernel contract:
[MCP](https://modelcontextprotocol.io/specification/2025-11-25) for tools/resources,
[A2A](https://a2a-protocol.org/) for independent agent collaboration, ACP for application/agent integration.

Related existing Phenix contracts:
- [Agent configurations](agent-configurations.md)
- [Plugin resolution](plugin-resolution.md)
- [Service layering](plugin-service-layering.md)
- [Plugin runtime bridges](plugin-runtime-bridges.md)
- [Plugin call binding](plugin-call-binding.md)
