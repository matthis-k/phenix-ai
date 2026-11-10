# Graph Consolidation Measurements

## Scope And Result

Read-only source audit on 2026-11-10, comparing baseline
`d758291c1db77abcff60ac3ac0e167c0f59261bd` with the current shared working files,
including untracked Rust files. This document is the only file written by this
audit. Refreshed after the public-book validation, matching/cross-kind regressions,
cohort-pair capture, and substitution assertion migration. The after state is the
current working-tree snapshot, not a committed release; test/build fixes could
still change it.

- A/B/projection combined: **72 fewer production physical lines**.
- Patch correctness: **650 additional production physical lines**.
- Full affected implementation is **not an overall code-size reduction**.

| Summary measure | Baseline | Current | Change |
|---|---:|---:|---|
| Production physical Rust LOC, affected implementation | 11,393 | 11,971 | +578; comments/blanks included, test spans excluded |
| Public structs/enums/aliases | 146 | 146 | No added public types |
| Public traits | 33 | 33 | No added public traits; 179 total public declarations unchanged |
| Private named action adapters | 6 | 0 | One shared private composition helper added |
| Zero/one transport-vector lowering bodies, adapter/source scope | 8 | 1 | Seven duplicate bodies eliminated |
| Runtime collections, outgoing adjacency plus `WaveActivity` only | 9 | 5 | Four removed, no replacement persistent cache |
| Non-doctest regression functions, package source inventory | 584 | 629 | 45 new: 26 library and 19 integration |

- No benchmarks were performed by this audit. No throughput, latency, allocation,
  binary-size, or compile-time improvement is inferred from these counts.

## Counting Rules

All paths below are relative to `packages/phenix-graphs/`. Scope is the complete
eight-file implementation inventory below, not just favorable diff hunks.
`deployment.rs` is deliberately included despite being unchanged. Unchanged
child modules such as `authoring/patching/{builder,route}.rs` are not new code and
are outside these absolute totals; their delta is zero. The derive subpackage,
examples, benches, and unrelated unchanged library modules are excluded.

**Physical LOC is not executable SLOC.** Counts include blank lines, comments,
Rust documentation comments, attributes, and braces in the selected spans.
Production physical LOC excludes the enumerated test-only spans, including
embedded tests and test-only helper implementations. It does not pretend that
documentation comments are executable code. The selection is a source inventory,
not a Rust parser or expansion of cfg/features/macros. Adjacent comments outside
excluded spans remain counted, even where they describe a test helper.

The whole-file column is also supplied so test exclusions cannot conceal growth.
Separate test files are not production code. No Markdown file contributes to any
Rust LOC number. Counts use `wc -l` and `rg`, not estimates from design documents.

## Implementation LOC

| Implementation file | Whole file before | Whole file after | Production before | Production after | Production delta |
|---|---:|---:|---:|---:|---:|
| `src/lib.rs` | 5,457 | 5,379 | 5,031 | 4,953 | -78 |
| `src/authoring.rs` | 1,907 | 1,919 | 1,907 | 1,919 | +12 |
| `src/authoring/interfaces.rs` | 495 | 453 | 495 | 453 | -42 |
| `src/authoring/operation.rs` | 1,193 | 1,331 | 1,193 | 1,248 | +55 |
| `src/scheduler.rs` | 1,806 | 1,751 | 1,756 | 1,701 | -55 |
| `src/authoring/patching.rs` | 862 | 877 | 862 | 821 | -41 |
| `src/authoring/observer.rs` (new) | 0 | 730 | 0 | 727 | +727 |
| `src/deployment.rs` | 149 | 149 | 149 | 149 | 0 |
| **Total** | **11,869** | **12,589** | **11,393** | **11,971** | **+578** |

The whole-file increase is +720. Its difference from +578 is 142 test-only
physical lines: 83 in operation, 56 in patching, and three observer test-module
declaration/separator lines. Existing excluded test spans in lib and scheduler
are unchanged in size.

### Attribution

Attribution follows the actual diff's independent source regions, not a claim
that each batch has an isolated commit. Shared-file totals above are authoritative.

| Work | Production physical delta | Source attribution |
|---|---:|---|
| A: action lowering | -69 | `lib.rs` -75; two source templates in `authoring.rs` +6 |
| B: scheduling state/adjacency | -58 | `lib.rs` -3; `scheduler.rs` -55 |
| Projection preflight/commit sharing and public-book validation | +55 | Production prefix of `operation.rs` |
| **A/B/projection subtotal** | **-72** | -69 -58 +55 |
| Patch observation/coverage correctness | +650 | Remaining `authoring.rs` +6; `interfaces.rs` -42; `patching.rs` -41; new observer +727 |
| **Full affected production** | **+578** | -72 +650 |

Projection is consolidation of duplicated validation/mutation with additional
correctness checks, **not itself a LOC saving**. It adds shared leaf preflight,
capacity checking, and commit helpers. Patch observation is a larger correctness
expansion: final-state accounting replaces incomplete event-derived authority.
The decrease in `patching.rs` alone is not the patch implementation's net cost.
In particular, omitting the untracked observer would hide 727 production lines.

Since the previous measurements snapshot, `operation.rs` adds five production
lines validating the public book's scope and definition token before bulk
projection planning. `matching_projection.rs` adds one test and 31 physical lines;
observer tests add one cross-kind test and 116 physical lines; `src/tests.rs` adds
14 net physical lines capturing/comparing buffered `(CohortId, Value)` pairs.
The substitution assertion changes to `Graph` with zero LOC/function-count delta.
Thus the refresh adds 166 Rust physical lines: five production and 161 test-only.

## Public Inventory

Count top-level source declarations matching `^pub (struct|enum|trait|type) `,
once at their definition, without double-counting reexports, impls, fields,
methods, generic instantiations, or enum variants. Inspection confirms three
additional indented `pub trait Sealed` declarations in private sealing modules
(one in lib, two in authoring); they are not externally public concepts and are
excluded. No other indented public type/trait declarations occur in this scope.
These are affected-file declaration counts, not a crate-wide API compatibility
proof or a count of every public method.

Every before/after pair in this table is unchanged:

| File | Public structs before/after | Public enums before/after | Public traits before/after | Public aliases before/after |
|---|---:|---:|---:|---:|
| `lib.rs` | 58 / 58 | 13 / 13 | 18 / 18 | 1 / 1 |
| `authoring.rs` | 25 / 25 | 2 / 2 | 8 / 8 | 0 / 0 |
| `authoring/interfaces.rs` | 2 / 2 | 0 / 0 | 0 / 0 | 0 / 0 |
| `authoring/operation.rs` | 17 / 17 | 0 / 0 | 4 / 4 | 0 / 0 |
| `scheduler.rs` | 12 / 12 | 3 / 3 | 3 / 3 | 0 / 0 |
| `authoring/patching.rs` | 7 / 7 | 4 / 4 | 0 / 0 | 0 / 0 |
| `authoring/observer.rs` | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 |
| `deployment.rs` | 2 / 2 | 0 / 0 | 0 / 0 | 0 / 0 |
| **Total** | **123 / 123** | **22 / 22** | **33 / 33** | **1 / 1** |

The new observer declares `pub(super) enum Requirement` and
`pub(super) struct Observation`, with seven requirement variants. These are two
**private internal declarations**, not two new public concepts. `mod observer`
is private. `AssemblyChange` is unchanged; its diagnostic vocabulary remains
partial. `GraphAssembly::contribute` changes from crate-visible to private and
returns private requirements. Public type counts alone would not reveal that
internal signature change or the corrected `UndeclaredWrite` target semantics.

## Duplication And State

These are manually verified semantic inventories from the source diff, not
counts of arbitrary matching identifiers.

| Scoped measure | Before | After | Interpretation |
|---|---:|---:|---|
| Private named action executor structs and corresponding `NodeAction` impls | 6 | 0 | Typed, function, local function, local prepared function, context function, multiport adapters removed |
| Shared private action composition helper | 0 | 1 | `compose_action`, using the existing closure blanket impl |
| Zero/one transport-vector construction bodies in those adapters plus two source templates | 8 | 1 | Scope excludes raw/custom external actions and unrelated vector creation |
| Independent typed/with-layout constructor bodies | 2 | 1 | `typed` delegates to `with_layout`; typed factory retains fresh construction |
| Manual/bulk projection leaf validation bodies | 2 | 1 | Shared `preflight_projection`; API-specific frontier checks remain |
| Manual/bulk mutation routes | 2 | 1 | Previously manual append/builder fallback versus bulk inline commit; now `commit_projection` |
| Shared projection helpers | 0 | 3 | Preflight, capacity, commit; not new public execution layers |
| Stored outgoing adjacency maps | 2 | 1 | `CompiledGraph::by_node` removed; `by_output` retained |
| Stored `WaveActivity` collections | 7 | 4 | `counts`, `nodes`, `active_partitions` removed |
| Private `ActivityCounts` struct | 1 | 0 | Its two counters no longer maintained |
| Diagnostic `refresh` methods / call sites | 1 / 4 | 0 / 0 | Test diagnostics now derive from buffers/producers |
| Partial claim-change scan used for enforcement | 1 | 0 | `interfaces::claim_changes` removed |
| Public-event-to-authority conversion | 1 | 0 | `PatchTarget::observed` removed |
| Dedicated final-state observer entry point | 0 | 1 | `observer::observe`; requirements separate from legacy events |

Four removed collections means one compiled adjacency map plus three wave
collections, not removal of all scheduling indexes. `dispatched`, `pending_ports`,
`pending_by_partition`, and `producers` remain the four `WaveActivity` collections.
Buffers, ready work, reservations, rejected cohorts, failed ports, settled colors,
incoming node/port reverse indexes, and structural reachability remain distinct
authorities/accelerators. They must not be counted as duplicates simply because
they reference nodes or edges. Three production adjacency traversals switch to
`by_output`: partition compilation, structural compilation, topological ordering.

`deployment.rs` is byte-unchanged against baseline (`git diff --exit-code`
succeeded). Its edge resolution already uses `by_output` (current line 106).
Scheduler adapter registration and publication likewise already use that map.
No new scheduler, deployment path, public execution trait, or persistent observer
index was added. Observer ownership/key/requirement maps are temporary authoring
work, not stored scheduling-state indexes. Legacy `assembly_changes` still runs
for diagnostics; the new observer does not eliminate every diagnostic traversal.

## Tests And Evidence

Static inventory counts exact `#[test]` attribute lines. Loops/matrix cases do not
inflate the function count. Feature-gated functions are included in source counts;
doctests and benchmark functions are not. These counts are not line/branch coverage.

| Changed/new test location | Test functions before | After | New | Whole-file physical delta |
|---|---:|---:|---:|---:|
| `src/tests.rs` | 175 | 179 | 4 | +353 |
| `src/authoring/operation.rs` embedded module | 0 | 2 | 2 | +138 (includes +55 production) |
| `src/authoring/patching.rs` embedded module | 0 | 1 | 1 | +15 (includes -41 production) |
| `src/authoring/observer/tests.rs` | 0 | 19 | 19 | +979 |
| `tests/action_lowering.rs` | 0 | 2 | 2 | +163 |
| `tests/consolidation_api.rs` | 0 | 11 | 11 | +340 |
| `tests/matching_projection.rs` | 4 | 10 | 6 | +215 |
| `tests/operation_substitution.rs` | 18 | 18 | 0 | 0 (existing assertion migrated to `Graph`) |
| **New functions** | | | **45** | |

Across the implementation and changed/new test files, net Rust physical growth
is **2,770 lines = 578 production + 2,192 test-only**. The latter includes
test-only helpers, fixtures, separators/module declarations and modifications to
existing tests, not just the bodies of the 45 new functions.

Coverage represented in the new source:

- Action lowering: Send-but-not-Sync prepared values, context/grants, failure
  short-circuiting, and handler factory freshness across physical nodes/engines.
- B: independent activity/work oracle, queued/dispatched/completed/cancelled and
  foreign/duplicate ticket transitions, sparse/cyclic adjacency and invalid output
  slots, 12 targeted-versus-conservative reevaluation cases, and one size probe.
  Buffered comparisons now retain cohort/value pairs rather than values alone;
  this strengthens existing checks without adding a test function.
- Projection: eight new functions covering atomic late failures, metadata/token
  consistency, duplicate targets, aggregate overflow, final valid input ID, and
  stale/foreign public-book rejection without export/token mutation.
- Patch correctness: 19 private observer functions plus one directional coverage
  function; pending/raw payloads, opaque identity, aliases, provenance, routes,
  tokens/indexes/cursors/order, exact lifts, Graph residuals, candidate rollback,
  and the explicit captured-original-store rollback limitation. The added
  cross-kind function checks both substitution directions, allowing endpoint-only
  retargeting while preserving public ownership of target multiplicity/order,
  metadata, and input tokens (six cases per direction, one test function).
- API regression matrix: 11 functions, including ten authoring/execution cases
  and one documentation-excerpt drift check; not eleven performance tests.

### Package Inventory And Run Status

| Static source inventory | Baseline | Current | Delta |
|---|---:|---:|---:|
| `src/**/*.rs` test attributes | 194 | 220 | +26 |
| `tests/**/*.rs` test attributes | 390 | 409 | +19 |
| **Total non-doctest attributes** | **584** | **629** | **+45** |
| `#[ignore...]` attributes in these trees | 3 | 3 | 0 |

This audit did not run Cargo, build a baseline checkout, or measure coverage.
There is therefore **no independently validated before/after package pass count**
from this audit. Existing local reports are useful but describe different shared
snapshots, not an exact final suite guarantee:

- `graph-consolidation-actions.md` records 610 passing, three ignored: 197 library,
  389 integration, 24 doctests. This is an intermediate run, not baseline.
- `graph-consolidation-state.md` records 198 passing library tests, before the
  final observer test matrix was present.
- `graph-consolidation-projections.md` records 57 targeted integration tests,
  two private projection tests, and 24 doctests, all passing at its snapshot.
- `graph-consolidation-patching.md` records an unfiltered intermediate package
  run with 644 passed, one failed, three ignored, before three more observer tests.
  Its later filtered run records 647 passed, three ignored, one explicitly filtered
  out, including 219 library tests and 24 doctests.

The historical filtered report reconciles with the previous source inventory:
219 +408 +24 =651 cases, matching 647 +3 ignored +1 filtered. It predates the two
additional functions in this refresh and is **not current-suite validation**.
Current source has 220 +409 =629 non-doctest functions. If the reported 24 doctests
remain collected, that would be 653 total cases; this is inventory arithmetic,
not a measured final pass count.

The formerly stale assertion in `tests/operation_substitution.rs:640-645`,
`operation_write_does_not_authorize_unrelated_root_requests`, now expects
`PatchTarget::Graph` instead of `Instance("")`. The negative test remains active;
the prior failure/explicit skip is historical, not a current unresolved assertion
migration. This audit has not run the refreshed integrated suite and does not
claim an unfiltered final green result. Final build/test evidence must correspond
to the integrated source after any test/build failure fixes, not an earlier
worker's filtered run.

No comparative benchmark exists here. The B report's x86_64 `size_of` observations
(352-byte compiled graph, 96-byte activity, 24-byte deleted adjacency header,
72-byte deleted activity headers, 12-byte edge) are target-specific inline-size
evidence, not a measured baseline heap total or speedup. This audit did not rerun
that probe and does not extrapolate allocator savings or timing results.

## Reproduction

Run from the repository root. These commands only read source; no temporary
checkout or generated source file is required. The missing baseline observer is
intentionally zero. `git diff` alone omits untracked files, so read them directly.

```bash
base=d758291c1db77abcff60ac3ac0e167c0f59261bd
root=packages/phenix-graphs
files=(lib.rs authoring.rs authoring/interfaces.rs authoring/operation.rs
       scheduler.rs authoring/patching.rs authoring/observer.rs deployment.rs)
for f in "${files[@]}"; do
  printf '\n%s: baseline/current whole-file lines\n' "$f"
  git show "$base:$root/src/$f" 2>/dev/null | wc -l
  wc -l < "$root/src/$f"
done
git diff --numstat "$base" -- "$root"
git diff --exit-code "$base" -- "$root/src/deployment.rs"
```

Production exclusions are explicit inclusive physical-line ranges, reviewed
against both versions. They are snapshot-specific, not a general Rust parser.
For lib, the common ranges cover existing cfg(test) helpers; later ranges move
with production deletions. Scheduler's three helper methods and trailing test
modules are excluded. Added trailing modules include their leading separator.

```bash
count_without() {
  local pattern='' range start end n
  for range in "$@"; do
    start=${range%-*}; end=${range#*-}
    for ((n=start;n<=end;n++)); do
      pattern+="${pattern:+|}$n"
    done
  done
  rg -n '^' | rg -v "^($pattern):" | wc -l
}
common=(754-761 1418-1426 1550-1572 1574-1584 1586-1601
        1605-1670 1672-1688 1693-1725 1728-1755 1762-1842 1845-1899)
git show "$base:$root/src/lib.rs" |
  count_without "${common[@]}" 3242-3246 3595-3654 5434-5445 5456-5457
count_without "${common[@]}" 3167-3171 3520-3579 5356-5367 5378-5379 \
  < "$root/src/lib.rs"
git show "$base:$root/src/scheduler.rs" |
  count_without 752-755 1194-1200 1444-1447 1772-1806
count_without 752-755 1139-1145 1389-1392 1717-1751 < "$root/src/scheduler.rs"
git show "$base:$root/src/authoring/operation.rs" | wc -l
count_without 1249-1331 < "$root/src/authoring/operation.rs"
git show "$base:$root/src/authoring/patching.rs" | wc -l
count_without 822-877 < "$root/src/authoring/patching.rs"
count_without 728-730 < "$root/src/authoring/observer.rs"
# authoring.rs, interfaces.rs, deployment.rs: production equals whole-file count.
```

Public counts (an absent `rg -c` result means zero; `|| true` handles exit code 1):

```bash
for f in "${files[@]}"; do
  for kind in struct enum trait type; do
    printf '\n%s %s before/current\n' "$f" "$kind"
    git show "$base:$root/src/$f" 2>/dev/null | rg -c "^pub $kind " || true
    rg -c "^pub $kind " "$root/src/$f" || true
  done
done
# Also inspect indented declarations to identify private sealing declarations.
rg -n '^ +pub (struct|enum|trait|type) ' \
  "${files[@]/#/$root/src/}" || true
```

Package test counts, repeated for each tree; restrict baseline input to Rust
files and include untracked current Rust files through `rg` directory traversal:

```bash
for tree in src tests; do
  printf '\n%s baseline/current test attributes\n' "$tree"
  git ls-tree -r --name-only "$base" "$root/$tree" |
    while IFS= read -r f; do
      case "$f" in *.rs) git show "$base:$f";; esac
    done | rg '^\s*#\[test\]' | wc -l
  rg --no-filename '^\s*#\[test\]' "$root/$tree" --glob '*.rs' | wc -l
done
# For each test-table row, use the same expression on git show output and file.
# Replace the expression with '^\s*#\[ignore' to reproduce ignored counts.
```

The adapter/path/index inventory requires inspecting definitions and callers in
`git diff "$base" --` the implementation paths, plus reading the new observer.
Identifier occurrence counts alone cannot establish independent execution paths,
public visibility, or authoritative state ownership.
