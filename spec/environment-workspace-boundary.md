# Environment, Workspace, and tool boundary

status: partial
coverage:
  - rust/crates/phenix-sdk/src/contracts/environment.rs
  - rust/crates/phenix-plugin-environment-local/src/lib.rs
  - rust/crates/phenix-plugin-workspace/src/component.rs
  - rust/crates/phenix-plugin-workspace/src/implementation.rs
  - spec/workspace-execution.md
  - spec/process-confinement.md

## Decision

Phenix separates execution reality, project semantics, and model exposure.

```text
model
  |
  v
Tool / callable
  |
  v
Workspace
  |
  v
Environment
  |
  +-- filesystem view
  +-- process creation and descendants
  +-- persistent process handles
  +-- network / IPC / secrets when implemented by that provider
```

An **Environment** is the replaceable provider that defines the world an operation can observe and affect.

A **Workspace** is a project binding inside one Environment. It owns project-relative semantics such as the workspace root, relative-path validation, exact file versions, conflict detection, search semantics, and Patch/Exec convenience operations.

A **Tool** is a model-facing projection. Tools decide which Workspace or Environment capabilities are exposed to the model and own model-visible aliases such as `shell-1`.

The kernel owns none of those domain policies. Core owns component/provider resolution, authority attenuation, generation pinning, plugin lifecycle, and call provenance.

## Boundary

### Environment owns execution reality

Only the selected Environment provider may perform the underlying filesystem or process operation for a Workspace.

Environment providers may implement execution using:

- the local host;
- a Linux namespace or other local sandbox;
- an overlay/copy-on-write filesystem;
- SSH;
- a container;
- a VM;
- another plugin-defined backend.

The Environment contract is an ordinary typed component interface. A plugin provides an Environment by exporting `EnvironmentInterface`. Workspace imports that interface through normal provider resolution. Workspace does not depend on a particular Environment plugin.

The default product currently selects `phenix.environment.local`. That provider intentionally preserves the existing unrestricted local-host execution semantics. It is a default provider, not part of the Workspace contract.

### Workspace owns project semantics

Workspace may:

- resolve a relative project path against its bound root;
- reject `..` or absolute paths at a project-relative API;
- compute and compare exact content versions;
- reject stale Patch/write operations;
- implement deterministic search over the Environment view;
- choose the workspace root as the default working directory for Exec.

Workspace must not:

- construct a sandbox;
- decide which host directories are visible;
- spawn a host process directly;
- use `std::fs` as an alternate path around its Environment;
- parse shell command strings to infer effects;
- own persistent OS/remote process handles;
- assume its Environment is local.

Workspace authority is therefore not the filesystem security boundary. It remains an admission/capability boundary for callers of Workspace operations. The Environment is the enforcement boundary for the resulting filesystem and process effects.

### Tools own model projection

Environment and Workspace interfaces are not implicitly model-visible.

A shell tool may expose:

```text
shell.open
shell.send(shell-1, ...)
shell.close(shell-1)
```

while internally retaining:

```text
shell-1 -> opaque Environment process handle
```

The model never receives the provider handle.

Another tool may expose only read/search operations even when the same Environment supports process creation or writes.

Tool configuration controls the model surface. It does not redefine Environment semantics.

## Filesystem-view invariant

All Workspace filesystem operations and all processes created for that Workspace must observe the same Environment filesystem view.

Invalid architecture:

```text
Workspace.Read  -> host filesystem
Workspace.Exec  -> sandbox overlay
```

or:

```text
Workspace.Patch -> overlay
shell/python/git -> host filesystem
```

Required architecture:

```text
                +-> Environment.Read/Write
Workspace ------+
                +-> Environment.Exec/OpenProcess
                           |
                           +-> child processes

all paths -> same Environment view
```

This is what makes arbitrary shell writes safe to reason about. Phenix does not classify command strings. If `python`, `git`, `sh`, or a child process writes a file, the operating environment itself determines where that write lands.

A sandbox Environment that cannot enforce this invariant must fail closed instead of advertising itself as a sandbox.

## Persistent processes

Persistent shells are an Environment capability, not a Shell-specific runtime primitive.

The generic Environment contract may provide:

- open process;
- write process input;
- poll process output/status;
- close process.

The provider owns the real process, PTY/channel, process tree, transport, and cleanup.

The model-facing tool owns:

- model-visible session identity;
- the alias-to-handle map;
- shell-specific framing;
- presentation of stdout/stderr;
- which lifecycle operations are callable by the model.

This allows the same Environment primitive to support shells, REPLs, debuggers, language servers, dev servers, and other long-lived processes.

A provider must declare whether persistent processes and PTYs are supported. Lack of PTY support is not silently represented as PTY support.

## Provider replacement

`EnvironmentInterface` is a normal replaceable component interface.

Examples:

```text
phenix.environment.local
  exports EnvironmentInterface

phenix.environment.sandbox
  exports EnvironmentInterface

acme.environment.ssh
  exports EnvironmentInterface
```

Composition chooses one provider using the existing component provider-resolution mechanism. No Environment-specific registry, fallback engine, or plugin API exists.

The Workspace plugin imports `EnvironmentInterface` and is unaware of the selected provider identity except through optional Environment description/inspection data.

Provider replacement must not require changing:

- Workspace commands;
- shell tool schemas;
- model prompts;
- agent loops.

## Environment paths

Paths passed across `EnvironmentInterface` are paths in the selected Environment's namespace, not promises about host paths.

For the unrestricted local provider, an absolute Environment path is the corresponding host path.

For a sandbox/container/VM provider, the same string names a path in that provider's filesystem view.

Workspace stores a root path in Environment namespace terms and resolves project-relative operations against it.

## Process inheritance

Every process created through an Environment must remain inside that Environment for its entire descendant process tree.

The following must not escape by changing executable:

```text
sh
python
git
nix
make
custom binary
child shell
```

A provider that claims filesystem/network confinement must enforce it at its process/backend boundary. Command inspection is not enforcement.

## Authority

Kernel authority and Environment confinement answer different questions.

Authority:

> May this caller invoke this Phenix capability/interface?

Environment:

> What can the resulting operation actually observe or mutate?

Authority attenuates monotonically through the existing kernel rules. An Environment provider cannot increase caller authority.

Filesystem path policy is not encoded as a new Core authority lattice. A sandbox/policy plugin may use configuration to construct or select an Environment with a restricted view.

## Policy and presets

Default presets may be opinionated. The mechanism is not.

A product-level preset may choose or configure an Environment such as:

```text
workspace
host-review
home
readonly-host
unrestricted
ssh
```

Those names are configuration conveniences. They are not kernel or Workspace semantics.

A plugin may provide different policy or Environment behavior without changing Core, Workspace, or Tool contracts.

## Review/staging

A future review Environment may expose a merged copy-on-write view:

```text
host lower
+ staged upper
= Environment view
```

Workspace writes and shell writes both target that view.

Promotion/rejection belongs to the Environment/policy implementation, not Workspace.

The user may review accumulated net changes asynchronously. Model execution does not need to stop for each review-gated write.

Acceptance should preserve the visible Environment state where possible. Rejection may invalidate persistent processes if their assumptions depend on rejected state; the provider must report or enforce that lifecycle transition explicitly.

## Current implementation slice

Implemented in this PR:

- typed `EnvironmentInterface`;
- filesystem metadata/read/write/directory primitives;
- scratch process execution;
- generic persistent process open/input/poll/close primitives;
- provider capability description;
- `phenix.environment.local` provider;
- Workspace required import of `EnvironmentInterface`;
- Workspace file operations routed through Environment;
- Workspace shell/Git process execution routed through Environment;
- default-suite selection of the local provider;
- first-party selected-suite defaulting to the local provider.

The local provider currently:

- uses the host filesystem/process namespace;
- supports persistent pipe-backed processes;
- does not provide a PTY;
- does not provide sandboxing or staged review.

Not implemented by this PR:

- Linux sandbox provider;
- overlay/review provider;
- SSH Environment provider;
- container/VM providers;
- Environment selection configuration/presets;
- tool-level persistent shell aliases;
- PTY implementation;
- network/secret/IPC Environment contracts;
- staged-change UI.

Those are independent follow-up providers/features built on this boundary.

## Required regressions

The architecture is considered preserved only if tests prove:

- Workspace cannot resolve with no Environment provider.
- A normal provider can satisfy the Environment import.
- Workspace direct read/write goes through Environment rather than host filesystem code.
- Workspace shell and Git execution go through Environment.
- Environment process execution uses the requested Environment working directory.
- persistent process handles are provider-owned opaque strings.
- persistent process cleanup happens on explicit close and provider stop.
- replacing the Environment provider does not require changing Workspace or tool contracts.
- future sandbox provider tests prove direct filesystem operations and shell-originated writes see the same view.
- future sandbox provider tests prove arbitrary child processes cannot escape the view.
- unsupported requested confinement fails closed.

## Ownership summary

| Concern | Owner |
| --- | --- |
| Component/provider selection | Core |
| Authority attenuation | Core |
| Environment implementation | Environment provider plugin |
| Filesystem/process enforcement | Environment provider/backend |
| Persistent process real handle | Environment provider |
| Workspace root/project semantics | Workspace plugin |
| Exact versions/conflict checks | Workspace plugin |
| Model-visible tool schema | Tool plugin/application |
| Model-visible shell alias | Shell tool |
| Presets/default provider choice | Product configuration |
| Sandbox/review policy | Replaceable policy/Environment plugins |

The boundary rule is:

> Environment owns the world. Workspace owns where the project is in that world. Tools own what parts of that world the model can invoke.
