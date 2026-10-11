# S3a typed value SDK freeze

This SDK-only split precedes S3b generated contract/property authoring. It adds
no kernel mechanism and uses the accepted S1a/S1b wire shapes and S2 callbacks.
S3a owns `phenix-contract/src/static_schema.rs`, `phenix-sdk/src/live.rs`, their
public exports, independent portable vectors and compile-fail examples. S3b
owns contract/plugin macros, offline metadata lowering and generated native
storage/glue. S4 and S5 remain separate prerequisites for reactive hooks.

`phenix_sdk::live::Portable` supplies a static structural schema plus v2 value
conversion. Scalars, named structs, unit variants, lists, options and typed
locations use the frozen canonical ValueSchema shapes. Existing M0 Portable,
Schema and wire Value remain unchanged. The `LivePortable` derive only generates
this new trait; unsupported tuples, pointers, references and generics reject.

`live::Value<T>` owns an immutable Arc<T>; Clone has no T:Clone bound, Deref and
AsRef return shared T, and From<T> constructs it. There is no DerefMut.
`ValueRef<T>` carries only a validated ValueLocation and type marker; it is
Clone/Copy, has no dereference, supplies no authority and is portable only with
the matching structural schema identity. Host access always remains fallible.
`Snapshot<T>` pairs an immutable Value<T> with Revision/Epoch.
`Edit<T>` is an isolated T:Clone draft from a snapshot. It supports DerefMut,
requires explicit fallible commit and never publishes during Drop. Conflicts
preserve expected/current revisions and the caller can reload explicitly.

`live::Host` borrows one negotiated StateHostApi and is neither Send nor Sync.
It provides declared slot calls, slot count, consistent epoch snapshots,
checked cell creation/read/CAS/batch and subscribe/next/unsubscribe. Creation
is an initialization-only operation enforced by the kernel, never Default.
Access errors distinguish state domain outcomes from kernel failures. A watch
cursor is nonportable and neither Send nor Sync; closing is explicit through
a current host. Root/ticket abandonment is the final host cleanup boundary.

`Cell<T>` holds an initial value before initialization, then a typed location.
Binding consumes initial data exactly once through Create. It exposes only
fallible host read/edit/ref methods, never direct mutation of published T.
S3b rewrites exposed authored fields to these cells and binds them through the
native initialize callback before admission. Unannotated fields remain plain.

Static schemas have canonical constant encodings and structural SHA-256 IDs
identical to the independent S1a producer. References hash the target schema,
not Rust type names. The constant implementation is verified against existing
independent vectors; no metadata registry, compiler TypeId or native loading is
used to learn schemas.

Acceptance requires immutable-clone behavior for a non-Clone T; typed ref
mismatch rejection; explicit edits with ordinary Vec::push; independent schema
bytes/hashes; an SDK-only native fixture; and compile failures for mutable ref
dereference, direct cell writes, invalid field types and cross-thread hosts.
S3b retains the complete generated authoring acceptance gate.
