# FBP issue–resolution–evidence ledger

Last reconciled: **2026-11-12**, against **the verified stacked candidate in PR #1274**
at `f81b49336dfce942f57dad1edc65add258be35fd`, passing
[all six exact-head checks (CI #43279922626)](https://github.com/matthis-k/phenix-ai/actions/runs/43279922626).
The candidate contains earlier PRs #1263–#1273 through the stacked lineage; individual
PRs remain unmerged and their separate base branches do **not** each contain all fixes.
Do not confuse these green candidate checks with a merged default branch or a
full product-acceptance milestone.

Vocabulary: **Verified** means the named, bounded behavior and regressions passed
exact-head CI; it is not a universal safety/liveness proof. **Partial** means
material use cases remain uncovered. **Open** is unresolved; **Policy** requires an
explicit decision. Historical Phase 0–4 notes are evidence, not current status.

**Current 33-finding classification: 10 verified / 7 partial / 16 open.**
This is a finding classification, **not a percentage of M7 completion**.
Since the prior ledger, H4 has a verified scoped optimization but retains its
O(V) scan; H7 moves from Open to Partial on correlated shared-session isolation,
capture matching, cancellation tests and capacity-one backpressure diagnostics.
H3b stays Partial although known host-drop/driver lifecycle and exclusive producer
ownership paths are now CI-verified.

| Issue | Status | Evidence / remaining acceptance |
| --- | --- | --- |
| H1 Shared-session FIFO correlation head blocking | Partial | Opt-in `Readiness::CorrelatedAll` handles crossed request ingress within the bounded queue and independent sessions remain available. Default FIFO and arbitrary shared-state fairness are not globally solved. See PRs #1269/#1270. |
| H2 Callback panic containment | Verified (scoped) | PR #1263 (`72d56e41`), `43185486539`: preparation, activation, host binding, producer wake and host event callback unwind paths return typed errors; `panic=abort` and callback side effects remain outside guarantee. |
| H3a Inventory lock inversion on producer admission | Verified | Native inventory, drain, and whole-drain retirement regressions pass in exact-head PR #1265 CI `43197663375`; polls/admissions run outside inventory mutex. |
| H3b Session/drain recursive callback reentrancy | Partial | PR #1265 covers cross-thread session↔drain cycle and driver lifetime lease; #1268 fixes host drop inside poll/wake callbacks; #1272 gives event-driven `Retired` and prevents self-retiring callback admission; #1273 establishes exclusive driver versus direct-drain ownership and typed errors. Final exact-head CI #43277415425 and #43279922626 passed. Arbitrary external plugin mutex cycles and formal exhaustive interleaving tests remain open. |
| H4 Full-inventory scheduling scans | Partial — scoped optimization verified | PR #1267 read-only idle/open-input prefilter avoids expensive readiness selection for provably idle processes; combined exact-head CI #43216093923, #43277415425 and #43279922626 passed. AMD EPYC 7763 256-process sparse-input data moved ~59.7→9.8 µs/event in separate runs on that CPU model, not a controlled paired or pinned benchmark. `refresh()` still scans O(V); no event-indexed scheduler. |
| H5 Expensive closure analysis | Partial | Compiled downstream adjacency removes repeated whole-edge scans, but request closure continues inspecting queued packets; missing measured stress and complexity envelope. |
| H6 No native performance baseline | Partial — reproducible raw samples | Release-mode initial microbenchmark [CI #43201132241](https://github.com/matthis-k/phenix-ai/actions/runs/43201132241) included three raw CSV repetitions and an O(V) sparse-input scale curve. H4 prefilter data is preliminary across runs. Still missing pinned-hardware paired comparisons, allocator/RSS, correlated joins, multiple simultaneously active branches and production-agent workload acceptance. |
| H7 Request isolation not complete | Partial | #1269 proves shared-session crossed-pair isolation and queued cancellation; #1270 permits authenticated request-scoped reads behind unrelated **buffered** FIFO heads; #1274 adds passive `RequestWork.capture_backpressure` and verifies capacity-one recovery after explicit consumption/cancellation (CI #43279922626). A full lossless queue with indefinitely unconsumed packets still blocks another request's **unpublished** output. Dynamic fairness, active-job cancellation, per-request buffer policy, shared effects remain open. |
| C1 Cancellation overwrites failure/completion | Verified | `fbp_terminal_transitions.rs`; Completed/Failed first-terminal-wins and cancelled Quiescent fixture verified in PR #1263 and #1265 exact-head CI. |
| C2 Replayed frozen obligations duplicate | Verified | `fbp_obligation_dedup.rs` and replay/transaction regressions; code passes exact-head six-job CI `43197663375`. |
| C3 Retired generations remain callable | Open — policy decision | Retired generations intentionally retain explicit execution capability; existing tests require this. Define revocation/archival policy without destroying admitted work before changing behavior. |
| C4 Full-network preparation inspection | Verified | Default-deny `CandidateContext` imported scope and per-contribution full-inspection grant covered by `prepare_inspection.rs`; exact-head CI green. |
| C5 No built-in watchdog for deferred jobs | Partial — host-supervised | Timeout inspection and supervision APIs exist; they require a driving host and cannot preempt nonyielding delegated work. No autonomous watchdog/service. |
| C6 Live process state not migrated | Open | New generation constructs a new session; no migration of live process state. Restart/disposal policy needs explicit accepted contract. |
| C7 No general in-process plugin sandbox | Open — architectural limit | Rust-native in-process plugins share host process privileges. Scoped FBP contribution authority is not a security sandbox. |
| C8 Code leases retain defeated snapshots | Verified | Selected contribution code leases avoid holding defeated snapshot revisions; detached object lease tests pass exact-head CI. |
| C9 `state()` is not passive | Verified | `NetworkSession::state(&self)` is a passive immutable snapshot; readiness policy side-effect regression passes exact-head CI. |
| C10 `run_until_complete()` aliases `run_until_idle()` | Verified | Removed redundant `run_until_complete` alias; all native references use `run_until_idle`; exact-head CI green. |
| A1 Replacement-only private nesting | Open | No general private composite extension beyond replacement; needs use-case and interface design. |
| A2 Rigid interface identity | Open | Exact interface identity constrains compatible replacement/projection; must specify safe compatibility before relaxing. |
| A3 Store coupled into graph-local types | Open | Graph resource/store boundary and coupled local types need a dependency-layer audit, not arbitrary renaming. |
| A4 Layered typed authoring | Open | Some authoring concepts overlap (`port_keys`, derives, builder). Evaluate consolidation with compiled example equivalence. |
| A5 Physical string paths | Open | Physical string paths still escape typed handles in compilation and binding APIs. |
| A6 Admission payload validation ergonomics | Open | Admission validation mixes runtime path/type checks; typed checked admission can improve ergonomics. |
| A7 Unbounded identifier sizes | Verified | 512-byte per-segment bound on graph and kernel identifiers with boundary tests; exact-head CI green. |
| Q1 Large responsibility concentrations | Open | `network.rs`, `runtime/mod.rs`, `compose/mod.rs` retain large responsibilities; refactor only after behavioral seams are tested. |
| Q2 Whole-candidate recompilation | Open | Whole-candidate composition/compilation remains; need measurement before incremental optimization. |
| Q3 Dependency/conflict selection scaling | Open | Dependency selection lacks systematic stress evidence for skew, diamonds, and adversarial priorities. |
| Q4 No static liveness guarantee | Open | Cycles remain legal without a sound static progress guarantee; no liveness verifier accepted. |
| Q5 Concurrency testing not exhaustive | Open — stronger scoped evidence | CI-verified two-thread regressions now cover session↔drain contention, callback-owned host drop, delayed retired-driver wake, a self-retiring producer's returned event, driver-versus-direct-drain admission, and release/retry. No Loom/model-checked interleaving coverage or external plugin-lock-order proof. |
| Q6 Diagnostics intentionally minimal | Open | Diagnostics still intentionally minimal; richer actionable deadlock/readiness explanations remain to build. |
| Q7 Error reporting uneven | Open | Error handling still mixes strings and typed outcomes; code lease/source preservation needs a public API pass. |
| Q8 Final reports stale | Verified — reconciled as a candidate | The report and 33-finding ledger were initially verified by #1266 CI #43202338522 but later became stale. This revision reconciles candidate PR #1274, exact-head CI #43279922626, and the current 10/7/16 classification; this **documentation-only commit** needs its own exact-head CI before being marked final. |

## Acceptance and nonclaims

- **M0–M3:** compiled native primitives/composites, typed authoring,
  graph-owned connections and declarative, host-authorized composition are
  substantively implemented and exercised by native tests.
- **M4:** persistent native sessions, bounded FIFO/atomic fanout, deferred
  jobs and request cancellation are implemented. H3b known kernel ownership
  and lifecycle corrections are verified, but general concurrency,
  H4/H5/H6 scaling and H7 request fairness acceptance are not complete.
- **M5–M6:** native Kernel/StorageGraph, plugin generation/lease wiring,
  legacy engine cutover and a deterministic iterative-agent composition
  are substantially implemented.
- **M7:** **INCOMPLETE**. CI success alone is not independent acceptance
  of graph liveness, all plugin threading, buffer-policy guarantees,
  benchmark budgets, retirement/migration decisions, or production use.
- **M8:** there is a tested agent-loop composition example, **not**
  a complete production model/tool/memory/streaming agent implementation.

### Verified evidence stack

| Scope | Candidate commit | Exact-head six-job CI |
| --- | --- | --- |
| Callback panic, same-thread recursion | `72d56e413321b58204280b8b6b1d29d6aa5e3f1a` (#1263) | [43185486539](https://github.com/matthis-k/phenix-ai/actions/runs/43185486539) |
| Cross-thread session↔drain and driver lease | `fe09a68da755a8a435a2df4927b47db0e3cc73eb` (#1265) | [43197663375](https://github.com/matthis-k/phenix-ai/actions/runs/43197663375) |
| Formal ledger and benchmark baseline | `e1b4b99b33a2c5c22599f4668ff069721a1d3756` (#1266) | [43202338522](https://github.com/matthis-k/phenix-ai/actions/runs/43202338522) |
| Callback-driven host retirement | `d77feb3c53b308b858a629f4505ec76a465c8073` (#1268) | [43210882861](https://github.com/matthis-k/phenix-ai/actions/runs/43210882861) |
| Shared-session request isolation | `ae746e796550434713e53302b259c52fc3702fbd` (#1269) | [43212196495](https://github.com/matthis-k/phenix-ai/actions/runs/43212196495) |
| Scoped capture matching | `5d0fac5e7e467a9e69bcc8cba35d5a7aa422b2f9` (#1270) | [43214710427](https://github.com/matthis-k/phenix-ai/actions/runs/43214710427) |
| Integrated H3b/H7 branches | `5d5744b2a257958543c0bf281871883eede5d9f6` (#1271) | [43216093923](https://github.com/matthis-k/phenix-ai/actions/runs/43216093923) |
| Driver retirement notification | `081c439c7cc5146c5b3f6cb3275f08913e2d995f` (#1272) | [43265588314](https://github.com/matthis-k/phenix-ai/actions/runs/43265588314) |
| Exclusive producer ownership | `08474b3d4cd8cdb257ed7a57097fde876afec754` (#1273) | [43277415425](https://github.com/matthis-k/phenix-ai/actions/runs/43277415425) |
| Capacity-one backpressure visibility | `f81b49336dfce942f57dad1edc65add258be35fd` (#1274) | [43279922626](https://github.com/matthis-k/phenix-ai/actions/runs/43279922626) |
| This documentation reconciliation | This branch | **Pending its own exact-head CI** |

The table represents separate stacked candidates; never add their changes
as though all independent branches had been merged. H1 opt-in correlated
selection is not default FIFO behavior. C3 archival generations keep explicit
session access after producer retirement; no silent revocation. C7 in-process
native plugins are **not** a security sandbox. H7 a saturated lossless output
requires consumer action or an explicitly budgeted alternate policy.

### Next issues to work

1. **H7:** Decide whether an explicit bounded *per-request* capture budget
   is a primary requirement. Document its global-output-ordering and memory
   tradeoffs before changing transport. No automatic eviction or false fairness.
2. **H4/H5/H6:** Measure closure and `refresh()` scan costs on pinned,
   representative workloads; only then design incremental scheduling and
   compare with the current idle prefilter.
3. **H3b/Q5:** Test remaining external plugin synchronization and tool-level
   delegated work races. Scoped driver/direct producer ownership is verified,
   not a universal lock-order proof.
4. **C3/C6/C7:** Set explicit archival, migration, and trusted/untrusted
   native-plugin policies.
5. **A1–A6 and Q1–Q7:** Apply the established design/adversarial/diff-review
   protocol, with real agent-loop examples and concrete API/test evidence.

**Do not carry forward historical `CI pending` labels or counts from older
snapshots. This ledger itself should be treated as a candidate until its
own exact-head six-job CI passes.**
