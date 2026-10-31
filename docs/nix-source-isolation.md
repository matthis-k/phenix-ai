# Nix Subset Workspace Isolation

This CI-focused follow-up is based on draft PR #1221 at
`64b0fe2bb2cdbf44d44ab9477e814f2ced949047`. It changes packaging and regression
gating, not plugin implementation, selection, transport policy or runtime behavior.
Keep it unmerged ahead of its dependency. Exact-head remote evidence belongs in
the owning PR; local results below do not claim remote acceptance.

## Source Construction

Previously, every Rust subset copied the original root `Cargo.toml` before
conditionally removing unavailable workspace entries. The original manifest's
store identity invalidated consumers even when the final pruned text was unchanged.
HTTP relocation consequently changed all 89 check derivations and dispatched 297
remote builds, as recorded in the [Run 3 inventory](run3-crate-consolidation.md).

`subset` now parses the root TOML, finds included package manifests, and emits a
deterministic manifest containing just the included root workspace members.
The original root manifest is excluded from the copied fileset. Workspace package
and dependency metadata, resolver, release profiles and other TOML fields remain
derived from the real root manifest rather than duplicated constants. Root member
ordering, formatting and overlapping globs do not affect the normalized result.
The nested examples workspace remains independent.

`ci/cargo-workspace.nix` discovers concrete declared members independently of the
selected source closure. It supports relative literal paths, component `*` and
`?`, and complete `**` components. Character classes, brace expansion, backslashes,
absolute paths, empty/`.`/`..` components, symlink membership paths and root package
workspaces are explicitly unsupported rather than silently approximated. Missing
literal paths, unmatched patterns and matched directories without regular package
manifests fail evaluation. Default-members use the same validated discovery;
defaults must resolve to actual non-excluded members, and an absent default-members
field remains distinct from an explicit empty list. Excludes are literal directory
prefixes, not globs: unmatched paths and directories without manifests are harmless,
and wildcard characters are literal, as verified with Cargo. Duplicate package
names fail. Repeated paths and overlapping globs are deduplicated, not rejected:
they describe the same member, as in Cargo.

`wholeWorkspace` is a separate completeness guarantee, not a property of an
isolated subset. `contractSource` automatically includes every discovered member
directory, replacing the manual crates/plugins/testing enumeration. Its guard
compares the declared inventory against package manifest paths in the actual
fileset before normalization. Deliberately removing a member from that fileset
fails evaluation. Discovery reads directory entries and member manifests, but only
the selected fileset and normalized closure metadata enter each source derivation.
There is still one source normalization implementation. Automatic dependency
closure, including Cargo's implicit path-dependency membership, is not implemented
here; callers still own dependency selections. Until dependency discovery is added,
any non-member local path dependency fails explicitly, including optional, dev,
build, target-specific and inherited workspace dependency tables. Member manifests
with `package.workspace`, their own workspace, or an intervening nested workspace
also fail explicitly. Independent workspaces outside the declared inventory remain
independent. This is not a general Cargo resolver.

Cargo.lock and vendoring configuration are unchanged. Existing offline Cargo
builds and vendor-lock consistency checks remain enabled. A separate strict
`cargo metadata --offline --locked` probe requests lock changes for both the old
and normalized SDK subsets; this change does not claim to establish strict
`--locked` support for partial workspaces.

## Regression And Gating

```sh
nix shell nixpkgs#python3 -c python3 tests/p2-runner-isolation.py
nix shell nixpkgs#cargo nixpkgs#python3 -c python3 tests/cargo-workspace.py
```

The test uses disposable snapshots, evaluates derivation paths without compiling,
and restores and reevaluates the baseline between every experiment:

- Root membership formatting/reordering and an overlapping unrelated HTTP glob:
  all five runners, kernel, Environment, SDK and HTTP remain unchanged.
- Physical HTTP directory relocation with matching Cargo/Nix path updates:
  HTTP changes, while the other eight derivations remain identical. Implementation
  and lock bytes remain unchanged.
- Release-profile positive control: all nine derivations must change, preventing
  a fix that simply ignores real build metadata.
- Five independent runner-source perturbations: exactly 1/5 runner derivations
  changes each time, with kernel/Environment/SDK controls unchanged.
- Independent whole-workspace inventory comparison, automatic inclusion of a new
  real package outside the old closure, and deliberate source omission rejection.
- Missing members/manifests, escaping and unsupported membership forms, duplicate
  package names, invalid default-members, harmless unmatched/literal-wildcard
  excludes, valid exclusion/defaults, implicit local dependency rejection,
  alternate/nested workspace rejection and
  valid overlapping globs, all exercised only in temporary snapshots.

Compared using `git show`/`git diff` with PR #1225 commit
`27aac0859d9de4fb69d0a3baf37f85f9e23cdde9`, this retains concrete normalized
membership rather than testing only whether declared patterns overlap selected
paths. That commit's manually enumerated contract source and prefix assertion do
not establish concrete inventory completeness. This implementation derives the
whole-workspace source automatically and independently checks its actual fileset.
The original five runner experiments and all three workspace-isolation experiments
remain in this regression.

The old `subset` implementation fails the membership experiment. Reproduce that
negative control using `--subset-from 64b0fe2bb2cdbf44d44ab9477e814f2ced949047`.

The existing isolation workflow now runs for every pull request without path
filters and checks out the explicit source head. Its failing test step fails the
job. This does not configure repository branch protection or claim the job is one
of the 89 Nix checks; enforcement through merge rules remains a repository setting.

## Local Verification

Review follow-up (2026-10-31): standalone dependency-free Cargo oracle fixtures
confirm literal/prefix exclusions, harmless unmatched/non-manifest exclusions,
and rejection of recursive member matches containing non-manifest directories.
Fixtures pre-generate their locks offline, then run
`cargo metadata --offline --locked --no-deps` and verify unchanged lock bytes.
The expanded isolation regression passed, focused `m1-contract` and
`m1-public-sdk` builds passed using already-realized outputs, and
`nix flake check --no-build path:.` passed evaluation with the existing custom-output
warnings. `devenv test` remains unavailable (`devenv: command not found`).

P0 follow-up on worktree base `882dbdd40ce0f45feea58b683eb4a3298b61c84f`
(uncommitted implementation, 2026-10-31): the Python regression passed through
`nix shell nixpkgs#python3`, focused `m1-contract` and `m1-public-sdk` Nix builds
passed (already-realized outputs, no fresh compilation log), and
`nix flake check --no-build path:.` passed evaluation. The custom regression
outputs produce unknown-output warnings, not failures. This is local evidence,
not exact-commit remote acceptance or a full flake build.
The historical `--subset-from 64b0fe2bb2cdbf44d44ab9477e814f2ced949047`
negative control failed at the expected unrelated-derivation membership assertion.
`devenv test` was attempted and could not start (`devenv: command not found`).

The membership, physical relocation, release-profile and five runner experiments
passed. The old implementation failed the negative control as expected.
`m1-contract` (formatting, Clippy and workspace tests), `m1-public-sdk`, and all
five runner product checks passed through Nix. Full `nix flake check --no-build`
passed evaluation only. `b4-http` and `basic-http-product` also passed with the
normalized sources; an initial short-timeout attempt was retried successfully.
`devenv test` could not start because the executable is
absent. No performance improvement is asserted from these evaluation results.

The normalization itself changes source derivations once. Whole-workspace checks
and genuine consumers of a moved plugin should still change on relocation;
the invariant concerns isolated, unrelated consumers. Shared lock, dependency or
profile edits can legitimately invalidate multiple consumers. HTTP resolved-peer
policy, codec-owned headers, credential-files relocation and cache trials remain
separate work.
