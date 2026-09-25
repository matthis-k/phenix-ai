# Primitive-agent continuation export

status: implementation-in-progress

Tracks #516 slice 9 and `token-efficiency.md`.

## Gap

Phenix can persist task/context/memory state internally, but it cannot yet project that state into a bounded continuation packet for a remote or primitive agent that lacks the rich Phenix runtime.

## Implementation progress

- [x] Added versioned continuation packet/export contracts with recipient resolver identity, exact/inline evidence, explicit freshness, byte/token budgets, and typed failure outcomes.
- [x] Added canonical content digesting plus ordered delta upsert/remove application bound to the exact base and target packet digests.
- [x] Added sender-side bounded export selection: exact acknowledged bases may use delta; missing/mismatched bases fall back to a full packet; byte and hard-token bounds fail explicitly without truncation.

## Required implementation

- [x] Define a budgeted continuation-packet contract.
- [ ] Include goal, fixed constraints, decisions, blockers, selected fresh memory, and relevant code entities when available.
- [x] Include only exact references validated against the recipient's concrete resolver binding, or inline required evidence; context requires matching resolver binding plus a verified digest.
- [x] Inline bounded required evidence when a reference cannot be resolved remotely; missing fallback fails required items and explicitly omits optional ones.
- [x] Support a deterministic delta packet from an exact prior packet digest.
- [x] Reject mismatched deltas at the receiver; send a full bounded packet when an exact compatible base is unavailable.
- [x] Mark freshness/currentness explicitly for exported items.
- [x] Keep exported packets as projections; import validation returns `ContextSource::Continuation` candidates only and has no canonical task/memory/code/authority mutation path.

## Acceptance

- [x] Export selection respects explicit byte bounds and hard token bounds when an exact tokenizer measurement is supplied; unavailable tokenizer enforcement is typed rather than estimated.
- [x] Every external reference is recipient-resolvable or replaced by bounded inline evidence before packet publication.
- [x] Delta export round-trips against the exact base packet digest; context derives deterministic remove/upsert operations and receiver verification reproduces the target digest.
- [x] A packet for an environment without code intelligence remains useful; goal/blocker/evidence-only packets project successfully without semantic code entities.
- [x] Import never upgrades stale derived facts to canonical truth; freshness is preserved in the serialized continuation item and import yields derived context only.

## Ownership

Context owns the projection. Memory/code/task owners supply typed source data. Transport/client plugins adapt the packet to the remote environment.

## Baseline and dependencies

Checked against `main` at `46aa246361a7`. Reuse existing exact context/checkpoint references and Core `ContentReference`/`ArtifactRevision`. #594/#595 add optional code/memory enrichment; neither blocks basic export of canonical task state and exact file evidence. #592's artifact resolver can supply bounded evidence. Export itself is deterministic and does not require another model.

## Implementation draft

1. Add a context-owned request with execution/checkpoint identity, recipient resolver identity/capabilities, current caller scope, mandatory byte limit, optional hard token limit plus tokenizer identity, and mode `Full` or `Delta { base_packet_digest }`. Typed outcomes distinguish budget exhaustion, unavailable exact evidence, unsupported token bound, stale snapshot, and incompatible packet format.
2. Pin an immutable source snapshot and versioned packet encoding. Assemble goal, fixed constraints, active blockers/decisions, exact changed facts, and selected memory/evidence. Carry source revisions, provenance, coverage, and freshness as of that snapshot. Optional code observations use current paths/revisions when semantic identity is unavailable. The export does not claim live truth after its snapshot.
3. A supported reference scheme alone does not prove recipient resolution. Check concrete resolver scope/access, resource identity, digest, and retention availability. Represent evidence as `ResolvableReference` with its resolver binding or `InlineExact` bytes. Inline mandatory evidence when remote resolution fails; omit optional evidence in stable priority order with omission metadata. A summary cannot stand in for required exact bytes. Permission revocation still applies when a reference is resolved.
4. Count the complete rendered packet, including metadata, references, inline encoding, and transport wrapper that enters recipient context. Enforce bytes directly. A hard token limit requires an exact tokenizer for the recipient rendering or a documented conservative upper bound; an estimate or byte cap is not that guarantee. With only an estimate, report it and return `UnsupportedTokenBound` if the caller requires enforcement.
5. Compute the exact packet digest using the shared content digest implementation and deterministic versioned encoding. Delta means ordered upsert/remove operations over stable item IDs in the exact prior packet. It binds base digest, target digest, recipient/scope compatibility, and schema version. A checkpoint ID alone cannot identify a recipient-specific packet.
6. The sender uses delta only when the recipient acknowledges that exact compatible base. Missing/wrong/evicted base causes the sender to build a full packet under the same bounds. The receiver rejects a delta whose actual base mismatches; that rejection requests full export instead of attempting fuzzy application. If the full packet cannot fit, return exhaustion.
7. Recheck authority and source snapshot validity at publication. If mutable source owners cannot provide a consistent snapshot, use revision checks and a bounded rebuild or `StaleSnapshot`. Appends after a completed immutable snapshot do not by themselves invalidate historical export. Retain referenced artifacts for the packet's declared retention lifetime.
8. Imported packets are untrusted derived context. Local scope and live verification determine admissibility and freshness; packet claims cannot restore authority or create canonical decisions. External text stays evidence with provenance, not a new instruction channel.

## Fixture sequence

- Matching scheme with missing recipient resource causes inline evidence or a typed failure.
- Unicode plus encoding/wrapper overhead is included in limits; no tokenizer plus hard token limit rejects.
- Applying a delta to its exact base produces the target packet/digest, including removals.
- Missing base triggers a full packet at the sender; mismatched receiver base never applies partially.
- Revoked access, deleted artifact, stale snapshot, and schema mismatch remain explicit.
- Mandatory content that exceeds the full-packet budget fails without truncation.
- Imported freshness/authority claims cannot override live local state.

## References

- [Export semantics](../token-efficiency.md#primitive-agent-export), [exact refs](../context-catalog.md#exact-references), [retention and commit](../context-compaction.md#admission-and-commit-boundary), [memory recall](../plugin-memory-freshness.md#recall).
- [Exact bytes, digest, and locator contract](https://github.com/matthis-k/phenix-ai/blob/46aa246361a7bfc1ea477d08ab40c204acc89a54/rust/crates/phenix-core/src/content_reference.rs).
