# Fallback cross-session memory recall

status: specification-only

## Goal

Recover omitted project/task context across sessions only when the current request and environment are insufficient.

For the motivating case:

```text
prompt: "work on prs"

inside an identified repository
  -> current context is sufficient
  -> memory stays cold
  -> use live repository state

outside a project, new conversation
  -> identify missing repository/task context
  -> search bounded prior workspace descriptors
  -> query normal per-workspace memory only for plausible workspaces
  -> validate the winning workspace/repository against live state
  -> bind the new root session to that workspace
  -> continue with live PR state
```

The feature extends existing `phenix.context` and `phenix.memory` contracts. It does not create a global transcript store, a second canonical memory database, or a mutable live-runtime workspace switch.

## Fixed ownership

| Concern | Owner |
| --- | --- |
| neutral context anchors and missing-context needs | `phenix-sdk` context contracts |
| context sufficiency/classification | replaceable `phenix.context-recovery@1` provider |
| semantic memory, provenance, freshness, associations | `phenix.memory` |
| per-workspace canonical durable state | existing workspace Store Binding |
| cross-workspace locator discovery | `phenix-harness` rebuildable descriptors |
| candidate runtime construction and pre-session handoff | `phenix-harness` |
| repository/path/session/task validation | the owning live service |
| model selection for context classification | existing model routing |
| primitive-environment relocation | `phenix-context` CLI + default skill adapter |

Memory never grants filesystem authority, changes directories, mutates Store Bindings, selects a PR, or declares remembered repository state current.

## Neutral context types

Context identity is not memory-specific. `phenix-sdk/src/contracts/context.rs` owns:

```rust
pub enum ContextAnchor {
    Workspace { workspace_id: String },
    Repository {
        canonical_remote: String,
        workspace_id: Option<String>,
    },
    Path { path: String },
    Project { key: String },
    Task { key: String },
    Session { session_id: SessionId },
    Resource { service: ServiceId, resource: String },
}

pub enum ContextNeed {
    Workspace { query: String },
    Repository { query: String },
    Project { query: String },
    Task { query: String },
    Session { query: String },
    Resource {
        service: Option<ServiceId>,
        query: String,
    },
}
```

`ContextAnchor` is a locator/identity observation. It is never authority.

`ContextNeed.query` is retrieval evidence. It is trimmed, non-empty, and limited to 512 UTF-8 bytes at the service boundary.

## Recovery service

Add first-party crate/package:

```text
rust/crates/phenix-plugin-context-recovery
plugin:    phenix.context-recovery
component: phenix.context-recovery
service:   phenix.context-recovery@1
```

The normal suite enables it. Kernel/basic fixture suites omit it. An alternate provider may replace it through the same interface.

The component exports `ContextRecoveryInterface` and requires `ModelRoutingInterface`. It owns no durable namespace.

The SDK contract is:

```rust
pub struct ContextRecoveryState {
    pub anchors: Vec<ContextAnchor>,
    pub has_durable_session_history: bool,
    pub has_explicit_resource: bool,
}

pub struct ContextRecoveryRequest {
    pub profile_id: RoutingProfileId,
    pub prompt: String,
    pub state: ContextRecoveryState,
    pub at: u64,
}

pub enum ContextRecoveryDecision {
    Sufficient,
    Missing { needs: Vec<ContextNeed> },
}
```

The model-backed classifier uses routed callable:

```text
context.identify_needs
```

It uses the supplied routing profile. No provider/model name appears in the plugin.

## Cold-path trigger

The Harness coordinator runs only before binding a **new root conversation**. It never relocates an existing durable session or an active execution.

The fast deterministic gate returns `Sufficient` without a model call when any condition holds:

1. the request resumes a durable session with history;
2. the user supplied an explicit `Content::Resource`/exact resource reference;
3. current anchors contain `Repository`, `Project`, or `Task`.

A bare `Workspace` or `Path` anchor is not enough. An arbitrary directory such as `~/Downloads` therefore does not suppress recovery.

Only when all three fast conditions are false does the coordinator call `phenix.context-recovery@1`.

Classifier input contains only the current prompt plus the structural state above. It receives no historical memory.

Classifier output rules:

- `Sufficient` ends recovery immediately;
- `Missing` must contain 1..=4 distinct needs;
- each need query must satisfy the 512-byte rule;
- malformed, empty, oversized, or duplicate output fails the recovery attempt and falls back to ordinary discovery;
- classification failure never triggers broad memory search.

The routed model response is parsed directly into `ContextRecoveryDecision`. Free-form explanatory text is invalid.

## Native recovery lifecycle

`phenix-harness` adds:

```text
src/context_recovery.rs
src/workspace_discovery.rs
src/runtime_manager.rs
```

The runtime manager owns a reusable product recipe plus one active workspace runtime.

Conceptually:

```rust
pub struct WorkspaceTarget {
    pub workspace_id: String,
    pub canonical_root: PathBuf,
    pub canonical_repository: Option<String>,
}

pub enum ContextRecoveryOutcome {
    Current,
    Switched { target: WorkspaceTarget },
    Ambiguous { candidates: Vec<WorkspaceTarget> },
    NotFound,
}
```

These are Harness-internal product types, not Core APIs.

### State rule

The manager has two lifecycle states:

```text
bootstrap
  no durable session created
  no root execution started
  workspace candidate replacement allowed

bound
  durable session or root execution exists
  automatic workspace replacement forbidden
```

A recovery handoff is legal only in `bootstrap`.

Preparing another workspace constructs a complete candidate Harness/runtime with its own Store Binding. It does not alter the current runtime.

Activation order:

1. canonicalize and validate candidate root;
2. derive/validate workspace identity;
3. open/prepare that workspace Store Binding;
4. build the same configured Harness recipe;
5. activate all required plugins and product configuration;
6. query candidate memory and live repository identity;
7. select candidate only if resolution rules below succeed;
8. swap the manager's bootstrap runtime pointer;
9. drop the losing bootstrap runtime;
10. create the durable session/root execution in the selected runtime.

If any step fails, the original bootstrap runtime remains unchanged.

There is no in-place Store Binding mutation.

## Workspace discovery descriptor

Canonical semantic memory remains inside each workspace database. Cross-workspace discovery uses one rebuildable descriptor next to that store:

```text
$XDG_STATE_HOME/phenix/workspaces/<workspace-key>/discovery.json
```

Version 1 shape:

```rust
pub struct WorkspaceDiscoveryDescriptorV1 {
    pub version: u32, // exactly 1
    pub workspace_id: String,
    pub canonical_root: PathBuf,
    pub repository_remotes: BTreeSet<String>,
    pub recall_terms: BTreeSet<String>,
    pub observation_count: u32,
    pub confirmed_recoveries: u32,
    pub last_observed_at: u64,
}
```

Limits:

```text
serialized descriptor <= 64 KiB
repository_remotes <= 16
recall_terms <= 256
one recall term <= 64 UTF-8 bytes
scan <= 4096 workspace descriptors per recovery attempt
prepare/query <= 3 candidate runtimes per recovery attempt
```

Descriptor writes use a same-directory temporary file, mode `0600`, file sync, atomic rename, and parent-directory sync.

The descriptor is never authoritative. Before use, the candidate runtime validates its recorded workspace ID/root against normal workspace persistence metadata. Invalid/malformed/stale descriptors are skipped and diagnosed.

Deleting every `discovery.json` loses no sessions, memory, decisions, or other semantic state.

An aggregate discovery database is explicitly out of scope for v1. Scan the bounded descriptors first; add an index only after measurement shows the scan is a bottleneck.

## Repository identity

`ContextAnchor::Repository.canonical_remote` and descriptor repository remotes use one normalized identity.

Normalization rules:

1. trim whitespace;
2. HTTPS/SSH URLs lowercase scheme and host;
3. strip default ports;
4. strip trailing `/` and one trailing `.git`;
5. scp-like `user@host:path` normalizes to `ssh://host/path`; username is not repository identity;
6. local-path remotes canonicalize to an absolute filesystem path;
7. an empty host/path is invalid.

Live validation reads repository identity through the selected workspace provider. A remembered path alone never proves repository identity.

## Discovery terms

Descriptors contain retrieval terms, not prompt text.

Normalize terms by:

1. Unicode lowercase;
2. split on whitespace and ASCII punctuation;
3. drop empty or one-character tokens;
4. cap each token at 64 bytes on a UTF-8 boundary;
5. retain at most 256 terms ordered lexicographically.

Terms may come from validated repository names, project/task keys, and prompts whose context selection was later confirmed. Raw transcript text is not copied into descriptors.

## Workspace candidate ordering

For each valid descriptor compute this deterministic ordering tuple, descending except final ID:

```text
exact repository identity mentioned/matched       bool
number of matching normalized recall terms        u16
confirmed_recoveries capped at 20                 u8
observation_count capped at 20                    u8
last_observed_at                                  u64
workspace_id                                      ascending tie-break
```

This ordering chooses at most three workspaces to prepare/query. It does **not** by itself authorize automatic switching.

Recency alone can make a workspace worth checking, but can never make it the automatic winner.

## Memory association service

`phenix.memory` additionally exports:

```text
memory.context@1
```

using the SDK types in `contracts/memory_context.rs`.

It remains part of the same `phenix.memory` plugin and durable namespace. Do not add a second memory plugin/store for associations.

### Canonical association state

Internally persist:

```rust
struct MemoryContextAssociationState {
    association: MemoryContextAssociation,
    observations: u32,
    confirmed_uses: u32,
    last_confirmed_at: Option<u64>,
}
```

Identity is `(memory_id, anchor)`.

The persistence key is:

```text
context/association/<memory-id>/<anchor-sha256>
```

`anchor-sha256` is SHA-256 of the canonical encoded `ContextAnchor` value used by the memory plugin. The digest is lowercase hex.

Maintain rebuildable secondary indexes:

```text
index/context/by-memory/<memory-id>
index/context/by-anchor/<anchor-sha256>
```

Counts saturate at `u32::MAX`.

### Associate

`Associate` requires:

- referenced `MemoryRecord` exists;
- at least one exact source reference;
- source references pass existing normalization/deduplication;
- anchor strings are non-empty and within existing structural limits;
- `observed_at >= memory.created_at`.

For an existing `(memory, anchor)`, merge exact source refs, increment `observations`, and set `observed_at = max(old, new)`. Do not create duplicates.

For a new pair, store `observations = 1`, `confirmed_uses = 0`.

### ConfirmUse

`ConfirmUse` requires an existing association. It increments `confirmed_uses`, sets `last_confirmed_at = max(old, at)`, and never creates a new association.

The caller invokes it only after the anchor survives live validation and is actually selected for the request.

Confirmed use is ranking evidence. It does not change memory freshness or authority.

## Memory recall

`MemoryContextRecallRequest` rules:

```text
scopes: non-empty
needs: 1..=4
limit: 1..=20
prompt: <= 4096 UTF-8 bytes
known anchors: <= 32
```

Recall uses existing memory records/freshness first.

Hard filters run before ranking:

1. scope/authority;
2. temporal validity;
3. supersession;
4. `MemoryFreshness::Current` only for automatic recovery;
5. anchor compatibility with `known` context;
6. resolvable exact provenance.

`NeedsValidation` and `Historical` records are not automatic recovery candidates. Revalidation may run separately, then a later recall can see the record as current.

Candidate generation combines:

- exact anchor secondary-index matches;
- exact source/resource matches;
- existing lexical memory recall bounded to 100 records;
- associations attached to those records;
- optional semantic candidates only when deterministic candidates do not produce a decisive result.

Optional reranking may reorder candidates only **within the same deterministic evidence class**. It cannot promote stale/out-of-scope memory or a weaker class over a stronger class.

## Memory evidence classes

Each candidate exposes signals, while selection uses this fixed class:

```text
4: ExplicitLink | ExactAnchor | ExactSource
3: ConfirmedUse
2: Lexical
1: RepeatedObservation
0: Recency | Semantic only
```

Within one class order by:

```text
confirmed_uses desc
observations desc
last_confirmed_at desc (None last)
last_observed_at desc
memory_id asc
anchor canonical ordering
```

The public result does not expose an implementation-specific float score.

## Automatic winner rule

After at most three workspace runtimes are prepared and queried, discard every memory candidate that fails live anchor validation.

Automatic handoff is allowed only when:

- exactly one valid workspace has a best candidate with evidence class >= 1; or
- the best workspace's evidence class is >= 2 and strictly greater than every other valid workspace's best class.

A class-0 result never causes automatic handoff.

A tie at the winning class is ambiguous. Return the candidates for user clarification; do not use recency as a hidden tie-break for execution.

Within the already-selected current workspace, the same rule chooses a memory association, except no workspace handoff is needed.

## Live anchor validation

Validation occurs through current owners:

| Anchor | Validation |
| --- | --- |
| `Workspace` | runtime workspace identity equals anchor and caller retains access |
| `Repository` | live normalized remote equals anchor; optional workspace ID also matches |
| `Path` | canonicalize inside authorized workspace; reject escape/missing path |
| `Project` | planning/project owner resolves the key as current or explicitly historical |
| `Task` | task/repository-work owner resolves the key and current state |
| `Session` | session owner resolves the accessible durable session |
| `Resource` | named service resolves the resource under caller authority |

Live state wins. A remembered PR number, branch, path, or status is never used as current state without its owning service confirming it.

A failed validation rejects that association for the attempt. When the failure proves underlying evidence changed, feed the change through existing memory freshness/revision/conflict paths.

## Recovery coordinator

`phenix-harness::ContextRecoveryCoordinator` performs this exact algorithm for a new root conversation:

1. construct `ContextRecoveryState` from current workspace/repository/session/resource facts;
2. apply the cold-path deterministic gate;
3. if needed, call `phenix.context-recovery@1`;
4. if `Sufficient`, stop;
5. query current workspace `memory.context@1` first;
6. live-validate current-workspace candidates;
7. if one wins, `ConfirmUse` and stop without workspace discovery;
8. otherwise scan/rank workspace descriptors;
9. prepare at most three candidate workspace runtimes in order;
10. query each candidate's `memory.context@1` with the same needs;
11. live-validate returned anchors in that candidate runtime;
12. apply the automatic winner rule;
13. if one wins, swap the bootstrap runtime, call `ConfirmUse`, and continue session creation there;
14. if ambiguous, return candidate labels/roots for user selection;
15. if none wins, fall back to ordinary discovery.

The original user prompt is preserved byte-for-byte through this process. It is submitted exactly once to the eventual root execution.

The coordinator performs at most one automatic workspace handoff for a user request. After a handoff it does not recursively search another workspace.

## Existing sessions

Automatic cross-workspace recovery is disabled once a durable session has history.

A user may explicitly start/select another workspace/session through ordinary product actions, but memory never silently migrates an existing conversation.

This rule keeps session history, workspace Store Binding, leases, authority, and provenance coherent.

## Discovery descriptor updates

Write/update the current workspace descriptor after:

- successful workspace startup/identity validation;
- validated repository identity change;
- new durable project/task anchor association;
- confirmed context recovery use.

`observation_count` increments for validated workspace observations. `confirmed_recoveries` increments only after `ConfirmUse` succeeds.

New normalized recall terms from a confirmed recovery are merged into the bounded term set.

Descriptor update failure is diagnostic only after canonical workspace/memory state already succeeded.

## Primitive agent adapter

Ship a small application package/executable:

```text
phenix-context
```

It reads the same workspace discovery descriptors. It never reads another workspace's memory database directly.

Command:

```text
phenix-context resolve \
  --cwd <path> \
  --need <workspace|repository|project|task|session|resource> \
  --query <text> \
  [--prompt <text>] \
  --json
```

Result is one of:

```json
{"status":"current"}
{"status":"switch","workspace_id":"...","root":"...","repository":null}
{"status":"ambiguous","candidates":[...]}
{"status":"not_found"}
```

The CLI performs descriptor ranking and live root/workspace/repository validation. It does not perform semantic task recall until the agent has moved into the selected workspace and can use normal `phenix.memory` there.

Default primitive-agent skill rule:

```text
If the request requires project/repository/task context and the current environment does not identify it, call phenix-context before asking the user. Validate a returned target. Change directory only for status=switch. Then inspect live repository/task state. Do not call it when current context already identifies the work.
```

The primitive adapter may `cd` because its shell process owns its working directory. That is not equivalent to native Phenix runtime Store Binding replacement.

## Model cost policy

The only mandatory model call in recovery is `context.identify_needs`, and the cold gate avoids it for established context.

Route it to the cheapest configured model that reliably returns the typed decision. The plugin carries no provider-specific model name.

Memory recall itself is deterministic by default. Embedding/reranking are optional and not required for correctness.

## Observability

Emit/record enough structured state to explain recovery without logging complete private memory content:

```text
recovery gate: skipped | classifier
classifier decision / need kinds
current-workspace recall attempted
workspace descriptor scan count
workspace runtimes prepared
candidate memory IDs + evidence classes/signals
validation accepted/rejected + stable reason code
handoff current/switched/ambiguous/not_found
selected workspace ID
selected memory ID
```

Stable IDs/signals are sufficient. Do not duplicate memory content in telemetry.

## Failure behavior

- recovery provider unavailable/fails: ordinary discovery;
- memory disabled/unavailable: ordinary discovery;
- malformed descriptor: skip + diagnostic;
- descriptor scan bound exceeded: ordinary discovery + diagnostic;
- candidate runtime preparation fails: keep original runtime, try next candidate;
- memory candidate stale/non-current: reject;
- live anchor validation fails: reject;
- automatic winner rule not met: user clarification or ordinary discovery;
- `ConfirmUse` write fails after valid selection: keep the valid current resolution, record diagnostic;
- discovery descriptor update fails: canonical state remains valid;
- candidate handoff fails: original bootstrap runtime remains active;
- no failure path broadens authority or performs hidden `cd`/Store Binding mutation.

## Implementation layout

```text
rust/crates/phenix-sdk/src/contracts/context.rs
  ContextAnchor
  ContextNeed
  ContextRecovery*

rust/crates/phenix-sdk/src/contracts/memory_context.rs
  MemoryContextAssociation
  MemoryContextRecallRequest
  MemoryContextCandidate
  MemoryContextInterface

rust/crates/phenix-plugin-context-recovery/
  component.rs
  implementation.rs
  classifier.rs
  tests.rs

rust/crates/phenix-plugin-memory/
  context_associations.rs
  context_retrieval.rs
  existing persistence/freshness integration

rust/crates/phenix-harness/src/
  context_recovery.rs
  workspace_discovery.rs
  runtime_manager.rs

rust/crates/phenix-context/
  main.rs

config/phenix/skills/
  context-recovery/SKILL.md
```

Do not put semantic memory, cross-workspace product policy, or repository validation into `phenix-core`.

## Implementation order

1. Finish neutral context/memory-context SDK contracts.
2. Add `phenix-plugin-context-recovery` with strict routed classifier parsing and cold-gate regressions.
3. Export `memory.context@1` from `phenix.memory`; implement association persistence, indexes, counts, and deterministic recall.
4. Add descriptor read/write/scan logic with atomic permissions, bounds, and validation.
5. Add cloneable Harness product recipe + bootstrap-only `WorkspaceRuntimeManager` candidate preparation/swap.
6. Add `ContextRecoveryCoordinator` with current-workspace-first recall, three-workspace bound, live validation, and winner rule.
7. Wire default new-root-session entrypoints through the coordinator before session creation.
8. Add descriptor publication from validated workspace/repository/memory events.
9. Add `phenix-context` and the default primitive-agent skill.
10. Add optional embedding/reranking augmentation without changing evidence-class precedence.
11. Reconcile PR body against exact HEAD and run Source, Rust, Product, Docs, Maintenance.

## Required regressions

- identified current repository + `"work on prs"` performs no classifier call, descriptor scan, or memory recall;
- durable resumed session performs no automatic cross-workspace recovery;
- explicit resource input performs no recovery;
- `~/Downloads` + `"what is 2+2"` classifier returns sufficient and memory stays cold;
- `~/Downloads` + `"work on prs"` produces repository/task needs;
- current-workspace memory is checked before cross-workspace discovery;
- descriptor deletion loses no canonical memory;
- descriptor is mode `0600` and atomically replaced;
- malformed/oversized descriptor is skipped;
- scan stops at 4096 descriptors and candidate preparation at three runtimes;
- stale root/workspace identity cannot win;
- candidate runtime failure leaves original bootstrap runtime unchanged;
- active/bound runtime rejects automatic handoff;
- repository remote mismatch rejects remembered repository association;
- path escape or missing canonical path rejects path association;
- `NeedsValidation` and `Historical` memory cannot auto-select;
- optional embeddings absent still resolves deterministic candidates;
- semantic-only/recency-only candidate cannot auto-switch;
- equal winning evidence classes produce ambiguity rather than recency selection;
- successful handoff submits original prompt once in target runtime;
- `ConfirmUse` increments only existing validated association state;
- live PR/task state overrides remembered content after handoff;
- memory/recovery failure falls back to ordinary discovery;
- primitive CLI returns `switch` only after live target validation;
- disabling context-recovery/memory preserves normal non-memory execution behavior.

## Non-goals

- memory retrieval on every turn;
- cross-workspace global transcript search;
- a canonical global memory database;
- moving an existing durable session between workspaces;
- mutating a live runtime's Store Binding;
- using recency alone to select a workspace;
- remembered authority or permissions;
- memory-owned repository/PR/task truth;
- mandatory embeddings/vector database;
- user persona/preference synthesis as part of context recovery.
