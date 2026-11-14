# Phenix FBP Migration Ledger

Historical status: native FBP migration was implemented in an uncommitted worktree.
The current campaign is tracked in [the campaign record](fbp-hardening-campaign-20261114.md)
and [33-issue tracker](fbp-issue-progress-estimates-20261113.md); the old commands and
"no commit requested" statements below describe that earlier execution only. The initial
baseline and intermediate measurements below are historical evidence, not current
milestone status. The final outcome section and [report](fbp-report.md) supersede
their pending labels. See [design](fbp-design.md), [critique](fbp-critique.md),
[regression mapping](fbp-regression-ledger.md), and [measurements](fbp-measurements.md).

## Verified-Stack Reconciliation

The 2026-11-14 campaign incorrectly relied on stale local remote-tracking
`origin/impl/graph-consolidation-20261110` at `16e77f6c6`, with inherited
callback/correlation work. Its first commit `0d145f3` is on
`fix/fbp-lifecycle-request-lanes-20261114` (PR #1302, based on the impl graph
branch). Actual remote impl `89e00d06498979a2f231dfc8ca780eba44140d27`
already includes later foundations. After fetching, exact verified PR #1300 head
`516e0883875d5ec483e3595d50b77b3d5e8979c4` is now being merged to preserve
the complete stack and all scoped gates. Historical baseline inventories and
verification below are retained, not relabeled as acceptance for this merge.

The canonical merged API uses only `SessionOptions.deferred_timeout` and existing
`overdue_jobs(now)` / `supervise_deferred(now)`; the candidate-specific timeout
field is not retained as an alias or second timer. C9 `state(&self)` remains passive
and never supervises expiry. `RunReport::next_deferred_timeout` bounds driver idle
waits, not callback duration. `execution_guards` is an ordered Vec in place of the
single replaceable guard slot: `ActivationJob::with_guard` appends, with reverse
token teardown preserving Kernel identity through application-token destruction
and process panic-payload cleanup. Exhaustive option literals and error matches
must account for the current fields and detailed non-`Copy` `RequestError`.

Kernel retains verified whole-drain retirement and exclusive blocking-driver
ownership. External publication waits for an admitted whole producer drain, not
explicit session work; same-drain publication rejects. Host self-destruction marks
retirement and discards a late poll result, with destruction deferred outside
locks. All Kernel session calls fail fast on cross-thread contention with
busy/`WouldBlock`; same-kernel callback/worker re-entry also fails fast. Callers can
retry after release. This preserves the verified lock-inversion contract for
ordinary callers as well as producers: initial waiter claims are historical and
superseded, never an accepted Kernel lock cycle. `lease_session` always uses
`try_lease_session`. `ExclusiveSlot` is only `Mutex<Option<T>>` with `try_take` and
idempotent release, without Condvar or waiters. The revised three-property Loom
verifier covers competing exclusive ownership, busy/no-theft plus retry, and
idempotent early release/state publication; initial Loom passes do not accept this
new head. The initial no-wait-publication proposal is also superseded.
The initial no-wait/empty-return competing-drain policy is discarded as well:
`Kernel::drain` returns a `Result`, manual drains serialize, recursive drains reject,
and a live blocking driver excludes manual polling even for `drain(0)`. The
whole-drain and driver/direct-drain tests remain merge acceptance obligations.

Optional `BoundedLanes` isolates per-request queues across connections, ingress
and captures, not independent process state. A staged continuation, blocked atomic
publication or nonconsuming reader still limits progress. The verified stack's
authenticated non-head buffered capture extraction is retained; it does not create
extra queue capacity or a fairness guarantee.

Local workspace, no-default-feature, Clippy, Loom and Nix checks passed for initial
`0d145f3`, not the new merged head. PR #1302 remote workflows initially conflicted;
manual [Flake #43588157944](https://github.com/matthis-k/phenix-ai/actions/runs/43588157944)
targets `0d145f3` and is not final acceptance. **New full acceptance is pending.**
The tracker is the sole current score authority: all 33 deltas against verified
`516e088` are +0.0 pp; H3b 78.0%, Q5 74.0%, mean 55.3%, classifications
10 scoped verified / 7 partial / 16 open. The incoming issue ledger remains
historical detail. No issue or milestone closure is claimed; M7/M8 and missing
production-agent and OS-isolation adapters remain incomplete. Strict C3 revocation
versus leased archival still needs owner decision; C6 live migration is absent.
The new merge head's performance is unmeasured. Historical cutover commands and
the immutable Nix snapshot remain in the [report](fbp-report.md#historical-cutover-evidence);
neither they nor earlier stack benchmarks establish this merge's performance.

## Exact Baseline

| Item | Recorded value |
|---|---|
| Repository | `matthis-k/phenix-ai` |
| Checkout / baseline | `32bbe60e6cd2ebd34a705486b4ef96e85245ee42` |
| Branch | `impl/graph-consolidation-20261110` |
| Initial worktree | Clean, tracking `origin/impl/graph-consolidation-20261110` |
| Reference PR | [#1260](https://github.com/matthis-k/phenix-ai/pull/1260), OPEN at recheck |
| Rechecked PR head | `671956269a2655084ae90eb0b1e09e506177d71c` |
| PR branch / base | `impl/operation-interface-20261109` / `impl/debt-cleanup-stacked-20261109` |
| Checkout difference | Six commits ahead; 26 changed files, 6,219 insertions, 405 deletions |
| Baseline CI | [42773370525](https://github.com/matthis-k/phenix-ai/actions/runs/42773370525), completed/success for exact checkout SHA |
| Final implementation commit / CI | Uncommitted worktree; no new commit/push/remote CI requested |

Recorded before editing with `git status --short --branch`, `git rev-parse HEAD`,
`git log --oneline -5`, `git diff --stat 671956269a2655084ae90eb0b1e09e506177d71c HEAD`,
`git rev-list --count 671956269a2655084ae90eb0b1e09e506177d71c..HEAD`,
`gh pr view 1260 --json number,state,headRefName,headRefOid,baseRefName,url`, and
`gh run list --branch impl/graph-consolidation-20261110 --commit 32bbe60e6cd2ebd34a705486b4ef96e85245ee42 --limit 5 --json databaseId,headSha,status,conclusion,url`.
No commit, push, PR mutation, or branch switch was requested or performed.

## Baseline Architecture Inventory

| Responsibility | Actual baseline authority | Migration constraint |
|---|---|---|
| Typed authoring | `authoring.rs`, `authoring/operation.rs` | `Operation` shares selectors but Node/Module declarations still differ |
| Primitive state | `NodeTemplate.logic_factory`, deployed engine action map | Factory calls already happen per physical node; remove implicit sharing, not freshness |
| Interfaces | `interface.rs`, `authoring/interfaces.rs`, physical ports/layouts | Separate shape, reconstruction, claims, and restricted views before consolidation |
| Composite lowering | `ModulePackage` -> `ModuleCatalog` -> declaration/expansion/triggers/index | Modules already flatten; no runtime module executor should be introduced |
| Identity | Scope/revision tokens, package definition IDs, `GraphOrigin`, compiled generation | Stable names do not replace these independent checks |
| Runtime | `scheduler.rs`, `deployment.rs`, `WaveState`, execution tickets | One scheduler, wave-owned queues, zero/one output, no bounded stream backpressure |
| Policy | Reservations, cohorts, colors, exact reachable active work | Preserve staged/correlated semantics as adapters with retirement |
| Patching | Planner + arbitrary candidate callbacks + stored-state observer | Replace with owned edits before removing observer authority |
| Provider selection | Kernel contributions/offers/import resolution | Host policy stays out of FBP core |
| Producer lifetime | Kernel activation/start, round-robin poll, wake, host generations | Add bounded admission without losing retries, retirement, leases |
| Observation | Storage subscription + StorageGraph notice tail / derived source | Preserve explicit overflow/rescan, exact revisions, and store authority |

Core normal dependencies are only `phenix-schema` and optional
`phenix_port_args_derive`; default feature `derive` enables the latter. Direct
normal-dependency consumers are `phenix-plugin`, `phenix-kernel`, and
`phenix-storage-graph`. `phenix-storage-plugin` is downstream through Plugin.
`phenix-storage` must not acquire a graph dependency.

## Baseline Measurements

All numbers are physical Rust newline counts, including comments and blanks,
read from tracked content at the exact baseline with `git show`. They are not
executable SLOC. Test-only files and the explicit inclusive inline ranges below
are separated. Shipped public `testing` utilities count as library code.

| Module/file relative to `packages/phenix-graphs` | Non-test LOC | Test/support LOC |
|---|---:|---:|
| `src/lib.rs` | 4,953 | 426 |
| `src/authoring.rs` | 1,919 | 0 |
| `src/authoring/additions.rs` | 119 | 213 |
| `src/authoring/addresses.rs` | 372 | 0 |
| `src/authoring/interfaces.rs` | 453 | 0 |
| `src/authoring/observer.rs` | 727 | 3 |
| `src/authoring/observer/tests.rs` | 0 | 980 |
| `src/authoring/operation.rs` | 1,248 | 83 |
| `src/authoring/patching.rs` | 821 | 56 |
| `src/authoring/patching/builder.rs` | 213 | 0 |
| `src/authoring/patching/route.rs` | 132 | 0 |
| `src/authoring/provenance.rs` | 26 | 0 |
| `src/authoring/substitution.rs` | 264 | 0 |
| `src/deployment.rs` | 149 | 0 |
| `src/interface.rs` | 932 | 107 |
| `src/output.rs` | 191 | 0 |
| `src/provenance.rs` | 82 | 23 |
| `src/scheduler.rs` | 1,701 | 50 |
| `src/testing.rs` | 255 | 0 |
| `src/testing/portable.rs` | 80 | 0 |
| `src/inspection_tests.rs` | 0 | 143 |
| `src/schema_ownership_tests.rs` | 0 | 187 |
| `src/tests.rs` | 0 | 11,259 |
| **Graph source** | **14,637** | **13,530** |
| `port-args-derive/src/contract_value.rs` | 115 | 0 |
| `port-args-derive/src/lib.rs` | 936 | 80 |
| **Graph plus derive source** | **15,688** | **13,610** |
| Graph `tests/**/*.rs`, including fixtures/support | 0 | 18,247 |
| **Source and tests** | **15,688** | **31,857** |

Separately: two example files, 173 LOC; two benchmark files, 317 LOC. All tracked
Rust under the graph directory, including derive, totals 48,035 LOC. There are
633 lexical `#[test]` lines across source and integration fixtures (220 graph
source, 409 integration/support, four derive), not 633 executed test cases.

| Lexical production declarations | Graph baseline | Final implementation |
|---|---:|---|
| Column-zero `pub struct` | 140 | Not measured |
| Column-zero `pub enum` | 24 | Not measured |
| Column-zero `pub type` | 1 | Not measured |
| Column-zero `pub trait` | 35 | Not measured |
| Explicit `pub ... fn` declaration lines | 429 | Not measured |
| Indented explicit public function lines | 423 | Not measured |
| Derive explicit public function lines | 7 | Not measured |

These are lexical counts, not a semantic rustdoc API inventory. Indented function
lines include associated constructors as well as receiver methods; implicit trait
methods are not counted. Private-module visibility, reexports, cfg expansion, and
generated methods are not resolved. Four indented sealing traits are excluded.
The derive count includes six proc-macro entry points and one internal expansion
method. Do not report 423 as an exact receiver-method count.

Duplication baseline is structural, not a token-similarity percentage: separate
node/instance collections and endpoint variants; overlapping interface metadata;
cross-kind incident rewrites; callback footprint observation. No clone detector
or exact duplicate-code metric has been run. Final review must compare removed
authorities, not invent a percentage from these counts.

### Reproduction Rules

Pin `H=32bbe60e6cd2ebd34a705486b4ef96e85245ee42`. Enumerate tracked Rust with
`git ls-tree -r --name-only "$H" -- packages/phenix-graphs`, read each using
`git show "$H:$path"`, and count with `wc -l`. Whole test files are excluded
entirely. The inline test partitions are:

| File | Inclusive test-only line ranges |
|---|---|
| `src/lib.rs` | 754-761, 1418-1426, 1550-1572, 1574-1584, 1586-1601, 1605-1670, 1672-1688, 1693-1725, 1728-1755, 1762-1842, 1845-1899, 3167-3171, 3520-3579, 5356-5367, 5378-5379 |
| `src/scheduler.rs` | 752-755, 1139-1145, 1389-1392, 1717-1751 |
| `src/authoring/additions.rs` | 120-332 |
| `src/authoring/operation.rs` | 1249-1331 |
| `src/authoring/patching.rs` | 822-877 |
| `src/authoring/observer.rs` | 728-730 |
| `src/interface.rs` | 933-1039 |
| `src/provenance.rs` | 83-105 |
| `port-args-derive/src/lib.rs` | 937-1016 |

Whole test-only source files: `src/tests.rs`, `src/inspection_tests.rs`,
`src/schema_ownership_tests.rs`, `src/authoring/observer/tests.rs`. Inclusive
spans intentionally include some preceding separators, matching the earlier
measurement convention. Count declarations after excluding these spans with
`^pub (struct|enum|trait|type) ` and functions with
`^[[:blank:]]*pub (const |async |unsafe |extern "[^"]+" )*fn ` using `rg -c`.
At the final head, review new cfg boundaries instead of reusing old line numbers.

### Downstream Size

These source totals contain inline tests and are deliberately not called
production LOC. Separate tests include nested fixture crates/support.

| Crate directory | Mixed `src/` LOC | Separate test LOC | Example LOC |
|---|---:|---:|---:|
| `phenix-plugins` | 1,279 | 242 | 0 |
| `phenix-kernel` | 1,391 | 4,699 | 742 |
| `phenix-storage-graph` | 1,019 | 2,871 | 455 |
| `phenix-storage-plugin` | 2,272 | 0 | 0 |
| `phenix-schema` | 147 | 0 | 0 |
| `phenix-storage` | 3,501 | 2,974 | 0 |

## Removal And Retention Ledger

The original ownership plan below is now implemented. Actual deletions, native
test mappings and deliberate syntax changes are recorded in the regression ledger.

| Old surface | Replacement / retained obligation | Gate |
|---|---|---|
| Large `lib.rs` implementations | Small facade; authoritative value/interface/component/network/compose/runtime modules | M1-M6 |
| `GraphBuilder`, `Module` builder dereference, raw public module builder | One `NetworkBuilder`, same typed handle from node/module | M1-M2 |
| `Operation`, `NodeHandle`, `ModuleHandle`, `DiscoveredOperation` | `ComponentRef<Ports>` and checked erased discovery, no parallel handle lineage | M1-M2 |
| `NodeSignature`, `InterfaceShape`, duplicated boundary declarations | One interface; reconstruction separate; restricted views retained where they enforce rights | M1-M2 |
| `NoPorts`, `SinglePort`, `Ports`, old layout derives | Native zero/N cardinality plus descriptor/reconstruction ergonomics; consolidate generated identifiers | M1-M2 |
| Separate address/route grammars and physical public references | `PortPath`, direction, canonical `ConnectionKey`, owner/revision checks | M1-M3 |
| `NodeTemplate.output`, canonical slot zero | Arbitrary `Interface.outputs`; ordinary unit remains a value | M1-M4 |
| `ModuleDef`, catalog/package/entry/instance boundary split | `ComponentDef::Body`, `InstanceDef`, immutable `NetworkDef`, one compiler | M1-M2 |
| `substitution.rs`, `ReplacementKind`, incident connection replacement claims | Same-interface symbolic replacement; explicit rewires separately owned | M2-M3 |
| `PatchSpec`, `PatchBuilder`, mutation callbacks and observer authority | Owned `NetworkPatch`/`Edit`, exact derived footprints, transactional candidate | M3 |
| `GraphAssembly`, `AssemblyRevision`, addition transactions | Composer-owned immutable candidate, outcome and provenance | M3 |
| `SplicePosition` | Dependency ordering if equivalent; otherwise retain exact coordinate with demonstrated use case | M3 |
| `Output<T>` | Native emission/selection adapter only, no one-output runtime restriction | M4 |
| `Engine`, `ExecutionSession`, `WaveState` | One persistent `NetworkSession`, bounded queues, one dispatch path | M4-M5 |
| `NodeAction`, borrowed action `Executor` | One process/factory with owned exclusive activation dispatch | M4-M5 |
| Cohorts/colors/reservation history | Correlated and staged policy accounting with bounded retirement, only after equivalence tests | M4 |
| `deployment.rs` | Session/dispatcher configuration; preserve dispatch guards and overrides | M4-M5 |
| `GraphContract` | Explicit semantic claims in contract module; same structural authority as wiring | M2 |
| Kernel `GraphOffer`, imports, compose/fire/drain | Network definitions/patches and bounded persistent admission; host policy/lifecycle retained | M5 |
| StorageGraph per-wave notices/queries | Bounded source admission, explicit correlation, capture and exact query origin | M5 |
| Physical node/edge IDs | Compiler-private deterministic indexes and source maps | M2-M6 |

## Historical Gates And Regression Evidence

| Milestone | Required evidence | Status |
|---|---|---|
| M0 | Exact baseline, source/API inventory, measurements, design and adversarial critique | Recorded before implementation; lexical metric limitations explicit |
| M1 | Same node/module handle, zero/N ports, typed compile-fail tests, exclusive process dispatch | Native model, typed keys/input records, owned jobs and regressions pass |
| M2 | Full validation then flattening; deterministic IDs; local/wire checks; nested claims/projections | Canonical compiler, exact structural authority and contract suites pass |
| M3 | Replay, precise conflicts, splices, dependencies, authorization, atomicity, unload | Declarative composer, authority suites and independent bounded model pass |
| M4 | Persistent streams, state, FIFO, capacity, atomic fanout, retry, cycles, joins, EOS/wakes | One native session; streaming/cycle/routing/request/ownership regressions pass |
| M5 | Provider/import/lease lifecycle, bounded producer polling, storage observation/query guards | Native Kernel/Plugin/StorageGraph suites and examples pass |
| M6 | Migrated callers, zero obsolete code references, no shims or second engine | Physical removals and grouped coverage mapping recorded |
| M7 | Independent review, measurements, example execution, final-head CI | Local matrix and immutable Nix snapshot pass; new commit/remote CI not requested |

Existing test anchors to migrate rather than delete for convenience:

- Interface/type authority: `canonical_interfaces`, `node_definition_modes`,
  `portable_wire`, `portable_codec`, `compiled_bindings`, and compile-fail doctests.
- Boundary/reuse/replacement: `typed_operations`, `module_projection_semantics`,
  `matching_projection`, `clone_lineage`, `operation_substitution`,
  `contract_dependency_identity`, and `composition_origin`.
- Dispatch/ownership: `action_lowering`, `lean_api`, `explicit_arc_input`,
  `scheduler_lifecycle`, `assembly_cancel`, and `execution_budget`.
- Runtime policies: existing `src/tests.rs` staged/optional/N-of-M, fresh cohort,
  coloring, feedback, and randomized small-step tests. Rewrite against packet
  sessions with explicit correlation scopes; do not simply rename waves.
- Composition: `patch_planner`, `patch_lineage`, `patch_syntax`, stored-state
  observer regressions until callbacks disappear, and `assembly_model_checks`.
- Kernel: `declarative_imports`, `host_portable_contracts`, `plugin_binding`,
  `activation_artifacts`, `generation_retirement`, `generations`, and `storage`.
- StorageGraph: `binding`, `derived`, `derived_plugins`, and `host_derived`.

Historical pre-implementation instruction: add the requested `fbp_components`, `fbp_types`, `fbp_streams`, `fbp_cycles`,
`fbp_composition`, `fbp_authority`, and integration regression suites only against
the production implementation. Compile and run `basic_fbp`, `streaming_fbp`, and
`plugin_composition`; none exists yet. Also rerun current graph/kernel/storage
derived examples after migration. No test-only scheduler or fake storage engine.

## Verification Record

Executed against baseline Rust sources before this documentation change:

| Command | Observed result |
|---|---|
| `cargo test --workspace --locked` | Passed; ordinary ignored/manual cases remain skipped |
| `cargo test --workspace --no-default-features --locked` | Passed; workspace feature unification can still enable graph derive |
| `cargo test -p phenix-graph --no-default-features --locked` | Passed separately, with derive-only cases gated out |
| `cargo fmt --all --check` | Host Cargo has no fmt subcommand |
| `nix develop --command cargo fmt --all --check` | Passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Host Cargo has no clippy subcommand |
| `nix develop --command cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed |
| `nix flake check --keep-going` | Exit 0; evaluated check derivations, reported `running 0 flake checks`; not evidence of fresh check builds |
| Explicit `nix build --no-link` of all ten check targets below | Passed (exit 0); outputs may be reused from the Nix store |

Host Cargo reported 1.96.2; Nix reported 2.34.8. The repository dev shell supplies
rustfmt and Clippy. `flake.nix` defines formatting, clippy, graph, kernel, storage,
derived, no-default, integration, workflow, and kernel-smoke checks. Baseline CI
is separately recorded above; no final implementation CI claim is made.

Because flake check reported zero checks run, all declared checks were also
requested explicitly, without creating result links:

```bash
nix build --no-link .#checks.x86_64-linux.format .#checks.x86_64-linux.clippy .#checks.x86_64-linux.graph .#checks.x86_64-linux.kernel .#checks.x86_64-linux.storage .#checks.x86_64-linux.derived .#checks.x86_64-linux.no-default .#checks.x86_64-linux.integration .#checks.x86_64-linux.workflow .#checks.x86_64-linux.kernel-smoke
```

An independent documentation review checked baseline provenance and headline
metrics and raised six design-contract gaps, addressed in the design record.
It did not review implemented FBP or rerun the local Cargo commands. `git diff
--check` passed; the new records are uncommitted, not part of baseline CI.

## Historical Initial M1 Step

The first implementation step moves existing authorities, with no duplicate
definitions, compatibility aliases, or extra execution engine:

- `src/value.rs` now owns `Value`, `TypeIdentity`, `ContractValue`, codecs and
  internal value-construction helpers. Value fields are private; existing tests
  use `from_wire_parts`/`schema` rather than accessing their representation.
- `src/contract.rs` now owns semantic claims, authoritative snapshots, sealed
  `GraphUnit`, restricted compiled bindings, catalog and final metadata checks.
  All actual semantic `interface::` call sites moved; no forwarding remains there.
- `src/interface.rs` retains structural shape and diagnostics and now owns
  `DeclaredValue`, removed from `authoring.rs`. Bound contract lookups use the
  existing shape accessors rather than widening field visibility.
- Root facade reexports refer to the sole definitions. Old owner-module paths
  `interface::GraphContract` and `authoring::DeclaredValue` are intentionally gone.
  `canonical_interfaces` imports the actual owner modules to exercise the split.

Snapshots/compiled inspection still live with contract binding for now; the final
network inspection move is pending. This step does not add `ComponentDef` or
generalize the current single-output `InterfaceShape` under a new name.

### Measured Worktree Delta

Same physical/lexical counting rules as the baseline, including new untracked
files. These are uncommitted worktree measurements, not a final-head report.

| Affected file | Baseline non-test LOC | Current non-test LOC | Baseline test LOC | Current test LOC |
|---|---:|---:|---:|---:|
| `src/lib.rs` | 4,953 | 4,726 | 426 | 426 |
| `src/value.rs` | 0 | 238 | 0 | 0 |
| `src/contract.rs` | 0 | 625 | 0 | 0 |
| `src/interface.rs` | 932 | 342 | 107 | 109 |
| `src/authoring.rs` | 1,919 | 1,898 | 0 | 0 |
| `src/authoring/interfaces.rs` | 453 | 453 | 0 | 0 |
| `src/tests.rs` | 0 | 0 | 11,259 | 11,251 |
| `tests/canonical_interfaces.rs` | 0 | 0 | 1,293 | 1,294 |
| **Affected total** | **8,257** | **8,282** | **13,085** | **13,080** |

Total graph-plus-derive non-test LOC: **15,688 -> 15,713 (+25)**. Test/support LOC:
**31,857 -> 31,852 (-5)**. All graph-directory Rust including examples/benches:
**48,035 -> 48,055 (+20)**. Growth is module imports/facade separation and accessor
lookup rather than new capabilities; no overall simplification percentage is
claimed. The five structural interface tests and two moved sealing doctests are
unchanged apart from test imports. Test LOC reduction replaces malformed-envelope
struct literals with the existing public constructor; no test was deleted.

Lexical public structs/enums/aliases/traits/functions remain **140/24/1/35/429**;
indented public function lines remain **423**, derive function lines **7**, and
test attributes **633**. The same API-count caveats apply. Exact-symbol inspection
finds one definition each of `Value`, `TypeIdentity`, `ContractValue`,
`DeclaredValue`, and `GraphContract`, and no old semantic `interface::` references
in actual Rust. These counts do not make the old graph model obsolete yet.

For reproducing test exclusions, baseline `lib.rs` spans now shift by -227 lines;
the current structural interface test span is 343-451, including its separator.
All other inline exclusion spans are unchanged. `src/tests.rs` and the integration
test remain wholly test/support code. A tracked-only `git diff --stat` omits the
863 lines in new `value.rs`/`contract.rs`, so it is not the complete size delta.

### Isolated Compile/Behavior Probes

Both experiments use a live path dependency on this checkout's `phenix-graph`
with default features disabled. They are local scratch artifacts, not persisted
repository tests, a second production runtime, or final CI evidence.

| Probe | Local path | Observed result |
|---|---|---|
| Owned activation dispatch | `/tmp/opencode/owned-process-prototype` | Eight unit tests and two compile-fail doctests pass |
| Fixed bounded correlation lanes | `/tmp/opencode/correlation-lanes` | Ten unit tests pass, including expected blocked cases |

Reproduction on this machine:

```bash
cargo test --manifest-path /tmp/opencode/owned-process-prototype/Cargo.toml --target-dir /tmp/opencode/owned-process-prototype/target
cargo test --manifest-path /tmp/opencode/correlation-lanes/Cargo.toml --offline
```

Owned handoff supports a Send-only, non-Sync mutable process and Send-only prepared
argument without borrowing session queues. It separates `Emit`/publication commit
from `Complete`, rejects stale/foreign/duplicate returns, and disposes mutated
state on panic. It does not prove wake registration, queue reservation, EOS,
supervised abandonment, or real code leases. The production dispatcher is unchanged.

Two fixed lanes can handle crossed correlations while preserving each upstream's
FIFO intake. Shared exact scope-to-slot bindings and fresh tokens are necessary;
hash routing can collide. Insufficient scope credit or lane capacity still
deadlocks under ordered arrivals. Static bounded lanes are not proof of arbitrary
arrival-skew progress, optional/staged joins, or request-wide accounting. These
results constrain the next design step rather than justify deleting cohort logic.

### Worktree Verification

Independent code review found no actionable regression in the extraction; it also
checked both feature configurations. Subsequent verification on the final Rust
worktree passed:

- `cargo test --workspace --locked --quiet`
- `cargo test --workspace --no-default-features --locked --quiet`
- `cargo test -p phenix-graph --no-default-features --locked --quiet`
- `nix develop --command cargo fmt --all --check`
- `nix develop --command cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo run -p phenix-graph --example grouped_contract --locked --quiet`
- `cargo run -p phenix-graph --example multi_contributor --locked --quiet`
- `cargo run -p phenix-kernel --example basic --locked --quiet`
- `cargo run -p phenix-storage-graph --example derived --locked --quiet`

Initial simultaneous workspace-test/Clippy attempts hit the 240-second tool
timeout while sharing Cargo build locks; serial reruns completed successfully.
Ordinary ignored/manual tests remain skipped. No post-extraction CI or Nix check
build is claimed: new source files remain untracked and a Git-backed flake source
would omit them. Baseline Nix/CI results above apply only to the baseline.

## Historical Final Worktree Outcome

The canonical model, compiler, composer and persistent runtime now replace the
old implementation. Kernel, Plugin and StorageGraph use that native path.
The legacy authoring tree, scheduler, deployment/output wrappers, physical public
endpoint model and old derives are physically removed, not forwarded or aliased.
`lib.rs` is a small facade; the package remains `phenix-graph`.

Primitive and composite definitions return the same `ComponentRef`. Replacement
checks their complete interface and inherited explicit claims, retains symbolic
neighboring connections and revokes old handles. Nested/repeated modules flatten
to independently spawned primitive instances, without runtime wrappers.

`NetworkSession` owns bounded FIFO ingress/connections/captures, reservations,
mutable processes, ready work, handed jobs, retained publication, request scopes
and wakes. One selected emission fanout commits atomically. Later blocking or
failure preserves earlier commits. Retry retains prepared packets and does not
repeat action side effects. EOS, quiescence, cycles, cancellation, source starts,
multiport processing and function adapters use the same engine.

Owned `NetworkPatch` edits derive exact footprints. Separate host registrations
bind grants, origins, dependencies and explicit priorities. Whole-candidate replay
and compile is atomic; revocation recomposes from base. Compatible replacement
does not claim incident edges. Splices require dependency order; introduced
semantic obligations cannot be stripped by later replay edits.

Kernel preserves provider selection, exact origins, candidate-local artifacts,
activation retry, producer retirement, wake/shutdown and typed error sources.
Full admission retains one pending producer event without repolling. StorageGraph
keeps authorized subscriptions/snapshots and uses native demand scopes/captures.
Explicit failure recovery can restart affected process state, never roll back
external effects. Values, factories, jobs, query results and callback errors keep
their code/resource leases independently of the original host generation.

See the final report for local verification and immutable Nix snapshot results.
At that historical cutover there was no new commit or remote CI result because
neither was requested; this is not the current PR #1302 verification status. The
retired-suite ledger records meaningful behavior mappings and intentional API
changes, not a claim that fewer tests prove every historical assertion equivalent.
No DFIR experiment, native performance baseline or formal progress proof is
claimed. FIFO arrival skew and cycles can block; cancellation is cooperative.
