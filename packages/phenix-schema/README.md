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

Manual Graph implementations must implement `ContractValue::schema()` without
macros. There is no nominal `SCHEMA` constant or optional descriptor fallback.
Primitive and derived record bytes are unchanged, but all wire identities include
the complete descriptor. Old name-only envelopes require explicit migration.

`Schema::identity()` returns an owned `Arc<str>` containing the canonical
descriptor. Graph values, slots, declarations, interfaces, diagnostics and compiled
metadata own or borrow these identities. Clones share their allocation, equality
compares content, and dropping the last consumer releases strong ownership. There
is no process-global interner or strong cache. Independent equal descriptors may
have different allocations; pointer equality is never contract authority. Local
Rust values remain a distinct `TypeId` identity and cannot alias a wire identity.

The ignored `distinct_descriptor_reload_retention` release test measures distinct
reload batches and verifies with `Weak` that unloaded identities have no strong
owners. This is an ownership measurement, not allocator RSS profiling. A retained
value, graph, interface snapshot, diagnostic or report legitimately retains its
identity until that consumer is dropped.

## R01 Measurements

Before editing the implementation, and again after migration:

```sh
cargo test -p phenix-schema distinct_descriptor_reload_retention --release -- --ignored --nocapture
```

| Distinct Descriptors | Before Retained String Bytes | After Retained String Bytes | Before Batch Time | After Batch Time |
|---|---:|---:|---:|---:|
| 10 | 670 | 0 | 17.678 us | 21.264 us |
| 100 | 6,790 | 0 | 79.885 us | 88.038 us |
| 1,000 | 68,890 | 0 | 900.478 us | 513.126 us |
| 10,000 | 698,890 | 0 | 16.184144 ms | 5.977940 ms |

These are single release runs on the same workspace, not statistical performance
claims. Counts and string-byte totals are cumulative; times cover only each new
batch (10, 90, 900, 9,000 descriptors). The after harness additionally creates a
Weak, drops the strong identity, checks for remaining strong ownership and drops
the Weak on every iteration. The same 698,890 canonical bytes are generated in
total, but none remain strongly owned after unload. No allocator, RSS, plugin
reload throughput or payload-clone measurement is claimed.
