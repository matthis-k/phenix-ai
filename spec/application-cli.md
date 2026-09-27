# Terminal CLI application

status: specification-only

Implementation follows repository cleanup.

## Terminology

Use the canonical terms from [Application integration terminology](application-integration-terminology.md).
The terminal CLI is an Application. It is not a Phenix runtime plugin.

## Goal

Provide a deliberately small Pi-like terminal application for Phenix.

The first-party package is `phenix-cli`. It owns the `phenix` executable and no runtime plugin identity.

Running `phenix` opens a line-oriented interactive conversation. Running `phenix <prompt...>` submits one prompt and exits.

The CLI owns terminal interaction and rendering only. Phenix remains authoritative for sessions, execution, routing, tools, authentication, permissions, and persistence.

## Command utility plugin

Command discovery and probing already use the canonical runtime-plugin identity:

- crate/package: `phenix-plugin-command-toolbelt`;
- plugin id: `phenix.command-toolbelt`;
- package-set entry: `phenixPlugins.${system}.command-toolbelt`.

That runtime plugin is not the terminal Application. The removed `phenix-plugin-cli` / `phenix.cli` identities are legacy names only and have no current compatibility alias.

## Application architecture

```text
phenix-cli
   |
   | application-side Client SDK
   v
ACP + negotiated Phenix extensions
   |
   v
phenix-adapter-acp
   |
   v
Phenix runtime
```

The CLI should use the same Client SDK as other first-party Applications and Bindings where practical. It must not import runtime internals or use the internal `phenix-client` wire as its public application API.

Transport stays below the protocol. Stdio may be used when the CLI owns/spawns an adapter process. A persistent deployment may reuse `phenix-transport-socket` from #436. CLI behavior must not change with transport.

## Interactive mode

`phenix` with no prompt enters a simple line-oriented REPL.

```text
> user prompt
assistant output
> next prompt
```

- create a session on first use unless an existing session was selected;
- stream assistant output and useful execution updates;
- keep tool output compact by default;
- send diagnostics to stderr;
- EOF exits cleanly;
- Ctrl-C cancels an active turn; Ctrl-C while idle exits.

Do not require a full-screen TUI framework.

## One-shot mode

`phenix <prompt...>` submits one prompt, prints final user-visible output, and exits.

Return zero on successful completion and non-zero for protocol, policy, authentication, routing, backend, tool, cancellation, or transport failure.

## Commands

Initial commands:

```text
/help
/new [name]
/sessions
/resume <session-id>
/model
/cancel
/quit
```

Use standard ACP operations when available. Use negotiated Phenix ACP extensions for Phenix-only behavior.

Do not add a CLI-specific protocol method for an operation already represented by ACP or a reusable Phenix extension.

## State

Keep only ephemeral Application state such as the selected session, active request, and rendering state.

Reconstruct durable state after reconnect. Do not persist a second transcript, session registry, model database, credential store, or execution journal.

## Authentication and permissions

Use ACP authentication, permission, and elicitation flows where they represent the Phenix operation. Use Phenix ACP extensions only when standard ACP cannot preserve the semantics.

Never grant runtime authority because the Application is local.

## Packaging

Expose `phenix-cli` as an ordinary Application package, not through `phenixPlugins`.

The default Phenix product may include the Application for convenience, but installing or omitting the CLI changes no runtime plugin graph.

## Regression coverage

- `phenix-cli` builds and runs without a runtime plugin identity;
- command discovery remains under `phenix.command-toolbelt` without behavior loss;
- the CLI can create a session and complete two sequential prompts;
- one-shot mode returns the correct exit status;
- list/resume use runtime-owned state;
- model selection uses ACP config or the negotiated Phenix routing extension rather than direct provider calls;
- Ctrl-C cancels the active execution and leaves the session usable;
- reconnect restores durable state without a local transcript store;
- stdout/stderr remain separated;
- stdio and socket-backed deployments have equivalent application semantics;
- no internal `phenix-client` envelope is required by the Application API.

## Completion

- [ ] `phenix-cli` is an Application package, not a runtime plugin;
- [ ] it owns the `phenix` terminal executable;
- [x] command discovery uses `phenix.command-toolbelt`;
- [ ] one-shot and line-oriented interactive modes work;
- [ ] the Application uses the shared Client SDK and ACP boundary;
- [ ] no duplicate durable state or runtime policy is introduced;
- [ ] Transport remains below protocol semantics;
- [ ] exact-head Source, Rust, Product, and Maintenance validation passes.
