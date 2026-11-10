# Graph Consolidation: Projections

## Scope

Implements only the independently accepted projection portion of Batch C from
[the design](graph-consolidation-design.md), [Phase 2 critique](graph-consolidation-critique.md)
and the independent projection section of
[the patching design](graph-consolidation-patching-design.md).
Patch observation, coverage, planner behavior and public patch events are not
changed. Concurrent edits in authoring.rs, lib.rs, scheduler.rs and tests.rs are
not part of this change. No commits were made.

## Phase 3: Implementation

Both Module::expose and Module::expose_matching now use one private nonmutating
leaf preflight for destination scope/revision, selected/destination identity,
physical target resolution, existing export port/slot/identity, existing token
presence and duplicate-target rejection. Manual selection still checks the
public selector's frontier, identity, scope and module-definition token before
calling this helper. Matching retains its full frontier and duplicate-leaf
checks before committing any leaf.

A private capacity helper checks aggregate additions with checked arithmetic;
nonempty totals permit total - 1 == u16::MAX. One private commit helper owns
append/new export mutation and token allocation. Existing exports retain their
IDs, tokens, metadata and target order; new exports allocate in projection order.
Bulk projection does not call the mutating manual API.

## Phase 4: Actual Diff Review

Self-review of the actual diff, not a separate independent implementation review:

- Removed the manual append-only validation gap and its fallback through
  GraphBuilder::expose_input_typed. That builder API remains unchanged for its
  existing consumers.
- Removed bulk's duplicated leaf identity/revision/export/duplicate checks,
  capacity arithmetic and append/new mutation body, replacing them with the same
  three helpers used by manual projection.
- Retained the existing Address, DeclaredValue, ModuleInputTarget and PublicInput
  representations. No public types, signatures, traits or execution concepts added.
- Preflight and capacity checks use shared borrows and allocate no declaration
  tokens. All Result-returning checks finish before the first commit in bulk;
  commit has no ordinary recoverable failure path. Allocation failure/panic is
  not a new rollback guarantee.
- Existing-token presence is now checked rather than allowing manual indexing
  to panic or bulk append to perpetuate a missing token. Private tests deliberately
  corrupt metadata/tokens; these are not presented as public mutation APIs.
- Public integration fixtures introduce incompatible exports through Module's
  dereferenced GraphBuilder surface. They compare complete debug snapshots of
  export fields/targets, and reuse old PublicInput handles as a token-validity
  probe. Private tests additionally compare the stored token map exactly.
- Late-leaf fixtures first plan an addition, then fanout, then encounter an
  incompatible existing identity or duplicate destination. Neither earlier write
  survives; subsequent manual projection receives the expected next ID and
  fanout retains the existing ID.
- Capacity tests use a synthetic private padded vector to avoid quadratic setup.
  They verify bulk aggregate overflow before mutation, successful manual ID 65535,
  and rejection of the next manual addition without changing stored tokens.
- Formatting in the owned operation.rs and matching_projection.rs files includes
  the inherited formatting failure. No unrelated source files were formatted.

Source/test diff counts relative to HEAD at verification, excluding this new doc:

| File | Added | Removed | Net |
|---|---:|---:|---:|
| src/authoring/operation.rs | 212 | 79 | +133 |
| tests/matching_projection.rs | 185 | 1 | +184 |
| Total | 397 | 80 | +317 |

Paths in the table are under packages/phenix-graphs. operation.rs's net increase
is 50 production lines plus 83 private-test lines. Matching tests increase from
4 to 9; 2 private tests are added. The correctness expansion is not a production
LOC reduction or a measured performance improvement. Existing linear export scans
remain; no new persistent state or authoring IR is introduced.

## Verification

All Rust commands ran through nix develop on the current uncommitted worktree:

| Command After `nix develop -c` | Result |
|---|---|
| cargo test -p phenix-graph --test matching_projection --test typed_operations --test module_projection_semantics --test canonical_interfaces --test symbolic_pending | 57 passed: 9 + 15 + 4 + 27 + 2 |
| cargo test -p phenix-graph --lib projection_tests | 2 passed, 198 filtered out |
| cargo test -p phenix-graph --doc | 24 passed: 3 ordinary and 21 compile-fail |
| cargo fmt --all -- --check | Passed |

Total targeted evidence: 83 passing tests, including 7 new test functions.
git diff --check also passed. Nix emitted dirty-worktree warnings and ignored
SQLite evaluation-cache busy messages during parallel checks; all commands exited
successfully. No full package test run, remote CI, performance measurement or
independent implementation approval is claimed. Patch-observation work remains
outside this implementation and subject to its separate review gates.
