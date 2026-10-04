# ACP stdio application

status: implemented

The crate provides the ACP server over stdio and a bounded channel transport for a configured application runtime. The supported product exposes this transport through `phenix --mode acp`, so ACP uses the same resolved product composition as other frontend modes.

Canonical application-integration terminology is defined by #442. ACP adapter semantics are defined by #437.

## Goal

Provide a spawnable ACP stdio mode for applications that want to launch Phenix as an ACP agent process.

`phenix-acp-stdio` owns the transport implementation. The supported `phenix` executable selects it with `--mode acp`; there is no separate ACP product executable.

It is not a new protocol, runtime plugin, Client SDK, or application UI. It is a frontend mode over the configured Phenix runtime boundary.

## Architecture

```text
Application
  Neovim / editor / ACP client
        |
        | ACP JSON-RPC
        | stdin / stdout
        v
phenix --mode acp
  phenix-acp-stdio
        |
        v
phenix-adapter-acp
        |
        v
configured Phenix runtime
```

The ACP mode reuses `phenix-adapter-acp`. It must not implement a second ACP translation layer.

## Stdio contract

- stdin carries ACP requests and notifications only;
- stdout carries ACP responses and notifications only;
- diagnostics and logs go to stderr;
- stdout buffering must not reorder protocol messages;
- EOF shuts down the connection cleanly;
- process termination cancels only process-scoped work unless canonical Phenix policy says otherwise;
- durable Phenix sessions remain resumable after process restart;
- malformed protocol input fails the ACP connection without corrupting durable runtime state.

Stdio framing follows the pinned ACP specification. Do not introduce Phenix-specific framing around ACP messages.

## Runtime boundary

The ACP mode must use the configured Phenix product through an `ApplicationTransport`. The channel transport in this crate is only the hand-off point. A runtime bridge must receive each invocation, dispatch the matching typed application operation, and return its typed result.

The ACP frontend owns no parallel session, transcript, routing, authentication, permission, tool, execution, or persistence state.

All durable semantics remain in Phenix. The stdio process keeps only connection and protocol state.

If the configured product is hosted out of process, the implementation may reuse an internal transport such as `phenix-transport-socket`. That choice stays below ACP and must not change ACP behavior.

## ACP extensions

Expose the same standard ACP methods and negotiated `_phenix/...` extensions defined by `phenix-adapter-acp`.

The stdio package adds no stdio-specific ACP methods or metadata.

## Application integration

This is the preferred simple process integration for editors that already support spawning ACP agents.

For example, `phenix-nvim` may either:

- spawn `phenix --mode acp` and speak ACP over stdio directly; or
- use `phenix-binding-lua` / `phenix-client-acp` when it wants an in-process application API.

Both paths must expose equivalent ACP and Phenix-extension semantics.

## Packaging

`phenix-acp-stdio` remains independently buildable as the transport crate. Product packaging exposes it through `phenix --mode acp` rather than a second executable.

Do not require the socket transport package for the stdio path.

Do not add a runtime plugin identity for the frontend mode. Runtime adapter identity remains `phenix.adapter.acp` from #437.

## Regression coverage

- spawning `phenix --mode acp` completes ACP `initialize` over stdin/stdout;
- session creation, prompt streaming, cancellation, list, and resume behave the same as the shared ACP adapter contract;
- negotiated Phenix extensions match `phenix-adapter-acp`;
- logs never appear on stdout;
- EOF exits cleanly;
- malformed ACP input does not corrupt durable Phenix state;
- restarting the stdio process can resume a durable session;
- stdio behavior does not require `phenix-transport-socket`;
- an in-process Client SDK connection and stdio connection produce equivalent protocol semantics for the same supported operations.

## Completion

The product path is `phenix --mode acp`. It reuses the configured Harness graph, the application worker, `phenix-adapter-acp`, and the stdio transport. Product validation must prove initialization, protocol-only stdout, durable session resume, and parity with the shared application contract.
