# Plugin do's and don'ts

> **Status:** target design and review guidance (2026-10-30), not an as-built claim. Read with the [folder/crate layout](plugin-layout.md), [state ownership](../spec/plugin-state-ownership.md) and [CONTRIBUTING](../CONTRIBUTING.md). Unmerged graph-v2 capabilities must not be treated as implemented.

**Core rule:** a plugin is a typed replaceable **domain component**. It declares what it **provides**, **consumes**, **observes**, **emits** and **owns**. Composition binds public ports and grants authority; the kernel enforces execution/cancellation/scheduling, without business or frontend knowledge.

| Do | Don't |
| --- | --- |
| Model durable domain capabilities: agent, session, model, tool, credential, storage, evidence. | Create coordinator plugins that manually call unrelated domains in a frontend-specific sequence. |
| Describe a small typed import/export/event/command surface, with explicit error types. | Reach into private graph nodes, private state, stringly named callbacks or undocumented call order. |
| Make optional features subscribe as authorized observers/projections or typed decorators. | Explicitly call an ACP/Neovim/JSONL/frontend, logging sink or analytics plugin from model/tool/agent machinery. |
| Keep frontend ingress as validation/translation into a public domain command. | Make a frontend responsible for the session lifecycle, model selection policy or required execution. |
| Distinguish command (request), event (fact), observation (view), trigger (scheduling) and graph edge (declared control/data semantics). | Assume a request succeeded; trigger work from an `Order`-only edge or call a second queue a kernel event bus. |
| Invoke selected Model/Tool/Storage operations explicitly for real required effects. | Convert every effect into broadcast events or invent a universal manager/service locator. |
| Use Strategy for replaceable provider auth/discovery/protocol behavior; provider declarations mostly supply data. | Replicate shared protocol/OAuth/model discovery logic for every provider. |
| Own state in one domain, with typed revision/CAS, durable recovery and cancellation semantics. | Let two plugins own the same mutable session/receipt state or trust restored data as present authority. |
| Keep contracts reusable in a small shared contract-set crate, grouped by domain module. | Make one interface crate per field/plugin or copy contracts into product implementations. |
| Select implementations and attenuated grants explicitly in the trusted composition. | Treat registration, catalog visibility, identity, or model listing as authorization to execute. |
| Use fixtures implementing public contracts in independent test compositions. | Depend on a test-only mock in a shipping product or use test-mode branches in production plugins. |
| Prefer domain Rust modules, with separate crates for genuine independent boundaries. | Create a crate for every handler, graph node, observer, decoration or design pattern. |

## Reactive integration: ports, adapters and observation

```text
terminal / JSONL / ACP / Neovim
       ingress adapters             observers / projections
               |                              ^
               v                              | public authorized facts
        typed domain input --> [domain graph] +--> journal / logging view
                              |      |
                             Model  Tools
                              |      |
                           Storage / state owner
```

**The invariant:** remove all frontends and passive observers, and the underlying admitted domain flow still runs. Frontends should attach to existing public machinery, not be explicitly integrated by that machinery.

- **Ingress** may issue authorized typed commands, but must not introduce frontend-specific domain transitions.
- **Observer** is detached, side-effect-constrained and **nonblocking** for domain completion. Define whether events are live-only or durable/replayable, and specify correlation, ordering, backpressure, reconnect and disconnect. Observer errors cannot silently fail the model turn.
- **Decorator / gate** may affect flow only at an explicit typed, ordered, authorized public boundary. Approvals are gates with declared failure/recovery, **not** passive observers.
- **Public subgraph export** means a named, typed interface independent of private node names. A consumer of `RunResult` must observe the public result, not `agent/finish` or `tools/each` internals.
- **Required effects** are not observers: a model request, storage write, a tool invocation or receipt persistence may need an explicit operation and durable checkpoint. Making everything asynchronous weakens guarantees.
- **No invented capabilities:** current graph-v2 public exports, authorized patches and nonblocking observer contracts are migration targets; verify the corresponding PR and exact-head acceptance before using them.

## Pattern selection

| Use | For | Not for |
| --- | --- | --- |
| Ports & Adapters | Model APIs, credential backends, storage, frontend ingress | Domain behavior dependent on an adapter identity |
| Observer / Projection | Frontend view, logging, diagnostic/evidence presentation | Synchronous required effects disguised as notification |
| Decorator / Gate | Approval or instrumentation at an authorized typed boundary | Arbitrary private graph rewrites |
| Strategy | Auth, discovery, preparation, protocol choices | A copied state machine per provider |
| State machine + owned data | Session, authentication, receipt recovery, compaction | Two authoritative state owners |
| Composition Root | Bind selected implementations and grants | Global runtime registry with implicit authority |
| Typed handler + trigger | Reactive work on accepted domain events | Another scheduler or event bus |

## Mandatory review questions

1. Which domain does this component own? Can it function without knowledge of a particular frontend?
2. What public typed commands, queries, events, exported ports and errors are used? What remains private?
3. Which consumers **observe** existing behavior and which truly need to request an effect? Could a passive observer be removed with no change to the domain turn?
4. What is the trusted composition/binding/grant, and how does denied or missing authority fail?
5. Where is persistent, per-activation or per-execution state owned? Are overlapping activations safe? How are cancellation and late results handled?
6. Do disconnected frontend views, failed providers, malformed events, restart and partial receipts have tested outcomes?
7. Could a new implementation be a module? If adding a crate, what independent lifecycle/build/privilege reason requires it?
8. Which **selected product** verifies the path end-to-end at an exact SHA, including failure cases? Is the PR merely implemented, actually verified, or merged?

**Anti-pattern:** `agent -> tool -> frontend.notify -> journal.write -> model`.  
**Desired:** graph execution uses typed Model/Tool effects; optional frontend observes public outputs; journal durability enters the critical path only through an explicitly required, owned storage operation.

The directory target and staged migration gates are in [plugin layout](plugin-layout.md); the specification target and code reality must not be conflated.
