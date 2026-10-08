# Optional interactive UI plugin

status: partial
owner: phenix.interactive-ui

## Goal

The full Phenix product includes an optional interactive UI plugin. Its model tools are visible only during an execution whose owning frontend advertises support for the required versioned UI contract. The kernel, basic product, model providers, and other plugins have no UI-specific behavior.

This is a plugin, not an application-native rendering subsystem. Nix selects and packages it but does not decide runtime capability admission. Explicit non-Nix plugin configuration must produce the same result.

## Composition

- New independently packaged runtime plugin: `phenix.interactive-ui`.
- `phenix.product.full` selects it through the ordinary dependency resolver.
- `phenix.product.basic` and `phenix.agent.basic` do not select it.
- `phenix.agent.advanced` remains a reusable agent composition; the full *product* determines the default optional UI integration.
- Removing the plugin from a custom product removes its tools and services. Presence of the plugin never implies presence of a capable frontend.
- The plugin depends on the generic `phenix.frontend-services` contract and standard execution/tool infrastructure. It does not import Neovim, Lua, ACP, or a provider-specific SDK.

## Ownership

| Concern | Owner |
|---|---|
| Versioned UI document and event schemas | Interactive UI plugin |
| Document validation, state, revision, session ownership, action admission | Interactive UI plugin |
| Tool declarations, lifecycle, and UI-specific authorization policy | Interactive UI plugin |
| Plugin selection and composition | Full product manifest or explicit configuration |
| Frontend connection identity, root-execution binding, capability catalog | Existing frontend-services plugin |
| Generic tool trigger admission and authority checks | Existing model/tool integration, extended only with a generic frontend-requirement filter if needed |
| Rendering, focus, keymaps, mouse, widget layout | Frontend renderer, initially phenix-ai.nvim |
| Backend-specific model tool encoding | Existing provider integrations |

The kernel must not understand UI components, documents, control events, or frontend renderers. Any generic graph admission change must refer to arbitrary capability contracts, not to UI names.

## Frontend advertisement

The frontend advertises a provider on its live connection through the existing `set_frontend_service_providers` mechanism. The UI provider advertises a versioned structured-document contract and a versioned action contract, for example:

```json
{
  "id": "phenix.interactive-ui",
  "capabilities": ["document.v1", "actions.v1"]
}
```

These strings are scoped to this provider, not global authority tokens. A frontend may advertise only document rendering if it cannot handle actions. A frontend may re-advertise when renderer availability changes.

The execution's *owning* frontend is the only eligible recipient. An unrelated frontend that advertises UI support must not cause UI tools to appear in this execution. Catalog discovery by itself is not sufficient.

A frontend call requires an active execution and active ancestors through its root. A completed child, failed child, or child of a finished root cannot use a retained root binding. Duplicate binding attempts cannot replace the root's owner, and withdrawn provider capabilities invalidate pending calls.

## Tool visibility

The plugin declares exact typed tool entrypoints rather than adding model tools from the application executable:

```text
ui.present(document)       -> { document_id, revision }
ui.update(document_id, expected_revision, patch) -> { revision }
ui.dismiss(document_id)    -> { dismissed }
```

An implementation may start with `ui.present` and add the other entrypoints after the state and event contract exists, but no placeholder tool may be advertised without a working implementation.

For each model dispatch, all conditions must hold:

1. `phenix.interactive-ui` is active in the execution's pinned resolved generation.
2. The exact UI tool entrypoint is resolved and allowed by execution authority.
3. The execution has a live owning frontend connection, not a global fallback connection.
4. That frontend advertises the UI provider and the capability version required by the entrypoint.
5. The model-visible tool is placed into the normal collision-checked descriptor list.

When any condition fails, omit these descriptors entirely. Do not emit a dummy schema, empty tool descriptor, advisory prompt, or tool that only fails at invocation. The model continues with ordinary text output. Other non-UI tools are unaffected.

The selected execution owner and capability generation must be rechecked at invocation. If the frontend disconnects, withdraws capability, or reconnects with a new identity, return a typed unsupported/disconnected/stale error. Never forward a call to a different frontend, even if another frontend advertises the same provider.

The current `application_model_tool_surface` projects graph triggers without a frontend-capability test. Extend its *generic* trigger-selection seam or move selection into a plugin-owned provider. Do not add hard-coded `ui.*`, `phenix.interactive-ui`, or Neovim cases to the application executable. A test must prove that removing this plugin removes UI model descriptors without altering application code.

## Data and event contract

`UiDocument` is a versioned, typed tree with stable node IDs. Initial supported nodes:

- text, label, badge, progress;
- row, column, card, table;
- button, checkbox, select, text input.

Add charts, sliders, and richer layouts later. The schema must reject unknown actions and unsupported node types before the frontend receives them. Negotiated capabilities may narrow the enabled node set.

Documents are content, not privileged frontend code. Never accept raw Lua, shell, JavaScript, HTML, escape sequences, commands, or arbitrary executable callbacks in a document. Renderers sanitize text and enforce depth, node count, string length, patch size, and update frequency limits.

Every document belongs to one session, with a stable ID and monotonically increasing revision. `ui.update` uses an expected revision to reject stale writes. A model must not mutate another session's document. An action identifies the document, node, revision, connection generation, and typed value. The plugin validates the event against the declared control and authorized session before applying it or waking a waiting agent.

Define action behavior explicitly. Display-only controls need no model continuation. Interactive controls either update local document state with a typed acknowledgement or resume the existing waiting execution through a normal plugin contract. Do not represent actions by adding hidden messages to an arbitrary session.

Store durable semantic documents and accepted input results only when their use requires resumption or history. The active renderer connection and pending frontend requests remain ephemeral. Reopen a compatible renderer from plugin-owned document state; reconnect cannot revive old action references.

## Neovim renderer

The first renderer lives in `phenix-ai.nvim`, not in the Rust UI plugin:

- use extmarks and highlights for badges and progress;
- use stable transcript node IDs and incremental rendering;
- handle buttons, checkboxes, selects, and forms through ordinary Neovim keymaps and floats;
- announce the exact supported contract only after renderer initialization succeeds;
- withdraw the advertisement on disconnect or renderer shutdown;
- render a static explanatory fallback for unknown noninteractive content.

Existing execution progress is separate from model-generated `progress` widgets. Execution progress comes from actual execution events, not fabricated model percentages.

## Tests and completion

Composition:
- full includes the plugin; basic and kernel-only do not;
- explicitly disabled plugin disappears from the resolved graph;
- Nix and non-Nix selection resolve identical plugin identities.

Capability matrix:
- UI plugin absent + UI-capable frontend: no UI tools;
- UI plugin present + unsupported frontend: no UI tools;
- UI plugin present + render-only frontend: only permitted display operations;
- UI plugin present + matching interactive frontend: expected UI tools;
- UI plugin present + supported frontend on *another* connection: no UI tools;
- frontend disconnect, withdrawal, reconnection, and model dispatch race: no stale tool exposure or cross-owner routing;
- provider/agent sees no UI-specific flag or alternate tool type.

State and security:
- reject invalid nodes, payloads, patch versions, duplicate IDs, and unauthorized events;
- exact session isolation, bounded document/update sizes, deterministic document replay;
- stale action references cannot act on a replacement connection or document;
- user input cannot bypass ordinary permission policy.

Product:
- packaged full-harness execution with a capable Neovim client displays a badge, live progress, and an interactive control;
- submitting the control yields a typed action and changes only its owning document;
- the same prompt in a frontend without the capability uses ordinary text without exposing UI tools;
- model, client, provider, and runtime test doubles assert tool projection rather than relying on UI screenshots alone.

## Implementation sequence

1. Add the standalone plugin, its typed contract, manifest, package, and full-product composition.
2. Implement plugin-owned document service, validation, action lifecycle, and session persistence.
3. Implement generic per-execution frontend-capability gating for plugin-declared tool triggers. Keep UI identities out of the generic integration.
4. Implement Neovim renderer and live capability registration using the existing frontend service transport.
5. Add end-to-end composition, tool-visibility, interaction, reconnection, and fallback tests before considering the feature complete.

Do not mark this feature complete after manifest or documentation changes alone.
