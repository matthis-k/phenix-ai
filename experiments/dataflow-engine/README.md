# Trigger/dataflow engine experiment

Experimental, unmerged; no shipping composition changes. Control parent: PR #1239
at `33d8c4bbec8d450e9352d71c7b790854fbb468a2`, itself based on #1220 at
`d9c4c9f62dcb94cc2d83679475d08b34fc9d50a9`. Research date: 2 November 2026.

## Decision

**B: retain Phenix execution semantics and evaluate selected mechanisms.** The
experiments demonstrate a real reduction in *progress-algorithm* work with Timely
capabilities/frontiers, and useful independent resource/cancellation components.
They do **not** demonstrate a safe substantial engine replacement (C), and do not
justify a different architecture (D). This is an experimental recommendation,
not a claim that an integrated replacement passed the full acceptance contract.

Timely handles routing, queued/in-flight messages, retained async capabilities,
feedback timestamps and frontier exhaustion without a shadow scheduler. A small
terminal-merge operator uses its input frontier to wait for possible late failures
and reject multiple values. No manually propagated branch-closure messages are
needed in that probe. However, Phenix must still own root admission, authenticated
rights, occurrence identity, selected native calls, pending leases, physical drain,
commit/retry policy and generation lifetime. Replacing those with Timely would
lose guarantees. A capability is permission to produce at a logical timestamp;
it is **not** a Phenix authorization grant or a durable physical-work lease.

DFIR expresses branch routing, keyed joins and async continuations concisely.
Its scheduler returning idle is not a root-completion proof: a controlled future
can remain pending. Its eager union also cannot replace terminal ExactlyOne;
the counterexample publishes a success before a possible late failure. This is
not proof that an appropriate DFIR operator cannot be written: it shows the
additional completion protocol that this adapter deliberately does not invent.

## Before: pinned architecture and status

| Layer | Inspected state | Evidence / boundary |
|---|---|---|
| Integrated main | `28cf5cd13916ad996daec2674a9966c05c48676e` | Graph V2 compilation/execution exists alongside M1. Older index prose saying no V2 runs is stale; source wins. #1175 Join is merged. |
| #1176 | `253b2bd7527af6e0068bef3b16fa6a8c3adc1916`, open | Optional/StaticMany finite readiness, not repeatable Any. |
| #1186–#1220 stack | open, ancestors of control | Native embedding, pending, repeated/deferred activations, cancellation drain, typed nested boundaries, invocation batches and placement are transitional, not main functionality. |
| #1239 control | `33d8c4bbec8d450e9352d71c7b790854fbb468a2`, draft/open | Existing Wave/ticket ownership plus settlement guard; 72 local V2 tests passed. GitHub exact-head run 41402562438 passed four builders, coordinator, shipping join and runnable tests. This is parent evidence, not this branch's CI. |
| #1238 reference/testkit | `7005aed7d5ee36a9d89bf3722107f2a0cb107597`, sibling | Bounded reference model and Wave helper, not a public GraphExecution trigger testkit; not transplanted into this experiment. |
| #1188 normative proposal | `9fd92666c6e761f19bb2c85c878fcf7fb4cb54c6`, independent draft | D39–D41 and §§7.7.1/9.4/9.5: per-occurrence progress, region Drain, testkit. Design constraints, not implemented guarantees. |
| Product descendants | #1221 → #1226 → #1227 → #1228 → #1229 → #1231 → #1237 | Unmodified; this branch is a sibling after #1239, not a product restack. |

Read [AGENTS](../../AGENTS.md), [spec index](../../spec/README.md),
[latest target](../../spec/graph-architecture-v2.4.md),
[layout](../../docs/plugin-layout.md), [plugin rules](../../docs/plugin-dos-and-donts.md),
[CI policy](../../ci/README.md), and source/tests before inferring behavior.
The #1188 version is a separate design input; this branch does not change it.

Triggers supply values, state access does not activate work, typed edges propagate
outputs, plugins retain domain state, and the kernel retains admission/authority,
causal identity, cancellation and completion. Placement and graph topology remain
independent. No frontend, agent policy or storage architecture is introduced.

## What was built

* `fixture/`: shared Message/Identity/Prepared/ResultValue types, normalization,
  branch choice and response operations. Explicit root/wave/activation/occurrence.
* `native-contracts`, `native-plugin`, `native-host`: independently compiled ELF
  plugin with embedded canonical V2 metadata, SDK contracts and ABI-3 host.
  Trigger → Prepare → Switch → Model/Tool → ExactlyOne Aggregate → Complete runs
  through `Prepared::prepare_graph_v2`, the verified loader and `GraphExecution`.
  Native Message, Identity and ResultValue are structural Portable records;
  Message fields are verified by the kernel, not hidden in a string port. A malformed
  outer Text-only payload must be rejected before admission. Explicit field
  conversions share the engine-independent logical operations. Earlier JSON-wrapper
  measurements below are historical and do not apply to this corrected fixture.
* `src/timely_engine.rs`: Timely operator graph with per-admitted-future capability,
  actual activator/waker integration and probe. No ready queue or progress counter.
* `src/dfir_engine.rs`: DFIR native graph, tee/filter branches, resolve_futures,
  union and output sink. No custom scheduler and no invented completion oracle.
* `src/frontier_tests.rs`: terminal merge, late failure, multiple producer conflict,
  native feedback/defer_tick, keyed correlation and scalar-frontier counterexample.
* `tests/remote.rs`, `remote-worker`: actual subprocess service and native two-worker
  Timely exchange. Serialized payload is not trusted authority. A coordinator holds
  the real capability while waiting, including timeout kill **and reap**.
* `../dataflow-components`: Bevy SystemState parameter extraction/conflict rejection
  and petgraph cycle/order analysis; neither installs another scheduler.
* `flake.nix`: two isolated checks discovered by the existing four-builder Nix DAG.
  No new workflow, root Cargo dependency, shipping selection or production ABI.

### Custom machinery accounting

| Mechanism | Owner in experiment | Production still needed |
|---|---|---|
| Graph scheduling/routing | Timely / DFIR | Lower arbitrary verified topology to operators; preserve generation lifetime |
| Async future polling | Timely adapter FuturesUnordered + native waker; DFIR resolve_futures | Bridge existing native tickets; the synthetic future is not ABI pending ownership |
| Progress | Timely native capability/probe | Choose causally isolated timestamp/region semantics; retain physical leases |
| Terminal ExactlyOne | Timely frontier-backed operator with buffered candidates | Generalize schemas, security and region policy; DFIR equivalent missing |
| Readiness | Native Phenix; DFIR keyed join probe | Optional/StaticMany/failure semantics over repeated occurrences are not a join alone |
| Identity | Explicit shared envelope, assigned by fixture caller | Kernel must allocate and validate identities; engines do not authenticate them |
| Authorization/dedup | Explicit ingress BTreeSet and boolean test grant | Real attenuation/selected operation grants, schema decoding and revocation |
| Cancellation | Plugin tokens + retained futures/capabilities | Fence every publication and drain ABI tickets/threads/process trees |
| Retry/effects | Plugin-owned occurrence map | Durable crash/retry receipts and explicit retry identity policy |
| Transport | Spawn, framed JSON-to-EOF, deadline, kill/reap | Connection protocol, remote leases, sequence/replay validation, bounded payloads |

`physical_pending()` counts only controlled oneshot work, not subprocesses. Timely
capabilities additionally retain subprocess futures. Neither this helper nor an
empty output list is used to infer successful root completion.

## Required scenario results

**P** = verified bounded behavior; **A** = adapter/plugin policy; **M** = missing
full requirement; **E** = existing native acceptance, not the shared fixture.
Green counterexample tests establish a mismatch; they do not pass the requirement.

| Required behavior | Phenix control | Timely prototype | DFIR prototype |
|---|---|---|---|
| External trigger/new root | P native fixture; kernel owns root | A caller-assigned identity, typed delivery; kernel admission missing | Same |
| Atomic several named ports | M shared multiport test; canonical Join tests E | M; batch admission atomic only on Prepare.in | M; keyed two-port join P, not transactional multiport ingress |
| State commit triggers | E owner-effect acceptance; M shared fixture | A explicit successful CAS then emit; no crash atomicity | Same |
| Read/Write nonactivation | E native state effects; M signature API parity | P plugin state access; Bevy adapter separate | Same |
| Conditional branch | P native Switch/ExactlyOne | P filter path | P tee/filter path |
| Multiple producers/readiness | E canonical Join/ExactlyOne | P frontier-backed terminal probe; generic repeated input readiness M | P keyed join; terminal ExactlyOne M (eager counterexample) |
| Repeated delivery occurrences | E deferred/InvokeEach; same-wave repeatable Any M | P distinct IDs, A duplicate rejection | Same |
| Pending/no premature execution | P controlled ABI-3 pending in same fixture + E acceptance | P retained future/capability | P resolve_futures |
| Cancellation/physical drain | P controlled same-fixture thread exit + E native panic drain | P controlled work, A token fence; native ABI/process-tree parity M | P controlled work; root completion M |
| Deferred root identity | E V2 Deferred | P native feedback and envelope identity | P defer_tick and envelope identity |
| Concurrent activations | E canonical native identity | P reverse completion, state snapshot; scalar timestamp isolates completion poorly | P reverse completion; causal completion oracle M |
| Inactive branches terminate | P native smoke + E acceptance | P probe exhausted, no fake deliveries | P selected result; quiescence ≠ completion |
| Remote identity/order | M native remote execution | P subprocess identity + native worker exchange; order/replay protocol M | P subprocess identity; order/replay protocol M |
| Failure/retry effects | E durable product tests, not shared fixture | A in-memory occurrence idempotency; crash/restart M | Same |
| Worker disappearance | Native pending transport not this fixture | P exit/deadline classification and capability release after reap | P exit classification; no root failure/completion API |
| Unauthorized/duplicate IDs | P native denied scope; E selected authority; duplicate native occurrence ingress M | A reject atomically before framework admission | Same |

Do not promote these prototypes to replacements: the shared scenario driver covers
both candidate adapters, while the native fixture covers synchronous branches and controlled ABI pending/cancel/denial probes. Existing native acceptance has broader guarantees but is **not**
claimed to be trace-equivalent to the shared synthetic suite.

## Native / adapter / missing capability matrix

| Capability | Phenix #1239 | Timely 0.31 | DFIR 0.16 | Smaller components |
|---|---|---|---|---|
| Typed flow graph | Native structural ValueSchema + ABI | Native Rust streams; ABI lowering adapter | Native macro Rust types; ABI lowering adapter | petgraph topology only |
| Runtime-selected plugins | Native verified packages and grants | Operator closures possible; native loader remains adapter | Closures possible; current inline topology generated at compilation | Bevy resources use host Rust types |
| General runtime graph edits | Frozen compiled generation | Build a new dataflow; coordinated graph construction across workers | Inline macro graph is static; old scheduled Core API documentation is transitional | petgraph mutable graph is not an execution engine |
| In-flight logical progress | Finite Wave/tickets; occurrence settlement guard | Native capabilities/frontiers | No tested causal frontier/region completion | TaskTracker only tracks registered physical tasks |
| Authorization | Native | Missing; retain kernel | Missing; retain kernel | None grant authority |
| Cancellation/drain | Native tickets/leases | Adapter + native progress retention | Adapter + futures; completion missing | CancellationToken cooperative; TaskTracker waits, does not abort |
| Native ABI independence | Established ABI-3 | Preserve at adapter boundary | Preserve at adapter boundary | Bevy Rust resources must not cross ABI |
| In-process execution | Native | Native | Native | All useful in process |
| Remote execution | Planned adapter boundary | Native worker exchange; plugin subprocess adapter | Process/stream network operators + transport adapter | Tokio process/tasks; no distributed safety |
| Failure recovery/failover | Domain receipts + kernel lifecycle; bounded evidence | No verified restart/reconfiguration protocol | Application protocol required | None supply durable effect semantics |

## Research and maintenance

Pinned source is Cargo.lock. Registry `.cargo_vcs_info.json` and upstream source
are more precise than an unversioned tutorial. Repositories were active/not archived
when checked: Timely pushed 31 Oct; Hydro 2 Nov; Bevy 30 Oct; Tokio 31 Oct;
petgraph 31 Oct 2026. Activity is not an API-stability or suitability guarantee.

| Candidate | Direct mechanisms | Remaining Phenix machinery / integration cost |
|---|---|---|
| Timely 0.31.0, Rust ≥1.86 | Typed streams, custom operators, capabilities, antichain progress, worker exchange, feedback | Different scheduler/runtime; coordinated worker graph construction. Per-root authority, typed dynamic schema bridge, terminal/failure policy and physical leases remain. Capabilities are worker-local, not serializable authorizations. Current API uses Scope by value, container builders, output-indexed retain, and tuple frontier handles. |
| DFIR 0.16.0 | Reactive operators, async futures, ticks, keyed joins, stream/network integration | Macro-generated inline graph; native contracts and verified dynamic graph compiler remain. No tested substitute for causal progress/terminal completion. Latest 0.17 alpha exists; not selected over stable. Old Core API docs explicitly warn about the inline transition. |
| bevy_ecs 0.19.1, Rust ≥1.95 | Function/SystemParam resource extraction, access validation, conflict detection | Optional in-process typed adapter. Do not map world change detection to activation. TypeId/world identity cannot replace structural schema, scoped plugin state or kernel grants. No ECS schedule used. |
| tokio-util 0.7.20 | CancellationToken + TaskTracker | Cooperative signal and wait-for-task-exit, not forced abort, root sealing, transitive graph completion or remote work ownership. Already uses Tokio runtime when spawned. |
| petgraph 0.8.3 | Toposort, stable graphs, cycle/reachability algorithms | No scheduler/runtime/ABI. Preserve canonical tie-breaking, logical IDs, typed endpoints, source satisfiability and authority. Existing tiny deterministic dag.rs may remain simpler; no demonstrated production code reduction yet. |

Upstream references (documentation and source):

* [Timely 0.31 API](https://docs.rs/timely/0.31.0/timely/),
  [capability source](https://docs.rs/timely/0.31.0/src/timely/dataflow/operators/capability.rs.html),
  [progress model](https://timelydataflow.github.io/timely-dataflow/chapter_5/chapter_5_2.html),
  [operator execution](https://timelydataflow.github.io/timely-dataflow/chapter_3/chapter_3_3.html),
  [changelog](https://github.com/TimelyDataflow/timely-dataflow/blob/master/CHANGELOG.md).
* [DFIR 0.16 source](https://docs.rs/crate/dfir_rs/0.16.0/source/),
  [architecture transition](https://hydro.run/docs/dfir/architecture/),
  [operators](https://hydro.run/docs/dfir/syntax/surface_ops_gen/),
  [single-core/process FAQ](https://hydro.run/docs/dfir/faq/),
  [Hydro source](https://github.com/hydro-project/hydro).
* [Bevy SystemState](https://docs.rs/bevy_ecs/0.19.1/bevy_ecs/system/struct.SystemState.html),
  [SystemParam source](https://docs.rs/bevy_ecs/0.19.1/src/bevy_ecs/system/system_param.rs.html).
* [CancellationToken](https://docs.rs/tokio-util/0.7.20/tokio_util/sync/struct.CancellationToken.html),
  [TaskTracker](https://docs.rs/tokio-util/0.7.20/tokio_util/task/struct.TaskTracker.html).
* [petgraph toposort](https://docs.rs/petgraph/0.8.3/petgraph/algo/fn.toposort.html),
  [StableGraph](https://docs.rs/petgraph/0.8.3/petgraph/stable_graph/struct.StableGraph.html).

### Build blockers and reproducer

`repro-dfir-016` is a separate, intentionally noncompiling workspace. Run
`cargo check --locked --manifest-path experiments/dataflow-engine/repro-dfir-016/Cargo.toml`.
The native partition → union minimal example fails E0283 in `demux_var` (Meta: Copy
cannot be inferred). This is **not** the working adapter: tee/filter compiles.
DFIR 0.14 also required an older tokio-util to avoid an Encoder inference failure;
that version was abandoned after stable 0.16 worked. Initial move-capture failures
were adapter mistakes, corrected rather than attributed to DFIR.

Local Rust 1.99.1 ThinLTO linking failed with undefined Timely/alloc symbols. This
is a build/toolchain observation, not an established framework defect. Recheck
with repository CI's supported toolchain and record exact build settings before
using any timings. The repository-supported Rust 1.96.1 succeeds with the same ThinLTO release settings. Do not infer performance from debug smoke timings.

## Historical measurements (JSON-wrapper control, not equivalent engines)

These measurements were collected before the structural-port correction, at
`4bba634c5f1ff27387622948a600f79d36cb18fb`'s experiment sources. They are retained
as historical evidence. The corrected structural native fixture has **not** been
benchmarked: the local execution environment became unavailable. Candidate
adapter code is unchanged. Source fingerprints in measurements.json identify
what was actually measured.

Rust 1.96.1 / LLVM as shipped with that toolchain, x86_64 Linux, AMD EPYC 9V74
virtual CPU; release, ThinLTO, one codegen unit, 2,000 alternating immediate
Model/Tool deliveries, traces retained. These numbers describe different admission
and dispatch boundaries. They cannot establish a production speedup or rank engines.

| Sample | Actual elapsed | Median / p95 delivery | Cumulative allocations | Cumulative allocated bytes |
|---|---:|---:|---:|---:|
| Timely in-process fixture | 13.782 ms | 5.439 / 8.503 µs | 47,104 | 108,733,272 |
| DFIR in-process fixture | 2.817 ms | 0.891 / 1.723 µs | 25,037 | 6,046,062 |
| Historical native GraphExecution / ABI-3 / JSON | 81.727 ms | not instrumented | not instrumented | not instrumented |

Observed sequential fixture rates, calculated from those samples: approximately
145,112 / 709,867 / 24,472 deliveries/s respectively. Driver polling, kernel
admission, ABI crossing, serialization and terminal semantics differ; this is not
an equal-throughput comparison. One recorded final sample (earlier exploratory runs varied substantially), shared virtual hardware, no CPU
pinning or confidence interval. The allocation counters include retained trace,
state and framework work; bytes are cumulative, not resident memory.

Supported-toolchain initial workspace release build: Cargo reported 1m01s with
registry downloads already cached. Source-change rebuild: 26.555s; fixture mtime
rebuild: 29.188s; subsequent true no-op build: 0.165s. These are whole-experiment
builds, not isolated per-engine incremental times. The large shared dependency
closure prevents attributing those times to either framework alone.

| Component | Resolved external package closure, including crate | Formatted Rust LOC (scope) |
|---|---:|---|
| Phenix kernel | 22 external, 25 including local crates | 1,186 V2 execution + 837 V2 compiler; excludes recursive compiler, shared graph/runtime/tickets |
| Timely | 25 | 118 adapter + shared plugin/boundary code and terminal probe |
| DFIR | 149 | 50 adapter + shared plugin/boundary code; no root completion implementation |
| tokio-util | 23 | shared controlled service/transport/boundary module: 179 (not all cancellation) |
| bevy_ecs (std only) | 74 | component probes total 53, shared with petgraph |
| petgraph (std/stable_graph) | 14 | current deterministic Phenix dag.rs is only 77 lines |

The measured JSON-wrapper native fixture added graph declarations (132), contracts
(26), plugin (96), host (203) and build hook (6), plus shared fixture (71). The
structural-port correction adds explicit field conversions and larger typed
contracts; it is not included in those historical native LOC/performance counts. Tests/probes, benchmark, worker,
source filters and lockfiles are additional. Raw adapter LOC does not measure
removed production machinery: the smaller adapters implement fewer guarantees.
Dependency counts are feature-unified Linux Cargo metadata closures, not disjoint
new dependencies; do not sum overlapping closures. Full counts/settings and source
paths are in [measurements.json](measurements.json); [measure.py](measure.py)
reproduces them and the benchmark/build samples.

## Reproduction

```sh
cargo test --locked -p phenix-kernel --lib graph::v2
cargo test --locked --manifest-path experiments/dataflow-engine/Cargo.toml --workspace
cargo test --locked --manifest-path experiments/dataflow-components/Cargo.toml
cargo build --locked --release --manifest-path experiments/dataflow-engine/Cargo.toml --workspace
experiments/dataflow-engine/target/release/dataflow-native-host --package \
  experiments/dataflow-engine/target/release/libdataflow_native_plugin.so /tmp/dataflow-native-package
PHENIX_BENCH_N=2000 experiments/dataflow-engine/target/release/benchmark
# Canonical remote Nix DAG includes existing acceptance plus the two isolated checks.
nix build .#checks.x86_64-linux.experimental-dataflow-engine \
  .#checks.x86_64-linux.experimental-dataflow-components
```

The benchmark retains traces and plugin state; allocations are cumulative allocator
calls/bytes, not peak memory. Timely is driven by 32 bounded worker steps per
sample, DFIR by run_available. The native host crosses serialization/ABI and creates
roots; candidate identities are caller-supplied and do not provide full terminal
semantics. These are payload-smoke measurements, **not equivalent-engine rankings**.
See `measurements.json` when present for actual results and environment.

## Incremental implementation plan and affected PRs

1. Keep #1176 and #1186–#1237 semantics/ancestry intact. Review this experimental
   sibling after #1239; merge/restack nothing as part of this evaluation.
2. Use #1238's reference oracle and #1188 §§9.4–9.5 to define one reusable native
   GraphExecution test driver, controlled ABI pending fixture and typed port test
   API. Port the complete common scenarios before claiming engine equivalence.
3. After #1220/#1239, separately prototype a **private** Timely-backed progress
   adapter. Kernel-issued occurrence rights mint/retain/drop capabilities; kernel
   physical leases remain authoritative. Explicitly handle independent roots,
   deferred cross-wave transfers and region ingress sealing. Do not serialize a
   local capability or equate frontier exhaustion to successful root outcome.
4. If that adapter removes more production state/algorithm complexity than it adds,
   run all #1238 ordering/failure cases and native cancellation/authority acceptance.
   Otherwise retain Graph V2's smaller specialized mechanism (A). The present
   isolated terminal operator is motivation to test this, not proof to adopt it.
5. Independently use tokio-util where plugin async task lifecycles already exist;
   try Bevy extraction only inside typed Rust ownership domains. Use petgraph only
   if analysis code actually shrinks and canonical ordering stays verified.
6. Revisit a DFIR replacement only with a concrete native completion/ownership
   protocol and dynamic composition plan. A macro DAG or successful message
   transfer does not justify changing native contracts, kernel authority or topology
   to fit an execution framework.

## Final state / scope of evidence

Implemented isolated native/Timely/DFIR prototypes, component probes and subprocess
experiments. Local results apply to the pre-correction sources: 19 candidate unit/probe tests,
6 process/worker integration tests, 3 component probes, the 72 pinned-control V2
tests, and native branch/pending/cancel/denial probes passed. The structural-port
correction requires its own exact-head CI; no new local verification is claimed. Reproducer fails as expected on Rust 1.96.1 and 1.99.1. CI status must be read at the experiment's
exact head, separately from the verified control parent. Mandatory gaps above
remain open. No production semantics, existing PR heads or product compositions changed.
