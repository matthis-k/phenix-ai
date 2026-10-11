# M1 portable reference wire contract

Status: S1a accepted at `2682be5f`; CI 38125329024
Date: 2026-10-11
Scope: portable schema and location bytes, independent of state-host admission
Parent semantics: [value RFC](m1-values-reactive-dag-rfc.md)
Tracker: [M1 roadmap](m1-roadmap.md)

## Profiles and negotiation

The caller selects `phenix.value.v1` or `phenix.value.v2` explicitly. The decoder never detects or falls back between profiles. V1 keeps its original tags, limits and bytes. V2 starts with four bytes `50 56 02 00`, followed by one canonical value. Tags 0 through 8 retain their v1 meaning. Tag 9 is a reference location. V1 rejects tag 9 and rejects encoding references, including nested references. The v2 prefix counts toward the existing 1 MiB payload limit. The existing depth and collection-count limits apply to both profiles.

ABI 1 and 2 continue to carry v1 exclusively. This gate adds no native entrypoint, state callback or automatic ABI upgrade. S1b must freeze a separate state-capable native profile and its negotiation before S2 admits live access.

## Location identity

Tag 9 is followed by the fixed fields below. Total location size is 113 bytes including the tag, or 117 bytes as a standalone v2 value.

| Field | Bytes | Meaning |
| --- | --- | --- |
| Generation digest | 32 | Raw bytes of the frozen generation SHA-256 identity |
| Owner incarnation | 32 | Host-issued identity for one instance or durable namespace incarnation |
| Value ID | 16 | Host-issued cell identity, unique within the owner incarnation |
| Schema ID | 32 | Portable schema SHA-256 identity |

All-zero fields reject. These bytes identify a location and carry no authority. Decoding proves only canonical shape. S2 must authenticate the caller and resolve the exact generation, owner and cell before checking schema and per-operation grants. A well-shaped forged location must fail there. Clone and serialization cannot add grants.

The host must never reuse an owner incarnation across process restarts, instance recreation or generation rollback. S2 owns issuance and collision rejection. A durable owner uses the same location shape with an explicitly configured durable namespace; this gate grants no persistence. Generation and owner remain separate identities even when executable bytes match.

`SchemaId`, `OwnerId` and `ValueId` have distinct Rust types. Constructors validate widths and reserved zero values. Public construction of a location creates no authenticated live handle. Revisions and epochs are unsigned 64-bit counters. Revision zero is the initial snapshot. Increment overflow must reject before publication in S2. Neither counter belongs in the stable location.

## Schema identity

`ValueSchema` describes portable structure without Rust `TypeId`. It lowers to a v1 canonical value and hashes `phenix.schema.v2\0` followed by those bytes with SHA-256. Its encodings are below.

| Schema | Canonical value |
| --- | --- |
| Bool, I64, U64, String, Bytes | The corresponding name as a String |
| List | `["List", element_schema]` |
| Option | `["Option", element_schema]` |
| Struct | `["Struct", {field_name: field_schema, ...}]` |
| Unit variants | `["Variants", [sorted_unique_names, ...]]` |
| Reference | `["Ref", target_schema_id_as_32_bytes]` |

Struct names follow the existing identifier grammar. Variant lists are nonempty, sorted and unique. Schema encoding obeys the existing wire limits. Schema identity is structural; contract IDs and behavioral revision remain S1b concerns. Payload-bearing enums and recursive schema definitions require a later explicit descriptor rule. Unit variants are represented by String values. Option uses Null for None and a one-element List for Some, so `Some(None)` stays distinct from `None`.

Schema validation checks portable shape. For Ref it checks the target schema ID and location shape; it cannot establish cell existence or authority. A `DynValue` pairs one schema ID with a validated portable value. The constructor checks its schema before admitting the pair.

## Error vocabulary

State access distinguishes `Conflict`, `OwnerGone`, `StaleReference`, `SchemaMismatch`, `Denied`, `RevisionExhausted` and `EpochExhausted`. These are state protocol outcomes, separate from native reply status and M0's `Code`. S1b owns their negotiated wire envelope. This gate reserves no new native status numbers.

## Independent acceptance

`tests/m1-contract-vectors.json` freezes scalar, nested reference, option and schema-identity bytes. The independent Python encoder/decoder must agree with a Rust public-contract probe in both directions. Negative inputs cover wrong profile, unknown version/tag, truncation at every location byte, trailing bytes, invalid UTF-8, duplicate/unsorted keys, depth/count/payload overflow and each zero identity field. V1 rejects nested references. Independent C layout and all existing M0 gates continue to run.

S1a acceptance proves portable shape and compatibility only. Live-cell authorization, owner issuance, subscriptions, transactions and native state calls remain subsequent gates in the tracker.
