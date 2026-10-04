# Application integration terminology

status: implemented


Terminology follows [the architecture glossary](glossary.md).

## Terms

**Application**

User-facing software or host integration. Examples: `phenix-nvim`, a terminal CLI, or a browser UI.

An application is not a runtime plugin merely because it consumes Phenix.

**Protocol**

The interoperable message contract between an application-side client and a Phenix-side adapter. Examples: ACP or an OpenAI-compatible API.

Protocol semantics are independent of transport.

**Adapter**

The Phenix-side implementation of an external protocol. An adapter translates protocol operations into canonical Phenix operations and events.

An adapter may be packaged as an ordinary Phenix plugin. `adapter` describes its role; `plugin` describes packaging, lifecycle, composition, and authority.

**Client SDK**

Reusable application-side library code for speaking a protocol and exposing typed application operations.

A Client SDK owns protocol client mechanics such as request correlation, capability negotiation, event projection, and extension schemas. It owns no Phenix runtime state.

**Binding**

A language-native API over a Client SDK. Examples: Lua or Python bindings.

A Binding converts language values and objects into Client SDK operations. It does not reimplement the protocol.

**Transport**

The mechanism that moves protocol bytes or messages. Examples: stdio, Unix sockets, HTTP, or WebSocket.

Transport owns delivery mechanics, not protocol or Phenix domain semantics.

## Layering

```text
Application
    |
Binding or Client SDK
    |
Protocol
    |
Adapter
    |
Phenix runtime

Transport carries the Protocol between the Client SDK and Adapter.
```

A direct Rust application may use a Client SDK without a Binding. A Lua application normally uses a Binding over the same Client SDK.

## Naming

Use role-specific package names where the role is stable:

```text
phenix-adapter-<protocol>
phenix-client-<protocol>
phenix-binding-<language>
phenix-transport-<mechanism>
```

Applications keep product names such as `phenix-cli` or `phenix-nvim` rather than entering the runtime plugin taxonomy.

Runtime plugin ids may encode the adapter role, for example `phenix.adapter.acp`.

Client SDKs, Bindings, Transports, and Applications have no runtime plugin id unless they independently implement a runtime plugin role.

## Internal protocol boundary

The internal `phenix-client` wire remains an implementation boundary for Phenix runtime communication. Applications should use an external protocol through a Client SDK or Binding unless a dedicated internal-management tool explicitly requires the internal wire.

Do not expose raw internal envelopes as the ordinary public application API.

## Rules

- Prefer standard protocol operations before protocol extensions.
- Put Phenix-only semantics in versioned, capability-negotiated protocol extensions.
- Keep extensions domain-oriented rather than application-specific.
- Keep transport choices below protocol semantics.
- Keep durable runtime state in Phenix rather than applications, Client SDKs, Bindings, Adapters, or Transports.
- Share protocol client logic in Client SDKs instead of duplicating it across applications or bindings.
- Share transport mechanics in transport libraries instead of embedding them independently in adapters and applications.

## Examples

Neovim:

```text
phenix-nvim
  -> phenix-binding-lua
  -> phenix-client-acp
  -> ACP + Phenix ACP extensions
  -> phenix-adapter-acp
  -> Phenix
```

Terminal:

```text
phenix-cli
  -> phenix-client-acp
  -> ACP + Phenix ACP extensions
  -> phenix-adapter-acp
  -> Phenix
```

Socket use is optional in either path. `phenix-transport-socket` may carry ACP between client and adapter without changing ACP or application semantics.
