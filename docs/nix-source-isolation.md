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
The nested examples workspace remains independent. The matcher covers current
workspace patterns; it is not a complete implementation of every Cargo glob form.

Cargo.lock and vendoring configuration are unchanged. Existing offline Cargo
builds and vendor-lock consistency checks remain enabled. A separate strict
`cargo metadata --offline --locked` probe requests lock changes for both the old
and normalized SDK subsets; this change does not claim to establish strict
`--locked` support for partial workspaces.

## Regression And Gating

```sh
nix shell nixpkgs#python3 -c python3 tests/p2-runner-isolation.py
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

The old `subset` implementation fails the membership experiment. Reproduce that
negative control using `--subset-from 64b0fe2bb2cdbf44d44ab9477e814f2ced949047`.

The existing isolation workflow now runs for every pull request without path
filters and checks out the explicit source head. Its failing test step fails the
job. This does not configure repository branch protection or claim the job is one
of the 89 Nix checks; enforcement through merge rules remains a repository setting.

## Local Verification

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
