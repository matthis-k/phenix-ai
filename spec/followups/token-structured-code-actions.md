# Structured semantic code actions

status: implementation-in-progress

Tracks #516 slice 7. This is separate from stable code identity: identity answers what code entity is being addressed; structured actions define how agents read and modify it.

## Gap

Phenix has file operations and language-intelligence reads, but no provider-neutral entity read/edit contract with revision-checked transactional writes.

## Implementation progress

- [x] Workspace now exposes a distinct crash-recoverable `CommitBatch` path with strict whole-batch version preconditions, durable operation/intent identity, exact before/after version receipts, idempotent replay, and startup roll-forward of prepared commits. Legacy `WriteBatch` remains explicitly precondition-checked sequential.
- [x] Commit journals are hidden from ordinary workspace read/search paths; shell-capable or external writers remain outside the cooperative atomicity guarantee.
- [x] Added a bounded provider-neutral entity-source read pinned to logical entity/revision, exact workspace revision, and negotiated UTF-8/16/32 position encoding; stale source fails before content is returned.

## Required implementation

- [ ] Add semantic reads for entity body, callers, references, implementations, and changed neighborhood.
- [ ] Add semantic edits for replace body, insert relative to entity, and remove entity.
- [ ] Bind every operation to exact logical entity and source revision.
- [ ] Apply writes transactionally and reject stale source revisions.
- [ ] Require declared syntax/structure validation for syntax-preserving edits; unsupported actions remain explicit textual operations.
- [ ] Keep textual file read/patch as the fallback for unsupported languages/providers.
- [ ] Reuse language providers for facts; do not expose raw LSP transport.
- [ ] Preserve exact diff/artifact evidence for applied edits.

## Acceptance

- [ ] A semantic edit fails rather than applying to a stale entity revision.
- [ ] Successful edits produce exact changed-file evidence.
- [ ] Unsupported language support falls back to file operations without claiming semantic guarantees.
- [ ] Reads use bounded entity neighborhoods instead of whole files when supported.
- [ ] Provider replacement does not change the public Phenix action contract.

## Ownership

Code-intelligence providers implement language mechanics. Phenix owns the portable action contract, revision checks, authority, and evidence.

## Baseline and dependencies

Checked against `main` at `46aa246361a7`. Depends on #594's unambiguous entity/view resolution. The workspace owner exposes `WorkspaceWrite`, `WorkspaceFileVersion`, and `WriteBatch`. Current `WriteBatch` prechecks versions and then calls sequential `fs::write`; it is not a crash-atomic multi-file commit and cannot support the draft's transaction claim alone. Implement the needed commit capability in the workspace owner within this slice, or reject actions requiring it with `UnsupportedAtomicScope`.

## Implementation draft

1. Define tagged request variants for read entity, read relations, changed neighborhood, replace body, insert before/after, and remove entity. Common fields pin logical ID, source/view revision, scope, and output bound. Each edit variant carries only its relevant payload and a durable operation ID. Authentication/authority comes from the caller's execution binding, not a caller-supplied claim.
2. Resolve through the code owner, then authorize each touched file and referenced source. Return distinct missing, ambiguous, stale source, provider changed, and unsupported-operation results. Reads pin an index revision, return coverage plus bounded continuation, and label unavailable semantic facts. Textual file operations remain an explicit separate choice.
3. The adapter prepares exact file replacements with `WorkspaceFileVersion` preconditions. Convert LSP positions to exact byte ranges using the pinned source and negotiated position encoding. Parse the edit target and proposed result; preserve surrounding attributes/comments and declaration boundaries according to the declared action. Syntax validation does not imply type correctness. Unsupported syntax-preserving edits are unavailable rather than silently downgraded.
4. Preparation writes no workspace bytes. Revalidate provider/view generation and all source preconditions at commit. Disk edits accept only workspace-backed evidence; editing a dirty buffer requires an editor-owned commit capability with its document-generation precondition. A disk writer cannot overwrite an unsaved buffer by treating its hash as a disk revision.
5. Strengthen the workspace commit path with staged replacements, serialized cooperating writes, a durable operation/preimage/target receipt, and explicit recovery. Recheck the whole read/write dependency set before first mutation. Recover interrupted commits before exposing them as complete. A failed validation or precondition has zero mutation; a failure after mutation reports/reconciles a pending or partial commit, never an unqualified rollback claim.
6. Publish the actual atomicity boundary. Multi-file OS visibility and uncooperative external writers are not covered merely by holding a Phenix lock. If the requested guarantee exceeds the backend capability, return `UnsupportedAtomicScope` before writes. Stronger backends can supply an isolated workspace/snapshot transaction through the same owner contract.
7. Retry by durable operation ID and exact intended transaction, not merely by observing matching desired bytes. Persist exact pre/post revisions and diff/artifact evidence and return the receipt. A reporting failure after commit must not rerun the edit. Update identity/lineage only from a confirmed commit result.

## Fixture sequence

- Concurrent cooperative edit after preview yields a conflict before mutation.
- UTF-16 positions, multibyte text, comments/attributes, and nested declarations resolve to the intended syntax range.
- Dirty buffer cannot be committed with disk authority/preconditions.
- Inject an I/O failure on the second file: recovery reports the true outcome and never claims untouched workspace.
- Crash after commit but before response returns the same operation receipt on retry.
- Backend without required atomicity rejects before mutation; external-race guarantees are tested only where advertised.
- Applied edits return exact evidence; bounded reads label partial neighborhoods.

## References

- [Structured actions](../token-efficiency.md#structured-code-actions), [language provenance](../language-intelligence.md#documents), [#594](https://github.com/matthis-k/phenix-ai/pull/594).
- [Workspace write and revision contracts](https://github.com/matthis-k/phenix-ai/blob/46aa246361a7bfc1ea477d08ab40c204acc89a54/rust/crates/phenix-sdk/src/contracts/workspace.rs), [Current precheck/sequential write implementation](https://github.com/matthis-k/phenix-ai/blob/46aa246361a7bfc1ea477d08ab40c204acc89a54/rust/crates/phenix-plugin-workspace/src/implementation.rs).
- [LSP 3.17](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/) is the primary reference for positions, document versions, and versioned edits; wire edits must be normalized before workspace mutation.
