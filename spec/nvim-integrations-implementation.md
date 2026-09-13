# Neovim integrations implementation handoff

status: partial

## Purpose

Mechanical contract for PR #506. `spec/nvim-integrations.md` owns product behavior. `spec/nvim-integrations-value-capability.md` and `spec/value-capability-sdk.md` own runtime ABI rules.

#506 starts implementation only after #504 exposes the canonical frontend/runtime surfaces. It extends that client. It does not create another runtime, transport, state store, or callback model.

## Fixed public modules

Add these public frontend modules:

```text
phenix_nvim.integrations
phenix_nvim.forms
phenix_nvim.tools
```

Internal files may be split as needed, but the public API stays under those three modules.

`phenix_nvim.runtime` remains the only module that imports native `phenix` or owns client userdata.

## Integration registry

### Surface kinds

Singleton surfaces:

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

Collection surfaces:

```text
reference_provider
transcript_renderer
image_source
```

Status is not replaceable through this registry. `phenix_nvim.status.get/listen` exposes cached projected state directly.

### Public API

```lua
local integrations = require("phenix_nvim.integrations")

integrations.set(kind, adapter_or_nil)
integrations.get(kind)
integrations.register(kind, id, adapter, opts) -- collection surfaces only
integrations.unregister(kind, id)
integrations.reset(kind)
integrations.reset_all()
```

Rules:

- every singleton always has a built-in effective adapter;
- `set(kind, nil)` restores that built-in;
- collection registration requires a non-empty stable string ID;
- duplicate collection ID is an error;
- `unregister` is idempotent;
- `opts.priority` is an integer, default `100`;
- collection order is `priority desc, id asc`;
- setup-time configuration calls the same validation/mutation functions as runtime registration;
- registry mutation runs only on the Neovim main thread.

No optional plugin is required during module load. Optional integration modules are resolved lazily when their adapter is first invoked or explicitly configured.

## Adapter invocation

All asynchronous adapters use one convention:

```lua
adapter(input, done)
```

`done(result, nil)` succeeds. `done(nil, error)` fails. A semantic cancellation uses the surface's documented `nil` or cancelled result, not a fabricated error.

The registry wrapper:

1. validates input shape owned by the frontend;
2. calls the adapter under `xpcall`;
3. permits exactly one `done` call;
4. ignores later completions and records a diagnostic;
5. converts thrown errors to a frontend diagnostic with traceback;
6. applies the fixed surface fallback below.

If invocation originates during a Neovim fast event, schedule it onto the main loop before calling adapter code.

Surface fallback:

| Surface | Fallback |
| --- | --- |
| picker | built-in `vim.ui.select`/native selector |
| transcript renderer | built-in renderer for that node only |
| review | keep review pending and open built-in reviewer |
| permission | `Cancelled` |
| elicitation | `Cancelled`, request remains runtime-owned |
| image renderer | textual attachment |
| notifier | one direct `vim.notify` call |
| navigation | typed failure; never guess a path/range |
| session picker | built-in selector |
| reference provider | drop only the failed provider result |
| image source | drop only the failed source result |

Fallback invocation never recursively re-enters the failed adapter.

## Shared editor types

Use one coordinate convention everywhere.

```lua
Position = {
  line = integer,   -- zero-based
  column = integer, -- zero-based UTF-8 byte column
}

Range = {
  start = Position,
  ["end"] = Position,
}

Location = {
  uri = string,
  range = Range|nil,
}

SelectionSnapshot = {
  location = Location,
  text = string,
}

EditorContext = {
  window = integer,
  buffer = integer,
  uri = string|nil,
  mode = string,
  cursor = Position,
  selection = SelectionSnapshot|nil,
}
```

`phenix_nvim.context.snapshot()` constructs `EditorContext` once. Core `Reference`, reference providers, context tools, navigation, and UI tools consume it. No tool/provider reinterprets Vim marks independently.

Blockwise selection remains unsupported until #504 implements exact rectangular snapshot semantics.

## References

A reference provider has this exact shape:

```lua
provider(EditorContext, done)

-- result
{
  {
    id = string,
    label = string,
    detail = string|nil,
    reference = ComposeReference,
  },
  ...
}
```

`ComposeReference` is the canonical #504 compose item constructor input. Providers do not return prompt prose.

Reference collection:

1. snapshot editor context once;
2. invoke providers in registry order;
3. concatenate successful candidates;
4. require candidate IDs to be unique after prefixing with provider ID;
5. pass the resulting picker items to the configured picker;
6. insert the selected reference through #504's normal compose path.

Provider failure does not discard candidates from other providers.

Built-in providers cover current location, visual selection, open buffers, diagnostics, and quickfix.

## Picker

Input:

```lua
{
  title = string,
  items = {
    { id = string, label = string, detail = string|nil, value = any },
    ...
  },
}
```

Output is the selected item ID or `nil` for cancellation. An adapter cannot replace `value`; the registry resolves the ID back to the original item.

The built-in adapter uses `vim.ui.select` when available and a dependency-free native floating list otherwise.

## Transcript renderers

Renderers receive immutable projected node data and return declarative render output. They do not receive a buffer handle or runtime object.

```lua
renderer(node) -> RenderBlock

RenderBlock = {
  lines = { string, ... },
  highlights = {
    {
      line = integer,
      start_column = integer,
      end_column = integer,
      group = string,
    },
    ...
  },
}
```

Coordinates in `RenderBlock` are relative to the returned block and use zero-based UTF-8 byte columns.

Renderer IDs are node-kind strings. One effective renderer exists per kind. Highest priority wins; equal priority resolves by registry ID. Failure falls back to #504's renderer for that node only.

Plain user/assistant prose is never delegated to a structured renderer. It remains Markdown text.

## Review adapter

Input:

```lua
{
  review = ReviewRecord,
  decide = function(decision, done) end,
}
```

`ReviewRecord` is the exact #504 application type. `decision` is `"accept"` or `"reject"`.

The wrapper captures `review.id` and `review.revision`. `decide` always calls the #504 runtime review-decision path with that expected revision. Adapters cannot supply another review ID or revision.

The adapter may open windows, navigate hunks, and invoke `decide`. It cannot mutate files or set terminal review state locally.

If adapter presentation throws, #504's built-in reviewer opens and the runtime review remains pending.

## Permission adapter

Input is the exact application `PermissionRequest` projected to Lua.

Output must convert to one of the exact application `PermissionResponse` variants. The wrapper rejects any other value and returns `Cancelled`.

Close, Esc, timeout owned by the UI, thrown error, double completion, or invalid output returns `Cancelled`.

The adapter never receives or mutates runtime authority objects.

## Form model

`phenix_nvim.forms` converts supported `PhenixSchema` into one deterministic frontend form tree.

```lua
FormNode =
  { kind = "string", path, label, multiline, value }
| { kind = "bool", path, label, value }
| { kind = "i64", path, label, value }
| { kind = "u64", path, label, value }
| { kind = "f64", path, label, value }
| { kind = "optional", path, label, present, child }
| { kind = "record", path, label, fields = { FormNode, ... } }
| { kind = "choice", path, label, options = { { id, label, value }, ... }, selected }
| { kind = "list", path, label, item_schema, items = { FormNode, ... } }
```

`path` is a list of structural field/index segments from the schema root. It is identity for validation errors.

Supported schema mapping:

```text
String -> string
Bool -> bool
I64 -> i64
U64 -> u64
F64 -> f64
Option<T supported> -> optional
Table/record with supported fields -> record
Variant where every case is Unit -> choice
List<T supported scalar or unit variant> -> list
```

`Callable`, `Object`, `Any`, `Never`, arbitrary `Map`, recursive/unbounded schemas, variants carrying compound payloads, and lists of compound records are unsupported in #506. Return `unsupported_schema` with the exact schema path. Do not produce a prose fallback.

Renderer flow:

```text
PhenixSchema
-> FormNode
-> configured elicitation adapter
-> candidate Lua value
-> native from_lua(original_schema, candidate)
-> exact PhenixValue or validation error
```

The original schema is retained outside the renderer and is always used for final validation.

## Elicitation adapter

Input:

```lua
{
  request = ElicitationRequest,
  form = FormNode,
  errors = { path_string -> message },
}
```

Output is a candidate Lua value, `nil` for cancellation, or an error.

Invalid candidate does not answer the runtime request. The built-in controller maps conversion errors back to form paths and reopens the same form state. Cancellation returns application `ElicitationResponse::Cancelled`.

## Image sources

Image source input:

```lua
{ context = EditorContext }
```

Output:

```lua
{
  mime_type = string,
  bytes = native_bytes_or_string,
  source_uri = string|nil,
  display_name = string|nil,
}
```

Every result is copied into #504's immutable attachment model before insertion.

The built-in file source is always registered. Optional clipboard/source plugins are lazy and may fail independently.

## Image renderer

Input:

```lua
{
  attachment = immutable_attachment,
  buffer = integer,
  window = integer,
  range = Range,
}
```

Output is a handle:

```lua
{
  update = function(new_window, new_range) end,
  close = function() end,
}
```

Both methods are idempotent. `close` is called on marker deletion, sidebar teardown, buffer wipe, disconnect, or renderer replacement. Renderer failure falls back to #504 textual attachment display.

## Notifier

Input:

```lua
{ message = string, level = integer, title = string|nil }
```

The adapter is synchronous and must not yield. Failure falls back once to `vim.notify`.

Durable runtime errors remain transcript/session state. Notifications are presentation only.

## Navigation

One navigation adapter implements:

```lua
open(Location, done)
reveal(Location, done)
highlight(Location, done)
focus(Location, done)
```

The wrapper validates URI and range before adapter invocation. `file://` is the built-in supported scheme. Unknown schemes return typed unsupported-location errors unless the configured adapter explicitly handles them.

The built-in file adapter resolves URI -> filename once, opens the buffer, and translates zero-based byte coordinates to Neovim API calls without changing semantic coordinates.

## Session picker

Input:

```lua
{
  sessions = { SessionInfo, ... },
  active_session_id = string|nil,
}
```

Output is one existing session ID or `nil`. The wrapper rejects an ID not present in the supplied list.

The adapter never creates, resumes, closes, or mutates sessions itself. The caller invokes #504 runtime methods after selection.

## Status API

Public API:

```lua
local status = require("phenix_nvim.status")
status.get()            -- immediate table, no yield/I/O
local stop = status.listen(function(snapshot) ... end)
```

`listen` fires only when the semantic snapshot changes by deep equality. Render ticks and cursor movement do not emit status updates unless they change a status field.

## Client-tool template registry

`phenix_nvim.tools` owns frontend-local registration templates. Templates are not runtime admissions.

Public API:

```lua
local tools = require("phenix_nvim.tools")
local stop = tools.register(definition, lua_function)
tools.enable_defaults()
tools.disable_defaults()
```

A template contains:

```lua
{
  id = string,
  description = string,
  input = PhenixSchema,
  output = PhenixSchema,
  requires_permission = boolean,
}
```

On active-session selection:

1. remove live admissions for the previous session when possible;
2. lift each template function under the current client owner generation;
3. call native #505 `phenix.tools.register` through `phenix_nvim.runtime` for the selected session;
4. store returned removal handles by template ID.

On disconnect, drop handles/templates' live-generation state. Runtime generation retirement removes stale admissions. On reconnect, every template is lifted again and re-admitted with new refs.

`stop` removes the template and its current admission. It is idempotent.

Third-party Neovim plugins use `phenix_nvim.tools.register`; they do not receive native client userdata and do not import private runtime modules.

## Exact default tool contracts

All schemas below are ordinary #505 tool schemas.

Shared values:

```text
Position { line: U64, column: U64 }
Range { start: Position, end: Position }
Location { uri: String, range: Option<Range> }
SelectionSnapshot { location: Location, text: String }
BufferInfo { uri: String, filetype: String, modified: Bool }
BufferSnapshot { info: BufferInfo, changedtick: U64, text: String, truncated: Bool }
Diagnostic { location: Location, severity: String, message: String, source: Option<String>, code: Option<String> }
QuickfixItem { location: Location, text: String, type: Option<String> }
Viewport { location: Location, text: String, truncated: Bool }
```

Context tools:

| Tool | Input | Output | Permission |
| --- | --- | --- | --- |
| `nvim.context.current_location` | Unit | `Location` | no |
| `nvim.context.selection` | Unit | `Option<SelectionSnapshot>` | no |
| `nvim.context.buffer` | `{ uri: Option<String>, max_bytes: U64 }` | `BufferSnapshot` | no |
| `nvim.context.buffers` | Unit | `List<BufferInfo>` | no |
| `nvim.context.diagnostics` | `{ uri: Option<String>, limit: U64 }` | `List<Diagnostic>` | no |
| `nvim.context.quickfix` | `{ limit: U64 }` | `List<QuickfixItem>` | no |
| `nvim.context.viewport` | `{ max_bytes: U64 }` | `Viewport` | no |

Bounds:

```text
max_bytes: default 65536, hard max 262144
limit: default 100, hard max 500
```

Truncation is UTF-8 safe. `BufferSnapshot` and `Viewport` set `truncated = true` when bounded.

UI tools:

| Tool | Input | Output | Permission |
| --- | --- | --- | --- |
| `nvim.ui.show_location` | `Location` | Unit | no |
| `nvim.ui.highlight_range` | `{ location: Location, duration_ms: U64 }` | Unit | no |
| `nvim.ui.focus_buffer` | `{ uri: String }` | Unit | no |
| `nvim.ui.pick_file` | `{ title: Option<String> }` | `Option<Location>` | no |
| `nvim.ui.show_diff` | `{ review_id: String }` | Unit | no |

`duration_ms` defaults to 1500 and is capped at 60000. `show_diff` resolves the current runtime review by ID and delegates the configured review adapter. It cannot accept or reject the review.

Default context/UI tools reuse the exact same context, picker, navigation, and review helpers as normal frontend actions.

## Privileged Lua tools

Config:

```lua
require("phenix_nvim").setup({
  tools = {
    lua = false,
  },
})
```

`false` is the default. `true` enables all three tools. All three always set `requires_permission = true`; #506 does not provide a bypass switch.

Contracts:

| Tool | Input | Output |
| --- | --- | --- |
| `nvim.lua.eval` | `{ expression: String }` | `Any` |
| `nvim.lua.exec` | `{ chunk: String }` | `Any` |
| `nvim.lua.reload_module` | `{ module: Option<String> }` | `{ module: String }` |

Execution:

- `eval` compiles `return <expression>`;
- `exec` compiles the supplied chunk;
- both execute in the live Neovim Lua global environment and make no sandbox claim;
- syntax/runtime errors become structured tool failure;
- result conversion uses the declared `Type::Any`; non-convertible Lua values fail structurally;
- `reload_module` defaults to `phenix_nvim`;
- allowed module names must equal `phenix_nvim` or start with `phenix_nvim.`;
- reload saves the exact prior `package.loaded[module]`, sets it to nil, calls `require`, and restores the prior value on failure;
- successful reload returns the module name and keeps the new `package.loaded` value.

## Optional third-party adapters

Do not add mandatory Telescope, fzf-lua, Snacks, Diffview, Gitsigns, DAP, or image-plugin dependencies.

If repository fixtures prove adapter compatibility, keep each fixture tiny and load it only in its focused Product check. The integration registry contract, not a third-party plugin API, is the acceptance target.

## Required tests

### Registry

- every singleton has a built-in after startup/reset;
- collection ordering is priority desc then ID asc;
- duplicate IDs reject;
- lazy optional module failure affects one adapter only;
- double async completion is ignored and diagnosed;
- thrown adapter error follows the fixed fallback table.

### Context/reference

- Reference and all default context tools consume one `EditorContext` snapshot;
- location coordinates round-trip without off-by-one conversion;
- provider failure does not discard other provider candidates;
- picker returns the original semantic value by ID.

### Transcript/review

- structured renderer cannot mutate buffers directly through the contract;
- renderer failure falls back for one node;
- review adapter receives runtime data and revision-bound `decide` closure;
- stale review decision surfaces runtime error;
- adapter failure leaves review pending and opens built-in reviewer.

### Permission/forms

- invalid permission output, close, throw, and double completion return Cancelled;
- supported form schemas round-trip through original-schema validation;
- unsupported compound schema reports exact path;
- invalid form candidate never answers the runtime request.

### Images/navigation/status

- image source snapshots bytes before compose insertion;
- renderer handle closes on every owner teardown path;
- unknown navigation scheme fails unless adapter supports it;
- session picker cannot invent an ID;
- status `get()` performs no runtime I/O;
- status listeners do not fire on render-only ticks.

### Tools

- default tool schemas match this document exactly;
- bounds/truncation are enforced;
- default tools execute on the Neovim thread through #505 admissions;
- third-party `phenix_nvim.tools.register` reaches model-visible invocation without private imports;
- session switch removes/replaces admissions;
- disconnect retires refs; reconnect creates new refs and admissions;
- privileged Lua tools are absent by default;
- all privileged Lua invocations require permission;
- failed module reload restores the exact old `package.loaded` value.

## Packaged acceptance

After #504 is dependency-ready, one realized Neovim check must:

```text
install one custom permission adapter
install one custom elicitation renderer
install one custom navigation adapter
install one custom review adapter
register one third-party Lua tool
connect to packaged phenix-acp
create/resume session
prove default nvim.context.* invocation
prove custom nvim.ui.show_location path
invoke third-party tool model -> runtime -> client -> Lua -> result
exercise structured review presentation and runtime decision
exercise permission and elicitation through custom adapters
optionally enable nvim.lua.eval and prove permission gate
force disconnect
reconnect
prove old refs/admissions stale and templates re-admitted with new generation
```

No handwritten ACP or private application schema may appear in production Lua or the acceptance fixture.

## Implementation order

1. Add the registry core and exact shared editor types.
2. Move #504 built-ins behind singleton adapters without behavior change.
3. Add reference-provider/picker collections and transcript renderer collection.
4. Add form model, renderer boundary, and original-schema validation loop.
5. Add image source/renderer, notifier, navigation, session picker, and status listener contracts.
6. Add frontend-local client-tool template registry.
7. Register bounded default context/UI tools with the exact schemas above.
8. Add privileged Lua tools behind `tools.lua = true` and mandatory permission policy.
9. Add focused deterministic regressions.
10. Add one packaged integration acceptance check.
11. Reconcile PR body against exact HEAD, remove `.phenix/work/pr-506.md`, run exact-head Source, Rust, Product, Docs, Maintenance.

## Completion gate

#506 is complete when #504 is a green dependency boundary, every adapter/tool behavior is determined by this document, no PR checklist item requires an architectural choice, the temporary working-state file is absent, and the packaged acceptance flow passes on exact HEAD.
