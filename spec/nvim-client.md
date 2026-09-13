# Canonical Neovim client

status: partial

## Purpose

`clients/nvim` is the canonical Phenix Neovim frontend. The standalone `phenix-nvim` repository is a deterministic export target, not a second implementation.

The frontend is an Application. It consumes the native `phenix` binding, the fixed application descriptor, and the generic value/capability SDK. It does not implement runtime plugins, session persistence, ACP framing, application schemas, model/provider logic, or file-mutation authority.

`spec/nvim-client-implementation.md` fixes the exact server/client mechanics. This file fixes product behavior.

## Module boundary

```lua
local phenix = require("phenix")
local frontend = require("phenix_nvim")
```

Rules:

- `require("phenix")` remains the native binding.
- `clients/nvim/lua/phenix/init.lua` must never exist.
- only `phenix_nvim.runtime` owns native client userdata;
- other frontend modules consume runtime methods or projected semantic state;
- production Lua contains no raw ACP JSON-RPC, Phenix extension IDs, or private application schema copies.

## State ownership

Phenix owns:

- sessions and transcript;
- execution and tool lifecycle;
- models and routing;
- permissions and elicitation requests;
- pending and terminal edit reviews;
- file mutation and conflict detection;
- persistence.

The frontend owns:

- sidebar layout and focus;
- active session selection for this Neovim instance;
- remembered compose cursor;
- unsent compose document and immutable attachment/reference snapshots;
- transcript render ranges/extmarks derived from runtime state;
- follow-tail state;
- transient presentation errors.

If required runtime state is absent, implement the owning application/SDK contract. Do not create Lua persistence or a private event protocol.

## Runtime adapter

`phenix_nvim.runtime` owns one native client and one bounded Neovim main-thread dispatch loop.

It:

1. starts/connects to the configured `phenix-acp` process;
2. verifies required application capabilities;
3. retrieves the SDK graph;
4. verifies `phenix.sessions.state.get/listen` exist;
5. installs permission and elicitation handlers through client-owned callable refs;
6. subscribes to session state;
7. exposes semantic updates to frontend projections;
8. tracks descriptor-backed async application requests;
9. retires local state on disconnect without mutating durable runtime state.

A missing required capability or SDK path is a connection failure naming the missing contract.

The completed #504 client does not use the standard ACP session helper as a semantic fallback.

## Sessions

Runtime sessions are durable. Neovim session selection is local presentation state.

Required behavior:

- first Send creates a session when none is selected;
- create, list, resume, select, close, and cancel use descriptor-backed application operations;
- resume reconstructs transcript from runtime state;
- Neovim restart creates a new client generation but may resume the same durable session;
- reconnect never revives old client-owned callback refs or client-tool admissions.

The selected session ID is never treated as proof that the runtime session still exists. Runtime errors update the UI.

## Compose and Reference

The compose buffer is an editable ordered multimodal document. `Reference` inserts context at the remembered compose cursor and never sends.

Required flow:

```text
select A -> Reference -> type question A
select B -> Reference -> type question B
attach image
Send
```

Wire order remains:

```text
Resource(A), Text("question A"), Resource(B), Text("question B"), Image(...)
```

Reference rules:

- visual selection captures exact text and source range before focus changes;
- normal mode captures current location;
- captured text is immutable after insertion;
- characterwise and linewise selections are supported;
- blockwise selection fails until exact rectangular capture is implemented;
- references use zero-based line and UTF-8 byte-column coordinates;
- duplicate source references remain distinct compose items;
- deleting a marker deletes only its local compose item;
- typed `@...`, picker results, and direct Reference actions use the same constructors.

Send maps segments to the existing application `Content::{Text, Resource, Image}` values. It never embeds editor metadata in prose.

The frontend records the submitted compose revision. Successful acceptance clears only that exact revision. Newer edits survive older in-flight requests.

## Transcript

`phenix.sessions.state` is authoritative. It exposes runtime `SessionProjectionState` through the generic observable ABI.

The frontend derives stable render nodes from ordered `SessionUpdate` values. It never parses rendered text back into state.

Required behavior:

- full `get()` initializes or repairs the projection;
- ordered observable diffs update it;
- observable version gap or session sequence gap triggers a full read;
- assistant deltas mutate one execution-scoped streaming node;
- tool call/result/failure mutates one call-ID node;
- review updates mutate one review-ID node;
- prose remains ordinary Markdown;
- full rerender is limited to initial render, explicit refresh, gap recovery, or invariant repair;
- manual scrolling disables follow-tail until the user reaches the end or invokes follow explicitly.

## Permissions

#504 ships a dependency-free built-in permission UI.

Rules:

- the frontend can return only a response represented by the application contract;
- close/Esc returns `Cancelled`;
- handler or UI failure returns `Cancelled`;
- the frontend never persists permission authority;
- no presentation outcome can expand runtime authority.

#506 may replace presentation but not these semantics.

## Elicitation

#504 ships a dependency-free schema-driven form for the supported subset fixed in the implementation handoff.

Flow:

```text
PhenixSchema
-> built-in form
-> candidate PhenixValue
-> validate against original schema
-> ElicitationResponse
```

Unsupported schema fails explicitly. No fallback turns a structured elicitation into free-form prompt parsing.

#506 adds richer replaceable presentation over the same candidate/validation path.

## Images

Images are first-class compose/transcript items.

- file acquisition is always available;
- sending does not depend on preview support;
- attachment bytes and MIME type are immutable compose data;
- `vim.ui.img` is optional presentation;
- unsupported renderer/terminal combinations show a stable textual attachment;
- closing or resizing views cleans renderer state without changing the attachment.

## Runtime edit review

A runtime review is a durable execution-owned pending mutation with exact file versions and structured hunks.

The frontend can:

- show files and hunks;
- navigate next/previous change;
- request Accept or Reject using review ID plus expected review revision;
- show conflicts and terminal state returned by the runtime.

The frontend cannot apply a patch or mark acceptance locally. `Accepted` is visible only after the workspace owner applies the prepared mutation successfully. A version conflict leaves the review `Conflicted`.

#506 may replace review presentation only.

## Sidebar

The sidebar uses separate transcript and compose buffers.

Requirements:

- toggle/focus/resize preserves compose state;
- remembered compose cursor survives leaving or hiding the sidebar;
- transcript is read-mostly Markdown;
- compose is a normal editable buffer;
- built-in operation requires no optional third-party plugin.

## Status

Status is a cheap projection of cached frontend/runtime state.

It may show:

- connection state;
- active session;
- active execution state;
- already-projected model/routing label;
- last runtime/presentation error.

Statusline rendering performs no client request, request polling, filesystem I/O, or timer creation.

## Packaging and export

The flake exposes:

- `packages.<system>.phenix-acp`, the configured ACP application executable;
- `packages.<system>.phenix-nvim`, frontend source plus compatible native `phenix.so` and the configured runtime command in its test closure;
- `packages.<system>.phenix-nvim-export`, frontend source plus conductor revision marker and no native binary/runtime executable.

The standalone mirror is generated from `phenix-nvim-export`. Conductor remains the implementation source of truth.

## Acceptance

#504 is complete when one realized flow proves:

```text
launch packaged Neovim
-> load packaged phenix + phenix_nvim
-> start packaged phenix-acp
-> create durable session
-> Reference A, type question A
-> Reference B, type question B
-> attach image
-> Send one ordered multimodal turn
-> stream assistant/tool state through phenix.sessions.state
-> answer one permission request
-> answer one supported elicitation
-> review one runtime edit and receive runtime terminal state
-> close Neovim/runtime process
-> reopen in the same workspace
-> resume the same durable session
-> reconstruct transcript and review state from runtime data
```

The exact head must pass Source, Rust, Product, Docs, Maintenance, frontend package checks, and mirror parity.

## Non-goals

- another socket or JSON protocol;
- frontend-owned application schemas;
- frontend-owned canonical session/transcript/review state;
- automatic Send from `Reference`;
- full transcript rerender per token;
- image preview as a send prerequisite;
- local edit application as runtime acceptance;
- callback semantics outside the generic callable path;
- plugin-host APIs in the client binding.
