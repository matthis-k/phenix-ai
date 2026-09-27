# Runtime inspection

status: implemented baseline

## Goal

Give agents and developers a read-only view of the live Phenix runtime so they can verify plugin behavior from runtime facts.

Runtime inspection answers current-state questions. Structured logs answer historical questions. Both use the same stable runtime identities.

## Model

```text
canonical runtime state
|
+-- resolved component graph
+-- execution projection
+-- observable values
|
+-- phenix.inspect     current state
|
`-- structured logs   ordered history + references
```

**Inspection is authoritative for current state.** It reads the state owned by the runtime subsystem being inspected.

**Logs are evidence of what happened.** Logs may reference values, executions, graph generations, components, calls, and retained content. Logs do not define current state.

A debug plugin must not maintain a second copy of runtime state.

## Agent tool

The default application agent receives the read-only `phenix.inspect` tool.

Input:

```text
{ query: string }
```

Baseline queries:

| Query | Result |
| --- | --- |
| `graph` | graph generation, resolved components, imports, selected providers, effective import authority |
| `execution` | current root execution record |
| `dag` | current root execution plus descendant executions and worker tasks |
| `values` | all values in the application ObservableStore with owner, schema, version, snapshot policy, and current value |
| `value <value-id>` | one observable root value with owner, schema, version, and current value |
| `help` | supported queries |

The tool returns `PhenixValue`. It does not render a second text-only diagnostic format.

## Generation semantics

Inspection runs inside the current model execution. Graph inspection reads the component graph pinned to that call's `GraphGenerationId`.

A resident trial generation therefore sees its own resolved graph. Inspection must not silently switch to the default generation.

Execution records carry their graph generation. A DAG query starts at the current execution and includes only descendants reachable through `parent_execution`.

## Values

Observable inspection reads `ObservableStore` directly.

Each reported root includes:

```text
id
owner
version
schema
snapshot_policy
value
```

`value <id>` addresses the stable `ValueId`.

Plugins that want generic live-state inspection should publish relevant state through the observable/value contracts. Private implementation state remains private unless the plugin exposes it through a typed inspection contract.

## Execution projection

The execution service owns the execution projection.

Read-only projection operations expose all execution and worker-task records. They do not mutate lifecycle state or authority.

The agent-facing DAG query filters that projection from the current root.

## Graph

The kernel owns resolved topology.

Plugin code may read the component graph pinned to its current call through `KernelAccess`. The inspection tool reports:

- component identity and owner;
- execution backend;
- imported interface;
- required/optional status;
- resolved provider component and plugin;
- provider execution backend;
- effective import authority.

This is observation only. Reading topology grants no service, capability, filesystem, network, or environment authority.

## Logs

Structured logs remain append-only diagnostics.

Runtime events should carry stable identifiers where available:

```text
graph_generation
session_id
execution_id
component/plugin
interface/service/callable
value_id + version
content/reference id
```

Large payloads stay behind content references.

An agent can use logs to find the relevant identity, then use `phenix.inspect` to inspect current state.

## Security

Inspection obeys the caller's generation and runtime boundary.

Inspection never returns secret values merely because a secret provider exists. Secret-bearing plugins expose redacted descriptors or explicit safe inspection values.

Inspection is read-only. It must not become a bypass for authority checks or plugin encapsulation.

## Follow-ups

- path-addressed `value <id>.<path>` queries;
- bounded filtering for large value sets and DAGs;
- model-turn projection inspection with source and revision provenance;
- query by component, service, callable, or invocation id;
- log-reference expansion from the same agent tool;
- application-interface `inspection` and `diagnostics` operations backed by these canonical readers;
- observable subscriptions for watch mode.
