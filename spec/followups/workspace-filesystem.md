---
status: planned
parent: pull/521
---

# Workspace filesystem ownership

## Goal

Replace hand-written generic workspace filesystem traversal/search mechanics with maintained Rust crates and make direct file access capability-rooted rather than ambient-path-rooted.

Phenix keeps workspace authority, version/conflict semantics, process/tool policy, and public responses.

## Theoretical problem

The workspace plugin currently mixes three generic concerns with domain policy:

1. capability-safe filesystem access below a configured directory,
2. repository-aware recursive traversal,
3. line-oriented text search.

These are established infrastructure problems and do not need Phenix-specific implementations.

## Decision

Use:

- `cap-std` for direct workspace filesystem authority/rooted file access,
- `ignore` for recursive repository traversal and ignore semantics,
- ripgrep's reusable matcher/searcher crates (`grep-searcher` plus the smallest matching crate needed) for text search.

Do not expose these crate types in Phenix service contracts.

## Capability boundary

The current lexical path normalization rejects `..` but ordinary `std::fs` calls can still follow an in-workspace symlink outside the configured root. Direct `Read`, `Write`, and `Search` operations must instead resolve through an opened capability directory rooted at the workspace.

Required behavior:

- API paths stay relative.
- Direct file operations cannot escape the workspace through `..`, absolute paths, symlinks, or platform path prefixes.
- Workspace construction opens/anchors the root once and operations use directory-relative handles.
- Existing Phenix `workspace.read` / `workspace.write` checks stay authoritative above the filesystem mechanism.

`Shell` and `Git` are separate process capabilities. `cap-std` does not make subprocesses filesystem-confined. Keep their existing authority distinction and do not claim direct-filesystem containment for them; process sandboxing belongs to the execution/sandbox layer.

## Traversal

Replace recursive `fs::read_dir` walking with `ignore::WalkBuilder` or the smallest equivalent API.

Policy must be explicit rather than inherited accidentally:

- honor repository ignore files according to the product decision,
- do not descend into `.git`,
- define hidden-file behavior,
- define symlink-follow behavior consistently with capability containment,
- define maximum error behavior: inaccessible entries become explicit errors or documented skips, not silent nondeterminism.

Traversal implementation ordering need not be deterministic internally. Final Phenix search results must retain deterministic `(path, line, ...)` ordering.

## Search

Replace per-file `read -> UTF-8 String -> lowercase -> line contains` with ripgrep's reusable searcher/matcher machinery.

Preserve the current service contract unless intentionally tightened:

- literal needle search, not regex, unless the command contract is separately extended,
- case-sensitive flag behavior,
- line numbers are 1-based,
- returned text remains the matched source line,
- binary/non-text input does not crash the search,
- results are deterministically sorted before returning.

Do not shell out to the `rg` executable. This plugin should depend on the library implementation so behavior is available out of the box and backend-independent.

## Read/write versions

Keep Phenix optimistic concurrency semantics:

- reads return a content fingerprint/version,
- writes require the exact expected version including `Absent`,
- conflict is checked immediately before the write through the same capability-rooted path,
- successful writes return the fingerprint of bytes actually written.

This PR may centralize hash helpers but must not change the public `WorkspaceFileVersion` wire representation without a separate contract migration.

## Tests

Add deterministic tests for:

- lexical `../` escape,
- absolute path escape,
- symlink-to-file outside root,
- symlink-to-directory outside root,
- ignored files/directories,
- `.git` exclusion,
- hidden file policy,
- UTF-8 and binary files,
- case-sensitive/insensitive literal matching,
- stable result ordering independent of traversal order,
- exact-version write conflict,
- absent-file creation,
- read-only authority cannot write,
- shell/git authority remains separate from direct filesystem authority.

Tests must not depend on an installed `rg` binary.

## Acceptance criteria

- [ ] No hand-written recursive directory walker remains in `phenix-plugin-workspace`.
- [ ] Direct read/write/search paths are capability-rooted and symlink escapes are covered by tests.
- [ ] Search uses maintained ripgrep library crates rather than line-scanning implementation code.
- [ ] Final search ordering and public responses remain deterministic.
- [ ] Ignore/hidden/symlink policy is explicit and tested.
- [ ] Shell/Git remain explicitly outside the direct filesystem capability boundary.
- [ ] Measure deleted vs added production LOC; wrapper code should be substantially smaller than the generic mechanics removed.

## Non-goals

- Replacing the execution sandbox.
- Sandboxing arbitrary shell/git subprocesses inside `cap-std`.
- Adding regex/glob search surface unless separately requested.
- Changing workspace authority names or public response types.
