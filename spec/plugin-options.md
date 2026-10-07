# Options plugin

status: implemented

## Purpose

Provide typed option state for common userspace behavior without moving product policy into core.

The options plugin is an ordinary userspace plugin. Other plugins read options through `phenix.options@1` and may define additional options through the same contract.

## Model

An option has:

- a parsed key;
- one typed default value;
- the scopes where callers may override it.

Supported value types are boolean, integer, string, and string-list.

Supported scopes are global, session, and agent. Session and agent scopes require a non-empty subject identity.

Scope values use these wire forms:

```json
"global"
{"session":"session-1"}
{"agent":"worker"}
```

Resolution order is:

```text
agent override
  -> session override
  -> global override
  -> definition default
```

A missing narrower override falls through. An override never mutates the broader scope.

## Commands

`phenix.options@1` supports:

```text
define
get_definition
set
unset
resolve
list
```

`define` is idempotent for an identical definition and rejects a conflicting redefinition. `set` rejects an unavailable scope or a value whose type differs from the definition.

Option state is durable and owned by `phenix.options`. Core only enforces the ordinary persistence namespace and authority rules.

## Built-in options

The first-party options plugin defines reasonable defaults for common Phenix behavior:

| option | default | scopes |
| --- | --- | --- |
| `session.auto_create` | `true` | global, session, agent |
| `session.reuse_existing` | `true` | global, session, agent |
| `session.max_turns` | `0` (unlimited) | global, session |
| `model.default` | `"default"` | global, session, agent |
| `tools.confirmation` | `"ask"` | global, session, agent |
| `skills.auto_load` | `true` | global, session, agent |
| `context.auto_load` | `true` | global, session, agent |
| `context.project_doc_max_bytes` | `32768` | global, session, agent |
| `context.project_root_markers` | `[".git"]` | global, session, agent |
| `context.project_doc_fallback_filenames` | `[]` | global, session, agent |
| `agent.max_parallel_tasks` | `1` | global, agent |

An option has no effect until a consuming plugin uses it. This keeps the options plugin generic and avoids hidden kernel behavior.

## SDK interaction

The default Phenix SDK session helper resolves `session.reuse_existing` and `session.auto_create` for the requested session and agent before dispatching to the ordinary session interface.

Other SDK modules may consume the corresponding model, tool, skill, context, and agent options as their behavior is implemented.

## Invariants

- Core has no option registry or option semantics.
- Option keys are parsed before entering state.
- Values preserve their declared type.
- Scope precedence is deterministic.
- Narrower scopes only override broader scopes.
- Durable option state belongs to the options plugin.
- Other plugins may add options without modifying the options plugin.
- Reading an option grants no authority.

## Required regressions

- agent overrides session, global, and default values;
- session overrides global and default values;
- global overrides the default value;
- invalid option keys fail parsing;
- global, session, and agent scopes round-trip through their wire forms;
- wrong value types fail before state changes;
- disallowed scopes fail before state changes;
- identical definitions are idempotent;
- conflicting definitions are rejected;
- the SDK session helper changes behavior when scoped session options change.


## Settings file

The wrapper can provide two startup sources: `settings.json` from `PHENIX_CONFIG_DIR`, and typed Nix `settings` materialized separately by `mkPhenix`.

Resolution is source-first. Runtime `Set` values always win. The preferred startup source is next, then the other startup source, then built-in defaults. Within each source, scope precedence is `agent > session > global`.

`mkPhenix.settingsPrecedence` is `"nix"` by default. Set it to `"file"` to let `settings.json` override Nix settings. Startup sources are applied as replaceable snapshots, so removing an entry removes that source's prior value. Runtime values are durable and remain separate.
