# C6 Restart And Persist

Status: user-approved policy, public integration proof added on 2026-11-16.
Independent final review supports the entire approved contract; exact-head hosted
CI remains pending. No production code,
public API, dependencies, or existing acceptance ledgers are changed by this slice.
No demonstrated production bug was found in the exercised paths.

## Accepted Contract

The original C6 finding in `fbp-issue-ledger-20261111.md` requires an explicitly
accepted restart/disposal policy, not necessarily live-state migration. The user
has explicitly accepted **Restart And Persist** for this work:

- Replacement constructs fresh process-private state. The host does not export,
  serialize, import, or copy arbitrary live `Process` fields into the successor.
- Continuity belongs only to explicitly granted, versioned durable resources.
  The host chooses resource identity, physical placement, schema, and grants.
  Compatible successors use the same durable resource, not a replacement cache.
- Under the separately approved C3 archival policy, retained generations and
  owned jobs keep their exact old process state and code leases. Publication
  retires producers; it does not revoke explicit archival execution or captured
  storage grants. Disposal follows the last executable owner's release, not the
  instant of publication.
- An incompatible schema must be rejected rather than silently converted. This
  proof does not define generic migration. A host must separately design and
  approve any future application-specific schema transition.
- Candidate rejection preserves the host's active identity and plugin inventory.
  It is not a transaction over arbitrary external effects from preparation,
  activation, binding, factories, or application callbacks.
- A shared grant permits archival and successor writers. There is no exclusive
  writer, revocation, fencing, effect rollback, or exactly-once promise.

This supersedes the policy-pending interpretation for this C6 slice, not unrelated
C3/C7 architecture work or broader production acceptance.

## Constraints And Alternatives

The change owns only `packages/phenix-kernel/tests/restart_persistence.rs` and this
note. It reuses the existing test support and `tempfile` dev dependency. SQLite
files live inside a test-owned temporary directory. There is no production edit,
new inspector, test framework, GraphStore, or StorageGraph dependency in the proof.
The workspace already has a StorageGraph dev dependency; this test does not use it.

| Alternative | Disposition |
| --- | --- |
| Copy/export/import arbitrary live process state | Not selected. Couples implementations and requires an additional compatibility, ownership, and in-flight-work protocol. |
| Reset everything, including durable data | Not selected. Loses explicitly approved durable continuity. |
| Share an in-memory `Arc` as persistence evidence | Insufficient. Cannot establish backend persistence or schema/revision continuity after all owners drop. |
| Revoke archival work or promise a sole successor writer | Not selected. Contradicts the accepted C3 contract and would require new fencing authority. |
| Treat failed candidate cleanup as rollback | Rejected. A preparation callback can commit an independent external operation before returning an error. |
| Restart private state and reopen granted SQLite resources | Selected. Uses existing public lifecycle, schema checks, session grants, typed cells, and CAS without a new mechanism. |

## Public Integration Evidence

Both tests use the real `KernelHost` with catalog staging, `prepare`, and `publish`.
The test process has an ordinary private integer initialized to zero by its
factory. On each input it increments that private integer, reads the durable
singleton through an actual scoped `ReadCell<Counter>`, and adds its private
integer to that durable value using scoped `Kv<Counter>::compare_exchange`.
`StorageKv` in the test is only an import alias for this existing `Kv` type.
The shared event log is observation-only; no process restores state from it.

### Restart, Continuity, And Disposal

`public_host_restarts_private_state_but_persists_granted_versioned_cells_and_archival_jobs`
combines the previously separate generation and scoped-storage evidence:

1. Stage, prepare, and publish plugin revision 1 at host epoch 1. No process write
   occurs before publication. Two executions produce private counts 1 and 2 and
   durable values 1 and 3.
2. Open a **new `Registry` and SQLite backend** on the same file, register the
   compatible schema, and inject a newly restricted `Storage` view into staged
   revision 2. Both readers see durable value 3 and its exact revision token.
   Preparing this replacement does not run the counter or change the durable cell.
3. Retain an owned old-generation activation job, then publish epoch/revision 2.
   The successor's first execution has private count 1, reads durable value 3,
   and writes 4. This is restart plus continuity, not live-state transfer.
4. Run the retained old job. Its private count is 3 and it writes 7 through its
   still-granted old backend connection. Delivering its completion to the successor
   returns `HandoffError::ForeignCompletion` and preserves the completion; the old
   session accepts it. Job completion authority remains generation-qualified even
   though storage authority intentionally shares one durable resource.
5. Explicit old execution writes 11 with private count 4. The successor independently
   advances to private count 2 and writes 13. A stale expected revision conflicts
   without replacing 13. Both old and new restricted views retain their denials.
6. Extract another old job and drop the last old `Generation` handle. The job can
   still execute old code and private count 5, writing 18. Its completion retains
   the old process and plugin until dropped; process disposal precedes plugin
   disposal. This is deliberately allowed archival writing, not an exclusivity bug.
7. Drop **all** hosts, generations, jobs, completions, views, and registries. Open
   another registry/backend and recover value 18 with the exact durable revision.
   Create-if-absent CAS conflicts; CAS with the recovered expected revision succeeds
   and produces value 19 with a new revision. The successor's private state and
   plugin also have observed disposal events.

The durable singleton identity is project `project-one`, class `State`, name
`phenix.c6/counter`, mapped by the trusted host to dataset `store://c6/counter`,
key `value`. Its registered schema is name `phenix.c6.counter`, revision `1`,
structure `counter:i64-le`, codec `i64-le`. `ResourceId` does not itself contain a
schema version; schema identity is checked separately by Storage. Row revision
tokens are a further, distinct expected-write condition, not a schema identifier.

### Grants And Rejection

The fixed host policy grants the counter dataset Read+Write, the reader Read only,
and the writer Write only. The plugin receives only the restricted counter view,
never a registry. `assert_grants` runs before replacement, on both retained and
successor views, and after a full reopen:

- Write-only ordinary/revisioned reads and `ReadCell` binding are denied.
- Read-only put and CAS are denied. Actual allowed reads and writes succeed in
  the execution/persistence assertions, so this is not an all-denied fixture.
- An actually registered and logically bound unrelated dataset rejects reads and
  writes on every view. Knowing a binding does not confer session authority.
- Delete and Subscribe remain denied. User-scope fallback is absent, and asking
  for SQL through the typed KV resource returns `WrongResourceKind`.

`rejected_schema_and_candidate_keep_active_identity_but_external_effects_are_not_rollback`
checks each persisted schema field independently with a fresh SQLite registry
reopen. Changing the name, schema revision, structure, or codec returns actual
`Error::SchemaMismatch`. Each failure leaves old epoch 1, plugin revision 1,
the same active generation pointer, and the counter's value/revision unchanged.
This schema check is a host storage-provisioning step **before** handing a view to
a candidate; it is not an invented schema validator inside `KernelHost::prepare`.

The same test then stages real reload candidates and rejects each from the public
host preparation binding callback with a deterministic application error:

- In the no-write case, no application mutation is attempted and both the counter
  and separate effect key remain unchanged. This is the tested unchanged-data case,
  not a general side-effect-free validation claim. Registry open/register and
  `ReadCell` binding can perform backend checks; they are not asserted to be no-I/O.
- In the external-effect case, the binding callback performs a real SQLite-backed
  put of `external-effect = 99` on the granted resource and then returns the same
  error. The host still retains the old pointer/epoch/plugin revision, but the write
  survives rejection and another registry reopen. The counter singleton is
  unchanged only because this callback wrote a different key, not because effects
  were rolled back. No candidate process step wrote anything.
- An old activation job extracted **before** all schema/candidate failures still
  runs and completes through the old session afterwards. Further explicit old
  execution succeeds, advancing the durable counter to 6.

## Local Verification

Executed through the Nix development environment with the existing lockfile:

```sh
nix develop -c cargo test --locked -p phenix-kernel --test restart_persistence
nix develop -c cargo test --locked -p phenix-kernel --test generations --test scoped_storage --test generation_retirement
```

The new proof has 2 passing tests. Existing `generations` has 5 passing tests,
`scoped_storage` has 4, and `generation_retirement` has 2. Those existing suites
also cover producer retirement, prepare/start failures, and schema-compatible
storage restart independently. This slice does not replace or weaken them.

The final combined run of all four targets passed all 13 tests. Focused Clippy
(`cargo clippy --locked -p phenix-kernel --test restart_persistence -- -D warnings`)
and owned-file rustfmt checking (`skip_children=true`) passed through Nix.
Whitespace checks passed for both new files. Independent concurrent worktree
changes were left untouched; these checks are local assembled-worktree evidence,
not exact-head hosted CI.

## Nonclaims And Gates

This is evidence for the approved C6 restart/disposal plus durable-continuity
contract, not automatic live-state migration. It does not prove crash/power-loss
recovery, multi-process application coordination, migration between schemas,
exclusive writing, cancellation of native effects, rollback of external work,
arbitrary plugin honesty, or an OS security sandbox. The archival-writer test is a
deterministic sequential schedule, not a universal concurrent-writer proof; CAS
is checked for explicit expected-revision behavior, not claimed as a host fence.
The external-effect counterexample concerns preparation rejection only, not every
possible cancellation/activation/error path.

Independent adversarial source/evidence review found no actionable defects and
supports the whole approved C6 contract. Full assembled workspace/default/no-default,
isolated graph no-default, formatting and strict Clippy checks pass. Final acceptance
remains pending exact committed-head hosted CI; no global closure is inferred from
local tests or the approved policy alone.
