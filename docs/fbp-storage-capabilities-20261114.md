# Independent Storage Capabilities

Status: implementation design adapting WIP #1257 onto the native FBP campaign at
`a2d7b4d388bae7eddda61e09fad2cec7d88d9d67`. Historical acceptance of #1257
at `1cb121c4` does not certify this adaptation. No graph scheduler, native loader,
SQL facade or storage engine is added.

## Phase 0: Actual Defects

Scoped `Kv::get` currently returns revisioned data; every scoped read therefore
requires CAS in resource planning. Plain/read-only backends cannot serve otherwise
authorized readers. Backend offers omit atomic batch support, so a CAS-capable
backend can pass planning but fail later when bound to atomic cells. Cell binding
currently preflights BestEffort even though its public read requires an atomic
snapshot. These are observable capability-contract bugs, not missing benchmarks.

Preserve existing `ResourceId`, schemas, per-plugin host approvals, mount
reservations, path-alias rechecks, SQLite authorizer/migrations, revisions,
subscriptions and exact provider leases. Storage remains independent from Graph
and Kernel; native graph adapters consume the same storage library.

## Phase 1: Alternatives And Contract

- **No change:** no migration, but preserves Read-to-CAS conflation and late cell
  capability failure.
- **Small flag workaround:** add batch support or an unversioned-read switch.
  Rejected because it leaves duplicate read modes and the incorrect implication.
- **Selected correction:** plain scoped `get -> Result<Option<T>>`, explicit
  `get_revisioned`, independent optional revisioned/batch/CAS predicates, and a
  requirement set on the existing `ResourceRequest`. Replace the CAS request flag;
  do not retain an alias or a compatibility layer.
- **Broader query abstraction:** rejected. Existing `TypedKv`, `SnapshotReader`,
  `CellSpec`, resource identity and plugin contracts cover the actual use cases.
  Rust's fallible optional lookup, object-safe optional methods and `BTreeSet`
  represent these contracts without a new dependency or trait hierarchy.

Required capabilities are lower bounds on backend support, not exact equality or
permission grants. Kind supplies the inherent KV/SQL requirement. Extra backend
support is accepted. Revisioned and batch reads require Read; CAS requires Write,
not Read. Each declaration must be valid independently; combining two plugins'
requests never combines their permissions or changes logical resource identity.

`CellSpec::request(operations)` derives `AtomicReadMany` only when Read is present.
Binding a read cell uses the existing AtomicWithinMount validator without reading
payloads. Atomic snapshots need batch support, not single-key revisioned reads or
CAS. BestEffort prefers a batch when available and otherwise requires revisioned
single-key reads. Support predicates are authoritative and rechecked before the
affected runtime payload calls; built-in memory/SQLite backends opt in explicitly.

Prepare validates demanded advertised support with zero provider-factory opens.
It creates no stores, registers no datasets and performs no migrations. Existing
SQLite schema preflight may still open an existing database read-only. Activation
validates demanded actual interfaces/predicates before publishing views. It cannot
prove that trusted implementations honor atomicity or undo factory/durable effects.

## Phase 2: Independent Adversarial Review

An independent source review rejected underspecified predicate semantics, an
unconditional RevisionedRead dependency for batch-only snapshots, unconditional
batch requirements for write-only cell declarations, and the literal claim that
prepare performs no file opens. The contract above incorporates those corrections.
It also requires preserving mount reservations, alias rechecks, authorization
before lookup/codec calls, backend-before-provider destruction and old-view
usability without claiming rollback of shared durable data.

Acceptance falsifiers:

- Plain-only backend reads populated scoped values without revision/batch/CAS calls.
- Revision-only backend serves explicit revision reads but rejects atomic cell binding.
- Batch-only backend with neither revisioned single reads nor CAS serves populated
  atomic cells, duplicate keys and absence; BestEffort uses the batch path.
- CAS-only backend serves write-only create/update operations, while reads stay denied.
- Missing demanded advertised capability rejects prepare before factory invocation;
  dishonest demanded actual support rejects activation before publishing views.
- Extra support is accepted; invalid kind/operation requirements reject collection;
  repeated requirements are idempotent; independent matching-schema resource
  declarations share identity but never permissions, regardless of registration order.
- Runtime capability loss fails explicitly before payload access; errors are not
  converted to absence. Existing snapshot, revision, persistence, SQL authority,
  reactive invalidation, lease and candidate failure regressions must remain intact.

## Phase 3 And Acceptance

Capability-specific behavior from #1257 is now adapted without merging its old
Engine-era integration code. Native scoped-storage observers use explicit
`get_revisioned` and declare `RevisionedRead`; writers require only CAS and Write.
Derived cell declarations use the descriptor's conditional request helper. All
callers of the removed CAS flag/helpers are migrated; no compatibility shim remains.

An independent post-implementation review found no correctness defect in the
adaptation and independently passed 122 focused tests. It verified populated
batch-only snapshots without revision/CAS support, write-only CAS, mutable support
loss before payload calls, zero-factory prepare rejection, actual-demand activation
checks, code lease cleanup, and independent matching-schema declarations in both
plugin orders without granting unioned authority. Current mount reservations,
filesystem checks, SQLite transactions and callback hardening are retained.

Scoped and storage-plugin error wrappers also now expose existing typed nested
storage/filesystem causes through `Error::source`. `error_sources` checks the
downcastable cause chain and filesystem error kind; existing structured SQL and
unsupported-capability errors are not flattened into strings.

Local workspace tests, API examples, scoped persistence/derived integration and
strict workspace all-target Clippy pass. Final exact-head hosted CI is the remaining
acceptance gate for this WIP scope. General graph/store separation, arbitrary
trusted-provider semantic honesty, live revocation, external subscription coverage
and cross-mount transactions are not claimed by this correction. Record completed
subproblems separately from whole FBP issue/milestone closure.
