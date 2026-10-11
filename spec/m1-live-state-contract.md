# S2 live state and native admission

Status: frozen before implementation
Date: 2026-10-11
Baseline: accepted S1b and A0, PR #775

This addendum implements the live semantics in the M1 RFC. It adds no agent
semantics. M0 package, descriptor, wire, generation identity and ABI profiles
remain unchanged.

## Package and preparation

`phenix.native-package.v2` uses the same six package record fields as native v1.
Its descriptor is canonical S1b `ContractMetadata`, execution `state-pending`,
embedded in the existing `.phenix` section. Descriptor and executable hashes
must match before loading. Definition-only packages may supply shared contracts
without executable code. Conflicting definitions of the same contract/revision
reject before constructors. Runtime normalization uses the existing resolver,
grants, dependency order and dispatcher.

The trusted `StateComposition` API wraps the existing `Composition`, adds
definition-package paths and explicit slot selections. A slot selection names
caller, require slot, provider and provide slot. Cardinality, revision ranges,
contract identity and schemas must match. There is no implicit provider lookup.
ABI 3 host imports name the require slot and canonical operation key. The
existing binding map stores the slot-qualified import identity. M0 imports use
an empty slot and retain their original identities.

Many selections carry explicit contiguous zero-based indices. One/Optional use
index zero and the plain slot name; Many imports use UTF-8 `slot#index` with a
canonical decimal index. A readonly host import with key
`phenix.host.slots@1/count` and the plain require slot returns its selected count.
It requires a declared slot and a live scope, and exposes no effects or grants.
Selected revisions of a Many slot must have identical operation schemas. The
indices, selected provide slots and definition-package digests participate in
generation identity. There is no provider guessing or unstable enumeration.

Property `p` on plugin `owner` contributes contract
`owner.property.p@1`, resource `owner.property.p`, and only the declared
`get`, `set`, `watch` operations. These identities must satisfy existing limits.
Consumer slots select the property owner exactly like ordinary services.
State callbacks authenticate the current scope and thread before reading bytes.
Read, write and watch intersect root, every caller ceiling and explicit caller
operation/resource grants before any effect. A serialized ref grants nothing.

## Initialization and identity

Create all instances in dependency order, then initialize each ABI 3 instance
under a temporary initialization scope. Only creation of its declared properties
is allowed during initialization. Each property is created exactly once and
must match its schema. Every declared property must exist before admission.
Initialize returns success Null. Any failure stops acquired instances in reverse
order and discards all cells. Host calls in `Default` remain unsupported.

Generation digest identifies the frozen prepared composition. Owner identity
also includes a process-unique incarnation, so reloading identical bytes does
not resurrect stale refs. Cell identity is owner-local and stable for the
property. All four location components must match. Unknown owners return
`OwnerGone`, wrong generations/cells return `StaleReference`, wrong schema
returns `SchemaMismatch`. Snapshots are immutable and remain useful after stop.

## State replies and atomicity

All ABI 3 requests and replies use the explicitly selected v2 wire profile.
Existing S1b state request opcodes remain frozen. Success is status 0, typed
`ValueAccessError` status 1, kernel failure status 2. Ordinary native pending
statuses and wake permits retain the accepted ticket protocol.

| Request | Successful value |
| --- | --- |
| Create | Ref |
| Read | `[revision, epoch, value]` |
| CompareExchange | committed snapshot |
| Batch | `[epoch, [snapshot, ...]]` in location order |
| Subscribe | `[cursor, snapshot]` atomically registered at current revision |
| Next | latest newer snapshot or Null if unchanged |
| Unsubscribe | Null |

Initial revision and epoch are zero. Each committed edit increases revision by
one, even if bytes compare equal. One successful batch advances one epoch. All
locations, schemas, authority, expected revisions and counter capacities are
checked before any mutation. Failed batches publish nothing. Competing edits
from the same revision have one winner and one Conflict; no implicit retry.

Subscriptions are logical root-owned cursors in the existing admitted runtime,
not a second executor. They authenticate caller and root, coalesce to the latest
revision and cannot lose an update during read/registration. Cursor identities
are not capabilities. Next and Unsubscribe require the original scope lineage
and recheck watch authority. Root cancellation/drop releases its subscriptions;
stop releases cells and cursors before physical retirement. Subscriptions do not
keep native work alive after cancellation. Native asynchronous work continues
to use existing physical tickets and leases.

The serial profile proves CAS conflict ordering. Concurrent admissions remain
S6. Consistent batch replies provide an epoch view; a new multi-read opcode is
not introduced in this stage. Reactive publication/execution remains S5.

## Acceptance

Independent public-contract native provider/client fixtures prove initialization,
typed collections and refs over ABI 3, cloning identity, unchanged historical
snapshots, schema checks, CAS conflicts, atomic batches, watch bootstrap and
coalescing, denied write with zero revision/effects, forged and stale references,
cursor ownership and root cleanup. Negative native/package/slot fixtures reject
before effects. All existing eighteen M0/M1/A0 gates must still pass.
