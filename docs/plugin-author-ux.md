# Plugin author experience: usage-first contract

**Status:** design target for the isolated graph-v2 rewrite. The current M1 SDK and two fixture plugins compile, but the graph-v2 author API and native executor described below are not complete. Latest normative graph proposal: [v2.4](../spec/graph-architecture-v2.4.md).

## The guiding constraint

A domain plugin declares **what it uniquely owns**, not how the kernel registers, routes, schedules, persists, or observes it. Most plugins should contain only a stable identity, optional configuration, and domain-specific handlers or declarative data. The SDK derives contract metadata, graph nodes, module boundaries, and standard wiring wherever types or conventions already determine them.

A graph module is a collection of nodes with a **small stable public interface**. Its internal nodes remain inspectable and patchable for advanced users. An interface is not a capability grant or another runtime executor.

### Domain-first Rust and thin Phenix annotations

**Design domain types and behavior exactly as ordinary Rust.** Keep state and its methods together in cohesive domain structs. Split files by domain responsibility only when a domain warrants it, never to satisfy a Phenix-wide `types/state/logic` convention. Private types need no ABI annotation; public contract values use `Portable`. A contract's signature is authored once per published interface and projected to ABI-3 metadata automatically.

This fixture implements ordinary methods, with minimal annotations for selected exports:

~~~rust
#[derive(phenix_sdk::Portable)]
pub struct Increase { pub by: u64 }
#[derive(phenix_sdk::Portable)]
pub struct CounterValue { pub value: u64 }
#[derive(phenix_sdk::Portable)]
pub enum CounterError { Overflow }

// Temporary explicit shared-interface signature; target is to infer it from
// annotated methods when the plugin owns the interface.
#[phenix_sdk::contract(id = "fixture.counter@1")]
pub trait CounterApi {
    fn increase(&mut self, change: Increase) -> Result<CounterValue, CounterError>;
    fn read(&self) -> Result<CounterValue, CounterError>;
}

#[phenix_sdk::plugin(
    id = "phenix.fixture.counter",
    define(CounterApi),
    provide(CounterApi, slot = "Counter")
)]
mod plugin {
    use super::*;

    #[derive(Default)]
    #[state]
    pub struct Counter { value: u64 }

    impl Counter {
        #[provide(CounterApi::increase)]
        pub fn increase(&mut self, change: Increase)
            -> Result<CounterValue, CounterError>
        {
            let next = self.value.checked_add(change.by)
                .ok_or(CounterError::Overflow)?;
            self.value = next;
            Ok(CounterValue { value: next })
        }

        #[provide(CounterApi::read)]
        pub fn read(&self) -> CounterValue {
            CounterValue { value: self.value }
        }
    }
}
~~~

This is **implemented** through a plugin module macro that recognizes `#[state]` on a domain struct and `#[provide(...)]` on its inherent methods. The original Rust methods remain callable in ordinary tests. The macro creates native ABI handler wrappers, metadata and typed port signatures. The domain struct need not be named `State` or moved into a separate `state.rs` file.

**Graph/dataflow is the remaining authoring integration:** describe selected edges and module exports declaratively, referring to these same contract method markers and inferred request/response/error types. Prefer simple typed `graph.flow(a.ok(), b.input())` authoring for nontrivial graphs and proc-macro packaging of that authored topology. Do not make authors repeat schemas or write a second ABI descriptor. A connection is not evidence of permission, a scheduling order, or a stream grant. The compiler must still validate selection, labels, joins, finite waves and patches. Offline typed graph-V0 authoring works today; embedding an offline graph-v2 descriptor and native execution remain separate unimplemented gates.

A cohesive plugin can be a **single Rust file**. Larger plugins should split on business/domain boundaries, e.g. `memory/recall.rs` for a recall engine with its records and methods, and `memory/evidence.rs` for evidence storage with its records and methods. A `types.rs`, `state.rs` or `logic.rs` split is never a Phenix requirement.

For sharing ABI-facing contracts with other plugins, publish one source-owned interface definition or generated definitions-only dependency. Do not move all domain structs into a central global contract crate.

### Target author-facing examples

These are *illustrative desired call sites*, not implemented macros:

~~~rust
// Pure domain implementation: no State, registration trait, or graph boilerplate.
#[phenix::plugin(id = "phenix.text")]
mod text {
    #[provide(Text::reverse)]
    fn reverse(text: String) -> String {
        text.chars().rev().collect()
    }
}

// Data-only provider definition: common API-key, OAuth, catalog and
// protocol handling are shared. This plugin holds no OAuth attempt map.
#[phenix::plugin(id = "phenix.provider.openai")]
mod openai {
    provider! {
        id: "openai",
        api: { endpoint: "https://api.openai.com/v1",
               protocol: OpenAIResponses, auth: ApiKey },
        chatgpt: { protocol: OpenAIResponses, auth: ChatGptOAuth }
    }
}

// Graph module: author only interface, selected domain operations and dataflow.
let recall = graph.module("memory.recall", 1, |m| {
    let retrieve = m.invoke::<RetrieveEvidence>("retrieve")?;
    let rank = m.invoke::<RankEvidence>("rank")?;
    m.entry("query", retrieve.input())?;
    m.flow(retrieve.ok(), rank.input())?;
    m.exit("result", rank.ok())?;
    Ok(())
})?;
graph.flow(agent.recall(), recall.input::<RecallQuery>("query")?);
graph.flow(recall.output::<RecallResult>("result")?, agent.context());

// Advanced customization keeps the *same* module interface.
graph.patch("my-ranker")
    .expect_module("memory.recall", 1)
    .replace_node(recall.internal("rank"), custom_rank);
~~~

No author writes a manual native ABI table, operation codec, singleton catalog, cross-plugin mutex, session bootstrap, callback poller, graph wave scheduler, or infrastructure-specific event dispatcher.

### Three contribution roles within one plugin, not separate runtimes

| Kind | Plugin author supplies | Shared machinery supplies |
| --- | --- | --- |
| Declaration | Domain identity and a small validated data record | Discovery, registration, strategy selection, state admission |
| Handler | Domain operation and private state when needed | Typed contracts, metadata, ABI, authority, cancellation |
| Module | Named typed entry/exit and the necessary domain nodes | Offline graph compilation, wiring, patch resolution, execution |

One native artifact can own more than one of these where cohesive. A graph node does **not** require its own plugin crate. Different providers do not require different auth engines.

## Target production plugin families

Every family below describes one domain owner or a replaceable concrete adapter. A module may live in that owner's graph without a native crate.

| Domain | Ordinary entry / exit | Internals open to graph patches | Author-owned unique logic | Must work at acceptance |
| --- | --- | --- | --- | --- |
| Frontend | input / output / selection | prompt adapter and rendering | ACP, Terminal, JSONL presentation | First input creates or resumes session; no manual session creation |
| Session | submit / run context | admission, generation selection, CAS | Session state and lifecycle | Restart, concurrent CAS, cancellation, isolation |
| Agent | run / final or continuation | prepare, model, branch, tools, deferred feedback | Agent policy and limits | Multi-turn, failed model/tool, deterministic behavior |
| Provider | register / catalog / auth result | API-key or OAuth strategy nodes | Tiny provider-specific data only | Live auth, refresh, discovery, persistence |
| Model | request / reply or error | protocol codec and streaming | Model wire protocol variance | Selected model, no phantom interface IDs, safe cancellation |
| Tools | call batch / per-call outcomes | route lookup and fan-out | Tool implementation and input validation | Attested routes, duplicate call IDs denied, partial failure |
| Memory read | query / evidence result | scope, retrieve, rank, verify | Recall policy | Scoping, provenance, expiry and withdrawal |
| Memory write | committed source / evidence revision | extraction, dedupe, validate, CAS | Curation strategy | No fabricated source, idempotent replay |
| Compaction | measured request / admitted request | meter, summarize, verify, checkpoint, remeasure | Summarization and compaction policy | Fail closed on mandatory budget; reversible journal |
| Workspace | select / scope | path resolution and validation | Workspace identity and policy | No cross-workspace authority leakage |
| Storage | transaction / acknowledgement | SQLite/file/OS adapters | Durable storage semantics | CAS, restart and error/rollback |
| Inspection | authorized query / graph view | provenance and trace projection | Optional user-facing formatting | Stable exports, readable patch diagnostics, redaction |

### Proposed default composition

- A **Basic** agent presents prompt, model and tool interfaces with defaults for session, journal, provider manager, storage and frontend.
- **Memory**, **compaction**, **workspace**, **inspection** and code intelligence are optional module contributors. Their interfaces attach at stable graph anchors without changing the Basic agent plugin.
- Plugin authors may override a stable module interface through a selected replacement. Advanced authors may patch internal nodes or edges with exact revision/digest preconditions.
- The default product has a coherent provider/auth experience. Provider definitions stay visible before authentication; persisted credentials and model selection are restored when valid. Failed credentials remain failures, never silent account fallback.

### Ergonomics acceptance

1. A single stateless operation needs one ordinary function and an export annotation, not a manually authored State or registration table.
2. A provider definition carries no transport, OAuth flow, token store or model enumeration implementation.
3. The Basic agent loop contains policy and typed node wiring, not scheduling infrastructure or handwritten cross-plugin sync.
4. A module author can connect two selected operations with one typed `flow` call; a consumer can connect modules without knowing any internal node IDs.
5. Advanced authors can inspect, address and patch internal nodes without bypassing security or relying on undocumented reflection.
6. The compiled graph's module public interfaces remain stable across equivalent internal refactors. Breaking changes require a new interface revision.
7. Attempting to omit a typed value, connect incompatible types, target a stale internal ID, invent authority or leave a required import unresolved must produce a precise error before an effect is admitted.
8. Deterministic mock model, scripted tool, storage-fault and auth-fixture compositions verify actual native dispatch rather than only helper functions.
9. Developers can answer `why is this node running?`, `what selected this provider?`, and `which patch changed this edge?` from correlated redacted traces.
10. A basic domain plugin has no dependency on a kernel internals crate. It compiles through published SDK and contract crates.

## What to reuse from the reference

The pinned reference branch is a **source of domain algorithms and behavioral tests**, not a supported API. Preserve and selectively port: protocol codecs, API-key/OAuth security checks, credential file safeguards, session/journal CAS, tool-call validation, evidence provenance, checkpoint/remeasure logic, SQLite behavior, ACP UX, and deterministic fixture scripts.

Do not copy the old framework, catalog multiplexer, manual plugin wiring, graph-v1 trigger adaptation, duplicate state mirrors, lifecycle wrappers or a separate event dispatcher. If an algorithm requires new shared machinery, move that mechanism to the appropriate reusable domain library or kernel/SDK layer; then simplify the author-facing call site again.

## Non-goals for the first vertical slice

No speculative fully generic workflow DSL, automatic permission widening, implicit values for missing request fields, extra graph manager plugin, second dispatcher, dynamic native Rust trait ABI, or claim that the new branch is P1-ready. `nix flake check` on the legacy product is not a clean graph-v2 acceptance test.
