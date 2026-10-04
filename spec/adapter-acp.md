# ACP adapter

status: partial
coverage:
  - rust/crates/phenix-adapter-acp
  - modules/package-sets.nix
  - modules/plugin-packaging.nix

`phenix-adapter-acp` now dispatches ACP over the fixed application interface. Standard ACP owns protocol-native behavior. Descriptor-backed `_phenix/...` extensions carry Phenix semantics that ACP cannot represent without loss.

## Goal

Provide ACP as the primary compatibility protocol for rich Phenix applications without exposing the internal `phenix-client` wire or conductor internals.

The first-party runtime package is `phenix-adapter-acp`, with plugin identity `phenix.adapter.acp` and package-set entry `phenixPlugins.${system}.adapter-acp`.

The adapter owns ACP translation only. It owns no Phenix session, transcript, routing, authority, credential, execution, persistence, process, or transport state.

## Application boundary

```text
Application
  Neovim / CLI / other UI
        |
        | ACP + negotiated Phenix ACP extensions
        v
phenix-adapter-acp
        |
        | canonical application operations/events
        v
Phenix runtime
```

Applications do not need the internal `phenix-client` protocol.

## Application contract

`application-interface.md` owns the protocol-neutral operation, event, callback, capability, type, and error contract.

The adapter consumes that passive contract. It does not scan runtime Plugin implementations to discover application semantics. It does not import the internal `phenix-client` wire.

Standard ACP method names and protocol rules remain ACP-owned. The adapter contains semantic conversions where ACP and Phenix differ.

Phenix-only `_phenix/...` methods, events, and callbacks project stable ids and structural schemas from the fixed application descriptor. The adapter does not keep a second handwritten extension schema set.

## Packaging

The package uses the adapter role:

- crate/package: `phenix-adapter-acp`;
- runtime id: `phenix.adapter.acp`;
- package-set entry: `phenixPlugins.${system}.adapter-acp`;
- the old `phenixClients.${system}.acp` / `mkPhenixClient` public category is removed;
- adapter selection, omission, replacement, authority, and configuration stay on the ordinary plugin path.

The adapter package does not own a process or stdio lifecycle. `phenix-acp-stdio` owns the transport implementation. The packaged `phenix` process selects that transport with `--mode acp`, so ACP runs over the same resolved product composition as the other frontend modes.

## Standard ACP mapping

The pinned ACP v1 baseline maps directly to typed application operations where semantics match:

- initialize and capability advertisement;
- session create, list, resume, load, close, and prompt;
- cancellation;
- text and resource-link prompt content;
- ordered text and tool-call updates;
- permission callbacks;
- primitive form elicitation;
- model selection through ACP model config;
- routing selection through an ACP config option.

Text and resource links are ACP v1 baseline prompt content, so no optional prompt content capability is advertised.

Authentication stays on the descriptor-backed Phenix extension path. The current application authentication result can return an external URI plus instructions, while ACP v1 `authenticate` has no response field for that result. The application authentication method declaration also does not identify an ACP auth flow type. Mapping it to standard ACP would lose contract information.

Workspace inputs that the application contract cannot represent are rejected before runtime dispatch. This includes extra working directories and MCP server provisioning.

## Mapping rules

- ACP session identity maps to canonical Phenix `SessionId`.
- Session creation creates exactly one canonical Phenix session.
- Prompting enters the canonical Phenix execution path rather than invoking a model directly.
- Cancellation targets the canonical session execution path.
- Session list, resume, and load reconstruct runtime-owned state.
- Load requests resume from sequence zero so the caller can replay the durable history.
- Adapter drop or transport disconnect does not imply session close or deletion.
- Unsupported security-sensitive workspace input fails before runtime dispatch.
- Adapter translation resolves application operations through `ApplicationClient<T>`.
- `ApplicationClient<T>` remains responsible for negotiated application capability checks.

## Phenix ACP extensions

Use versioned `_phenix/...` extensions when standard ACP would lose Phenix semantics.

The extension catalog is projected from the fixed application descriptor after application capability negotiation. It contains method, event, callback, capability, input, and output metadata from that descriptor.

Advertised extension operations are executable. The adapter decodes their structural input into the typed application operation, dispatches through `ApplicationClient<T>`, and encodes the typed result back to ACP extension framing.

Descriptor-backed extension callbacks use the same rule. The adapter resolves the callback from the request type, checks its negotiated capability, validates request and response values against descriptor schemas, and returns the resolved callback id for response correlation. Permission remains on standard ACP. Rich elicitation and client callables can use the extension callback path.

Current extension families include:

- authentication discovery and selection;
- skill discovery and activation;
- callable discovery and invocation;
- session lineage;
- execution-tree inspection;
- invocation provenance;
- structured diagnostics;
- Phenix updates that have no lossless standard ACP form.

Extension names use the Phenix namespace. The obsolete `_phenix/client/envelope` path is not part of the adapter API.

## Streaming

Canonical Phenix events map to standard ACP updates where ACP preserves their meaning.

Text updates preserve session sequence and execution correlation metadata. Tool calls and tool results use standard ACP tool-call updates. Progress, execution-state changes, non-text messages, diagnostics, rename, and close events fall back to descriptor-owned extension notifications when needed.

The adapter does not synthesize a second transcript or persist projection state.

## Transport independence

ACP semantics are transport-independent.

- PR #489 provides the spawnable stdio composition;
- stdio requires no socket library;
- socket-backed deployment may reuse `phenix-transport-socket`;
- future transports can carry the same ACP mapping;
- transport choice does not change application sessions, capabilities, extensions, authority, or event semantics.

## Regression coverage

Current adapter coverage proves:

- initialize projects standard capabilities and descriptor-owned extensions;
- session create, list, resume, load, close, prompt, and cancel use typed application operations;
- model and routing state round-trip through ACP config options;
- unsupported extra workspace and MCP inputs fail before runtime dispatch;
- standard text and tool updates preserve sequence and execution identity;
- non-standard updates fall back to descriptor-owned extensions;
- permission and primitive elicitation callbacks map through standard ACP;
- extension operation dispatch preserves typed input, output, and application capability checks;
- extension callback framing derives identity and schemas from the descriptor;
- callback responses with the wrong structural shape are rejected;
- adapter drop does not close the durable session;
- `_phenix/client/envelope` cannot reappear through extension metadata.

Process and transport equivalence are covered by the stdio composition PR rather than this translation package.

## Completion

- [x] `phenix-adapter-acp` is the ordinary first-party ACP adapter plugin;
- [x] the adapter consumes the protocol-neutral application interface contract;
- [x] standard ACP is used for application concepts it represents without loss;
- [x] Phenix-only concepts use versioned capability-gated ACP extensions sourced from the application descriptor;
- [x] applications do not require the internal `phenix-client` protocol;
- [x] the adapter introduces no parallel session, transcript, routing, authority, credential, execution, or persistence state;
- [x] the adapter package owns no transport lifecycle;
- [x] stdio process ownership lives in PR #489 rather than this adapter package;
- [x] obsolete `phenixClients.acp` packaging is removed;
- [ ] exact-head Source, Rust, Product, Docs, and Maintenance validation passes.
