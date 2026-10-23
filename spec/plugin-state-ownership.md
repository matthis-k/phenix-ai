# Plugin state ownership

Status: proposed authoring invariant. The M1 ABI, canonical ValueSchema, graph
scheduling, and kernel admission rules remain unchanged. Migration is staged.

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

## SDK changes required

The existing `#[get]`, `#[set]`, and `#[watch]` field annotations in
`phenix-macros/src/state.rs` rewrite fields to host-backed `Cell<T>`.
`Cell<T>` creates a host value and leaves only a `ValueRef<T>` in the plugin.
That is not the intended default for a plugin-owned field.

Introduce an opt-in owned-field publication path before migrating dependent
graph implementations:

- Preserve the authored Rust field as the internal source of truth.
- Generate the authorized read/watch view and typed publication metadata.
- Separate owner publication rights from externally writable `set` rights.
  An owner-only publish cannot depend on a public `set` grant.
- Stage and validate changes before committing them. Provide a clear failure
  policy for publication and persistence, including rollback or explicit
  reconciliation when atomic commit across systems is unavailable.
- Generate checked external setters that call plugin-owned mutation logic.
- Retain raw host value cells for explicitly shared value-state use cases.
  These are a separate abstraction, not the default meaning of an exposed
  Rust field.

Changing the current `Cell<T>` macro expansion alone is insufficient because
the kernel must still authorize and order publication from the owner. Do not
make host mirrors writable to every consumer just to make publication work.

## Migration inventory

| Plugin | Ownership adjustment | Status |
| --- | --- | --- |
| Basic Agent | Own pending and prepared run transitions; publish read-only request views | Pending SDK and graph protocol |
| Advanced Preparer Graph | Own its prepared output, publish it, and submit a source-bound handoff | Pending Basic Agent protocol |
| Basic Memory | Own skills and context per session; restore/save declared durable data | Pending; coordinate with memory validation PR |
| Workspace Selection | Retain selected snapshots per authorized session while preserving durable CAS and cross-instance refresh | Compatibility step in this PR |
| Basic Session | Own active session transitions while preserving recovery snapshots and reconciliation | Pending; coordinate with session validation PR |
| Event Journal | Preserve durable append/observer semantics; consider owned indexes only after concurrency review | Review |
| Compaction Checkpoints | Preserve append-only durable checkpoint and source ancestry guarantees | Review |
| Evidence Store | Preserve durable provenance and version rules | Review |
| Other plugins | Already internal-state or stateless, or a dedicated durable backend | No ownership migration |

## Migration stages and acceptance

- [x] Inventory plugin state and the SDK's current host-backed field behavior.
- [x] Record ownership, external writer, and persistence invariants.
- [x] Begin retaining Workspace Selection snapshots in plugin state without
      removing the current cross-instance refresh and durable CAS.
- [ ] Add SDK conformance fixtures for a plugin-owned private field, published
      read/watch view, denied external mutation, and checked owner update.
- [ ] Implement typed owner publication with failure and cancellation tests.
- [ ] Move Basic Agent's published fields onto that API.
- [ ] Move Advanced Preparer's prepared request to its own output and replace
      the cross-plugin write with an owner-checked handoff.
- [ ] Separate Basic Memory state from durable restoration and save logic.
- [ ] Separate Basic Session live transitions from durable recovery data.
- [ ] Review journal, checkpoints, and evidence caches under concurrent writers.
- [ ] Run full M1 contract, graph, cancellation, generation, and persistence
      conformance before declaring the migration complete.

The workspace compatibility step is not the final storage abstraction. It
continues to refresh from durable storage on each entry because another
authorized instance can change a workspace selection. Removing that refresh
requires a documented invalidation or single-writer protocol.
