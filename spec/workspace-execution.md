# Workspace execution

status: specification-only

## Goal

The workspace API should expose only operations that still benefit from typed tool boundaries.

The target agent-facing contract is:

- generic process execution in the selected workspace;
- conflict-checked patch application.

Common CLIs such as `git`, `gh`, `jj`, `rg`, `fd`, `jq`, `cargo`, `nix`, Python, Node, and package managers execute through the generic process path. The command toolbelt remains discovery metadata that tells callers which commands are available and, where useful, whether they are authenticated.

## Target contract

`WorkspaceInterface` converges on two operations.

### Exec

`Exec` runs a command in the workspace provider's execution environment.

The request carries the command plus execution metadata needed for deterministic placement, cancellation, and bounded output. Relative working directories resolve inside the selected workspace. Environment injection is explicit. The provider owns process spawning and transport.

`Exec` is location-independent. The provider may execute locally, over SSH, inside a container, VM, pod, or another process bridge. Callers do not change tools when the provider changes.

### Patch

`Patch` applies one or more file edits against explicitly observed versions.

Each affected file carries its expected version. The provider validates all expected versions before mutation. A stale version rejects the patch before any file changes. Multi-file patches use all-or-nothing validation and should commit atomically when the provider can guarantee it.

Patch responses return the resulting versions for changed files.

## Authority

Process admission and filesystem reach are separate.

Workspace capabilities decide whether the caller may invoke a Workspace operation. The selected Environment decides what the resulting process can observe and mutate.

For the current unrestricted local Environment, Shell or future Exec processes may read and write anywhere allowed by the host OS. The workspace root is their default working directory, not a confinement boundary.

Future confined Environments may instead enforce either:

- read and write only inside the working directory tree;
- host-wide reads with writes limited to the working directory tree.

Those policies must be enforced by the Environment for the full process tree. Command inspection is not enforcement.

`Patch` requires `workspace.write`. Direct Workspace read/write capability checks remain independent from process filesystem policy.

## Placement

`Exec` and `Patch` resolve through the same workspace provider and therefore the same workspace identity.

A remote provider owns both file mutation and process execution. It must not expose remote files while spawning workspace-sensitive commands on the runtime host.

Workspace paths exposed through the contract remain relative. Provider-specific roots, mount paths, remote directories, container paths, and transport identifiers are implementation details.

## Common CLI discovery

The command toolbelt answers availability questions such as:

- whether `git` is installed;
- which version of `cargo` is available;
- whether `gh` is authenticated.

It does not provide separate execution wrappers for those commands. Probes execute through `WorkspaceInterface::Exec` so their result describes the selected workspace rather than the runtime host.

## Transitional operations

The current workspace contract also contains `Read`, `Write`, `Search`, `Shell`, and dedicated `Git` operations.

They migrate in this order:

1. Add `Exec` with explicit placement, cancellation, and bounded output semantics.
2. Route current shell behavior and command-toolbelt probes through `Exec`.
3. Remove the dedicated `Git` operation and `workspace.git` capability.
4. Add conflict-checked `Patch` while retaining the existing exact-version guarantee.
5. Add confined Environment policies for working-directory-only and working-directory-write with host-wide reads; keep `phenix.environment.local` unrestricted.
6. Prove the same contract through a provider that does not use the local filesystem and local process namespace.
7. Remove agent-facing `Read`, `Write`, and `Search` unless benchmarks show a concrete token, latency, reliability, or policy advantage.

The migration keeps compatibility only while required to land the authority boundary safely. The final contract should not retain duplicate ways to execute the same common CLI operation.

## Output and cancellation

`Exec` output is bounded. Responses report exit status, stdout, stderr, and whether either stream was truncated. Streaming transports may additionally emit ordered output updates, but the final result retains the same semantic fields.

Cancellation terminates the workspace-side process or process group. Remote providers must propagate cancellation across their transport rather than only abandoning the local request.

## Invariants

- Workspace-sensitive commands execute through the selected workspace provider.
- Workspace authority gates API admission; Environment policy governs process filesystem reach.
- Confined process policies are enforced by the Environment, never by command classification.
- `Patch` rejects stale observed versions before mutation.
- Common CLI availability metadata describes the selected workspace.
- Provider replacement can relocate execution without changing agent-facing tools.
- No dedicated CLI wrapper is added when `Exec` provides the same capability without losing structure, policy, or reliability.
