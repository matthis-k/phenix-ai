# Neovim client value/capability precedence

status: partial

## Purpose

Runtime-facing ABI rules for `spec/nvim-client.md`. Product behavior stays there. `spec/nvim-client-implementation.md` owns the mechanical implementation.

`spec/value-capability-sdk.md` is authoritative for the generic ABI. #503 and #505 are merged and fixed.

## Boundary

```lua
local phenix = require("phenix")
local frontend = require("phenix_nvim")
```

`phenix` projects the authoritative `(PhenixSchema, PhenixValue)` graph into Lua. Structural values become ordinary Lua values. Callable values become proxies backed by exact callable schemas. Lua functions crossing the runtime boundary become client-owned `CallableRef`s.

`phenix_nvim` owns editor behavior only.

## Required final path

`phenix_nvim.runtime` must use:

```text
client:application()
  fixed descriptor-backed application operations

client:sdk()
  generic SDK value graph

phenix.sessions.state.get/listen
  production value-level session observable

generic callable lifting
  permission and elicitation handlers
  #506 client tools
```

The standard ACP session helper may exist in the native binding for interoperability, but #504 production code does not use it after the final application path lands. It is not a fallback path in the completed client.

## Application operations

Standard application actions remain fixed descriptor-backed operations. The frontend may call the generated Lua application projection for session creation/list/resume/close, prompt, cancel, model/routing selection, interaction-handler registration, and review decision.

Frontend Lua contains no operation IDs. It does not reconstruct operation schemas.

## Observables

Session/transcript state uses one value-level resource:

```lua
local full = phenix.sessions.state.get()
local stop = phenix.sessions.state.listen(opts, function(delivery)
  -- reduce exact typed state/diff
end)
```

The resource value is `SessionProjectionState` from the fixed application contract. The listener is an ordinary Lua function lifted through the generic callable ABI.

The frontend never creates a second event database. Observable or session-sequence gaps trigger a full `get()` and projection rebuild.

## Permission and elicitation

The application handler-registration operation accepts client-owned callable refs. `from_lua(expected_callable_schema, function)` performs the lift.

The exact callable contracts are:

```text
PermissionRequest -> PermissionResponse
ElicitationRequest -> ElicitationResponse
```

The runtime verifies owner generation and callable schema before installing a handler. Disconnect retires the refs and clears the slots.

ACP permission/elicitation callbacks map to those same slots below the frontend. There is one Lua-facing semantic path.

## Review

Review records are structural application values carried through session state. Decisions use the fixed review-decision application operation.

The frontend receives no patch-application capability. It can only request an allowed runtime decision by review ID and expected review revision.

## Client tools

#506 model-visible Neovim behavior follows #505 exactly:

```text
Lua function
-> client-owned CallableRef
-> ClientToolDefinition
-> session admission
-> ordinary model tool
-> generic capability invocation
```

No Neovim-specific runtime callback type or backend tool kind is added.

## Threading and lifetime

- one Neovim main-thread polling point dispatches native callbacks;
- worker/background threads never call Lua;
- client-owned refs are scoped to one connection generation;
- reconnect lifts new refs and creates new admissions;
- stale refs fail structurally;
- pending callbacks fail on disconnect;
- queue-full is explicit.

## Completion

This precedence is satisfied when the realized #504 acceptance path uses the fixed application descriptor plus generic SDK/callable/observable ABI, and production frontend Lua contains no handwritten transport, private schemas, callback-specific semantic path, or ACP session-helper fallback.
