# Lazy tools and compact observations

status: implementation-in-progress

Tracks #516 and the tool strategy in `token-efficiency.md`.

## Gap

The runtime now has typed tool schemas/results, per-step tool budgets, structural provider conversion, and exact context references. It still needs end-to-end catalog loading, exact process capture, and compact observation admission. Existing artifact/read-reuse services must be wired into that path.

## Implementation progress

- [x] Artifact content identities are bounded digest references independent of payload size.
- [x] Workspace process capture now reports stdout/stderr completeness explicitly; truncation can no longer masquerade as exact output.
- [x] Process capture computes full-stream byte counts and content digests before truncation, so bounded views retain a stable identity even before artifact persistence is wired.
- [x] Workspace process capture persists exact stdout/stderr artifacts before bounded model projection and reports typed references or explicit persistence errors per stream.
- [x] The tool service exposes bounded, revision-bound descriptor search with schema identities and selected schema loading; stale cursors and catalog revisions fail closed.
- [x] The agent loop accepts conflict-checked schema activations from an authorized tool executor and projects newly activated descriptors only into the following model turn, enabling portable deferred loading without replaying the session.
- [x] Usage planning now makes eager schemas the deterministic default: all allowed tools are initial and bounded by `max_tool_schemas`; deferred initial/expandable sets require the explicit `tools.deferred_schemas` routing capability.

## Required implementation

- [x] Expose authorized tool descriptors through a searchable catalog.
- [ ] Load full schemas only for the selected task-relevant tool set.
- [x] Preserve a deterministic eager fallback for providers without deferred tool support.
- [ ] Define one typed tool observation with compact model view, exact source/artifact reference, content identity, and invalidation metadata.
- [x] Promote large raw outputs to artifacts before collapsing their model view.
- [ ] Reuse unchanged observations only when the tool declares safe invalidation semantics.
- [ ] Run deterministic filtering/joining/aggregation outside the frontier-model context.
- [ ] Keep tool call/result groups intact across pruning and compaction.

## Acceptance

- [ ] Inactive tool schemas do not enter a deferred-capable model request.
- [x] Large Bash/test/compiler output can enter context as a bounded view plus recoverable exact reference.
- [ ] Repeated unchanged observations avoid reinserting the full payload.
- [ ] Volatile shell output is never reused merely because command text matches.
- [ ] Disabling lazy/result reduction restores a bounded eager path or typed exhaustion.

## Ownership

Tool plugins own semantic parsing and invalidation. Artifacts own exact payloads. Context owns admission. Core gains no tool-specific compression policy.

## Baseline and dependencies

Checked against `main` at `46aa246361a7`. Reuse `StepPlan.tools`, `ContentReference`, and the artifact plugin's `RecordRead`/`LookupRead` dependency checks. They already implement parts of exact retention and reuse. `phenix-plugin-command-toolbelt` inventories executables; process capture belongs to the workspace/process provider. Process capture now persists the exact raw stdout/stderr artifacts before constructing the bounded UTF-8-lossy model view; failed artifact persistence is explicit per stream. Coordinate schema epoch changes with #591 and usage attribution with #599.

## Implementation draft

1. Add authorized descriptor search and schema-load operations to the callable catalog. Descriptor fields: callable ID, description, input/output type identity, catalog revision, and requirements visible under caller scope. Search is bounded and paginated with a revision-bound cursor. Load validates catalog revision, authority, and the plan's expandable set again. Invocation independently checks current authority.
2. Keep discovery/load and artifact expansion in the required bootstrap tool set and count their schemas in `max_schemas`. Required tools stay reachable. Portable loading changes the next request's schema set even without provider-native deferred tools. Fixed-tool sessions use a bounded eager set; reset/replay is allowed only when acknowledged by that adapter. Insufficient schema capacity returns a typed error.
3. Define a tool observation with call-occurrence ID, status/exit outcome, typed semantic view, exact `ContentReference`, completeness, source revision, and optional declared reuse dependencies. Keep occurrence identity separate from content deduplication. Use a bounded digest-based locator: the current artifact service embeds hex-encoded payload bytes in its legacy ID, which cannot be a compact model-facing reference. Migrate that owner's IDs/references to the shared content-digest scheme instead of serializing the old ID into prompts. A reuse key includes provider/implementation/configuration and scope; arbitrary shell commands are non-reusable.
4. Capture stdout/stderr as raw byte streams before decoding or truncation. Spool under the process owner and persist exact artifacts before admitting a compact view. Preserve stream identity and exit/signal outcome; do not invent interleaving. Use bounded memory and explicit artifact quotas. A capture/quota failure marks output incomplete and cannot claim recovery of discarded bytes or rerun a command automatically.
5. Parse known command formats in their tool adapters using structured producer output where available. Unknown formats use a bounded raw view plus exact reference. Store per-invocation provenance independently even when an artifact is reused. Expansion rechecks authorization and digest; retain referenced artifacts while active projections/checkpoints need them.
6. Context admits only the compact view and references. Ordinary filtering/joining runs through authorized tools/processes and returns the same observation type. Reuse and compaction preserve call/result groups. Explicit reduction IDs link later recovery work; matching bytes alone do not prove reacquisition.
7. Separate toggles for schema deferral, parsing, and reuse. Disabling them restores a correct bounded path or typed exhaustion; it does not guarantee every full payload fits.

## Fixture sequence

- Discovery cannot reveal unauthorized IDs; stale cursor/schema load rejects with a refreshed catalog revision.
- A provider without native deferral can load portable schemas on the next request; fixed sessions report required-tool unavailability.
- Binary output exceeding the old capture cap round-trips byte-for-byte through references; stdout/stderr remain distinct.
- Artifact quota/store failure is explicit and leaves no falsely complete reference.
- Identical volatile commands run twice; reusable reads invalidate on scope/provider/dependency change.
- Reused payload retains two call occurrences and their distinct provenance; the encoded reference stays bounded as raw payload grows.
- Disabled reduction with an oversized mandatory payload returns exhaustion, not silent truncation.

## References

- [Tools](../token-efficiency.md#tools), [catalog loading](../context-catalog.md#loading), [artifact invariants](../context-artifacts-pruning.md#invariants), [tool turns](../model-turn-protocol.md#tool-result-turn).
- [Current capture implementation](https://github.com/matthis-k/phenix-ai/blob/46aa246361a7bfc1ea477d08ab40c204acc89a54/rust/crates/phenix-plugin-workspace/src/implementation.rs), [Existing artifacts and read reuse](https://github.com/matthis-k/phenix-ai/blob/46aa246361a7bfc1ea477d08ab40c204acc89a54/rust/crates/phenix-plugin-artifacts/src/implementation.rs), [Shared content reference contract](https://github.com/matthis-k/phenix-ai/blob/46aa246361a7bfc1ea477d08ab40c204acc89a54/rust/crates/phenix-core/src/content_reference.rs).
