# Process confinement

status: implemented

## Goal

Define filesystem reach for process execution without coupling it to Workspace API authority.

Environment policy determines what a process can observe and mutate. Workspace authority only decides which Workspace operations a caller may invoke.

## Filesystem behaviors

The product may expose these behaviors as presets. Their names are configuration, not Core or Workspace semantics.

| Behavior | Reads | Writes | Status |
| --- | --- | --- | --- |
| Unrestricted local | Entire host filesystem | Entire host filesystem | Implemented by `phenix.environment.local` |
| Working-directory only | Working directory tree plus read-only runtime dependencies | Working directory tree plus private scratch | Implemented on Linux by `phenix.environment.local` |
| Working-directory write, host read | Entire host filesystem | Working directory tree plus private scratch | Implemented on Linux by `phenix.environment.local` |

For unrestricted local execution, the configured root is the default working directory for relative paths and processes. It is not a security boundary. Absolute paths and child processes may access the rest of the host according to the host OS permissions.

The two confined behaviors use the Linux local backend. Direct restricted filesystem access is rooted through `openat2`. Process execution uses Bubblewrap to materialize the filesystem view before spawn. Descendants inherit that view. Missing kernel support or Bubblewrap fails closed. Restricted modes never fall back to unrestricted local execution.

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

## Filesystem resolution rules

Policy applies to the resolved filesystem object, not only to the caller's path string.

A confined implementation must account for symlink traversal. A path lexically under the working directory does not grant write access when resolution crosses into a read-only or hidden host path. Direct Environment filesystem operations must use resolution that cannot be raced into an out-of-policy target.

Hard links use path-based policy. Writing an inode through an allowed working-directory path is permitted even if another hard link to that inode exists outside the working directory. Stronger object-level isolation requires a copy-on-write or otherwise isolated filesystem implementation.

Nested host mounts under the working directory remain subject to the effective Environment view. The backend must define whether they are recursively exposed rather than inheriting host mount behavior accidentally.

## Scratch and runtime dependencies

Restricted local execution needs runtime support without turning support paths into policy bypasses.

A working-directory-only Environment may expose read-only runtime dependencies required to execute an admitted program. On NixOS this includes the required `/nix/store` closure. Such mounts are execution dependencies, not general host-readable project data.

Writable scratch is Environment-private. Working-directory-only uses a private `/tmp`. Host-read/cwd-write keeps host `/tmp` readable but read-only, mounts a separate private tmpfs, and points `TMPDIR` at it. Restricted modes never make host `/tmp` writable as a convenience exception.

Host home/config paths remain governed by the selected policy. A host-read/cwd-write Environment may read them but cannot write them. Rewriting cache or home variables is a separate product choice and must not silently broaden host writes.

## Persistent process policy

A persistent process is pinned to the effective Environment policy and filesystem view used at creation.

The local provider's policy is immutable for its lifetime. Replacing that provider stops its persistent processes before a provider with another policy becomes active. Child environment variables cannot change the provider's effective policy.

Scratch and persistent processes use the same launcher and confinement rules.

## Trusted embedded code

Environment confinement covers execution mediated through Environment.

Embedded native plugins execute inside the trusted Phenix host process. Native plugin code that directly calls host process/filesystem APIs is trusted code and is outside the isolation guarantee. Untrusted plugin execution must use a process-backed or otherwise isolated runtime whose operations are mediated by the relevant Environment/runtime provider.

First-party code that performs model- or tool-triggered execution must route it through Environment. A direct native process spawn is not an allowed alternate execution path.

## Scope of the guarantee

Filesystem confinement limits filesystem reads and writes only.

It does not by itself constrain network access, Unix sockets, DBus, Docker sockets, devices, secrets, credentials, or other IPC. Those are separate Environment policy dimensions. Restricted filesystem modes must not be described as a complete sandbox until those dimensions are also enforced.

## Explicit broader access

Broader filesystem access is represented as an explicit Environment policy selection before process creation.

Executable names, tool identity, command contents, child behavior, or helper processes never imply elevation. Any broader policy must pass normal authority/configuration checks and should be recorded in execution provenance with requested policy, effective policy, caller, and reason.

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

`phenix.environment.local` defaults to unrestricted execution. `PHENIX_LOCAL_FILESYSTEM_POLICY` selects an explicit local filesystem policy:

- `local` or `unrestricted`;
- `working-dir` or `working-directory-only`;
- `workdir-write` or `host-read-working-directory-write`.

Invalid configuration fails provider activation.

Unrestricted mode preserves the host filesystem/process namespace. The configured root is the default cwd only.

Restricted modes on Linux:

- pin the canonical workspace root with a directory descriptor;
- use `openat2(RESOLVE_BENEATH | RESOLVE_NO_MAGICLINKS)` for restricted direct filesystem access;
- use Bubblewrap for scratch and persistent process creation;
- bind the workspace root read/write;
- give host-read/cwd-write a read-only host root;
- expose only declared runtime dependencies in working-directory-only mode;
- provide private writable scratch;
- keep the same view for every descendant process;
- fail activation or operation when required enforcement is unavailable.

These modes constrain filesystem access only. Network, IPC, devices, secrets, and embedded trusted native code remain separate concerns.

## Deferred implementations

Later providers or policies may add:

- network confinement;
- SSH;
- containers or VMs;
- stronger local sandboxes.

These implementations must preserve the Environment/Workspace boundary from `environment-workspace-boundary.md`.
