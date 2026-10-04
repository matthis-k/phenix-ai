# Product packages and process surface

status: partial

## Purpose

Define the supported Phenix product packages and the process argument surface contributed by loaded plugins.

The product package and the process mode are separate axes.

A package selects a fixed Phenix composition. A mode selects which loaded frontend adapter owns the process after that composition is resolved.

## Product packages

Phenix ships three supported package compositions.

```text
phenix-kernel
  pure kernel/runtime composition

phenix-basic
  kernel
  + phenix.agent.basic
  + the minimal plugins required by the basic agent
  + default process mode providers

phenix-full
  phenix-basic
  + phenix.agent.full
  + every first-party plugin and skill designated as normal Phenix functionality
  + the settings required to make those features operational
```

The packages are distinct packaged binaries or wrappers around distinct immutable Phenix configurations.

There is no runtime `--preset`, `--basic`, or `--full` selector. Selecting the package selects the composition.

The generic kernel remains usable without any agent behavior. Basic remains intentionally small. Full is batteries-included.

Full means operationally enabled. A first-party feature included in Full must not remain behind an additional hidden opt-in unless the feature itself is intentionally conditional on external authority, credentials, or availability.

## Modes

Basic and Full package one or more default process modes.

Examples include:

```text
--mode acp
--mode jsonl
```

`--mode` is the canonical mode selector. Do not add parallel `--acp`, `--jsonl`, or `--frontend` selectors.

Mode names are not a closed enum owned by the executable. Loaded plugins contribute available modes through inspectable metadata.

A mode contribution identifies:

```text
name
description
owning plugin
handler identity
input/output contract if required
```

The process rejects a requested mode unless the selected package's resolved plugin set contributes that mode.

The same executable implementation may back more than one package. Package identity comes from the packaged composition, not argv.

## Extensible process arguments

Loaded plugins may extend the accepted process argument surface.

For example, if a loaded plugin contributes:

```text
--plugin-handled-value <u64>
```

then:

```text
phenix-full --mode jsonl --plugin-handled-value 7
```

is valid.

The same argument is invalid when the contributing plugin is absent from the selected package.

Plugins do not read `std::env::args()` directly. The process frontend owns argv parsing.

## Two-phase parsing

Parsing is intentionally split.

```text
argv
  |
  | bootstrap parse
  |   only process-owned bootstrap syntax such as --mode
  v
packaged composition
  |
  v
inspect loaded plugin surface contributions
  |
  v
build exact accepted argument schema
  |
  v
parse remaining argv
  |
  v
canonical attributed values
  |
  v
resolve/activate generation
  |
  v
selected mode
```

This avoids a global hard-coded option registry while keeping unknown arguments deterministic errors.

No plugin code executes merely to discover its CLI contract. Surface metadata must be inspectable before activation, like other composition metadata.

## Process surface contribution

Process surface declarations are plugin-owned metadata.

The initial contribution vocabulary is:

```text
ProcessModeContribution
  name
  description
  handler

ProcessArgumentContribution
  long_name
  description
  value_schema
  binding
```

A contribution always records stable plugin ownership. Duplicate mode names or long argument names from simultaneously loaded plugins are resolution errors unless a future explicit interposition policy defines otherwise.

Names must be valid long-option names and exclude the leading `--`.

## Argument values

Argument parsing produces typed Phenix values according to the contributed schema.

The initial parser needs deterministic support for scalar startup values:

```text
bool
string
u64
i64
f64
```

Structured values may be added through an explicit encoding contract later. Do not infer arbitrary JSON merely because a string happens to parse as JSON.

Repeated arguments are rejected unless the contribution explicitly declares repeatability.

## Binding

Raw argv is not plugin state.

A parsed argument is first lowered to one canonical attributed process-input value. The owning plugin consumes that canonical value through its declared binding.

The first supported binding should be plugin configuration because startup flags normally describe startup configuration:

```text
argv
-> process CLI frontend
-> canonical ConfigContribution
-> normal configuration resolution
-> plugin-owned typed configuration
```

The contribution identifies the destination configuration namespace and field/path required to materialize the typed value.

This makes startup arguments part of configuration provenance and semantic identity instead of ambient process state.

Operational one-shot argument bindings may be added later as ordinary typed entrypoints. They must not bypass configuration identity when the argument changes stable runtime semantics.

## Precedence

CLI-derived configuration is one configuration frontend source. It does not become a privileged mutation channel.

Its precedence relative to Nix, files, project configuration, and other frontends must be explicit and inspectable.

Equivalent values from different frontends should resolve through the same canonical plugin configuration contract.

## Plugin authoring

Plugin authors should be able to declare process surface metadata without editing the Phenix executable.

The authoring path should lower declarations into the same canonical metadata used by packaged/runtime plugins.

Conceptually:

```rust
#[phenix(process_arg(
    long = "plugin-handled-value",
    config = "example.plugin@1",
    field = "value"
))]
value: u64
```

The exact macro syntax may differ. The semantic requirements do not.

Dynamic/runtime-hosted plugins expose the same metadata through package metadata before activation.

## Modes are plugin-provided

ACP and JSONL are frontend adapters, not product presets.

A package may include both mode providers:

```text
phenix-basic
  phenix.agent.basic
  phenix.adapter.acp
  phenix.adapter.jsonl

phenix-full
  phenix.agent.full
  phenix.adapter.acp
  phenix.adapter.jsonl
```

Then either package can run:

```text
--mode acp
--mode jsonl
```

without changing the agent composition.

A third-party frontend adapter can contribute another mode without changing the executable's mode enum.

## Relationship to existing CLI plugin

`spec/plugin-cli.md` describes a userspace service for discovering and probing external executables such as `git`, `gh`, or `jj`.

That service is unrelated to the Phenix process argument surface defined here.

Use "process surface" for this contract to avoid overloading "CLI plugin".

## Inspection

Runtime/composition inspection must expose:

```text
package composition identity
available modes and owning plugins
available plugin-contributed arguments and owning plugins
selected mode
resolved CLI-derived configuration values with source attribution
```

Help output is generated from the same resolved process surface metadata. It must not maintain a second option list.

## Failure behavior

Reject before running the selected mode when:

- `--mode` names a mode not contributed by the loaded plugin set;
- an argument has no loaded owner;
- two loaded contributions claim the same mode or long option;
- a value does not satisfy the declared schema;
- required argument values are missing;
- CLI-derived configuration cannot be lowered through the declared plugin configuration contract.

Errors identify the argument or mode and, when applicable, the owning plugin.

## Required regressions

- kernel package activates without agent plugins;
- basic package resolves `phenix.agent.basic` and excludes Full-only plugins;
- full package resolves `phenix.agent.full` and all normal first-party plugins;
- Full enables normal first-party behavior rather than only installing providers;
- Basic and Full expose their packaged mode providers;
- `--mode acp` selects ACP only when an ACP mode provider is loaded;
- `--mode jsonl` selects JSONL only when a JSONL mode provider is loaded;
- no parallel frontend selector exists;
- a fixture plugin contribution makes `--plugin-handled-value 7` valid;
- removing the fixture plugin makes the same argument an unknown-argument error;
- the parsed value reaches the fixture plugin as typed canonical configuration;
- invalid values fail before plugin execution;
- duplicate contributed long names are rejected deterministically;
- help is derived from resolved contributions;
- plugins never need direct argv access;
- CLI-derived semantic configuration changes the resolved configuration identity/provenance.

## Completion gate

This contract is complete when:

- kernel, basic, and full are separately packaged immutable compositions;
- Basic and Full select frontend adapters only through `--mode`;
- available modes are contributed by loaded plugins;
- loaded plugins can extend the valid process argument surface through inspectable metadata;
- argv values lower through canonical attributed configuration rather than ambient plugin state;
- unknown/unowned arguments fail deterministically;
- plugin-contributed flags work in packaged products and third-party package metadata;
- inspection and help expose one source of truth for the process surface.
