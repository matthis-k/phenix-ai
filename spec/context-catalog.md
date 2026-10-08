# Context catalog

status: implemented

## Ownership

The conductor owns durable context identity, discovery, loading, versioning, and injection history. Backends receive projected model context; backend conversation state is not canonical context.

This slice introduces exact references and the common context catalog. Context projection, pruning, compaction, decisions, and history search remain later slices.

## Exact references

Durable provenance uses typed exact identities. Persisted records never store moving aliases such as `current-plan`.

Initial reference kinds are:

```text
objective:<id>
plan:<id>
execution:<id>
event:<sequence>
file-observation:<id>
lsp-observation:<id>
context:<id>@<revision>
```

Later slices add artifact, decision, and checkpoint references to the same resolver contract.

A query-time alias may resolve to an exact reference before persistence. Durable state stores only the resolved identity.

Reference resolution is conductor-owned. Resolution returns the typed referenced record or a typed unavailable result. A reference never silently resolves to a newer revision or replacement entity.

## Resource identity

Every model-visible resource has two identities:

- logical identity, stable across revisions;
- immutable content identity, tied to the exact bytes or durable revision shown to the model.

Loading a mutable project document therefore records its content hash or immutable observation. Loading a skill records the exact configured skill revision/content identity. Loading a durable entity records its exact typed reference.

Replay can identify the exact material that was available to an execution even after files or configuration change.

## Context tiers

The catalog classifies context into three tiers.

### Mandatory content

The conductor includes the bytes required for correct execution. Initial mandatory content is:

- active objective identity and current goal;
- current relevant plan and step identity where assigned;
- execution authority and conductor constraints;
- applicable scoped agent instructions such as `AGENTS.md`.

Mandatory content is not loaded through optional discovery.

### Mandatory metadata

The conductor exposes compact descriptors for resources the execution may load.

Initial descriptor sources are:

- model-eligible skills;
- optional project context documents;
- durable objectives and plans relevant to the execution.

Later slices add decisions, history results, and artifacts.

### Discoverable content

The resource body enters model context only after an explicit conductor load operation or another authorized conductor policy requests it.

`CONTRIBUTING.md` and `DEVELOPMENT.md` are optional project context. They are descriptors by default rather than unconditional prompt content. Scoped `AGENTS.md` or `AGENTS.override.md` remains mandatory instruction content.

## Descriptor

The common descriptor contains:

```text
id
kind
title
description
scope
revision
estimated_cost
```

`id` is a logical context-resource identity. `revision` identifies the exact available content revision. `estimated_cost` is deterministic size metadata for later context budgeting; it is not authority.

Resource-specific APIs may expose extra fields, but discovery and loading use this common identity contract.

## Scope

A descriptor declares the workspace, execution, objective, path, or configuration scope that controls its applicability.

Catalog construction filters resources before exposing descriptors. A descriptor does not grant access to material outside the execution's existing authority or callable delegation.

## Loading

Optional context enters an execution through a conductor operation. The operation resolves the descriptor revision before recording the load.

A durable context injection records:

```text
execution
source_ref
source_revision
requested_by
reason
lifetime
content_identity
```

`requested_by` is one of:

```text
agent
user
orchestration
context_policy
hook
frontend
```

`lifetime` is one of:

```text
single_request
execution
objective
```

Loading a resource does not make it session-global. The injection lifetime controls later projections.

A stale descriptor revision causes a typed conflict. The conductor does not substitute the current revision for the requested revision.

## Skills

Skills use the common catalog instead of a separate discovery model.

Model-eligible skills appear as mandatory metadata. Manual-only skills remain available only through explicit user activation and are not advertised to the model.

`phenix_skill_load` may remain as a compatibility-shaped semantic operation while this slice lands, but its implementation must resolve and record the same canonical context resource and injection as the general loader. The completed slice has one semantic loading path.

Skill resources inherit the loaded skill's exact revision and authority checks. A resource read cannot escape the skill root.

## Project context

Scoped agent instructions remain mandatory content because they constrain execution behavior.

Startup instruction discovery is path-based and bounded. The default project-root marker is `.git`. Discovery walks ancestors from the session working directory to the nearest configured project-root marker. If no marker is found, only the working directory is considered. An empty marker list also selects working-directory-only behavior.

For each directory from project root to working directory, discovery selects at most one instruction file in precedence order:

1. `AGENTS.override.md`;
2. `AGENTS.md`;
3. configured fallback filenames, in configured order.

Selected files are loaded root-to-leaf so deeper instructions are later and more specific. Siblings and descendants below the working directory are not scanned. The default shared project-instruction budget is 32768 bytes. The budget is consumed root-to-leaf; the final file is truncated when it exceeds the remaining budget and later files are omitted.

The default options are:

```text
context.project_root_markers = [".git"]
context.project_doc_fallback_filenames = []
context.project_doc_max_bytes = 32768
```

General project documents such as `CONTRIBUTING.md` and `DEVELOPMENT.md` remain discoverable context through explicit project-context discovery. They are not part of the default mandatory startup instruction scan. Discovery records path, scope, content identity, description metadata, and estimated cost. Loading records the exact revision consumed by the execution.

A changed file produces a new resource revision. Existing injections retain the old immutable content identity.

## Objectives and plans

The catalog can describe durable objectives and plans without copying their bodies into a second store.

Descriptors point to exact durable identities. The objective and plan stores remain authoritative for meaning and lifecycle.

An execution's primary objective and assigned plan step are mandatory context identities. Other related objectives and plan history are discoverable metadata until explicitly loaded.

## Persistence

Context resource revisions and injections live in the canonical workspace SQLite database. The journal records ordered semantic injection events. SQLite stores the same facts relationally and reconstructs them without backend conversation state.

Persisted context data stores known-secret-redacted material only. Exact source identity remains available even when the bytes are withheld by policy.

## Concurrency

Mutable resource discovery is revisioned. A load request names the descriptor revision it observed. If that revision is no longer current, the conductor returns a conflict or resolves the historical immutable revision when it is already durable. It never silently changes the requested content.

## Invariants

1. Durable provenance stores exact typed references.
2. Every model-visible optional resource has logical and immutable content identity.
3. Skills, project context, objectives, plans, and later knowledge sources use one discovery/loading contract.
4. Optional context enters through recorded conductor injections.
5. Context lifetime is explicit and never defaults to session-global.
6. Discovery and loading preserve existing authority and callable delegation.
7. Scoped agent instructions remain mandatory; general project documents are discoverable by default.
8. Backend conversation state is not a context store.
9. Reference resolution never follows supersession or revision changes implicitly.
10. Later context projection may omit bytes, but this slice preserves the exact identities needed to recover them.

## Slice completion

This slice is complete when exact typed references resolve across existing durable entities, the common descriptor/catalog replaces independent skill and project-document discovery paths, optional loads create versioned durable injections, project documents use mandatory-versus-discoverable classification, SQLite and replay reconstruct the same resource/injection state, and focused regressions cover stale revisions, scope, authority, lifetime, and restore.