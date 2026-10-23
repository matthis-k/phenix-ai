# Plugin state ownership

Status: implemented ownership invariant; remaining SDK ergonomics are staged.
The M1 ABI, canonical ValueSchema, graph scheduling, and kernel admission
rules remain unchanged.

## Ownership rule

A plugin owns its live state. Ordinary Rust fields are the default. Exported
Phenix values are authorized, versioned observations of selected plugin data,
not an independent copy that another plugin can modify to control the owner.

A selected value may be readable or observable without being externally
writable. Mutations must enter the owner through a declared operation, action,
or checked setter implemented by the owner.

Persistent storage is optional for normal plugin data. Settings and selected
state may be restored during initialization or explicit hydration, and saved
according to the plugin's durability policy. Storage is not the general home
for plugin instance variables.

There are deliberate exceptions. Storage backends, durable session recovery,
event journals, compaction checkpoints, credential stores, and evidence ledgers
own durable records by contract. Their persistence is domain semantics, not an
accidental substitute for internal fields.

## Invariants

1. One plugin is the writer for each owned live value. Read and watch grants
   do not grant mutation authority.
2. An external request cannot mutate an owner-private field by editing its
   published Phenix value. A requested update goes through an owner operation.
3. Observable publication preserves the kernel's typed schema, admission,
   resource authority, revision, generation, and watch guarantees.
4. A returned success must reflect the committed internal state and any
   publication required by the operation's contract. Failed publication cannot
   silently report success while leaving observers with stale state.
5. A durable write and its acknowledgment must satisfy the chosen persistence
   policy. Failed CAS or invalid storage acknowledgment must not overwrite
   plugin-owned state as if the write succeeded.
6. Restoration validates schema, scope, revision, and plugin-domain invariants.
   A restored snapshot is a proposed initial state, not an authority grant.
7. Independent instances sharing durable records must prevent stale writes and
   define how they detect external changes. In-memory caching alone cannot
   replace cross-instance CAS.
8. Graph producers own their outputs. Consumers receive typed results or
   watch a producer's published values. Consumers cannot take ownership by
   writing another plugin's exposed state.
9. Removing durable calls must not remove interruption recovery, replay,
   provenance, exact-account credential protection, or persistence guarantees.

## SDK implementation and mutation policy

The existing `#[get]`, `#[set]`, and `#[watch]` field annotations in
`phenix-macros/src/state.rs` rewrite fields to host-backed `Cell<T>`.
`Cell<T>` creates a host value and leaves only a `ValueRef<T>` in the plugin.
That is not the intended default for a plugin-owned field.

The `#[expose]` owner-held publication path is merged (PR #1108). It
preserves the Rust field and generates typed reference/publication methods.
PR #1114 adds generated candidate-update helpers. The invariants are:

- Preserve the authored Rust field as the internal source of truth.
- Generate the authorized read/watch view and typed publication metadata.
- Separate owner publication rights from externally writable `set` rights.
  An owner-only publish cannot depend on a public `set` grant.
- Stage and validate changes before committing them. Provide a clear failure
  policy for publication and persistence, including rollback or explicit
  reconciliation when atomic commit across systems is unavailable.
- External mutation remains an explicitly provided plugin operation; it can
  validate proposed data with `try_update_FIELD(host, |current| Result<T, E>)`.
  Never generate a directly writable host `set` property for owned state.
- Retain raw host value cells for explicitly shared value-state use cases.
  These are a separate abstraction, not the default meaning of an exposed
  Rust field.

Changing the current `Cell<T>` macro expansion alone is insufficient because
the kernel must still authorize and order publication from the owner. Do not
make host mirrors writable to every consumer just to make publication work.

## Migration inventory

| Plugin | Ownership adjustment | Status |
| --- | --- | --- |
| Basic Agent | Owner-held, read-only published pending Model request | Merged #1109 |
| Advanced Preparer Graph | Own prepared output and invoke selected Basic advance | Merged #1110 |
| Basic Memory | Own session Skills/Context; persist by CAS | Merged #1111 |
| Workspace Selection | Own session selection; restore once, persist by CAS | Merged #1103 and #1113 |
| Basic Session | Own session aggregate; persist restart/reconcile snapshots | Merged #1112 |
| Event Journal | Durable event and observer records remain canonical | Reviewed; retain durable authority (#1107) |
| Compaction Checkpoints | Durable checkpoint lineage and source ancestry remain canonical | Reviewed; retain durable authority (#1107) |
| Evidence Store | Durable evidence ledger and source validation remain canonical | Reviewed; retain durable authority (#1107) |
| Other plugins | Already private-state, stateless or dedicated durable providers | No migration necessary |

## Migration stages and acceptance

- [x] Inventory plugin state and the SDK's old host-backed field behavior.
- [x] Record ownership, external writer and persistence invariants (#1103).
- [x] Introduce `#[expose]` with owner-held Rust state, typed read/watch
      publication, and denied public setters (#1108).
- [x] M1 native authoring fixture validates read-only publication and
      resource-authorized mutation (#1108).
- [x] Basic Agent owns its pending request (#1109).
- [x] Advanced Preparer owns its output and hands off through the selected
      Basic advance operation (#1110).
- [x] Basic Memory, Basic Session, and Workspace Selection separate live
      Rust state from durable CAS/recovery (#1111, #1112, #1113).
- [x] Durable journals, checkpoints and evidence retain persistent records
      as intentional domain state (#1107).
- [ ] Transactional `update_FIELD` / `try_update_FIELD` helpers and native
      rejected-candidate conformance (#1114; verification pending).
- [ ] Close SDK ergonomics (#1104) only after #1114's complete CI evidence.

## Coherence and API limitations

Owned state is private and remains the source of truth during a live instance.
Generated `update_FIELD` helpers require the change closure to produce a
new `T`; no clone of `T` is needed. Validated `try_update_FIELD` rejects
candidates without mutating state or publishing a Phenix value. Owner-only
publication commits the Rust field only after CAS succeeds; no external
property setter is generated.

Regular `#[provide]` operations still own the external mutation contract,
including validation and authorization. Direct assignments to a Rust field
bypass publication, so authors must call the generated owner-update method
for observable changes; macro-level interception of arbitrary Rust field
writes is not promised.

Live session caches do not silently absorb concurrent writes from other
instances. Durable CAS detects stale writers, and a rejected/ambiguous
write invalidates the affected cached entry. Immediate cross-instance
read refresh requires a separate revision-watch/invalidation contract.

The kernel continues to own graph scheduling, admission, cancellation,
generation validity, and resource authority. This work adds no new M1
request opcodes or wire format.
