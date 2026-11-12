# FBP Source Measurements

**Provenance caveat (2026-11-12):** The counts below describe the
original native migration snapshot and do **not** purport to count current
PR #1263/#1265 or the later benchmark branch. They are source-size
measurements only; H6's runtime performance benchmark is a separate gate.
See [fbp-report.md](fbp-report.md) and [the reconciled issue ledger](fbp-issue-ledger-20261111.md).


Baseline: `32bbe60e6cd2ebd34a705486b4ef96e85245ee42`. After: current uncommitted
native FBP worktree, including new files and excluding deleted files. No final
commit was created. Counts use the same physical/lexical conventions as the
baseline inventory in `fbp-migration.md`; they do not measure runtime performance.

## Totals

| Graph and derive scope | Baseline | Native | Change |
|---|---:|---:|---:|
| Non-test physical Rust LOC | 15,688 | 7,645 | -8,043 |
| Test/support LOC, source plus integration fixtures | 31,857 | 12,845 | -19,012 |
| Source plus tests | 47,545 | 20,490 | -27,055 |
| Examples, separately classified | 173 | 660 | +487 |
| Bench source | 317 | 0 | -317 |
| All Rust under graph directory | 48,035 | 21,150 | -26,885 |

Non-test physical LOC decreased about 51%, substantially more than the desired
directional target. That is not a performance or full-equivalence claim. The old
authoring/compiler variants, pending layouts, mutation-observer authority, wave
accounting and compatibility wrappers were removed. The new persistent runtime
adds request, closure, cancellation, wake and lease accounting. Tests were
reorganized around the native model: the regression ledger explicitly records
replacements, changed semantics and limits rather than counting deletion as proof.

## Physical Ownership

Each row counts actual files, not overlapping favorable diff hunks. New paths
have no baseline file; their responsibilities previously occurred in the deleted
or consolidated files. Non-test LOC includes comments, blanks and doctests.

| Module/path group | Baseline non-test | Native non-test |
|---|---:|---:|
| `src/lib.rs` | 4,953 | 44 |
| `src/authoring.rs` and all authoring child modules | 6,294 | 0 |
| `src/interface.rs` | 932 | 203 |
| `src/provenance.rs` | 82 | 50 |
| `src/testing.rs`, `testing/portable.rs` | 335 | 82 |
| `src/scheduler.rs` | 1,701 | 0 |
| `src/deployment.rs` | 149 | 0 |
| `src/output.rs` | 191 | 0 |
| `port-args-derive/src` | 1,051 | 405 |
| `src/value.rs` | 0 | 330 |
| `src/contract.rs` | 0 | 81 |
| `src/component.rs`, `src/adapters.rs` | 0 | 220 |
| `src/network.rs` | 0 | 1,622 |
| `src/compose/mod.rs` | 0 | 723 |
| `src/process.rs` | 0 | 635 |
| `src/connection.rs` | 0 | 408 |
| `src/runtime/mod.rs` | 0 | 1,191 |
| `src/runtime/readiness.rs` | 0 | 330 |
| `src/runtime/closure.rs` | 0 | 168 |
| `src/runtime/request.rs` | 0 | 417 |
| `src/runtime/recovery.rs` | 0 | 144 |
| `src/runtime/wake.rs` | 0 | 92 |
| `src/runtime/diagnostics.rs` | 0 | 119 |
| `src/port_keys.rs` | 0 | 338 |
| `src/resource.rs` | 0 | 26 |
| `src/limits.rs` | 0 | 17 |
| **Total** | **15,688** | **7,645** |

Current graph source is 7,357 whole-file LOC; derive source is 606. Explicit
test-only exclusions include preceding separators: `network.rs:1623-1716` (94),
`provenance.rs:51-73` (23), and derive `lib.rs:234-434` (201). Integration/support
Rust is 12,527 LOC in 20 files. Thus test/support totals 94 + 23 + 201 + 12,527 =
12,845. Shipped testing helpers remain non-test library code. The four example
files include two example test attributes and are classified separately in both
inventories, not silently included in the source subtotal.

## API Counts

| Graph production lexical declarations | Baseline | Native |
|---|---:|---:|
| Column-zero `pub struct` | 140 | 69 |
| Column-zero `pub enum` | 24 | 20 |
| Column-zero `pub type` | 1 | 0 |
| Public types subtotal | 165 | 89 |
| Column-zero `pub trait` | 35 | 8 |
| Explicit public function declaration lines | 429 | 286 |
| Indented public function lines | 423 | 285 |
| Derive explicit public function lines | 7 | 3 |

These are declaration-line counts, not semantic rustdoc API size. Indented
function lines include constructors and associated functions, not only receiver
methods. Implicit trait methods, generated expansions, reexports and effective
visibility are not resolved. The derive function count includes a quoted generated
`interface` method, consistently with the baseline's physical lexical convention.

The eight public traits are `Process`, `ProcessFactory`, `ProcessDispatcher`,
`ProcessExecutionGuard`, `ReadinessPolicy`, `EdgeAdapter`, `ContractValue` and
`InputRecord`. The first five express actual execution/selection/guard seams;
edge preparation remains pure; codecs retain portable encoding authority;
input-record reconstruction preserves structured argument ergonomics without a
second port topology. There is no Node/Module trait hierarchy.

Lexical `#[test]` attributes in library/derive/integration sources decreased from
633 to 258 (five graph source, three derive source, 250 integration/support).
Doctests and example attributes are separate. These are not executed-case counts
or coverage percentages. One generated model test can exercise many deterministic
cases; no equivalence is inferred merely from those case counts.

## Downstream Inventory

The following are whole `src` trees with mixed inline tests, not production-only
counts. Separate tests include fixture/support source; example counts include
shared example support. Storage and Schema implementation sizes are unchanged.

| Crate directory | Mixed source before/after | Test files before/after | Examples before/after |
|---|---|---|---|
| `phenix-plugins` | 1,279 / 1,338 | 242 / 449 | 0 / 0 |
| `phenix-kernel` | 1,391 / 1,603 | 4,699 / 5,725 | 742 / 941 |
| `phenix-storage-graph` | 1,019 / 1,132 | 2,871 / 2,594 | 455 / 634 |
| `phenix-storage-plugin` | 2,272 / 2,272 | 0 / 0 | 0 / 0 |
| `phenix-schema` | 147 / 147 | 0 / 0 | 0 / 0 |
| `phenix-storage` | 3,501 / 3,501 | 2,974 / 2,974 | 0 / 0 |

The migrated host/adapter source grows to preserve bounded admission, exact leases
on detached executable declarations and errors, request recovery, authenticated
capture and scoped storage lifecycle. It does not hide another execution engine.

## Reproduction

Read baseline files using `git show "$H:$path"`, where `H` is the exact baseline,
and enumerate them with `git ls-tree -r --name-only "$H"`. The full original
baseline exclusion table is retained in `fbp-migration.md`. For the working tree,
enumerate actual Rust with `rg --files packages/phenix-graphs -g '*.rs'`, including
untracked native files, and count `wc -l`. Review the explicit cfg spans above;
do not reuse obsolete baseline line numbers.

Count types using `^pub (struct|enum|trait|type) ` and explicit functions using
`^[[:blank:]]*pub (const |async |unsafe |extern "[^"]+" )*fn `, after test exclusions.
Counts are physical/lexical, not parsed Rust. A tracked-only `git diff --stat`
omits the new modules and cannot supply a complete after total.

Exact obsolete-symbol search over workspace Rust returns zero for `GraphBuilder`,
`CompiledGraph`, `NodeAction`, `WaveState`, `ExecutionSession`, `ModuleDef`,
`ModulePackage`, `GraphAssembly`, `AssemblyRevision`, `NodeHandle`, `ModuleHandle`,
`GraphOffer`, `InterfaceShape`, `DeclaredValue`, and the deleted derives. Historical
design documents are intentionally excluded from this code-authority check.
`GraphOrigin` and Storage's own `Notice` remain intentional independent concepts.

No clone-detector percentage, throughput, allocation, binary-size or compile-time
improvement is claimed. Structural duplication is evidenced by the physically
removed second authorities and zero obsolete code references, not a guessed score.
