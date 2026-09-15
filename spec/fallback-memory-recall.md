# Fallback cross-session memory recall

status: specification-only

The SDK structs on this branch are an initial scaffold. The typed bootstrap
operation scope, selection receipt, coverage/completeness result, and idempotency
fields specified below remain implementation work; their behavior is not yet shipped.

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

Model routing is a service dependency, not a dependency on one provider package.
Use the helper defaults and effective capabilities in `model-routing.md` and
`model-turn-protocol.md`. A concrete root target needs no separately configured
routing profile: resolve the classifier selection from that target or an explicit
helper policy. The implementation replaces the scaffold's profile-only request
field with the existing canonical selection/request context, not a second enum.

The initial SDK scaffold is:

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

Default limits: one classifier attempt, 512 output tokens, 10 seconds for that call,
and 30 seconds for the whole recovery attempt. Lower caller deadlines win. These
limits belong to the pinned product policy and may be configured. Cancellation
ends preparation and leaves the current runtime active. No helper may recursively
invoke recovery, compaction, or delegation. A bounded structural classifier view
uses at most 4096 UTF-8 bytes of prompt evidence and 32 anchors; an oversized input
skips model recovery rather than silently losing explicit constraints. The original
root prompt stays intact.

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

Preparing another workspace constructs a staged candidate from the configured
Harness recipe with its own Store Binding. It does not alter the current runtime.

Activation order:

1. canonicalize and validate candidate root;
2. derive/validate workspace identity;
3. open/prepare that workspace Store Binding;
4. build the same configured Harness recipe;
5. prepare only the declared discovery/memory/live-validation dependency closure;
6. query candidate memory and live repository identity;
7. select candidate only if resolution rules below succeed;
8. prepare normal services for the winner, keeping autonomous effects suspended;
9. commit the selected runtime and root admission receipt;
10. activate normal execution once, then drop unused prepared runtimes.

If preparation fails before admission is recorded, the original bootstrap runtime
remains unchanged. After admission is recorded, recover the
selected runtime from its durable receipt; do not create a second root in the old
workspace. Candidate preparation may read authorized state and perform necessary
store initialization, but it cannot start workers, run repository hooks, load
untrusted workspace code, or dispatch model/tool work. Providers must declare a
preparation mode without autonomous effects. A provider that cannot do so is
unavailable for automatic probing; explicit workspace opening remains possible.

Each recovery attempt carries caller identity, discovery authority, request ID,
deadline, and a shared helper budget before any workspace exists. Access filters
apply before reading descriptors or memory. Candidate scopes attenuate from that
bootstrap authority; stored workspace permissions are never reused as grants.

Serialize new-root admission per bootstrap manager. First persist a prepared
request-ID receipt that pins the selected workspace and execution identity. Create
the root in a target-store transaction with a unique request ID, then mark the
receipt committed. On restart, reconcile a prepared receipt against that same
target store and finish admission; do not select another workspace. A definitive
pre-root failure may abort the receipt explicitly. `ConfirmUse` follows successful
admission and is idempotent by receipt ID plus association identity. The receipt uses product lifecycle
persistence and contains no global semantic memory. External model dispatch has
the separate uncertainty semantics in `model-turn-protocol.md`.

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

Version 1 descriptors and shell switching cover local filesystem workspaces.
Keep them in the Harness adapter, outside the memory and model contracts. A remote
workspace requires its owning provider's locator/open/validation operations; a remote
path is never interpreted on the local machine. Missing remote discovery disables
that optimization without preventing explicit use of a remote backend/workspace.

## Repository identity

`ContextAnchor::Repository.canonical_remote` and descriptor repository remotes use one normalized identity.

Normalization rules:

1. trim whitespace;
2. HTTPS/SSH URLs lowercase scheme and host; remove credentials, user info, query,
   and fragment before persistence or diagnostics;
3. strip default ports;
4. strip trailing `/` and one trailing `.git`;
5. scp-like `user@host:path` normalizes to `ssh://host/path`; username is not repository identity;
6. local-path remotes canonicalize relative to the owning repository root;
7. an empty host/path is invalid.

Live validation reads repository identity through the selected workspace provider. A remembered path alone never proves repository identity.

Transport normalization is not universal repository equivalence. A trusted forge
adapter may establish HTTPS/SSH aliases; generic hosts require exact normalized
identity or an explicit alias. Case-sensitive repository paths remain unchanged.
A local repository with no remote is a valid current Repository anchor using the
workspace owner's identity. For cross-session recovery, treat it as a validated
workspace/path association and detect deletion or replacement through that owner.

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

Discovery returns candidate-set completeness and the excluded count. The scan
bound, three-runtime limit, timeouts, and recall truncation must not make an
unexamined plausible competitor appear absent. When omitted candidates cannot be
excluded by authoritative identity/scope evidence, return ambiguity or incomplete
discovery and keep the current binding. Never claim a globally unique winner from
an arbitrary top-three sample.

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

Replaying the same admission receipt changes no counts. Automatic confirmed use
can strengthen an already relevant match; it cannot make an unrelated query a
match. Repeated observation means distinct canonical source events, not retries.

### Producing associations by default

The normal suite records a small derived episode through existing memory operations
when a user selects a workspace or a durable task/project is bound to it. Its exact
source is the owning service's selection/binding record. Associate the validated
workspace/repository/task anchors with that episode using `memory.context@1`.
Use source-event identity to make event replay idempotent. This requires no model
extraction and does not promote transcripts or compaction summaries.

Install consumers before publishing startup/binding events. Rebuild missed derived
associations from retained owner records within the same scope. Rebuild descriptors
from those associations. A fresh installation with no such sources returns no
memory match and proceeds through ordinary discovery.

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

Match evidence must be tied to each requested need. A valid repository association
does not alone resolve an unrelated task need. Group compatible associations into
a proposed resolution and require coverage of every need needed for the automatic
handoff. Partial coverage may narrow a clarification but cannot silently select a
task. Recall reports truncation, so the coordinator can detect incomplete decisions.

Optional reranking may reorder candidates only **within the same deterministic evidence class**. It cannot promote stale/out-of-scope memory or a weaker class over a stronger class.

## Memory evidence classes

Each candidate exposes signals, while selection uses this fixed class:

```text
4: ExplicitLink | ExactAnchor | ExactSource
3: ConfirmedUse on a current exact/lexical need match
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

Automatic handoff requires live validation, complete required-need coverage, and a
decisive candidate set as defined above. It is allowed only when:

- exactly one valid workspace has a best candidate with evidence class >= 2; or
- the best workspace's evidence class is >= 2 and strictly greater than every other valid workspace's best class.

Class 0 or 1 never causes automatic handoff. Observation counts alone prove use,
not relevance to the present request.

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
7. if a complete current-workspace resolution wins, select it without discovery;
8. otherwise scan/rank workspace descriptors;
9. prepare at most three candidate workspace runtimes in order;
10. query each candidate's `memory.context@1` with the same needs;
11. live-validate returned anchors in that candidate runtime;
12. apply the automatic winner rule;
13. if one wins, commit root admission in that runtime, then record `ConfirmUse`;
14. if ambiguous, return candidate labels/roots for user selection;
15. if none wins, fall back to ordinary discovery.

The original user prompt is preserved byte-for-byte through this process. It is submitted exactly once to the eventual root execution.

The same admission/confirmation rule applies to a current-workspace winner. Exactly
once here means one durable root admission per request ID, not guaranteed one-shot
delivery to an external provider after a network failure.

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

The CLI calls the shared Harness resolver through an existing application/service
adapter. That resolver performs scoped candidate preparation, memory recall,
need coverage, and live validation using the same native winner rule. The CLI does
not implement a second ranking algorithm or open memory databases itself.

A descriptor-only fallback can list plausible candidates but returns `ambiguous`
or `not_found`. It may return `switch` only for an explicit uniquely identified and
live-validated workspace target; observation counts or recency never suffice.
Known `--need` input bypasses the model classifier, not validation or authority.

Default primitive-agent skill rule:

```text
If the request requires project/repository/task context and the current environment does not identify it, call phenix-context before asking the user. Validate a returned target. Change directory only for status=switch. Then inspect live repository/task state. Do not call it when current context already identifies the work.
```

The resolver returns a target; the invoking agent/shell changes its own directory
after validating `switch`. A child CLI process cannot change its parent's working
directory. This is separate from native Phenix runtime admission.

## Model cost policy

The only model call in default recovery is `context.identify_needs`, when a compatible
helper is available and the cold gate cannot establish context. Explicit needs from
the CLI bypass it. Ordinary work remains possible when that helper is unavailable.

Use the inherited selected deployment by default. A configured helper policy may
choose a cheaper compatible target. The plugin carries no provider-specific name.

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
   Include deterministic association production, idempotent admission/confirmation,
   candidate-set completeness, and preparation without autonomous effects in their
   owning slices before enabling automatic recovery in the normal suite.
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
- fresh install creates reusable associations from canonical bindings without an LLM;
- classifier and recall timeouts preserve the current runtime and original prompt;
- candidate preparation starts no worker/hook or untrusted repository code;
- unrelated confirmed use and repeated observations cannot win;
- unresolved needs or unexamined plausible competitors prevent automatic switching;
- native and CLI resolution apply the same evidence/coverage rule;
- retry/restart admits one root and confirms each association once per receipt;
- credential-bearing remotes never leak into descriptors or diagnostics;
- a repository without remotes still takes the current-repository cold gate;
- a concrete target works without configuring a second routing profile.

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
