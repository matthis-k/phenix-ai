# Nix Subset Workspace Isolation

P0 normalized subset manifests; the automatic-closure increment selects sources by
Cargo package name and follows local dependencies without manual source lists.
Plugin selection, transport policy and runtime behavior are unchanged.

## Source Construction

`subset` parses the real root TOML and emits a deterministic manifest containing
only included root workspace members. The original root manifest is excluded from
the copied fileset, so its store identity no longer invalidates an otherwise
unchanged normalized subset. Member ordering, formatting and overlapping globs do
not change the normalized result. The nested examples workspace stays independent.
Full root workspace dependency/package metadata, resolver, profiles and other
root TOML fields are retained rather than replaced with duplicated constants.

`ci/cargo-workspace.nix` discovers concrete declared members independently of the
selected closure. Supported membership syntax is relative literal paths, component
`*` and `?`, and complete `**` components. Character classes, braces, backslashes,
absolute paths, empty/`.`/`..` components, symlink membership paths and root package
workspaces fail explicitly. Missing literals, unmatched patterns and matched
directories without regular package manifests also fail. Repeated paths and
overlapping globs are deduplicated; duplicate package names fail.

Default-members must resolve to non-excluded members; absent and empty differ.
Excludes are literal directory prefixes, not globs. Unmatched/non-manifest excludes
are harmless; wildcard characters are literal, as checked against Cargo.

`wholeWorkspace` is a separate completeness guarantee. `contractSource` includes
every discovered member directory automatically, then compares the declared
inventory with package manifests in the actual fileset before normalization.
Deliberately omitting a member fails evaluation. Discovery evaluates directory
entries and manifests, but the package-name index and unrelated manifests are not
serialized into isolated sources. Only selected files and normalized closure
metadata enter each source derivation; normalization has one implementation.

`sourceFor` accepts one package name or a list of seeds. It recursively follows
normal, build and dev dependencies in every target table, including optional
dependencies regardless of enabled features. This is a conservative all-target,
all-optional-dependency source superset, not feature-minimal resolution. Inherited
keys resolve through workspace dependencies, aliases use `package`, inherited
paths are root-relative, and proc macros are ordinary local packages.

Cargo implicit path-dependency membership is unsupported and fails closed, even
for optional, dev, build, target-specific and inherited dependencies. Root
patch/replace, external/nonmember local dependencies, member `package.workspace`,
member or intervening nested workspaces, symlinks in package trees and out-of-package
build scripts fail explicitly. Independent workspaces outside the inventory remain
independent. Legacy `dev_dependencies`/`build_dependencies` tables, unsupported
dependency shapes, mixed path/git/registry sources and workspace source overrides
are rejected rather than ignored. Explicit lib/bin/test/bench/example paths and
`package.build` must stay within the package. This is not a general Cargo resolver.

Whole package directories retain assets and build scripts; Cargo publishing
`include`/`exclude` metadata does not filter them. Arbitrary macros, procedural
macros, build scripts and runtime code can compute paths that manifest inspection
cannot infer. External asset ownership therefore requires explicit `sourceFor`
extras and appropriate build inputs. Missing compile inputs may fail compilation
rather than evaluation; runtime inputs need declarations and tests of their own.

Artifact, kernel, conformance, SDK, runtime, generic native plugin, provider and
Advanced constructors select by package name. HTTP has no path argument or manual
source override. Ordinary plugin additions need package declarations, not copied
dependency lists across source constructors. Native library names, curl/SQLite
inputs, features and checks are preserved. Kernel closures explicitly own the
external generation fixture; SDK distribution adds `include`. The full contract
gate retains explicit non-Rust test/example assets. Independent examples and
low-level non-Rust/test subsets intentionally continue to use `subset`.

The five P2 executables remain targets of one package. Only that package is
projected to its manifest and selected Cargo bin; dependencies remain whole
directories selected automatically. Projection rejects shared libraries, build
scripts, extra package/src entries, non-Rust bin assets and external module/include
declarations. Its guard rejects identifier tokens `mod`, `include`, `include_str`
and `include_bytes`, including raw identifiers and intervening comments, and also
rejects them in comments/strings. This conservative false-positive policy supports
the current standalone runners, not arbitrary Rust parsing. Layout expansion
requires revisiting the guard. Selection uses Cargo targets, not runtime selection.

## Limitations

**The root `Cargo.lock` and vendoring remain shared and unchanged by this work.**
Original lock bytes and full root workspace dependencies/profile metadata are
retained. Actual lockfile changes when adding a package can still invalidate
unrelated consumers, as can shared dependency or profile changes. Not all crate
additions are fully cache-isolated. Whole-workspace checks and genuine consumers
of a moved plugin should change; normalization itself changes source identities once.

Existing offline builds and vendor-lock consistency checks remain enabled. Strict
`cargo metadata --offline --locked` requests lock changes for both old and
normalized SDK subsets: strict locked support for partial workspaces is not
established. The new declared-member addition experiment leaves the lock unchanged
and proves evaluation/inventory behavior only, not a runnable, lock-consistent
new-package build or a successful locked full-workspace build with that addition.

## Regression Coverage

```sh
nix shell nixpkgs#python3 -c python3 tests/p2-runner-isolation.py
nix shell nixpkgs#cargo nixpkgs#python3 -c python3 tests/cargo-workspace.py
```

Disposable snapshots restore and reevaluate the baseline between experiments:

- Membership formatting/reordering and overlapping unrelated HTTP globs preserve
  all five runners, kernel, Environment, SDK and HTTP derivations.
- Cargo-only physical HTTP relocation changes HTTP but preserves eight unrelated
  identities, with implementation and lock bytes unchanged.
- Release-profile changes invalidate all nine sampled consumers. Five independent
  runner mutations each change exactly one runner, preserving unrelated controls.
- HTTP implementation, dependency and package-asset changes affect HTTP; unrelated
  source changes do not. Inventory tests cover automatic member inclusion and
  deliberate fileset omission, with the evaluation-only addition caveat above.
- Discovery/closure negatives cover unsupported membership, defaults, exclusions,
  implicit membership, alternate/nested workspaces, dependency metadata, legacy
  tables, target/build escapes and commented/raw module/include tokens.
- Cargo oracles compare inventory and local edges for inherited aliases,
  build/dev/target/optional dependencies and proc macros. Dependency-free fixtures
  pre-generate offline locks; membership probes verify unchanged lock bytes.

Compared with PR #1225 at `27aac0859d9de4fb69d0a3baf37f85f9e23cdde9`, coverage is
equivalent for runner/workspace isolation, with stronger concrete discovery,
automatic inventory and actual-fileset omission tests than manual enumeration/prefix
assertions, not duplication of every named control. The historical implementation at
`64b0fe2bb2cdbf44d44ab9477e814f2ced949047` fails the membership negative control
(`--subset-from` that commit). Earlier relocation churn is recorded in the
[Run 3 inventory](run3-crate-consolidation.md).

The isolation workflow runs on every pull request without path filters, checks
out the explicit source head and fails the job on regression failure. It is not
one of the 89 Nix checks. No repository merge-rule enforcement is claimed.

## P0 Remote Evidence

Exact P0 commit `a86ad3a0ea947098e942e8ac63a5d044b0dfec98`, parent `882dbdd40`:
CI run `41211818738` finished successfully with all 89 checks passing, using four
builders and two testers. Isolation run `41211818359` passed the Cargo oracle and
new guards. Relative to the parent, 83/89 check derivation hashes were unchanged
and six changed; seven actual builds ran. Scheduler time was 50.41 seconds and
overall time was 4m06s with warm caches. These remote results belong to exact P0,
not to the subsequent automatic-closure change.

## Local Verification

- For the automatic-closure change, the expanded isolation regression and standalone
  Cargo oracle passed, including source/dependency controls and new rejection guards.
- Nine focused Nix builds passed: `m1-contract`, `m1-public-sdk`, all five P2 runner
  products, `b4-http` and `basic-http-product`. Reduced-concurrency retries completed
  after initial 1,200-second timeouts. The final contract build freshly ran formatting,
  Clippy, workspace tests and the full vendored offline/locked metadata oracle for
  the existing workspace, not the synthetic declared-member addition.
- `nix flake check --no-build path:.` passed evaluation with existing custom-output
  warnings. Helper/Python formatting and `git diff --check` were checked locally.
- `devenv test` could not start because the executable was unavailable.
