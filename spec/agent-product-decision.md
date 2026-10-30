# Fully customizable coding agent decision

Status: Accepted, 2026-10-15
Scope: Product identity and composition boundaries
Implementation owners: [M1 roadmap](m1-roadmap.md) and [P2 Advanced roadmap](p2-advanced-agent-roadmap.md)

## Context

The clean-start M1 kernel is deliberately generic, but Phenix's product goal is a fully customizable coding agent. Optimizing the kernel as an independent general-purpose platform would create requirements that do not help the agent. Hardcoding the agent into the kernel would prevent users from replacing its behavior.

## Decision

- **Plugins define what Phenix does.** Model and tool orchestration, agent workflows, memory, context, workspace behavior, permissions policy, routing, frontends and diagnostics are independently selectable plugin contributions.
- **The kernel defines how they execute.** It owns typed contract verification, composition resolution, authority enforcement, graph scheduling, cancellation, generation identity and resource lifecycle. It contains no privileged Basic or Advanced agent implementation.
- **Basic and Advanced are both examples and real products.** Basic is a usable minimal preset. Advanced is a full coding agent with its own acceptance requirements. Users may extend either preset or compose a different agent loop from scratch without modifying the kernel.
- **Nix is the current distribution path.** Presets expose conventional Nix package `.override` behavior through overridable functions and reuse the same plugin artifacts. Changing a preset produces a new package and composition rather than editing the shipped preset. Installed providers and declared graph contributions may be selected at runtime through authorized, validated generation changes.
- **Version boundaries are explicit.** The repository maintains its current contracts, not a library of compatibility implementations. Contract identity/revision and native ABI checks reject incompatible compositions before execution. Consumers may package older contracts separately; this does not imply backward compatibility between repository revisions.
- **Frontend contributions are ordinary providers.** An authorized application client may offer MCP tools, resources and Agent Skills over ACP-declared MCP servers, including negotiated native MCP-over-ACP. The frontend owns its application code, the selected plugins adapt standard MCP into existing Tool/Skills/Context contracts, and the kernel continues to enforce effects. Client capabilities do not introduce another protocol, kernel-owned tool catalog, or frontend agent loop. See the [M1 client-capabilities contract](m1-roadmap.md#client-contributed-mcp-capabilities-post-p1) for lifecycle, permissions, protocol versions and tests.
- **Packaging does not grant authority.** The runtime checks selected bindings and explicit grants. Native plugins run as trusted process code under the current in-process profile, so cooperative grants alone are not an OS isolation boundary.
- **Testing domains are separate mixes.** Model-history, provider-loopback and Graph V2 Python proof-of-concept fixtures must be selected only through explicitly named testing mixes. They are not implicit Basic/Advanced dependencies. The primary plugin product is Rust-authored native `.so`; the experimental Python frontend is absent from the production Rust SDK, default agent compositions and binary closures. An opt-in consumer composition is separate from the shipped preset, and CI tests both selected artifacts and realized closures. See [mixes](../mixes/README.md) for the concrete implementation.
- **Non-Nix packaging is planned, not a P2 gate.** Keep the artifact format, composition semantics and SDK independent of Nix. A later distributor must provide equivalent verification and trusted artifact handling without requiring another kernel or resolver.

## Consequences

Judge changes by whether they improve correct, customizable coding-agent behavior. A generic abstraction belongs in the kernel only when required to execute independent plugin contributions. Preserve one implementation of resolution, scheduling, authority and generations.

The [M1 roadmap](m1-roadmap.md) owns runtime behavior and client management. The [P2 roadmap](p2-advanced-agent-roadmap.md) owns preset composition, advanced capabilities, storage semantics and acceptance. This decision records the product boundary and its rationale; it does not duplicate either implementation specification.
