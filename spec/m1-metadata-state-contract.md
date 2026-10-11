# M1 S1b: offline metadata and state native profile

Status: frozen before implementation; acceptance pending
Depends on: accepted S1a reference wire profile in PR #773
Target: trusted native x86_64 Linux; no live state implementation in this gate

## Metadata

`phenix.contract-metadata.v2` is a canonical `phenix.value.v2` map with exactly
`format`, `plugin`, `version`, `execution`, `definitions`, `requires`, `provides`,
and `properties`. Plugin/version are nonempty identifiers. Execution is `none`
for definitions only or `state-pending` for native ABI 3.0, features 3. M0
metadata and ABI 1/2 remain frozen and cannot select this profile.

A definition has `contract` (identifier followed by canonical positive `@major`),
`revision` (positive U64), and `operations` (nonempty list sorted by name).
Each operation has `name`, `request`, `response`, `error`; schema fields are Bytes
containing the frozen S1a canonical schema descriptor, not native Rust types.
Definitions are sorted by contract; repeated identities reject, even if identical.

A require has `slot`, `contract`, `revision_min`, `revision_max`, `cardinality`,
and `minimum`. Exactly-one uses `one`/1, optional uses `optional`/0, many uses
`many`/a bounded U64 minimum. Revisions form a positive inclusive range.
Requires are sorted by slot; distinct named slots may import the same contract.
A provide has `slot`, `contract`, `revision`; provides are sorted by slot.
Import and export names have separate namespaces. A decorator may use the same
contract in both; provider selection and direct self-import rejection belong to
the existing resolver in later gates. No provider is selected by these bytes.

A property has `id`, `schema`, `get`, `set`, `watch`; the three flags are Bool,
at least one is true. Properties are sorted by id and owned by `plugin`.
Its generated contract is `<plugin>.property@1/<id>`; accessor symbols append
`/get`, `/set`, `/watch`. These are property identities, not v1 operation keys.
Properties do not invent reusable shared interfaces. Duplicate/conflicting ids
reject. Definition-only execution rejects imports, exports and properties.
All maps have exact keys. Arrays are strictly sorted; decoders reject alternative
orders. Encoders sort authored declarations, then reject duplicates. Existing
wire size/depth/count limits apply. Structural schema matching and provider
composition remain later resolver work, not a second metadata resolver.

## Metadata-only packages

`phenix.contract-package.v2` package.json has exactly `format`, `descriptor`,
and `descriptor_sha256`. The descriptor must have `execution=none`. The record
names no artifact or native target; verification only reads metadata. Relative
normal path components and containment checks match the existing package reader.
The descriptor hash is SHA256 of exact canonical bytes. Package identity is
SHA256(`phenix.contract-package.v2\0` + descriptor digest bytes), independent of
JSON whitespace and descriptor filename. Sources are trusted read-only, as in M0.
`phenix-describe --metadata PACKAGE` verifies and prints inspection JSON and
both digests without loading code. No artifact compatibility fallback is added.
S3 owns macro generation; this gate's external public-SDK fixture authors typed
contract objects and calls the canonical metadata encoder without kernel sources.

## ABI 3.0 state-pending

Export `phenix_native_v3`, returning a header pointer. Major 3, minor 0,
features 3 (pending=1, state=2) and exact table length are required. Reject header
mismatch before reading later members; then reject null callbacks. Native table:
header, create, initialize, invoke, stop, poll, cancel. Sizes: header 16, table 64.
Create takes `void **`; initialize takes `(instance, StateHostApi*)`; invoke takes
`(instance, StateHostApi*, operation Bytes, request Bytes)`; stop takes instance;
poll/cancel take `(instance, StateHostApi*, token Bytes)`.

StateHostApi has header, scope U64, ticket U64, permit U64, invoke, state, wake.
Size 64, invoke offset 40, state 48, wake 56. Invoke takes `(scope, slot Bytes,
operation Bytes, request Bytes)`. State takes `(scope, opcode U32, request Bytes)`.
Wake takes `(ticket, permit)` and returns U32. Host table header uses the same
version/features with its own exact length. Borrowed inputs/table expire on
return. Only copied wake function/ticket/permit may outlive the call. All effect
callbacks authenticate the current scope and thread via the single dispatcher.
Only initialize may issue cells; admission follows successful initialization.
Create/Default has no host argument. Failed initialization drains/stops through
existing lifecycle ownership before admission. S2 implements this behavior.

Every reply uses existing producer-owned Reply layout and release exactly once,
but its body MUST select `phenix.value.v2`. Status meanings and quiescence match
ABI 2 (0 success, 1 domain, 2 kernel failure, 3 pending, 4 cancelled, 5 quiescent
failure). Pending/cancelled bodies are Null. ABI 1/2 reply decoding cannot accept
v2. Null release rejects before reading. Native Rust objects never cross the ABI.

State opcodes and exact request/success shapes:

| Opcode | Request | Success |
| --- | --- | --- |
| 1 Create | `[property String, initial Value]` | Ref |
| 2 Read | Ref | `[revision U64, epoch U64, value]` |
| 3 CompareExchange | `[Ref, expected U64, value]` | snapshot as above |
| 4 Subscribe | Ref | `[subscription U64, snapshot]` |
| 5 Next | subscription U64 | Null or one-item list containing snapshot |
| 6 Unsubscribe | subscription U64 | Null |
| 7 Batch | nonempty list of compare-exchange triples, sorted by location | `[epoch U64, list of snapshots]` |

Subscription ids are nonzero, scoped host handles, never portable authority.
Batch duplicates reject; sorted order is generation/owner/cell/schema byte order.
Atomic read+subscribe, leases, ticket polling and atomic publish are S2 duties.
Status 1 state errors are lists: `["Conflict", expected U64, current U64]` or a
single string item for OwnerGone, StaleReference, SchemaMismatch, Denied,
RevisionExhausted, EpochExhausted. Unknown names/arity/tags reject. Status 2 uses
the existing Failure map. Unknown opcodes reject without invoking state effects.

## Acceptance

An independent Python encoder supplies descriptor bytes and malformed cases to
an offline Rust verifier. The SDK-only definition fixture produces identical
canonical bytes without kernel sources. A separately compiled C producer calls
Rust-host callbacks through the public header; counts owned reply releases,
checks ref bytes and initialization order, and exercises malformed headers,
null callbacks and incompatible reply profiles. Full Nix CI must execute all
fourteen M0 gates, S1a, and this gate on the exact implementation head.
