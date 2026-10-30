# Legacy plugin disposition

Reference baseline: [pinned old plugin tree](https://github.com/matthis-k/phenix-ai/tree/reference/pre-clean-plugins-20261026/crates).
All 29 legacy native plugin directories were removed in rewrite commit `3448a694`. This table describes *target responsibility*, not an implemented replacement.

| Removed crate suffix | Clean target | Disposition |
| --- | --- | --- |
| `advanced-model-adapter` | Selected Model gateway/transport | Merge shared transport and authorized invocation, retain opt-in strategy only |
| `advanced-preparer-graph` | Typed compiled graph edges | Retire dedicated wiring plugin |
| `advanced-preparer-noop` | Identity branch of selected preparation policy | Retire dedicated no-op plugin |
| `advanced-summarizer` | Compaction summarizer | Reimplement as focused selected strategy |
| `basic-agent` | Agent coordinator graph | Rebuild small domain loop without its own scheduler |
| `basic-frontends` | Presentation adapters | Rebuild Terminal/JSONL/ACP adapters; no agent semantics |
| `basic-memory` | Session context owner + evidence-backed recall | Split Skills/Context settings from durable memory retrieval |
| `basic-session` | Session owner | Rebuild one admission/CAS/recovery plugin |
| `basic-tools` | Tool manager | Rebuild attested, selected registry and batch execution |
| `compaction-checkpoints` | Compaction checkpoint owner | Rebuild one source-verified CAS owner |
| `environment` | Shared environment resolution | Prefer library; plugin only for an authorized runtime effect |
| `event-journal` | Event journal owner | Rebuild durable semantic event and cursor service |
| `evidence-journal` | Evidence source verifier | Move source validation into shared evidence library |
| `evidence-store` | Evidence owner | Rebuild single revisioned, source-checked evidence service |
| `model-history-test` | Deterministic model verification | Rebuild after minimal Model fixture with history/tool scenarios |
| `model-http` | Selected transport implementation | Rebuild common protocol adapters, separated from provider definitions |
| `model-meter-fixture` | Meter test fixture | Rebuild as tiny deterministic meter in verification composition |
| `model-scripted` | Model test fixture | Replaced first by small `phenix-plugin-model-fixture`; richer scenarios pending |
| `project-storage-sqlite` | Scoped storage adapter | Reassess record/scoping semantics before coalescing SQL backends |
| `provider-builtins` | Provider manager declarations | Replace fallback auth stubs with inert data-only definitions |
| `provider-chatgpt` | Provider definitions + shared OAuth strategy | Retire duplicate account/auth state machine |
| `provider-credential-files` | Credential store implementation | Rebuild isolated secure file backend |
| `provider-loopback-fixture` | Provider verification | Rebuild test-only local strategy |
| `provider-openai-api` | Provider definitions + shared API-key strategy | Retire duplicate catalog/auth runtime |
| `provider-secret-service` | Credential store implementation | Rebuild distinct OS Secret Service adapter under same contract |
| `storage-fixture` | Storage verification | Rebuild deterministic CAS/fault injection fixture |
| `storage-sqlite` | Storage owner | Rebuild one transaction API implementation |
| `tool-fixture` | Tool contributor verification | Replaced first by small `phenix-plugin-tool-reverse` |
| `workspace-selection` | Workspace owner | Rebuild scoped project/workspace selection with durable CAS |

## Intended plugin budget

Aim for one owner per *authoritative domain state machine*: providers, sessions, tool catalog/routing, agent loop, journal, evidence, checkpoints, workspace and selected storage/credentials. Models, summarizers and frontends are focused replaceable implementations. Tiny provider declarations may be grouped in one data-only crate. Graph edges, no-op policies and pure validation helpers need no native plugin.

No universal service registry. The composition explicitly selects implementations and grants. Each new native plugin must justify its ownership, contracts and unique runtime effects.

## Removal acceptance

- A replacement's compiled graph contains no dependency on a removed plugin ID.
- Contract shape and runner semantics are derived from current approved graph-v2 target, not copied from an old implementation.
- A behavioral scenario from the reference tree passes against the new selected native composition.
- Replaced code is absent from the rewrite branch; retained pure libraries receive explicit review before selection.
- A test-only fixture cannot enter the public production closure or grant itself a capability.
