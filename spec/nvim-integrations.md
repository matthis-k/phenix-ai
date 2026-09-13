# Neovim integration extension points

status: partial

## Purpose

PR #506 makes editor-local Phenix behavior replaceable without changing runtime semantics. It extends the canonical #504 client and #505 callable/tool admission.

Every replaceable integration has one contract, one dependency-free built-in, and optional overrides. Integrations own presentation or editor-local behavior only. They do not own transport, application schemas, session/transcript truth, observable semantics, authority, or edit acceptance.

`spec/nvim-integrations-implementation.md` fixes the exact Lua APIs, data shapes, tool schemas, bounds, fallback behavior, and tests.

## Registry

One validated registry owns all replaceable behavior.

Singleton adapters:

```text
picker
review
permission
elicitation
image_renderer
notifier
navigation
session_picker
```

Collection adapters:

```text
reference_provider
transcript_renderer
image_source
```

`nil` restores the singleton built-in. Collection entries have stable IDs and deterministic priority ordering.

Optional plugin code loads lazily. One broken adapter affects only its operation. The sidebar and built-in path remain usable.

All Neovim/Lua behavior runs on the Neovim main thread. Async adapters use the one-shot completion rule from the implementation contract.

## Shared editor semantics

#504's normalized editor context is the only source of editor coordinates and snapshots.

Coordinates are zero-based lines and UTF-8 byte columns. Reference providers, navigation, default context tools, and UI tools reuse this representation.

No integration reinterprets Vim marks or converts coordinates into a second semantic format.

## References and picker

Reference providers receive one immutable `EditorContext` snapshot and return typed compose references with labels. They never return prompt prose.

Built-in providers cover current location, visual selection, buffers, diagnostics, and quickfix. Third-party providers join the same deterministic collection.

The generic picker receives stable item IDs and presentation text. It returns an ID. The registry resolves that ID back to the original semantic item, so an adapter cannot alter the selected value.

## Transcript presentation

User and assistant prose remains real Markdown.

Structured nodes may use replaceable renderers. A renderer receives immutable node data and returns declarative lines/highlights. It receives no native client, runtime object, or writable buffer handle.

Renderer failure falls back for that node only. Markdown plugins may decorate the rendered buffer but never own transcript state.

## Review

The review adapter receives the exact runtime `ReviewRecord` and a revision-bound decision function.

It may present files/hunks and request Accept or Reject. It cannot choose another review ID/revision, mutate files, or set terminal state locally.

The runtime applies accepted edits through the workspace owner before reporting `Accepted`. Adapter failure leaves the review pending and opens #504's built-in reviewer.

## Permission

The permission adapter receives the fixed application `PermissionRequest` and may return only a valid `PermissionResponse`.

Close, Esc, adapter failure, invalid result, or duplicate completion resolves to `Cancelled`. Presentation never mutates authority.

## Elicitation and forms

Questionnaires use one fixed flow:

```text
PhenixSchema
-> deterministic FormNode tree
-> built-in/custom renderer
-> candidate Lua value
-> native conversion against the original PhenixSchema
-> ElicitationResponse
```

The supported schema subset and exact `FormNode` variants are fixed by the implementation contract. Unsupported compound schemas fail explicitly with a schema path. There is no prose-parsing fallback.

## Images

Image acquisition and rendering are separate integration collections/surfaces.

Every source returns bytes + MIME metadata. The frontend snapshots those bytes into #504's attachment model before insertion.

The renderer receives an immutable attachment plus target window/range and returns an idempotent lifecycle handle. Text display remains the universal fallback.

## Notifications

One notifier adapter handles transient presentation notifications. `vim.notify` is the built-in fallback.

Durable runtime errors remain runtime/transcript state. Notifications never replace them.

## Navigation

One navigation adapter handles validated semantic `Location` values for open, reveal, highlight, and focus.

The built-in supports `file://`. Unknown schemes fail unless the configured adapter explicitly supports them. Tools and normal frontend actions call the same adapter.

## Sessions and status

The session picker receives runtime-owned `SessionInfo` values and returns one existing ID. It does not perform session operations.

`phenix_nvim.status.get()` is immediate and non-yielding. `status.listen()` emits only when the projected semantic snapshot changes. Statusline rendering performs no runtime I/O.

## Neovim client tools

`phenix_nvim.tools` stores frontend-local templates. Active admissions are ordinary #505 client tools.

Default context tools:

```text
nvim.context.current_location
nvim.context.selection
nvim.context.buffer
nvim.context.buffers
nvim.context.diagnostics
nvim.context.quickfix
nvim.context.viewport
```

Default UI tools:

```text
nvim.ui.show_location
nvim.ui.highlight_range
nvim.ui.focus_buffer
nvim.ui.pick_file
nvim.ui.show_diff
```

Their exact schemas, bounds, permission policy, and output types are fixed in the implementation contract. They reuse normal frontend context/picker/navigation/review code.

Third-party plugins register model-visible behavior through `phenix_nvim.tools.register(definition, function)`. The frontend stores the template, while runtime admissions remain tied to the current session and client generation.

Session switch replaces live admissions. Disconnect retires current refs. Reconnect lifts functions again and creates new admissions.

## Privileged Lua tools

Development mode may enable:

```text
nvim.lua.eval
nvim.lua.exec
nvim.lua.reload_module
```

They are absent by default. Enabling them makes all three ordinary client tools with mandatory permission checks.

They run in the live Neovim Lua environment. No sandbox claim is made. Lua/conversion failures are structural tool failures. Failed module reload restores the exact previous `package.loaded` entry.

Structured context/UI tools remain the normal editor API.

## Acceptance

#506 is complete when a realized Neovim flow proves:

- custom permission, elicitation, navigation, and review adapters;
- deterministic fallback from a failing structured renderer;
- default bounded `nvim.context.*` tools;
- default `nvim.ui.*` tools through configured adapters;
- one third-party model-visible Lua tool with no private runtime import;
- privileged Lua tools absent by default and permission-gated when enabled;
- session-switch admission replacement;
- disconnect cleanup;
- reconnect with new capability refs/admissions;
- exact-head Source, Rust, Product, Docs, Maintenance.

The acceptance path uses #504's packaged `phenix-acp` application. It contains no handwritten ACP or duplicate application schema.
