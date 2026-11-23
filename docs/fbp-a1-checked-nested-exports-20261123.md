# A1 Checked Nested Exports

Base: `origin/fix/fbp-h1-bounded-correlation-20261122`, `ab7f5e93d`.
Scope: original A1 additive private-extension finding; not M7/M8 production acceptance.

## Accepted Source

Original A1 is **scoped verified: 18.0% -> 100.0%, +82.0 pp** at
`923ccd6376b4cd8e3980b61ae26ad78c08e9c6a9`, draft
[PR #1388](https://github.com/matthis-k/phenix-ai/pull/1388). No merge or rebase.
The final independent whole-original-scope source review found no blocking
correctness defects, fixture false positives, or implementation criteria left
after the actual nested-agent workload was added. This is separate-agent review,
not external human approval. Earlier local-only/pending statements below describe
historical stages and are superseded by this acceptance.

All triggered exact-source workflows passed, queried through GitHub:

| Workflow | Exact source | Result |
| --- | --- | --- |
| [Flake CI 45249107088](https://github.com/matthis-k/phenix-ai/actions/runs/45249107088) | `923ccd6376b4cd8e3980b61ae26ad78c08e9c6a9` | All six jobs SUCCESS: inventory, build, Graph, workspace/default/no-default, formatting and aggregate |
| [Session Ownership Interleavings 45249106208](https://github.com/matthis-k/phenix-ai/actions/runs/45249106208) | Same source | Both actual-source Loom and native ownership jobs SUCCESS |

Inspected Graph job `137954137681`: the new `fbp_a1_nested_agent_loop` matrix
passed and the three original M8 tests passed. Workspace job `137954138188`
ran both `checked_nested_exports` tests successfully under default and workspace
no-default configurations; isolated Graph no-default checks also passed.
The final evidence-only revision requires its own exact-head CI and cannot inherit
source acceptance as a claim that its documentation checks have already run.

The accepted contract is opt-in additive editing, same-scope Connect and explicit
nested exports with exact grants at every ancestor, preserved claims/requirements,
deterministic conflicts/dependencies, COW generations and failure atomicity.
Arbitrary destructive private rewiring is not authorized by extension permission.
This does not infer A2 compatibility, H7 state isolation, Q4 liveness, a native
sandbox, preemption, bounded arbitrary payload memory, or production model/tool
integration. These are retained boundaries, not silently removed A1 criteria.

## Phase 1: Requirements And Constraints

The original [A1 finding](fbp-issue-ledger-20261111.md) and
[nested replacement design](fbp-nested-replacement-design-20261111.md) distinguish
replacement from extension. The [original critique](fbp-critique.md), especially
findings 8-11, requires fresh generation handles, immutable declarative replay,
exact footprints and preserved contract obligations. Historical documents saying
extension is design-only do not describe this remote stack: it already has
`allow_extensions`, nested Add/Connect and `ExposeNestedInput/Output`.

Missing: checked lowering from bound endpoints into those existing export edits,
and a public Kernel integration exercising actual private additions, connections
and exports rather than replacing a leaf. Keep symbolic declaration boundaries;
never flatten a composite endpoint into its physical process. Retain exact host
grants, every ancestor's opt-in, explicit dependencies, copy-on-write, provenance,
exact interfaces/claims, typed error causes and snapshot/code leases. No new edit,
authority, compatibility shim, runtime mode, or inspection permission.

## Phase 2: Alternatives And Design

| Alternative | Decision |
| --- | --- |
| Hand-copy `symbolic_path` into enum payloads in every caller | Reject: repeats generation/scope checks and leaves the public authoring gap |
| Infer parent from physical endpoint resolution | Reject: crosses composite boundaries and loses declaration identity |
| General endpoint projection/compatibility framework | Reject: unnecessary abstraction and potential authority expansion |
| Two focused `Edit::expose_nested_bound_input/output` helpers | Select: same checked symbolic bridge as root exports, existing payload/replay |

Helpers accept the compiled network, exact parent, export name and bound endpoint(s),
plus input capacity. Authenticate endpoints via `symbolic_path(network)` first.
Require each symbolic process's slash-delimited immediate parent to equal the
nonempty requested parent. Return existing `NetworkError::InvalidProjection` on
scope mismatch, unchanged `ForeignScope` on generation mismatch. Preserve full
symbolic paths in existing nested edit variants; replay remains responsible for
author opt-ins, grants, names, fanout/type validation and frozen obligations.
The input helper validates every endpoint before checking projection membership.
Empty input lists retain existing replay validation, not a new fanout policy.

`CandidateContext` delegates only, using its private compiled view; it grants no
new topology inspection. A real prepared contribution self-imports its statically
declared side stage using the existing authorized preview, imports the provider's
leaf, then returns actual Add/Connect/nested exports and an enclosing monitor.
Its final payload repeats the preview Add, not a replacement. No new preview API.

## Phase 3: Adversarial Review

Before implementation, no `task`/subagent tool was exposed in this session. The
following standalone adversarial critique was recorded then; it was not independent
review or external approval. Subsequently, a separate agent independently reviewed
the initial implementation, as relayed by the user. That review found no production
defect but identified two coverage gaps: the nested input was not executed, and no
negative proved exact contracts on the growing INNER composite itself. The follow-up
below addresses those findings. Independent agent review is not external approval,
and the follow-up changes have not themselves been independently re-reviewed here.

- Same paths/types in another compilation are not identity: reject foreign and
  stale-generation endpoints, including a foreign member after a valid fanout head.
- `m/inner2` must not match `m/inner`; a sibling, parent itself, or grandchild must
  not project directly. A composite immediate child's boundary is valid even when
  its physical targets are deeper. Test both directions and ordered fanout.
- Checked lowering is not authorization. Retain and rerun exact nested/root grant
  separation, every ancestor opt-in, dependency ordering and failure atomicity.
- A self-import preview must not silently drop its static Add from the returned
  patch. Assert new side and monitor origins, unchanged provider leaf identity and
  claims, and preservation of original enclosing boundary ports. The follow-up
  explicitly adds one public alternate input route rather than changing the
  original run/reply path.
- A prepared candidate must not publish early. Retained old sessions must keep
  their topology; preview endpoints must fail validation against final compilation.
- Queue-level behavior must be exercised under SharedFifo and BoundedLanes with
  real request admission/capture, without claiming arbitrary fairness.
- Preparation and final-binding failures must leave active generation and plugin
  revisions untouched; escaped errors/handles keep existing leases. Reuse adjacent
  lease/error lifecycle tests explicitly rather than changing their machinery.

## Phase 4: Implementation And Evidence

### Original Agent-Loop Acceptance

The whole-original-A1 review identified an additional acceptance gap after the
monitor follow-up: [follow-up design line 36](fbp-followup-design-20261111.md#deferred-independent-design-requirements)
requires an executable agent-loop use case, and [ledger line 114](fbp-issue-ledger-20261111.md#next-issues-to-work)
requires real agent-loop examples. A nested arithmetic DAG and a separate
root-public agent-cycle test do not jointly meet this requirement. It is not
waived or reclassified out of A1.

Four-phase amendment for that requirement:

1. Requirement/constraints: execute a real model/tool/feedback/stop cycle created
   through private author-approved Add/Connect/new-export seams. Preserve exact
   grants, every ancestor opt-in, dependencies, frozen contracts, request identity,
   finite packet/step budgets, old sessions, failed-update atomicity and revoke.
2. Alternatives: retain the disjoint DAG/root-public tests (insufficient); add a
   privileged agent API or destructive rewiring (reject); reuse M8's unchanged
   deterministic memory/model/tool constructors in a test-only module and add a
   private additive cycle plus independent manual graph (select). No new API.
3. Adversarial critique before editing: the base must not secretly contain the
   feedback edge; only the authorized extension can close the cycle. Each emitted
   and completed step must retain its request identity, the final branch must
   stop, revocation must remove both tool and dependent exports/edges, and failed
   batches/updates must preserve published handle identity. The manual oracle must
   author its own topology, not replay the tested patch. Use finite sequential
   turns and packet capacities; do not claim arbitrary-stream fairness, bounded
   payload bytes or request-local memory from session-local state. No task tool is
   exposed for a new independent pre-edit critique; this amendment is standalone,
   not external approval.
4. Implementation/evidence: add the shared test constructors and a graph regression
   executing both queue policies, then record exact commands/results and the
   original acceptance mapping below. CI and whole-scope re-review remain required.

### Agent-Loop Evidence

Implemented `phenix-graphs/tests/fbp_a1_nested_agent_loop.rs`. It reuses the existing
M8 deterministic memory, model and tool constructors, moved without behavioral
changes into `tests/support/agent_loop.rs`. `fbp_m8_agent_loop.rs` imports that module;
its three original tests remain in their original test binary, not duplicated in
the new workload. The support module contains no tests and exposes no production API.

The base author provides private `agent/loop/{memory,model}`, a memory-to-model
connection, prompt/reply projections and `allow_extensions` at both ancestor
levels. The base contains no tool or feedback edge. Its tool-call output receives
only an explicit, generation-checked discard permission in the baseline session,
so an ordinary prompt demonstrably does not complete an agent turn before extension.
That discard permission is absent from the working agent session.

An ordinary host registration adds `agent/tool`. A dependent registration uses the
checked nested helpers to export `agent/loop.call` from `model.tool` and
`agent/loop.feedback` into `memory.input`, then connects only same-enclosing-scope
endpoints. This closes the actual cycle:

`memory -> model.tool -> call export -> tool -> feedback export -> memory -> model.final`

The test host explicitly enumerates the one Add, two connection and two directional
nested-export grants. No Replace, Disconnect, Splice, new API or destructive rewire
is used. The enclosing `agent` retains its exact prompt/reply contract claim and
root requirement; the inner composite was explicitly extensible, not exact-claimed.
The earlier Kernel growing-INNER negative still proves exact claims cannot be
relaxed to permit these added ports. Original memory/model declaration keys and
origins survive; the added tool has the extension registration's declared origin.

A separately authored typed `NetworkBuilder` graph defines the complete cycle
directly. It does not use the preset or replay the tested patch. Comparison covers
all flattened edges and capacities, public ingress/projections, root/composite/leaf
interfaces and claims, full replies, and ordered emission traces. The expected
trace visits the same physical memory and model twice, with one intervening tool
activation; this is not a monitor DAG or a monolithic process pretending to loop.

Both `SharedFifo` and `BoundedLanes` run two ordinary turns, another after a failed
update, another on the retained generation after revoke, and a direct-stop turn
that issues no tool call. Each successful graph produces four tool effects and nine
model activations. Every dispatched and completed job carries the admitted request,
and the final capture authenticates that request and the model's `final` output.
Each scope settles with exactly one reply, releases its credit, and is stale after
release. The stop branch leaves no runnable work after capture consumption/release.

All edges, ingress and captures have capacity one, with one admitted request at a
time: the scoped connection/ingress/capture packet-slot envelope is asserted to be
five. Each turn has a hard 32-owned-step test budget and a four-step idle check.
The workload is finite sequential conversational turns with per-session history,
not a claim of request-local memory, arbitrary simultaneous-request fairness,
bounded payload bytes, or preemption of nonyielding application code.

The same workload rejects a batch missing the exact feedback-edge grant without
publishing its tool Add. A fully granted dependent update naming a nonexistent
nested input returns the existing typed `UnknownInput` cause, with its existing
enclosing-network-relative diagnostic path. Published bound-handle identity,
registrations and connections remain unchanged. Revoking the tool removes the
dependent exports/connections and restores the base interface/edges; retained
pre-extension and extended sessions keep their respective behavior.

### Checked Lowering

Implemented only two graph lowering methods and two thin Kernel delegates (82
production lines). No planner, replay, runtime, origin, contract or lease machinery
changed. The graph regression proves ordered immediate-child fanout, symbolic
composite boundaries despite deeper physical resolution, parent/sibling/grandchild/
prefix scope rejection, a foreign member after a valid input, foreign output and
stale endpoints after publication. Existing nested variants and footprints remain
the only replay payloads and write authority.

The new public `checked_nested_exports.rs` integration uses ordinary provider and
extension plugins. The provider opts in at both ancestor levels and freezes the
leaf contract. Preparation uses declared imports only, returns the actual side
Add, leaf-to-side Connect, both checked nested exports, enclosing monitor Add and
export-to-monitor Connect. The root reply stays `input + 1`; the new monitor sees
`input + 8`. The old session sees no monitor effect. Unload removes the extension
without changing retained old generations. Both queue policies exercise real
request admission, authenticated capture and request release.

The review follow-up adds only two fixture projection edits and their exact grants:
root `alternate` -> `service/alternate` -> `service/inner/alternate` -> `side/run`.
A real request with value 5 through that route produces monitor value 12, not the
original-route value 13, and produces no `reply`. The existing original route still
produces reply 6 and monitor 13 for input 5, and remains usable after the alternate
request and after unload through a retained generation. This intentionally adds
one enclosing public input; original input/output definitions and claims remain
unchanged. No production helper or contract relaxation is involved.

A second test reuses the same provider/extension fixture. Its published baseline
claims the exact `checked-inner@1` contract on INNER itself, and the enclosing
network requires that same contract on its `inner` child. Reloading the ordinary
extension with all exact grants and both ancestor opt-ins fails before final binding
with `NetworkError::Interface`, `InterfaceReason::ExtraPort`, compiled phase,
contract `checked-inner@1`, and endpoint `input alternate`. Both queue policies
verify unchanged active generation identity, all plugin revisions, INNER interface
and claims, no side/monitor additions, rejected preview identity, and continued
original-route execution. This is a growing-composite contract negative, not an
inference from leaf claims. It checks the combined export patch; the reported first
extra port is the input, not a separate output-only contract-failure case.

Preparation projection refusal, missing exact nested-output grant and final-binding
refusal each leave the active generation and plugin revision unchanged, and their
preview endpoints cannot validate against the active compilation. Typed preparation
errors remain behind the existing leased callback source; replay still reports
the exact unauthorized footprint. Kernel rebuilds issue fresh contribution child
origins: tests check unchanged provider-revision ancestry and leaf definition key,
not false cross-build origin equality. The fixture explicitly releases escaped
preview handles to avoid a plugin/preparation-lease reference cycle.

### Local Verification

The initial validation commands below ran inside this isolated worktree with prefix
`nix develop -c env CARGO_BUILD_JOBS=2`. All listed runs passed, including doctests
for the full test invocations. The initial full-suite compile exceeded a 120-second
tool timeout; rerunning with a 600-second limit completed successfully.

| Command after prefix | Result |
| --- | --- |
| `cargo test --locked -p phenix-graph --test fbp_nested_exports -p phenix-kernel --test checked_nested_exports` | Initially 8 graph tests and 1 Kernel matrix test passed; the follow-up adds a second Kernel matrix test |
| `cargo test --locked -p phenix-graph -p phenix-kernel --quiet` | Full graph and Kernel suites passed |
| `cargo test --locked -p phenix-graph --no-default-features --quiet` | Full isolated graph suite passed without derive feature unification |
| `cargo test --locked -p phenix-kernel --no-default-features --quiet` | Full Kernel suite passed; Kernel declares no features and its graph dependency still enables graph defaults |
| `cargo clippy --locked -p phenix-graph -p phenix-kernel --all-targets -- -D warnings` | Passed |
| `cargo clippy --locked -p phenix-graph --no-default-features --all-targets -- -D warnings` | Passed |
| `cargo fmt --all --check` | Passed |

`git diff --check` also passed. The full suites explicitly reuse adjacent evidence:

| Preserved constraint | Existing tests rerun, not newly claimed coverage |
| --- | --- |
| Every ancestor opt-in, exact Add/Connect grants, COW and provenance | `phenix-graphs/tests/fbp_nested_extension.rs` |
| Exact nested/root directional grants, dependency conflicts and reverse-lexical replay, invalid-export atomicity | Other seven tests in `phenix-graphs/tests/fbp_nested_exports.rs` |
| Frozen obligations, exact interface identity, compile failure atomicity | `fbp_contracts.rs`, `fbp_composition.rs`, `fbp_nested_replacement.rs` |
| Final binding error/panic and preparation failure atomicity | Kernel `bound_handles.rs`, `callback_errors.rs`, `generations.rs` |
| Escaping preparation/binding causes, detached topology, selected-only snapshot leases and teardown order | Kernel `code_leases.rs`, `activation_artifacts.rs`, graph `fbp_declaration_leases.rs` |

### Review Follow-Up Verification

For the earlier monitor/contract review follow-up, only `checked_nested_exports.rs`
and this note changed. No production code changed. Using the same
`nix develop -c env CARGO_BUILD_JOBS=2` prefix, all following commands passed on that
follow-up fixture:

| Command after prefix | Result |
| --- | --- |
| `cargo test --locked -p phenix-graph --test fbp_nested_exports -p phenix-kernel --test checked_nested_exports --test composer_nested_extension --test bound_handles --test code_leases` | 19 passed, including both new Kernel matrix tests and adjacent authority, generation and lease coverage |
| `cargo test --locked -p phenix-graph --no-default-features --test fbp_nested_exports` | 8 passed with graph defaults disabled in isolation |
| `cargo test --locked -p phenix-kernel --no-default-features --test checked_nested_exports --test composer_nested_extension --test bound_handles --test code_leases` | 11 passed; Kernel's graph dependency still enables graph defaults |
| `cargo clippy --locked -p phenix-graph -p phenix-kernel --all-targets -- -D warnings` | Passed |
| `cargo clippy --locked -p phenix-graph --no-default-features --all-targets -- -D warnings` | Passed |
| `cargo fmt --all --check` | Passed |

`git diff --check` passed as well. These are targeted follow-up reruns, not a new
claim that the earlier full suites or doctests were rerun after the fixture edits.

### Agent-Loop Verification

After the nested-agent additions and test-only extraction, the following commands
passed with the same `nix develop -c env CARGO_BUILD_JOBS=2` prefix:

| Command after prefix | Result |
| --- | --- |
| `cargo test --locked -p phenix-graph --test fbp_a1_nested_agent_loop --test fbp_m8_agent_loop --test fbp_nested_exports --test fbp_nested_extension --test fbp_nested_replacement -p phenix-kernel --test checked_nested_exports --test composer_nested_extension --test bound_handles --test code_leases` | 35 passed: 24 graph and 11 Kernel tests, including the new two-queue-policy agent matrix and all three original M8 tests |
| `cargo test --locked -p phenix-graph --no-default-features --test fbp_a1_nested_agent_loop --test fbp_m8_agent_loop --test fbp_nested_exports --test fbp_nested_extension --test fbp_nested_replacement` | 24 passed; isolated graph defaults disabled |
| `cargo test --locked -p phenix-kernel --no-default-features --test checked_nested_exports --test composer_nested_extension --test bound_handles --test code_leases` | 11 passed; Kernel's graph dependency still enables graph defaults |
| `cargo clippy --locked -p phenix-graph -p phenix-kernel --all-targets -- -D warnings` | Passed |
| `cargo clippy --locked -p phenix-graph --no-default-features --all-targets -- -D warnings` | Passed |
| `cargo fmt --all --check` | Passed |

`git diff --check` also passed. These are targeted current-diff runs, not reruns of
the earlier full suites/doctests, a full-workspace matrix, or hosted CI. No production
code changed for this agent-loop addition. The extraction diff preserves M8 process
bodies and changes only their test-module visibility/imports.

### Original A1 Mapping

| Original acceptance requirement | Current evidence / outstanding gate |
| --- | --- |
| General private extension beyond replacement, not private-by-default writes | Existing remote `allow_extensions`, nested Add/Connect/export replay; graph extension/export suites and actual Kernel Add/Connect integration |
| Baseline/extension/redesign comparison and adversarial review | Phases 1-3 and the explicit agent-loop amendment above; prior independent agent review covered the initial implementation, not final whole-scope acceptance |
| Executable nested agent-loop use case, follow-up design line 36 and ledger line 114 | New `fbp_a1_nested_agent_loop` actually executes private Add/Connect/export-created feedback and stop branches, both queue policies, compared against independently authored manual topology |
| Exact grants and every ancestor opt-in; no implicit descendant authority | Explicit agent-workload grants plus missing-feedback-grant atomic batch rejection; adjacent nested extension/export refusal matrices |
| Dependencies/conflicts, revoke, copy-on-write, provenance and old generations | Agent tool-to-cycle dependency and transitive revoke; invalid-update identity/topology retention; unchanged original declaration keys/origins; existing conflict matrices and Kernel lifecycle tests |
| Preserve exact contracts, typed causes and leases | Exact outer agent claim/requirement survives; Kernel exact-INNER growth refusal; typed graph update causes and adjacent Kernel lease/error suites rerun |
| Final original-scope review and exact changed-head CI | Completed for `923ccd6376b4cd8e3980b61ae26ad78c08e9c6a9`; independent whole-scope review and all triggered exact-head CI green, as recorded above |

### Historical Local Review And Limits

Standalone exact-diff review found no new authority, interface relaxation,
compatibility API, broad inspection permission, or altered replay/lease ownership.
The checks authenticate generation and declaration scope only; export naming,
empty fanout, type consistency, exact grants, opt-ins and dependencies intentionally
remain the existing replay/compiler contracts. Symbolic edits are not final
execution handles.

This is local evidence and separate-agent review, not hosted CI, external approval, a general sandbox,
static liveness proof, performance result, arbitrary shared-request fairness proof,
or closure of the original A1 finding. Both new nested export directions now execute
in the monitor and real feedback-cycle workloads under both queue policies. The exact-contract negative asserts the
combined export patch's first extra input; it does not separately isolate output-only
growth or each claim/requirement mechanism. No full-workspace test run or hosted CI
was performed. A separate agent reviewed the initial implementation; no independent
whole-original-scope re-review of the complete follow-up diff is claimed. Remaining
acceptance gates are that review, the current-diff required full-workspace/feature
matrix and exact-head CI. The deterministic finite serialized agent use case is now
implemented, not waived; review must still judge the original scope, including the
disclosed workload and combined-contract-negative limits above. It is not a complete
production LLM/tool/memory system or an arbitrary agent liveness guarantee.
No commit or push; H4 and Q3 changes are outside this worktree. The original
workspace's untracked `composer_private_extension.rs` was not read or modified.
