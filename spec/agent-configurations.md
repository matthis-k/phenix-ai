# Agent configurations

status: partial
coverage:
  - rust/crates/phenix-agent-configurations/src/lib.rs
  - rust/crates/phenix-harness/src/lib.rs
  - rust/crates/phenix-harness/src/main.rs

## Contract

Agent configurations are resource-only assembly manifests. They select existing plugins through the normal plugin dependency resolver. They do not add a second agent runtime or configuration resolver.

`phenix.agent.basic` is the minimum first-party agent execution chain:

```text
phenix.agent.basic
├── phenix.agent-loop
├── phenix.context
├── phenix.execution
├── phenix.harness.invocation-defaults
├── phenix.models
└── phenix.step-runner
```

The basic configuration does not depend on `phenix.options`. Invocation defaults use the literal `default` routing profile when no options provider is bound.

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

The advanced manifest must depend on `phenix.agent.basic`. It must not copy the basic dependency list. Changes to the basic configuration therefore flow into advanced through ordinary dependency resolution.

Provider packages, ACP adapters, and other external protocol bindings are separate composition choices. Agent configuration does not decide which model provider or client protocol is installed.

## Resolution

The harness adds both assembly manifests to the same first-party catalog used by explicit plugin selection.

Selecting only `phenix.agent.basic` must resolve the basic dependency closure and build without `phenix.options`.

Selecting only `phenix.agent.advanced` must resolve:

```text
advanced
-> basic
-> basic dependencies

advanced
-> advanced-only dependencies
```

The resolved harness contains both assembly manifests. This keeps the configuration ancestry inspectable.

The normal default harness is composed from `phenix.agent.advanced` and then layers model-provider packages on top. It must not maintain a second hard-coded copy of the advanced agent plugin list.

CLI plugin overrides use the same advanced configuration as their default seed. The resolver rejects disabling a dependency while its owning configuration remains enabled.

## Regression requirements

- basic builds without `phenix.options`
- basic includes the minimum execution chain
- advanced directly depends on basic
- advanced does not repeat basic direct dependencies
- advanced resolves basic dependencies transitively
- the default harness resolves through advanced and therefore preserves the basic ancestry
- advanced adds options, memory, compaction, planning, workers, language support, hooks, diagnostics, and other optional first-party services
- CLI and environment plugin selection accept both configuration ids
