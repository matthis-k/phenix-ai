# Process confinement

status: specification-only

## Goal

Define filesystem reach for process execution without coupling it to Workspace API authority.

Environment policy determines what a process can observe and mutate. Workspace authority only decides which Workspace operations a caller may invoke.

## Filesystem behaviors

The product may expose these behaviors as presets. Their names are configuration, not Core or Workspace semantics.

| Behavior | Reads | Writes | Status |
| --- | --- | --- | --- |
| Unrestricted local | Entire host filesystem | Entire host filesystem | Implemented by `phenix.environment.local` |
| Working-directory only | Working directory tree only | Working directory tree only | Future confined Environment |
| Working-directory write, host read | Entire host filesystem | Working directory tree only | Future confined Environment |

For unrestricted local execution, the configured root is the default working directory for relative paths and processes. It is not a security boundary. Absolute paths and child processes may access the rest of the host according to the host OS permissions.

The two confined behaviors require an Environment backend that can enforce the policy for direct filesystem operations and the full descendant process tree. They must fail closed when enforcement is unavailable. They must never fall back to unrestricted local execution.

## Authority

Workspace capabilities are admission checks:

- `workspace.read` admits direct Workspace read/search operations.
- `workspace.write` admits direct Workspace writes and future Patch operations.
- `workspace.shell` or future Exec authority admits process execution.
- `workspace.git` admits the transitional dedicated Git operation.

Those capabilities do not redefine the selected Environment filesystem policy.

A process admitted through Shell or Exec may write wherever its Environment permits. Phenix does not inspect command strings to decide whether a process is read-only or mutating.

Authority still attenuates through the kernel. Environment policy is an additional execution boundary, not a replacement for capability checks.

## Transitive enforcement invariant

Filesystem policy is attached to the Environment process boundary, not to Shell.

Every process start must resolve the active Environment policy before spawning. Any non-unrestricted policy must be materialized into an enforcing filesystem view first. The resulting restriction applies to the process and every descendant, including tools spawned by tools.

There is no executable allowlist that silently bypasses this rule. Shell, Git, Python, build tools, language servers, plugin tools, and custom binaries all inherit the same view.

A broader view is allowed only through an explicit, separately authorized Environment policy selection before spawn. Child processes cannot widen their own view.

If the backend cannot enforce the selected policy, spawn fails. It never falls back to native unrestricted execution.

## Backend contract

A confined Environment must enforce its declared filesystem view for:

- direct read, write, stat, and directory operations;
- scratch process execution;
- persistent processes;
- every descendant process.

Changing executable must not escape the policy. This includes shells, Python, Git, Nix, build tools, and custom binaries.

A confined backend reports the guarantees it can enforce before process start. If it cannot satisfy the requested policy, execution fails.

Network, IPC, secrets, PTYs, and transport are separate Environment capabilities and policies.

Direct filesystem checks and process confinement must derive from the same declared policy. A provider must not enforce one policy for `ReadFile`/`WriteFile` and a different implicit policy for `Exec`/`OpenProcess`.

## Current implementation

`phenix.environment.local` is intentionally unrestricted:

- relative paths resolve from its configured root;
- absolute paths name host paths directly;
- processes default to the configured root as their cwd;
- processes retain normal host filesystem reach;
- persistent processes use the same host namespace.

No working-directory confinement is implemented yet.

## Deferred implementations

Later providers or policies may add:

- working-directory-only confinement;
- working-directory-write with host-wide reads;
- network confinement;
- SSH;
- containers or VMs;
- stronger local sandboxes.

These implementations must preserve the Environment/Workspace boundary from `environment-workspace-boundary.md`.
