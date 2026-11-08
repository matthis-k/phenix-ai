# Shared Schema Contract

`Schema` is the authoritative descriptor used by Graph and Storage. Equality
compares name, revision, structure, and codec. Canonical strings use versioned,
length-framed components rather than hashes or Rust type names.

Graph's `ContractValue` derive mechanically builds ordered record descriptors
from field names, optionality, and nested field contracts. Its codec identifier
specifies the existing little-endian u64 length-framed record format. Independent
structs with the same declared name and structure interoperate; different field
names, order, types, or optionality do not. Graph uses canonical identities for
authoring, compilation, admission, and checked value decoding.

Storage reexports this same `Schema` type. A storage `Codec::schema` can return
`T::schema()` from a Graph contract directly. `SchemaCodec` binds an existing
codec to that descriptor. Registry registration and session operations compare
the full descriptor before invoking the codec. Codec implementations and manual
descriptors are trusted assertions, not verification of arbitrary codec code.

Manual Graph implementations can override `schema_descriptor()` without macros.
Existing manual implementations default to their shipped name-only identity;
primitive graph wire names and bytes are unchanged. Derived record bytes are
unchanged, but their graph identity now includes structure. Old name-only derived
envelopes must be explicitly migrated, not silently decoded as structural data.

Canonical graph identities are interned for the lifetime of the process to retain
the existing static-string API. Applications should only register trusted schema
definitions, not arbitrary unbounded input descriptors.
