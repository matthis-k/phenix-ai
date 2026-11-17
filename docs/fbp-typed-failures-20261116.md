# Q7 Typed Failures: Current Status And Historical Checkpoints

## Current Status: 2026-11-17

**Original Q7 is scoped verified at
`6f6e751b36f77273da8799900f665bc026447418`: 26.0% -> 100.0%, +74.0 pp.**
Independent whole-slice reviews, all local assembled gates including the Nix
snapshot and actual committed-head `nix flake check`, and all fifteen exact-head
hosted workflows passed; hosted SUCCESS was confirmed through `gh`. This supersedes all
historical phase-1 scope restrictions, API descriptions, file-ownership lists,
and verification/remaining-work assertions below. Those checkpoints are retained
as history, not as the current implementation contract.

The accepted code implements the full native migration across Graph failures,
Process/Value/guards/helpers/codecs/macros/adapters/handoff/runtime/request/session,
Kernel, StorageGraph source local Failure DATA/query, typed framework reasons,
plugin registry/binding/lifecycle errors and lifetimes, and adjacent storage causes.
Acceptance is not narrowed to a core-only seed.
See [Final Review And Erasure Inventory](fbp-q7-final-review-20261117.md) for
current boundaries, constraints, whole-slice review results, tests and all fifteen
exact-head run URLs. This evidence-only docs revision requires its own checks;
the implementation's accepted gates do not certify this revision or newer code.

- Final reviews reproduced six more P2 issues, now fixed: recursive secondary
  leases in `from_value`; an unchecked virtual Process disposer; a discarded
  emission snapshot's final lease panic; nested secondary leases in Kernel
  `lease_failure`; late-cleanup request/session COW divergence; and invalid-emission
  secondary metadata. Public `with_lease` now enriches existing cleanup recursively,
  and `with_cleanup` recursively inherits known parent leases. Earlier escaped
  clones remain unchanged.
- `NetworkError::IncompletePublicBoundary` distinguishes the framework sentinel
  from interface rejection. The network Interface display forwards the human
  cause; Kernel `MissingUnit` retains the original `NetworkError` source. Typed
  claim tests preserve actual Compilation versus Interface validation precedence.
- An independent final fix reviewer found only two test expectation mistakes,
  since corrected, and no additional production issues. Plugin review covered all
  requested manager conditions; 38 plugin tests and strict Clippy passed. Subsequent
  independent whole-slice reviews accept the original full Q7 migration; focused
  reviews alone were not substituted for that acceptance.
- Assembled local gates passed: default/no-default workspace tests, standalone
  Graph no-default tests, explicit workspace doctests in both modes and standalone
  Graph no-default doctests, all-target compilation, strict workspace Clippy,
  formatting, and all ten Nix snapshot checks. Actual committed-head
  `nix flake check` also passed, followed by all fifteen exact-head hosted workflows.
- Benchmark CI adaptation passed independent review. The same three
  harnesses select baseline-only String versus candidate Failure through explicit
  cfg, preserve baseline `516e088`, and record API selection and `RUSTFLAGS`.
  Production has no shim. Candidate/baseline release builds, Clippy, and example
  tests passed; this is not performance acceptance.
- The flake now includes default doctests through the per-package helper,
  Graph-plus-plugin-plus-derive doctests, and workspace no-default doctests, without a new
  workflow. Pre-commit snapshot verification included the then-untracked sources
  through `builtins.path`, excluding `.git` and `target`; a Git-filtered snapshot
  would have omitted them. Snapshot `/nix/store/cbqspiik1gij7zk12fw7fmidwz2w8nxy-source`
  passed all checks, including those modules and tests, before committed-head checks.

Authoritative counts are **19 scoped verified / 6 partial / 7 open / 1 approved
exclusion**, leaving **13 partial/open** findings. The subjective issue mean is
**75.4% (2489 / 33)**, not product completion. C7's old 12.0% is unchanged with no
sandbox credit; prior Q1 acceptance and its broader Kernel residual remain intact.
Leases are per independently escaping handle and nonretroactive; cached host
observation remains passive, and ordinary Drop cannot report retroactively.
Trusted native abort/double-panic exclusions remain. Green benchmark execution
does not establish performance acceptance or M7/M8 completion.

## Historical Checkpoints

Everything below records its own earlier scope and evidence. In particular,
"Stable Phase-1 API", "Remaining Full Q7 Work", and the final no-workspace-tests
statement are historical and are superseded by Current Status above.

## Runtime Review Fixes: 2026-11-17

**Q7 OPEN.** This focused follow-up addresses the three runtime P2 findings only.
Edits are confined to `packages/phenix-graphs/src/runtime/execution.rs`,
`packages/phenix-graphs/src/runtime/recovery.rs`, appended cases in
`packages/phenix-graphs/tests/fbp_failure_protocol.rs`, and this checkpoint in
`/tmp/opencode/fbp-q7-typed-failures-7c611912`. Existing unrelated work was left
intact. No main-worktree edits, staging, commits, or pushes.

- Preparation failure plus emitted-packet destructor panic now terminates exactly
  once. Cleanup-only termination is confined to successful preparation. The
  original connection failure remains primary, and the payload cleanup secondary
  carries the accepted completion's real ticket, output, request, and producer
  identity. A queued next request survives recovery under both queue policies.
- Request cleanup secondaries are attached before request-originated escalation.
  After `fail` appends session-stop peer cleanup, its enriched envelope is copied
  back to the initiating request, including recovery-constructor escalation.
  Both current slots retain the same original concrete non-Clone cause pointer
  and ordered request-cleanup / recovery-construction / session-stop inventory.
  Previously escaped clones remain unchanged. Inherited late cleanup still
  appends to the original session cause and returns typed operation errors;
  cancellation under FailSession remains Cancelled without a fabricated request
  primary. Cancellation cleanup is retained on the actual escalation diagnostic.
- Recovery-constructor faults explicitly retain the retiring RequestId after the
  runtime slot is cleared. Outstanding tickets and known accepted-failure tickets
  are captured before retirement; unknown tickets remain None. Tests cover primary
  and secondary RecoveryConstruct faults, host failure, accepted step failure,
  cancellation with no prior primary, handed-job refusal, and disposal idempotence.
- Four appended protocol tests exercise these paths across both queue policies;
  escalation inventory also covers both failure policies. Known leases, exact
  handed-job inventory, original cause identity, immutable escaped clones, and
  request/session outcomes are checked. No Job/Completion cloning or Q6 changes.

Actual verification passed with the isolated `CARGO_TARGET_DIR` and Nix tool PATH
from Verification below (rustfmt used the cached 1.96.1 binary):

```bash
cargo test --locked --offline -p phenix-graph --lib --test fbp_failure_protocol --test fbp_failures --test fbp_terminal_cleanup --test fbp_deferred_timeout --test fbp_inflight_worker_recovery --test fbp_request_session_isolation --test fbp_request_lanes --test fbp_owned_steps --test fbp_connections --test fbp_streams
cargo test --locked --offline -p phenix-graph --no-default-features --lib --test fbp_failure_protocol --test fbp_failures --test fbp_terminal_cleanup --test fbp_deferred_timeout --test fbp_inflight_worker_recovery --test fbp_request_session_isolation --test fbp_request_lanes --test fbp_owned_steps --test fbp_connections --test fbp_streams
cargo test --locked --offline -p phenix-storage-graph --test query_cleanup --test typed_failures
cargo clippy --locked --offline -p phenix-graph --lib --test fbp_failure_protocol --test fbp_failures --test fbp_terminal_cleanup --test fbp_deferred_timeout --test fbp_inflight_worker_recovery --test fbp_request_session_isolation --test fbp_request_lanes --test fbp_owned_steps --test fbp_connections --test fbp_streams -- -D warnings
cargo clippy --locked --offline -p phenix-graph --no-default-features --lib --test fbp_failure_protocol --test fbp_failures --test fbp_terminal_cleanup --test fbp_deferred_timeout --test fbp_inflight_worker_recovery --test fbp_request_session_isolation --test fbp_request_lanes --test fbp_owned_steps --test fbp_connections --test fbp_streams -- -D warnings
rustfmt --check --edition 2024 --config skip_children=true packages/phenix-graphs/src/runtime/execution.rs packages/phenix-graphs/src/runtime/recovery.rs packages/phenix-graphs/tests/fbp_failure_protocol.rs
git diff --check
```

- Graph default: 207 passed; no-default: 206 passed. Counts by target: library 18,
  protocol 15/14 (derive-only codec case), failure core 12, terminal cleanup 5,
  deferred timeout 11 (including sibling expiry), inflight worker recovery 1,
  request/session isolation 7, request lanes 8, owned steps 25, connections 28,
  streams 77. No failures in either final grouped run.
- StorageGraph: query_cleanup 2 and typed_failures 2 passed. Existing global
  retained-capture cleanup expectations were not edited or weakened.
- Strict scoped Graph Clippy passed in both feature modes. Scoped rustfmt check
  and `git diff --check` passed. Initial new-test runs exposed fixture scheduling
  and source-less application-message assertion mistakes; those were corrected
  before the passing grouped runs above.

Remaining verification: full-workspace default/no-default reruns, final all-target
compilation, docs in both modes, strict workspace Clippy, production erasure
inventory, independent review/main CI, and Nix snapshot verification if feasible.
This is not a full-suite or Q7 closure claim; older checkpoint status below is
historical.

## Review Fixes: 2026-11-17

Focused follow-up for the two independent-review P2 findings. **Q7 remains
INCOMPLETE**; this is not a full-suite or closure claim. This follow-up edited
only `packages/phenix-graphs/src/process.rs`,
`packages/phenix-graphs/tests/fbp_failure_protocol.rs`, and this checkpoint, in
`/tmp/opencode/fbp-q7-typed-failures-7c611912`. No main-worktree edits, staging,
commits, or pushes.

- Invalidated emissions keep their exact known leases through value destruction
  and caught-payload destruction, then release each cloned lease in reverse order
  under actual containment. Each caught final lease panic is reported as a typed
  `ProcessCleanupError::Emission` secondary without replacing the guard primary.
  Successful payload disposal does not silently defer independent lease cleanup.
- Accepted job identity is captured before restoration consumes the execution.
  New completion/rejection `OwnedState` and emission-cleanup faults carry the
  actual ticket, session, generation, request, process, and path. Enrichment keeps
  each fault's phase, guard index, and field; standalone jobs keep unknown network
  identities as `None`. Late completion lease faults also retain the still-live
  handoff code lease before the failure escapes.
- Two added protocol regressions cover a nonpanicking emitted payload with one
  sole independent final-panicking lease and with three final-panicking leases;
  all releases and reverse guards are attempted. Non-string panic payload drops
  observe live job/input leases and guard objects, while the consumed input Value
  itself has already dropped. Real scoped/unscoped NetworkSession jobs cover six
  cases each: successful Complete plus context panic, successful Complete plus
  final input-lease panic, accepted rejection cleanup, original step failure plus
  cleanup, emission payload panic, and emission lease panic. Assertions inspect
  exact accepted-job metadata on primaries/secondaries, preserve original fields
  and guard identity, and retain escaped cleanup leases after terminal teardown.

Verification passed with the isolated `CARGO_TARGET_DIR` and Nix tool PATH from
the Verification section below:

```bash
cargo test --locked --offline -p phenix-graph --lib --test fbp_failures --test fbp_failure_protocol --test fbp_owned_steps
cargo test --locked --offline -p phenix-graph --no-default-features --lib --test fbp_failures --test fbp_failure_protocol --test fbp_owned_steps
cargo test --locked --offline -p phenix-kernel --test worker_guards
cargo clippy --locked --offline -p phenix-graph --lib --test fbp_failure_protocol --test fbp_failures --test fbp_owned_steps -- -D warnings
cargo clippy --locked --offline -p phenix-graph --no-default-features --lib --test fbp_failure_protocol --test fbp_failures --test fbp_owned_steps -- -D warnings
```

- Default Graph results: library 18, failure core 12, failure protocol 11, owned
  steps 25; all passed.
- No-default Graph results: library 18, failure core 12, failure protocol 10,
  owned steps 25; all passed. The derive-only codec test is feature-gated.
- Kernel worker guards: 4 passed. Strict scoped Clippy passed in both Graph
  feature modes; scoped rustfmt check and `git diff --check` passed.
- Full workspace/main verification is deliberately left to the requested root
  rerun. The older checkpoints below are historical, not a Q7 closure claim.

Status: **Q7 INCOMPLETE**, uncommitted core seed, not a shipping compatibility layer.
Base: `7c61191222a19cefccde9c38adedbc9ad290bf01`.
Worktree: `/tmp/opencode/fbp-q7-typed-failures-7c611912`.

## Checkpoint

Final-gate continuation, 2026-11-16 (supersedes older verification status below):
Q7 remains open pending final gates and independent review/main CI. Work is only
in this isolated worktree, with its own explicit `CARGO_TARGET_DIR`; no staging,
commits, pushes, merges, or main-worktree edits.

- StorageGraph focused rerun passed: derived (30), error_sources (5),
  host_derived (7), query_cleanup (2), typed_failures (2). The terminal-close
  skip is present and passed the original scope-failure/ticket cleanup tests.
- Remaining gates: workspace default/no-default tests, isolated Graph no-default
  tests, all-target compilation, docs both modes, strict workspace Clippy, fmt,
  focused Graph core/protocol reruns, and production erasure inventory.

Latest continuation checkpoint (supersedes the earlier continuation checkpoint
below): native protocol migration is implemented across Graph, Kernel, and
StorageGraph, and all callers have undergone signature migration. **Q7 remains
unverified and open.** All changes are uncommitted in this worktree.

Completed continuation phases:

- Graph process/guard/job/completion/handoff, runtime/request/session, Value and
  input reconstruction, adapters and partial fanout all carry Failure. Managed
  Value/Packet/Process/Job/Completion cleanup reports real caught panics; ordinary
  Drop contains but cannot retroactively report. Jobs snapshot only lease Arcs
  from actual owned inputs, not candidate Values or scheduler snapshots.
- Both derive generators and independent portable provider/consumer codecs use
  Failure. Codec rejection preserves field/tag/offset/length details and actual
  UTF-8, integer-conversion, and slice-conversion sources. Portable conformance
  utility errors are typed as well.
- Kernel exposes `SessionAccessError::{NotStarted, Reentrant, Busy}` for access
  and Generation forwarding, `ResolutionError::Session(Failure)`, and
  `DriverError::SessionStopped { cause: Option<Failure>, .. }` with source.
  DriverError no longer pretends arbitrary failures have equality. Kernel's
  `lease_failure` attaches its existing execution lease before query clones escape;
  it does not add a candidate-snapshot retention lease.
- StorageGraph source `error` port and projection are local Failure DATA. Read and
  decode failures preserve StorageError and phase, with immutable read/codec
  configuration ownership, not the process/context. Storage now offers
  `CodecCause { summary, cause }` to retain actual external codec errors without
  introducing a Graph dependency.
- Derived errors expose `Execution(Failure)`, `Wave(Vec<Failure>)`, typed Access
  and Transport. Query normalization reuses Execution directly or moves the
  non-Clone initiating derived error once into external Failure; cancellation is
  classified before that move. Cleanup remains outer, RequestInterrupted remains
  outermost, inventory is collected before release without early `?`, and
  `Cleanup::inventory` records typed `QueryCleanupStage` errors. Already-terminal
  requests are not spuriously closed again.
- Native runtime/readiness/value/input/codec/correlation faults have typed host
  fields in their owning modules. Q6 copies cached summaries only. Tests were
  migrated to cached presentation or typed cause/secondary assertions, not legacy
  conversions or fake arbitrary-error equality.

Verification so far:

- Workspace `cargo check --locked --offline --workspace --all-targets` passed
  before the final adversarial additions (three nested-result warnings since fixed).
- Full default workspace test run passed Graph and Kernel, their portable
  compiler fixtures, and workspace doctests. It reported only two StorageGraph
  test targets with old top-shape/lease expectations, now migrated; final rerun
  remains required.
- New Graph `fbp_failure_protocol`: seven focused tests passed. They cover worker
  pointer identity and original non-Clone causes after owner drops in both queue
  and failure policies, zero external formatter/source callbacks in repeated Q6
  formatting, independently leased helper errors and guard secondaries, reverse
  guard cleanup, partial adapter failure plus hostile prepared payload cleanup,
  successful emission disposal with extra leases and non-string panic payloads,
  actual standard codec sources, and generated codec malformed/truncation/tag/
  overflow/trailing fields.
- Latest focused Graph group passed: fbp_authoring_equivalence (8),
  fbp_failure_protocol (then 5, now 7), fbp_port_keys (13), fbp_streams (77).
- Latest StorageGraph focused command passed derived (30), error_sources (5),
  host_derived (7), but query_cleanup caught a redundant Close stage on terminal
  requests. The implementation now skips redundant terminal close. Rerun this
  command; it also includes the new `typed_failures` query/driver test, which was
  not reached after query_cleanup failed.

Immediate next commands, with explicit isolated target and rustc PATH as below:

```bash
cargo test --locked --offline -p phenix-storage-graph --test typed_failures --test derived --test host_derived --test query_cleanup --test error_sources --quiet
cargo test --locked --offline --workspace --no-fail-fast --quiet
cargo test --locked --offline --workspace --no-default-features --no-fail-fast --quiet
cargo test --locked --offline -p phenix-graph --no-default-features --no-fail-fast --quiet
```

Still required: strict workspace Clippy, docs in both modes, final all-target/example/
bench compilation, final fmt and erasure audit/inventory, full focused reruns, and
Nix snapshot check if feasible. Cargo-fmt was absent from the original PATH; use
`/nix/store/153dix264d7rc2abmlfvqhr5n3ddgwmk-rustfmt-1.96.1/bin` on PATH. A workspace
format pass has run; repeat after subsequent edits. No external execution owners
are promised abort-proof cleanup or retroactive lease enrichment.

2026-11-16 continuation checkpoint: **FULL Q7 STILL INCOMPLETE**. Work has
expanded beyond the original phase-1 ownership list, in this isolated worktree
only. No commits, pushes, merges, or main-workspace edits.

Implemented in the continuation so far (not independently reviewed):

- Added `FailureContext::guard_index`; framework internal lease enrichment copies
  known leases into each secondary handle as well as its parent.
- `Process::step`, guard entry, worker result, and handoff completion now carry
  `Failure`. Guard entry/step preserves first failure; reverse token teardown
  appends secondaries. Panic payloads become owned typed `CallbackPanicked` data.
- `ProcessHandoff::reject` now returns
  `Result<Result<(), Failure>, (HandoffError, ActivationJob)>`: outer refusal
  returns an untouched foreign/stale job, inner failure means ownership was
  accepted but guard-object cleanup failed. Runtime reject consumes the inner
  execution failure into the session/request, without inventing a refused job.
- Guard objects are disposed separately during restoration; explicit process
  cleanup uses `Process::dispose_catching` so `LeasedProcess` can hold its own code
  lease through inner destruction and panic-payload destruction.
- Value fallible access, input reconstruction, and edge preparation use Failure;
  errors copy known input leases. Value now has contained ordinary Drop and
  explicit managed `dispose_catching`; packet/process cleanup callers must use
  the managed report, not an outer catch around the contained Drop.
- Runtime/session/request native failure slots use Failure; first-terminal
  propagation and Q6 snapshots use original handles and cached summaries,
  respectively. Typed runtime and readiness faults are in their owning modules.
- Partial fanout preparation individually disposes prepared packets and retains
  cleanup secondaries. Invalid emissions are explicitly disposed under leases.

Verification at this checkpoint:
`cargo test --locked --offline -p phenix-graph --lib --test fbp_failures` passed
18 + 12 tests after the native Graph changes. This is NOT a workspace gate and
does not verify the changed handoff/runtime APIs or adversarial combinations.

Immediate remaining work: finish both codec generators and portable codecs;
migrate Kernel startup/access/driver and StorageGraph source DATA/query; migrate
all external callers/tests/examples/benches/doctests/compiler fixtures; audit
cleanup/lease completeness and add adversarial propagation tests; run all frozen
gates. The workspace is expected not to compile yet because these downstream
callers still implement String-returning step/guard/codec signatures. Continue
using the explicit isolated target/tool PATH in Verification below. Latest
workspace command was `cargo check --locked --offline --workspace --all-targets`
before downstream migration; Graph-only compilation and the focused command
above now pass. Do not interpret old phase-1 scope statements below as a current
restriction on the full migration.

Phase-1 checkpoint complete: core and 12 focused integration tests implemented.
Default and no-default Graph library + failure tests passed (18 + 12 each), as did
scoped fmt, scoped Clippy in both feature modes, and final scope review. Resume
full Q7 by inspecting this note, the new files, and `git diff` in this detached
worktree (new files remain untracked). Do not commit/push or edit the
main workspace. No Process, Guard, existing Value fallible API, codec, macro,
runtime, Kernel, or storage migration belongs in this phase.

## Stable Phase-1 API

Root exports (implementation module private): `Failure`, `FailureKind`,
`FailureContext`, `CallbackPhase`, `WrongFailureValue`.

```rust,ignore
Failure::application_message(message: impl Into<String>) -> Failure
Failure::external<E: Error + Send + Sync + 'static>(summary: impl Into<String>, cause: E) -> Failure
Failure::from_value(value: &Value) -> Result<Failure, Failure>
Failure::kind(&self) -> FailureKind
Failure::summary(&self) -> &str
Failure::context(&self) -> &FailureContext
Failure::cleanup(&self) -> &[Failure]
Failure::with_kind(self, kind: FailureKind) -> Failure
Failure::with_context(self, context: FailureContext) -> Failure
Failure::with_lease(self, lease: Arc<dyn Send + Sync>) -> Failure
Failure::with_cleanup(self, failure: Failure) -> Failure
Failure::dispose_catching(self) -> bool
WrongFailureValue::actual(&self) -> &TypeIdentity
```

`Failure` implements `Clone`, `Display`, `Debug`, `Error`, `Send`, and `Sync`.
`WrongFailureValue` implements `Clone`, `Debug`, `PartialEq`, `Eq`, `Display`,
and `Error`. No blanket `From<String>`, `From<&str>`, or `From<E: Error>` exists.
No public raw record Arc, owned cause, or owned source extraction exists.

`FailureKind` is non-exhaustive, with `Application`, `External`, `Framework`,
`Panic`, `Cleanup`; it implements `Clone`, `Copy`, `Debug`, `PartialEq`, `Eq`.
`CallbackPhase` has the same traits and is non-exhaustive, with `Construct`,
`Step`, `GuardEnter`, `GuardExit`, `Adapter`, `Encode`, `Decode`, `Cleanup`.

`FailureContext` implements `Clone`, `Debug`, `Default`, `PartialEq`, `Eq`.
Its plain public host-data fields are all optional: `session: Option<u64>`,
`generation: Option<u64>`, `request: Option<runtime::RequestId>`,
`process: Option<usize>`, `path: Option<String>`,
`ticket: Option<process::StepTicket>`, `output: Option<String>`,
`field: Option<String>`, `phase: Option<CallbackPhase>`. Default means unknown,
not fabricated zero identity. Context is diagnostic, not execution authority.
`with_context` replaces metadata; it does not merge or add a recursive wrapper.

## Ownership And Cleanup Contract

`Failure` privately owns `Arc<FailureRecord>`. The record owns host-only typed
kind, cached summary, context, optional private `Arc<ArcCause>` containing the
original boxed error once, secondary failure handles, and lease Arcs. COW clones
only host data and Arcs, never the external error. Source pointer identity stays
stable across record enrichment. Typed framework details will live as causes in
their owning modules; `with_kind` supplies broad classification, not string codes.

Construction with an external cause requires an explicit host summary and never
calls the cause's Display, Debug, or source. Failure Display, Debug, and summary
use host data only, even for secondaries. Error::source borrows the actual original
error directly, without calling its source. Deliberately invoking methods on that
borrowed error is the caller's choice and can execute external code.
`application_message` is a genuine String leaf with source None.

Lease completeness is **per handle and nonretroactive**. Attach all necessary
leases before escape. An earlier application clone does not acquire leases later
attached to another handle; keeping that earlier clone is the application's
responsibility. Enriched clones retain independent lease ownership across COW.
Each handle dropping a shared cause only releases its own ownership; the final
cause owner runs destruction. A secondary that escapes via cloning must already
have its own complete leases, not merely depend on its parent's leases.

Record cleanup detaches the cause and secondary handles first, then individually
disposes them, keeping ALL this record's leases alive through each destruction
and caught panic-payload destruction. Only then are leases dropped individually,
with each panic caught so later leases still run. The existing bounded helper
drops a caught payload under catch_unwind and forgets a second panic payload.
It does not guarantee recovery from aborts or arbitrary double-panic behavior.

Managed `dispose_catching` returns true if this operation actually caught a cause,
secondary, or lease destructor panic. It invokes the same cleanup used by ordinary
Drop, rather than inferring success from an outer catch around a swallowing Drop.
Ordinary Drop ignores this report because it cannot retroactively report failure.
A shared-owner disposal can return false because final destruction is deferred;
this is not a promise about another owner's later cleanup. Panic-abort builds,
panicking hooks, and destructors that abort are outside the containment contract.

`from_value` requires exact local `Failure`, not wire data, Arc<Failure>, nested
wrapper, matching schema string, or another type. Both success and typed rejection
copy every known envelope lease. It never retains the whole Value or local payload
Arc, avoiding an extra payload owner or envelope retention cycle. It cannot infer
unknown external leases. The only Value edit is a crate-private `known_leases`
accessor; existing public String-returning APIs are unchanged.

## Remaining Full Q7 Work

Migrate full Process contracts and helpers, execution guards, adapters, Value
fallible APIs and codec paths, derive macros, runtime propagation and managed
cleanup, Kernel integration and source DATA/query handling, and all affected caller
and compiler fixtures. Put each framework fault's typed details in its owner module.
Verify original cause inspection, context attribution, lease completeness, and
cleanup reporting at every boundary before claiming Q7 complete. Do not introduce
legacy blanket conversion shims. No Job or Completion changes have been made.
The core adds no Graph-to-storage or Graph-to-Kernel dependency.

## Verification

Build artifacts use only
`/tmp/opencode/fbp-q7-typed-failures-7c611912/target`, explicitly selected through
`CARGO_TARGET_DIR`, not the main compiler-fixture target. Rustc/Clippy are the
existing Nix-store 1.96.1 tools; rustfmt is the existing cached 1.96.1 tool.

The ordinary shell lacks rustc on PATH. Reproduce from this worktree with:

```bash
export PATH="/nix/store/8qpzf385bdb0dgzz85dvb1lfvf2fxipk-rustc-wrapper-1.96.1/bin:/nix/store/siyjmcsvpx074kb5ab9571s3zs95rr1v-clippy-1.96.1/bin:$PATH"
export CARGO_TARGET_DIR=/tmp/opencode/fbp-q7-typed-failures-7c611912/target
```

Passed:

- `cargo test --locked --offline -p phenix-graph --lib --test fbp_failures`:
  18 library and 12 failure integration tests.
- `cargo test --locked --offline -p phenix-graph --no-default-features --lib --test fbp_failures`:
  18 library and 12 failure integration tests.
- `rustfmt --check --edition 2024 --config skip_children=true` on only the four
  touched Rust files, using
  `/tmp/opencode/rustup/toolchains/1.96.1-x86_64-unknown-linux-gnu/bin/rustfmt`.
- `cargo clippy --locked --offline -p phenix-graph --lib --test fbp_failures -- -D warnings`.
- `cargo clippy --locked --offline -p phenix-graph --no-default-features --lib --test fbp_failures -- -D warnings`.
- `git diff --check` and final scope review: only the five allowed source,
  test, and documentation files changed; HEAD remains at the detached base.

Changed files: `packages/phenix-graphs/src/failure.rs`,
`packages/phenix-graphs/src/lib.rs`, `packages/phenix-graphs/src/value.rs`
(four-line crate-private lease accessor only),
`packages/phenix-graphs/tests/fbp_failures.rs`, and this note.

The focused tests cover non-Clone nested causes and pointer identity through COW,
panic-on-call external Display/Debug/source with zero invocations by host APIs,
String leaves, all parent leases alive during individual cause/secondary drops
and panicking payload destruction, individually caught lease panics, managed
versus ordinary Drop, ordinary Drop during unwind, deferred shared-owner reports,
COW lease ownership, nonretroactive clones in both drop orders without unsafe,
exact-local Value extraction and every known lease, typed rejection, no payload
Arc retention, wire schema spoofing, and wrapped Failure rejection.

No workspace-wide tests or compiler fixtures have been run, by design.
