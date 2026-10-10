# M0.0 implementation contracts

The attached RFC is preserved in `clean-start-rfc.md`. This document fixes the smallest concrete Linux profile needed by its P0 and M0.0 gates. Later gates remain deferred.

## Values and schemas

`phenix.value.v1` uses RFC section 8 tags, big-endian integers, sorted UTF-8 map keys, complete-message decoding, a 1 MiB limit, depth 32 and collection count 65,536. Named structs require every declared field and reject extra fields. Unit enum variants encode as strings. Bytes remain opaque. Contract schemas carry explicit interface major versions through `name@major` IDs. Struct schemas carry sorted field schemas; declared domain errors carry sorted variant names.

## Contributions and descriptor

One `#[plugin]` module owns one native artifact. Typed operation markers and annotated imports/exports generate both dispatch and immutable contributions. Every operation contribution has an owner, a role, a stable ID `owner/role/interface/operation`, kind `phenix.operation.v1`, schema payload and source provenance. Declare contributions derive from the union of required and provided contracts. Duplicate IDs and conflicting schemas reject.

`phenix.descriptor.v1` is a canonical value map. Its fields are `abi_major`, `abi_minor`, `contracts`, `execution`, `exports`, `features`, `format`, `imports`, `plugin_id`, `plugin_version`, `schema_version`. The authority ceiling is exactly the union of declared operation keys, so metadata cannot introduce an undeclared effect. `execution` is `sync`. Each contract has `key`, `request`, `response`, `error`; each contribution has `id`, `kind`, `operation`, `owner`, `provenance`, `role`. Contracts and contributions sort by key/ID. Schema objects use `kind`; structs add `fields`, enums add `variants`.

P0 tests one production candidate: an exported macro-generated static in `.phenix`. A four-byte big-endian length precedes canonical payload bytes in a fixed 16 KiB section, with zero padding. Extraction parses ELF without loading code. SHA-256 identity covers payload bytes only. The section symbol is exported to retain it under garbage collection; `#[used]` alone is insufficient. Lost, repeated, malformed or noncanonical metadata rejects. The descriptor mechanism remains unaccepted until the actual Nix release/LTO/linker/strip matrix passes. No `build.rs` inspects macro expansion.

## Packages and preparation

`phenix.package.v1` JSON has `format`, `artifact`, `descriptor`, `target`, `artifact_sha256`, `descriptor_sha256`. Paths are relative normal components within one trusted package. Digests cover final `.so` bytes and canonical descriptor bytes. Extracted bytes must equal packaged bytes. The deployment is a trusted immutable Nix closure. Hashes prove consistency, not authorship or isolation.

`phenix.deployment.v1` JSON has `format`, `artifacts`, `entry`, `bindings`, `grants`. Entry and binding objects have `interface`, `operation`, `provider`. Grants have `caller`, `interface`, `operation`. Unknown fields reject. There is no implicit provider or grant. Every package, descriptor, ABI requirement, import, structural schema and policy object is checked before any `dlopen`.

## ABI and lifetime

`phenix_native_v1` returns a pointer to a C header. The header has `u32 major`, `u32 minor`, `u32 table_bytes`, `u32 features`. Validate it before accessing the full table. ABI 1.0 has no features. The table then has create, invoke and stop function pointers. C layout details are authoritative in `phenix-contract/src/abi.rs` and `include/phenix.h`.

Borrowed bytes contain a pointer and `size_t` length. Null is valid only at length zero. Replies have a status, an owned pointer/length pair and a producing-side release function. The receiver copies and releases exactly once, including error replies. Allocators never cross images. Status 0 is typed success, 1 is declared domain error, 2 is canonical kernel failure. Creating an instance returns its opaque pointer through an output parameter. Stop destroys it. All catchable Rust panics at generated FFI boundaries become `PluginFailed`. Abort, signals and process termination are fatal and provide no cleanup guarantee.

A loaded image owns its instance and remains resident until stop and buffer release finish. M0.0 uses one serial admission thread. A host call issues a nonserializable scope valid only until that call returns. The host authenticates the scope and thread before touching kernel state. Nested imports use the prepared binding map and same dispatcher. Calling an active instance, using an expired scope or using another thread rejects. Synchronous cycles reject; no scheduler or asynchronous lifecycle exists.

## Selection and authority

The trusted CLI owns deployment preparation, root admission and inspection. Plugins receive only scoped import invocation. The kernel derives plugin identity from the issued scope. An import must be declared. The root ceiling contains the trusted deployment's explicit operation grants. Every nested call intersects that ceiling with caller grants, selected provider declarations and each active ancestor's declared operation ceiling. Environment scope is empty. There is one enforcement boundary immediately before provider entry.

An unselected required import with zero candidates gives `MissingBinding`; multiple candidates give `AmbiguousBinding`; even one candidate requires explicit selection. A selected incompatible provider gives `SchemaRejected`. Resolution creates one immutable map before loading.

## Errors and inspection

Failure codes are `DescriptorInvalid`, `ArtifactInvalid`, `AbiIncompatible`, `MissingBinding`, `AmbiguousBinding`, `SchemaRejected`, `Denied`, `PluginFailed`, `InfrastructureFailure`, `ReentrancyRejected`, `ScopeRejected`. Failure maps have `code` and `message`. Domain errors remain separate outcomes.

`phenix.inspect.v1` JSON contains sorted selected plugin records with artifact/descriptor digests, entry, frozen bindings, root and call summaries, last failure and denied count. It records offline verification separately from loaded ABI negotiation. It exposes no request payloads or host scopes.
