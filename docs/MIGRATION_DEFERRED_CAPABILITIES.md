# Single-graph migration: reduced functionality ledger

**Branch:** `refactor/single-graph-purge-20261106` / draft PR #1241  
**Policy:** Get **one small correct graph engine** working before expanding plugin authoring. A deliberately reduced feature is never represented as a passing product acceptance test.

## Stable goal

Every plugin contributes a portable Patch. Each leaf node performs **one task**, with typed input slots and named outputs. Input policy controls *when* it runs; output policy controls *which declared edges* receive emitted values. An environment, model provider, tool, or security check may be a module exposing exactly the same kind of typed public ports as a leaf node; its internals are flattened into the same graph at compilation. Nodes never decide downstream node identities. The store is accessed through authorized node/policy operations; store reads and writes do not implicitly trigger execution.

```text
prompt ──► Prepare ──► ModelFetch ──► [output selector] ──► Provider A
                                                  ├─────────► Provider B
                                                  ├─────────► Provider C
                                                  ├─────────► Provider D
                                                  └─────────► Provider E

model_reply ──► output policy ──► (tools / finish / retry)
```

The output policy is a function that returns **any subset of existing edges** (zero, one, several, or all). Selecting exactly one of five identically typed provider nodes is one possible policy function, not a separate graph capability. The selector does not grant permissions.

## Preserved and required even during cutover

| Guarantee | Owner / acceptance gate |
|---|---|
| One Patch IR, one compilation, one GraphSession scheduler | `phenix-contract::graph` and `phenix-graph`, single-graph invariant CI |
| Typed edge compatibility and rejection of invalid routing | Graph compiler + conformance unit tests |
| Deterministic composition and generation identity | Patch planner, graph identity tests |
| No authority granted by a graph string or selector | Kernel verified operation resolver, denial tests |
| Native pending-ticket identity, cancellation, physical draining | Kernel host adapter; end-to-end tests **still required** |
| Wave failure does not corrupt other waves | GraphSession and host acceptance tests |
| No second Basic-agent build or alternative agent execution app | Single-graph invariant CI |

## Temporarily reduced, not silently removed

| ID | Current cutover limitation | Restore without a second engine | Required acceptance |
|---|---|---|---|
| R1 | Full Basic prompt/model/tool loop does not operate through the new graph; retired `BasicInput::submit` deliberately rejects | Split Basic into Prepare, ModelFetch, ParseReply, ToolInvoke, JoinTools, Finish and Journal nodes, with edge policies expressing continuations | Plain prompt; two tools and second model turn; repeated prompts; error/turn-limit checkpoints |
| R2 | B5 native graph acceptance binary (`phenix-b5-agent`) removed because it calls deleted `GraphExecution` and old direct-submit behavior | Rebuild equivalent black-box conformance on `begin_graph`/`NativeGraphRun`, never restore old facade | Correct results; denied grants; nested denial reconciliation; durable session recovery; domain/error/turn-limit; tool order; usage counters |
| R3 | Advanced preparer's property watch/state-trigger graph is not yet native | Store change is a typed explicit trigger event; admission and coloring handled by the existing GraphSession | Two contributions, property update, selected preparer, no implicit reruns, denied reads/writes |
| R4 | `OutputPolicy::Select` executes with local `ActionHost::route` only | Bind selector through verified plugin operation; support sync then pending policy calls with same native ticket lifecycle | Five identical-schema providers choose one by function; zero/one/several/all existing edges allowed; unknown/duplicate edges rejected; policy reads authorized store, permission denied, pending cancellation |
| R5 | Only `Readiness::{All, Any}`; incomplete cross-wave producer settlement | Extend input policy/slot state in the current GraphSession, never in plugins | Any-two, deferred/immediate, omitted upstream branch, independent waves and joins |
| R6 | Module/subgraph flattening and public-port boundaries absent | Compile reusable module declarations into flat nodes/edges with stable instance paths and public-only endpoints | Reused module, nested module, independent configured instances, private boundary denied, flat equivalence |
| R7 | Patch planner has only ordered edit conflict detection | Expand the current Patch resolver with priorities, splice/replacement policies and exact edge/slot conflict footprints | Independent diamond contributors, dependency ordering, collision, deterministic conflict resolution, rollback |
| R8 | Optional `patch!` authoring macro and visualization metadata not integrated | Macro lowers to ordinary PatchBuilder API; add node label/summary metadata without embedding routing logic in nodes | Macro/manual equivalence; endpoint inference/ambiguity; graph inspector renders simple one-sentence nodes |
| R9 | Existing V1/V2 conformance suites were removed with the old engine | Port behavioral guarantees into tests against the sole compiler/session/native host | Prior native authorization, cancellation, state scope, generation pinning, backpressure and failure isolation cases |
| R10 | Native selector timeout/pending and output-store policy unavailable | Route policy work through the kernel dispatcher with the same authority and ticket drain, then deliver selected edges atomically | Abort during selector, late completion ignored, no partial fanout, deterministic delivery |

The coverage loss in R2 and R9 is an explicit cutover concession. Removing these tests is **not evidence their old guarantees are supported by the new engine**. Acceptance is gated on replacement tests, not on changing CI labels to green.

## Fastest implementation order

1. **Minimal graph correctness.** Prove selected/broadcast delivery, typed connections, deterministic patches, activation/cancellation and native selected-operation request/reply wiring. A small synchronous native graph must run and report a result.
2. **Native policy and store hooks.** Bind output selectors to verified native operations; make store access explicit and authorized. Test choosing one of five providers and fail-closed invalid selections.
3. **Module definitions.** Public typed endpoints, compile-time flattening, patching through published module boundaries; the runtime stays flat.
4. **Small Basic nodes.** Implement a graph-native single model turn, then tool fan-out/join and continuation. Do not put a miniature loop inside a node.
5. **Reintroduce guarantees.** Rebuild B5/Advanced acceptance on one engine, including physical drain and durable session recovery.
6. **Ergonomics.** Add typed PatchBuilder helpers, optional `patch!`, and diagram labels once the semantics are stable.

No new parallel app, runner, graph executor or legacy compatibility layer is permitted. A terminal, JSONL, ACP and HTTP adapter are transport interfaces over the same kernel, not separate agent runtimes.

## Current verification boundary

The graph-unit conformance tests cover selection of zero, one, several, or all five compatible provider edges. The native host cannot yet run a selector and the graph-native Basic product is not yet functional. Rust formatting/CI and full native integration are **not passing/verified** at this stage. Keep PR #1241 draft until exact-head verification confirms the minimum migration gates; further functionality can remain listed as an explicit reduction when the reduced baseline itself is operational.
