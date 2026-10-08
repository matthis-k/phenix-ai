# Agent configurations

status: partial
coverage:
  - rust/crates/phenix-agent-configurations/src/lib.rs
  - rust/crates/phenix-harness/src/lib.rs
  - rust/crates/phenix-harness/src/main.rs

## Contract

Agent configurations are resource-only **profile identities**. The associated Phenix-native profile defaults select ordinary plugins **before** expanding true manifest dependencies; profile defaults are not hard implementation dependencies. Neither Nix nor an alternate graph resolver interprets them.

`phenix.agent.basic` is the minimum first-party agent execution chain:

```text
phenix.agent.basic
├── phenix.agent-loop
├── phenix.application-agent-tools
├── phenix.context
├── phenix.execution
├── phenix.harness.invocation-defaults
├── phenix.models
└── phenix.step-runner
```

The basic configuration does not depend on `phenix.options`. Invocation defaults use the literal `default` routing profile when no options provider is bound. `phenix.application-agent-tools` is an explicit, independently selectable default. It requires sessions but does not require the Basic loop. A foreign agent can reuse the adapter, or a different adapter can replace it.

`phenix.agent.advanced` extends the basic configuration:

```text
phenix.agent.advanced
├── phenix.agent.basic
├── phenix.options
├── phenix.memory
├── phenix.planning
├── phenix.repository-workers
├── phenix.session-tree
├── phenix.language
├── phenix.efficiency-evaluation
├── phenix.jobs
├── phenix.hooks
├── phenix.debug
└── supporting first-party services
```

The advanced **profile** inherits `phenix.agent.basic` as a default profile, not a hard `PluginManifest.dependencies` edge. It must not copy Basic's implementation list. Changes to Basic's defaults flow into Advanced through Phenix-owned profile expansion. Plugins themselves retain genuine hard manifest dependencies.

Provider packages, ACP adapters, and other external protocol bindings are separate composition choices. Agent configuration does not decide which model provider or client protocol is installed.

## Resolution

The harness adds both assembly manifests to the same first-party catalog used by explicit plugin selection.

Selecting only `phenix.agent.basic` expands Basic's **default provider set** and then resolves the actual selected implementation dependency closure. It must build without `phenix.options`.

Selecting only `phenix.agent.advanced` must resolve:

```text
advanced
-> basic
-> basic dependencies

advanced
-> advanced-only dependencies
```

The resolved generation includes both selected profile identities unless explicitly excluded, keeping ancestry inspectable. The runtime graph contains only the effective provider selection plus actual required dependencies; excluded default plugins are not started.

## Regression requirements

- basic builds without `phenix.options`
- basic includes the minimum execution chain
- Advanced's **profile defaults** include Basic as their parent, without a hard manifest dependency
- Advanced does not repeat Basic's default implementation choices
- Advanced inherits Basic defaults during Phenix profile expansion
- advanced adds options, memory, compaction, planning, workers, language support, hooks, diagnostics, and other optional first-party services
- CLI, portable JSON file, environment defaults, and builder expansion accept both profile identities
- Disabling an inherited optional default excludes its plugin from the active manifest set
- Hard implementation dependencies still reject invalid overrides
- Nix-generated and directly supplied portable configurations resolve under the same Phenix logic

## Architectural direction

Basic is the canonical executable *reference graph*, not a privileged or mandatory agent
implementation. Full extends its selected capabilities and may replace providers through the
same graph resolution used for third-party implementations. The kernel remains agent-agnostic.
An independent consumer may select memory/context without choosing Basic or Full.

The shared `AgentLoopInterface` contract is owned by `phenix-sdk` rather than by the
`phenix-plugin-basic-agent` implementation. Basic continues to re-export those symbols
for compatibility. Contract ownership, initial provider composition APIs and portable JSON/CLI profile selection
are implemented on the PR branch. The adapter is independently selectable and a regression exercises a foreign loop without Basic. Application entry-point contract routing and capability-driven tool exposure, replacing provider
*implementations* with independently packaged alternatives, deterministic Basic compaction,
and external forwarding remain outstanding.

See [Composable agent runtime](composable-agent-runtime.md) for the source audit, contract
boundaries, reference graphs and per-slice acceptance tests.
