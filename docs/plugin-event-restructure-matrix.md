# Phenix plugin event-restructure matrix

> Source audit: 31 native plugin crates on `feat/graph-wave-settlement-20261025` (25 Oct 2026). LOC is **only `src/lib.rs`**, not total crate size. Benefit measures expected value of the event-driven restructuring, not current code quality. The matrix is a proposal and does not claim that the migrations are implemented.

**Totals:** 31 plugins; 9 high-benefit, 7 medium, 9 low, 6 no event refactor recommended.

| Plugin | src/lib.rs LOC | Role | Benefit | Current | Event/hook target |
| --- | --- | --- | --- | --- | --- |
| [providers](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-providers/src/lib.rs) | 775 | Provider manager | High | Partial | Kernel.begin → populate → settled → ready |
| [provider-openai](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-provider-openai/src/lib.rs) | 114 | Provider declarations | High | Migrated (draft) | on(ProvidersPopulate) → register(OPENAI) |
| [provider-builtins](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-provider-builtins/src/lib.rs) | 50 | Legacy builtin contributor | High | Legacy pull | on(ProvidersPopulate) → register standard provider data |
| [provider-openai-api](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-provider-openai-api/src/lib.rs) | 541 | Legacy concrete API-key provider | High | Duplicated catalog/auth | Declaration-only OpenAI API data; shared auth/protocol preset |
| [provider-chatgpt](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-provider-chatgpt/src/lib.rs) | 1151 | Legacy concrete OAuth provider | High | Duplicated OAuth/catalog | Declaration-only ChatGPT OAuth data; shared OAuth state machine |
| [provider-credential-files](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-provider-credential-files/src/lib.rs) | 446 | Credential backend | Low | Contract-backed | Optional on(kernel.begin) for one-time backend initialization |
| [provider-secret-service](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-provider-secret-service/src/lib.rs) | 241 | Credential backend | Low | Contract-backed | Optional backend readiness only for real async attach |
| [provider-loopback-fixture](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-provider-loopback-fixture/src/lib.rs) | 325 | Provider test fixture | Low | Test-only contracts | Exercise populate/ready path as integration fixture |
| [basic-agent](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-basic-agent/src/lib.rs) | 1156 | Agent orchestration | High | Graph prompt + state triggers | Plugin-owned turn started/finished; on session/input events only when needed |
| [advanced-preparer-graph](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-advanced-preparer-graph/src/lib.rs) | 168 | Advanced graph workflow | Medium | State-reactive | Keep watched-state triggers; optionally subscribe to turn-ready semantic event |
| [advanced-model-adapter](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-advanced-model-adapter/src/lib.rs) | 777 | Model interception | Medium | Selected model contract | Optional model-requested / completed notifications owned by model subsystem |
| [advanced-preparer-noop](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-advanced-preparer-noop/src/lib.rs) | 42 | Identity preparer | None | Selected contract | No new events |
| [advanced-summarizer](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-advanced-summarizer/src/lib.rs) | 288 | Summary provider | Low | Selected contract | Optional on compaction-requested from owner if multicast ever needed |
| [compaction-checkpoints](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-compaction-checkpoints/src/lib.rs) | 346 | Compaction durability | Medium | Storage contract | Possible checkpoint-created domain event after successful commit |
| [basic-frontends](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-basic-frontends/src/lib.rs) | 772 | Frontend adapters | Medium | Decode/render/cancel contracts | Convert user input to submitted action; opt in to lifecycle and session notices |
| [basic-memory](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-basic-memory/src/lib.rs) | 348 | Session context and skills | Medium | Storage-backed catalog | Optional context-updated/skills-updated after authenticated changes |
| [basic-session](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-basic-session/src/lib.rs) | 355 | Session aggregate | High | Session contract | Owner-defined opened/checkpointed/closed events (only actual transitions) |
| [basic-tools](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-basic-tools/src/lib.rs) | 407 | Tool catalog and executor | High | List/execute contracts | Manager onBegin → tools.populate → settled → tools.ready, if dynamic contribution is needed |
| [tool-fixture](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-tool-fixture/src/lib.rs) | 63 | Tool contributor fixture | High | Pull registration | on(ToolsPopulate) → tools.register(data); implementation retained |
| [environment](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-environment/src/lib.rs) | 138 | Environment resolution | Low | Pure contract | Optional onBegin only if path configuration needs eager caching |
| [event-journal](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-event-journal/src/lib.rs) | 432 | Durable journal | Medium | Storage backed contract | Optional appended/checkpointed notices after successful commit |
| [evidence-journal](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-evidence-journal/src/lib.rs) | 176 | Evidence adapter | None | Selected contract | No event required |
| [evidence-store](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-evidence-store/src/lib.rs) | 575 | Evidence storage/search | Low | Transactional provider | Optional evidence-index-updated after committed write |
| [model-http](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-model-http/src/lib.rs) | 290 | Async HTTP model transport | Low | Pending model service | Optional model-ready/error semantic notices at agent layer, not transport tokens |
| [model-scripted](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-model-scripted/src/lib.rs) | 280 | Deterministic Model fixture | None | Selected model | No production events |
| [model-history-test](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-model-history-test/src/lib.rs) | 357 | History test Model fixture | None | Selected model | No production events |
| [model-meter-fixture](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-model-meter-fixture/src/lib.rs) | 320 | Token meter fixture | Low | Selected test contracts | Optional integration with run-finished for benchmark reports |
| [storage-sqlite](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-storage-sqlite/src/lib.rs) | 277 | Persistent CAS storage | None | Direct contract | No event per storage write |
| [storage-fixture](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-storage-fixture/src/lib.rs) | 81 | In-memory storage fixture | None | Test contract | No event per storage write |
| [project-storage-sqlite](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-project-storage-sqlite/src/lib.rs) | 682 | Project-scoped store | Low | Transactional contracts | Optional project-selected/invalidation only owned by project manager |
| [workspace-selection](https://github.com/matthis-k/phenix-ai/blob/feat/graph-wave-settlement-20261025/crates/phenix-plugin-workspace-selection/src/lib.rs) | 314 | Workspace selection | Medium | Get/set contract | workspace.changed after validated committed selection |

## Detailed task and acceptance matrix

| Plugin | Preserve (contracts/authority) | Proposed change | Acceptance test | Risk |
| --- | --- | --- | --- | --- |
| providers | Retain ProviderRegistry, ProviderCatalog, ProviderAuth, ProviderRuntime contracts | Finish async wave failure/cancel handling; reduce shared host scaffolding; remove pull fallback after migration | Native 2+ independent declarations; cancellation rollback; ready after publish | High |
| provider-openai | Only registry import; immutable provider data | Minimize plugin State and hand-written graph boilerplate with generic on sugar once SDK accepted | API key + OAuth definitions resolved through shared manager only | Medium |
| provider-builtins | Registry import | Move built-in declarations behind existing hook; retire ProviderRegistrations::list | Builtins + OpenAI may coexist without duplicate registrations | Medium |
| provider-openai-api | No private API-key/auth/catalog contract copies | Cut over ACP consumers, preserve account identity & credentials, delete obsolete plugin | API-key env/direct flows and model list against shared host | High |
| provider-chatgpt | Credential store backend remains separate | Extract OAuth flow/adapters into shared manager; retire duplicate catalog and runtime | PKCE/browser callback, refresh, cancel, profile isolation and reconnect | High |
| provider-credential-files | Keep ProviderCredentials CAS/import/revoke/resolve | Avoid forcing events for secrets; config initialization only when needed | Atomic file permissions, isolation, no plaintext in events | High |
| provider-secret-service | Keep ProviderCredentials operations | No event wrapper for each credential call; credential state remains backend owned | DBus unavailable/locked paths fail without catalog corruption | High |
| provider-loopback-fixture | Keep deterministic ProviderRuntime test operations | Adapt fixture only for new product tests, not production features | Deterministic negative event and auth tests | Low |
| basic-agent | Keep Model/Tools/Session calls; state watches for prepared requests | Make turn lifecycle owner explicit and use settled fanout for extensible preparers; avoid redundant token events | Multi-turn, pending model/tool, cancel, deterministic replay | High |
| advanced-preparer-graph | Keep ModelPreparationChange contracts | Replace hand-coded DAG wiring with SDK hook sugar, not event bus | No premature model calls; state CAS + cancellation | Medium |
| advanced-model-adapter | Keep Model complete and session boundary | Do not duplicate agent loop; use existing graph for preparation pipeline | Metered contexts and selector authority maintained | High |
| advanced-preparer-noop | Keep ModelPreparation::prepare | No changes expected | Identity behavior remains deterministic | Low |
| advanced-summarizer | Keep CompactionSummarizer/M1 model calls | Don't emit token-level events or move summary authority into kernel | Reference and revision checks preserved | Medium |
| compaction-checkpoints | Keep transactional CompactionStore put/latest/get | Notify consumers only if independent cache/UI observers require it | No phantom checkpoint; journal lineage still verified | Medium |
| basic-frontends | Keep Frontend decode/render/cancel | Graph actions already fit; avoid making frontend own agent lifecycle | ACP and terminal behavior same, input cancellation valid | Medium |
| basic-memory | Keep Skills/Context get/set | Use typed state-watch when consumers need current values; avoid broad memory events | Session scope and CAS consistency | Medium |
| basic-session | Keep Session open/checkpoint/reconcile | Emit semantic session events from owner; no kernel session knowledge | Revision and nonidempotent tool reconciliation unaffected | High |
| basic-tools | Keep ToolCatalog and ToolExecute, runtime authority | Separate tool declaration registration from execution, avoid bespoke pull catalog | External tool contributors discoverable and deduplicated | High |
| tool-fixture | Keep actual tool execution contract | Fixture becomes proof of generic event-driven tool population | No duplicated tool / stable deterministic availability | Low |
| environment | Keep Environment::resolve | Avoid environment-changed events without mutable source | XDG/HOME fallback and purity preserved | Low |
| event-journal | Keep EventJournal append/read/observer | Do not conflate transient graph events with durable journal entries | Journal append atomicity and replay semantics | High |
| evidence-journal | Keep SourceEvidence::observe | Only subscribe if source invalidation has real consumers | Immutability and reference authorization | Medium |
| evidence-store | Keep EvidenceStore put/get/search | Prefer state/explicit cache invalidation when appropriate | No evidence access escalation | High |
| model-http | Keep HttpModelSession and async stream | Transport must not emit per-token graph events | Streaming, cancellation, provider auth unaffected | High |
| model-scripted | Keep Model::complete | Retain stable mock oracle | Turn transcript deterministic | Low |
| model-history-test | Keep Model::complete | Retain recall tests, no event wrapper | Only ModelRequest history determines reply | Low |
| model-meter-fixture | Keep ModelTokenMeter and summarizer calls | No runtime migration needed | Token accounting exact | Low |
| storage-sqlite | Keep Storage read/transact | Storage is not plugin authoritative runtime state | SQL concurrency/CAS tests | High |
| storage-fixture | Keep Storage read/transact | Remain minimal conformance fixture | Deterministic CAS test | Low |
| project-storage-sqlite | Keep ProjectMembership/ProjectStorage/SourceEvidence | Avoid broad event bus for sqlite writes | Project membership authority and CAS | High |
| workspace-selection | Keep Workspace get/set | Emit scoped semantic changes; never imply file permissions | No traversal, session isolation, notification after commit | Medium |

## Migration lanes

1. **Foundation (gate):** #1146–#1149, complete runtime failure/cancellation tests, exact-head Nix acceptance and graph lifecycle admission. Do not merge untested stacked graph changes.
2. **Provider consolidation (P1):** Move `provider-openai-api`, `provider-chatgpt`, and `provider-builtins` to pure declaration hooks + the shared `providers` manager. Migrate ACP deployment compositions and remove old duplicate registration/auth/catalog code.
3. **Extensible tool registry:** Use the same generic `begin → tools.populate → settled → tools.ready` pattern where independent declarations exist; migrate `tool-fixture` without changing tool-execution authority.
4. **Agent/session semantics:** Basic agent owns turn events; session plugin owns session lifecycle events; maintain graph state watches and directed contracts for Model, Tool, Storage and Journal.
5. **Optional notices:** Memory, workspace, evidence and compaction changes become semantic events only with concrete independent consumers; do not use graph events for CAS operations or model token streams.

## Scope exclusions

- No second kernel event dispatcher; no provider/tool-specific scheduler.
- Do not make plugin metadata grant authority or credentials.
- Do not convert direct operation requests into a multicast event without a real extension need.
- No assumption that every plugin needs onBegin; lazy initialization and selected contracts are often simpler.
- All event handlers, schema versions and plugin-owned state are generation scoped.
