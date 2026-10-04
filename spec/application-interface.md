# Application interface

status: partial
coverage:
  - rust/crates/phenix-application-interface/src/tests.rs
  - modules/development.nix

The passive contract, versioned snapshot, capability checks, and first Rust generation target are implemented. ACP dispatch and production Rust/Lua clients remain in the dependent PRs. The Rust emitter supports every schema used by the fixed descriptor; it rejects unsupported schema kinds explicitly.

## Purpose

Define one protocol-neutral application contract for Phenix clients and generated language bindings.

Applications should depend on stable application concepts instead of the internal `phenix-client` wire, runtime services, Plugin implementations, or one protocol adapter.

The first implementation target is the ACP application path used by `phenix-nvim`. The same contract must support generated Rust and language bindings later without introducing another application API.

## Ownership

A passive package named `phenix-application-interface` owns the application contract and its generator metadata.

It owns no runtime Plugin identity, transport, protocol connection, persistence, authentication state, session state, execution state, or provider selection.

Runtime Plugins remain authoritative for behavior. Protocol adapters translate external protocols to this contract. Client SDKs and language bindings project this contract for applications.

```text
application
   |
   | generated or typed application API
   v
application interface
   |
   +-> ACP client -> ACP adapter -> Phenix
   |
   +-> future protocol client -> matching adapter -> Phenix
```

## Fixed descriptor

The application contract has one deterministic, versioned descriptor.

Rust declarations use a constrained descriptor vocabulary backed by the existing `PhenixSchema` structural type system. They define stable operation, event, callback, capability, input, output, and error identities.

The build emits a canonical descriptor such as:

```text
share/phenix/interfaces/phenix.application@1.json
```

The repository keeps a deterministic snapshot and Source validation checks that regeneration is clean.

Generators consume the descriptor. They do not scan runtime Plugin implementations or infer public operations from the internal conductor wire.

The descriptor is data, not executable policy. It cannot grant authority, select a provider, choose persistence, or bypass kernel dispatch.

## Contract model

The descriptor contains these first-class entries:

| Entry | Required data |
| --- | --- |
| operation | stable id, input schema, output schema, errors, capability gate |
| event | stable id, payload schema, ordering scope, capability gate |
| callback | stable id, request schema, response schema, authority-sensitive semantics |
| capability | stable id, version, dependencies |
| named type | stable id and `PhenixSchema` |

Stable ids survive Rust symbol renames. A contract version changes when compatibility rules require a new application contract.

The descriptor uses the existing Phenix structural vocabulary. It does not introduce JSON Schema, `serde_json::Value`, or a second dynamic type system as the canonical representation.

## Initial application operations

The first version covers the application behavior required by an editor client:

- capability discovery;
- authentication discovery and selection;
- session create, list, resume, rename, close, and lineage lookup;
- prompt submission and cancellation;
- ordered session and execution updates;
- tool-call, progress, permission, and elicitation callbacks;
- model and routing configuration exposed to applications;
- skill discovery and activation;
- callable discovery and invocation;
- execution-tree and provenance inspection;
- structured diagnostics;
- bounded log queries and verified log-reference reads.

An operation may map to one runtime service or several runtime calls. The application contract does not expose the internal service topology.

The optional `logs` capability keeps local diagnostic storage behind the application boundary. `log-query` returns bounded structured pages with opaque cursors. `log-reference-read` resolves a typed `ContentReference` and verifies its digest. Frontends do not parse log files or derive CAS object paths.

## ACP mapping

`phenix-adapter-acp` maps standard ACP semantics onto the application contract.

Use standard ACP methods and updates whenever they preserve the Phenix semantics. The adapter owns the semantic conversion where ACP and the application contract differ.

Phenix-only ACP extensions use versioned `_phenix/...` names and map to stable application operation, event, or callback ids. Extension schemas come from the application descriptor instead of a second handwritten schema set.

ACP protocol versioning remains owned by the ACP adapter. The application contract does not copy the ACP wire specification.

## Generated clients and bindings

Code generation starts from the fixed descriptor.

The first generator target is Rust. It should generate application request, result, event, callback, capability, and typed operation wrappers used by `phenix-client-acp`.

Later targets may generate Lua, Python, JavaScript, or other bindings from the same descriptor.

Generated code owns repetitive type and operation projection. Handwritten client code owns behavior that cannot come from a declarative contract, including:

- transport lifecycle;
- ACP request correlation and capability negotiation;
- reconnect and resume behavior;
- host event-loop integration;
- foreign-runtime object lifetime;
- conversion errors at a language boundary.

A language binding must not redefine operation ids, schemas, capability ids, or error variants already present in the descriptor.

## Neovim path

The shortest usable editor path does not wait for the Lua generator.

```text
phenix-nvim
   |
   | ACP JSON-RPC over stdio
   v
phenix --mode acp
   |
phenix-adapter-acp
   |
application interface
   |
configured Phenix runtime
```

After the generated Lua binding exists, `phenix-nvim` may use it instead of maintaining ACP framing in Lua. Both paths must expose the same application behavior.

## Versioning and compatibility

Every generated artifact records the application interface id and version it targets.

A client may use only operations and capabilities advertised by the connected runtime path. Optional capabilities stay optional in generated APIs through explicit feature checks.

Compatible additive changes keep the current contract version when existing generated clients remain valid. Breaking semantic or structural changes require a new versioned contract id.

Do not preserve prerelease aliases after all first-party consumers migrate.

## Required regressions

Add coverage proving:

- the descriptor is deterministic and regeneration leaves the repository clean;
- every operation, event, callback, capability, and named type has a stable id;
- descriptor schemas use `PhenixSchema` vocabulary;
- generated Rust types and wrappers match the descriptor;
- an ACP adapter maps standard ACP behavior without importing the internal client wire;
- Phenix ACP extension schemas are sourced from the descriptor;
- a generated client cannot invoke an unavailable capability without an explicit unsupported result;
- generated bindings do not depend on runtime Plugin implementation crates;
- a runtime implementation can change behind the same application contract without regenerating consumers.

## Completion

- [x] `phenix-application-interface` is an independently buildable passive package;
- [x] one deterministic versioned descriptor owns application operation and event schemas;
- [x] the descriptor uses `PhenixSchema` rather than a parallel schema system;
- [ ] ACP maps to this contract without exposing the internal conductor wire;
- [x] Rust client types and operation wrappers can be generated from the descriptor;
- [x] language generators can consume the same descriptor without runtime implementation dependencies;
- [x] the initial descriptor covers the application behavior required by `phenix-nvim`;
- [ ] exact-head Source, Rust, Product, Docs, and Maintenance validation passes.
