# Neovim integrations value/capability precedence

status: partial

## Purpose

Runtime-facing ABI rules for `spec/nvim-integrations.md`. `spec/value-capability-sdk.md` and merged #505 are authoritative below this layer. `spec/nvim-integrations-implementation.md` fixes the frontend API and tool schemas.

Frontend adapter registries are local Lua behavior. They do not create runtime protocols.

## Boundary

Only `phenix_nvim.runtime` owns the native client, SDK graph, application projection, and native request handles.

Integration modules receive frontend-owned functions and typed projected values. They never receive native client userdata, `PluginHost`, `CallableRef`, application operation IDs, or transport objects.

When behavior must cross the Phenix boundary, `phenix_nvim.runtime` performs the native operation on behalf of the integration layer.

## Local adapters

Picker, renderer, review, permission, elicitation, image, notification, navigation, session-picker, and status listeners are frontend-local Lua functions.

They stay ordinary Lua values because they never need runtime identity. Their one-shot completion semantics are owned by the frontend registry, not by `CallableRef`.

Only the outer #504 permission/elicitation handler registered with Phenix is lifted. That handler delegates locally to the current integration adapter and returns one typed application response.

Therefore replacing a permission or elicitation adapter does not mint a new runtime handler or modify runtime state. The already-installed frontend handler reads the current registry entry when invoked.

## Permission and elicitation

The runtime-facing functions keep the exact #504 callable contracts:

```text
PermissionRequest -> PermissionResponse
ElicitationRequest -> ElicitationResponse
```

The native binding lifts them against those authoritative schemas.

A custom permission adapter returns only a frontend candidate. The #504 handler checks it against the application response contract before returning it across the callable boundary.

A custom elicitation renderer returns a candidate Lua value. The #504 handler converts it with `from_lua(request.schema, candidate)` before constructing `ElicitationResponse::Accepted`.

No adapter may return a raw `PhenixValue` tagged as trusted or bypass the final schema conversion.

## Review

`ReviewRecord` is ordinary structural application data. The revision-bound `decide` closure supplied to the adapter is frontend-local and calls #504's descriptor-backed `review-decide` operation.

It is not a runtime capability ref and must not be serialized, persisted, or exposed as a model tool.

Model-visible `nvim.ui.show_diff` receives only a review ID. Its lifted Lua handler resolves the current projected review, then invokes the same frontend review adapter.

## Default and third-party tools

Every model-visible Neovim tool uses merged #505 semantics:

```text
frontend Lua function
-> native from_lua(Type::Callable, function)
-> client-owned CallableRef
-> ClientToolDefinition.invoke
-> session admission
-> ordinary ModelToolDescriptor
-> generic capability invocation
-> bounded client callback queue
-> Neovim main-thread Lua execution
-> validated PhenixValue result
```

There is no Neovim-specific tool kind, callback ID, provider branch, transport method, or execution path.

`phenix_nvim.tools.register` is frontend authoring sugar. It stores the definition and Lua function as a local template, then asks `phenix_nvim.runtime` to call the native #505 registration API for the active session.

Third-party plugins import `phenix_nvim.tools`, not native `phenix` and not private runtime modules.

## Tool schemas

The schema passed to #505 is exactly the schema declared in `spec/nvim-integrations-implementation.md`.

`from_lua` validates input/output at the generic capability boundary. Tool handlers may perform additional semantic checks such as byte limits, known URI schemes, or current review existence. They do not weaken the declared schema.

`nvim.lua.eval` and `nvim.lua.exec` use `Type::Any` for results. The native generic conversion remains authoritative. A Lua function, userdata, thread, or other unsupported value fails conversion rather than being stringified.

## Permission policy for tools

Default context and UI tools set `requires_permission = false` because their contracts only expose editor context or presentation and cannot mutate runtime authority or files.

The three privileged Lua tools always set `requires_permission = true` whenever they are enabled. This flag is part of the admitted `ClientToolDefinition`; frontend code cannot clear it per invocation.

Runtime permission handling executes before the client-owned callable. A denial means the Lua handler never runs.

## Session and connection lifetime

A live client-tool admission is identified by:

```text
client owner generation
session id
tool id
```

The frontend-local template has no runtime identity.

On active-session switch:

1. call current removal handles for the old session;
2. stop waiting after structural stale/disconnected failures because runtime generation cleanup is authoritative;
3. lift every enabled template function under the current client generation;
4. create new admissions for the selected session.

On disconnect:

1. stop local polling/subscriptions through #504 teardown;
2. discard admission removal handles and lifted-generation state;
3. retain only frontend templates;
4. rely on connection retirement to invalidate all client-owned refs and remove admissions.

On reconnect, every enabled template is lifted again. No old `CallableRef` or removal handle is reused.

## Threading

All Lua execution occurs through #503's bounded host callback queue and the single #504 Neovim polling point.

Adapter functions invoked by model-visible tools therefore run on the Neovim main thread too. Tool handlers must never call Lua directly from the ACP/runtime worker.

A client-owned capability callback may call non-conflicting frontend/runtime APIs through the normal async request path. It must not block the Neovim main thread waiting synchronously for another callback that itself requires host polling.

## Failure mapping

Use existing structural capability/tool failures.

```text
stale owner/generation -> stale capability failure
client disconnected -> disconnected failure
callback queue full -> queue-full failure
input conversion failure -> invalid input/schema failure
Lua error -> tool/capability execution failure with message
output conversion failure -> invalid output/schema failure
permission denial -> ordinary denied tool result/failure before handler execution
```

Do not inspect error strings to recover behavior or retry against another callback path.

## Completion

#506 is ABI-complete when every runtime-facing Lua function is either:

- one #504 interaction handler lifted through the generic callable ABI; or
- one #505 client-tool handler lifted/admitted through the generic callable ABI.

All other adapters remain frontend-local. Production Lua contains no callback-specific runtime semantic model, no private application schema, no raw `_phenix/...` method ID, and no direct background-thread Lua execution.
