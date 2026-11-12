# FBP issue–resolution–evidence ledger

Last reconciled: **2026-11-12**, against the verified stacked candidate:
[PR #1263](https://github.com/matthis-k/phenix-ai/pull/1263) at `72d56e413321b58204280b8b6b1d29d6aa5e3f1a`
and [PR #1265](https://github.com/matthis-k/phenix-ai/pull/1265) at `fe09a68da755a8a435a2df4927b47db0e3cc73eb`.
The latter passes [exact-head six-job CI #43197663375](https://github.com/matthis-k/phenix-ai/actions/runs/43197663375);
the former passes [exact-head CI #43185486539](https://github.com/matthis-k/phenix-ai/actions/runs/43185486539).
**Both PRs remain unmerged; the original `impl/graph-consolidation-20261110` branch does not contain these fixes.**
The status applies to the *full stacked candidate*, not to any unmerged individual base.

Vocabulary: **Verified** means intended code behavior plus named regressions passes
the exact-head CI, not that a universal security/progress proof exists;
**Partial** means the named issue retains material uncovered cases; **Open**
is not accepted as a solved limitation; **Policy** needs an explicit decision.
Historical phase documents remain source records, not current status authorities.

At this snapshot, **9 verified, 4 partial, 20 open/in progress**.
This ledger/report update itself is still awaiting exact-head CI; Q8 remains
in progress until that gate passes.

| Issue | Status | Evidence / remaining acceptance |
| --- | --- | --- |
| H1 Shared-session FIFO correlation head blocking | Partial | `Readiness::CorrelatedAll` enables opt-in bounded correlated selection; independent requests can use separate sessions. Default strict FIFO / shared-state fairness remains a distinct design limit. |
| H2 Callback panic containment | Verified (scoped) | PR #1263 (`72d56e41`), `43185486539`: preparation, activation, host binding, producer wake and host event callback unwind paths return typed errors; `panic=abort` and callback side effects remain outside guarantee. |
| H3a Inventory lock inversion on producer admission | Verified | Native inventory, drain, and whole-drain retirement regressions pass in exact-head PR #1265 CI `43197663375`; polls/admissions run outside inventory mutex. |
| H3b Session/drain recursive callback reentrancy | Partial | PRs #1263/#1265: recursion guard lifetime, typed same-thread errors, known cross-thread session/drain cycle, and exclusive driver lease verified. `KernelHost::drop`, external callback synchronizers, direct-drain vs driver ownership remain open. |
| H4 Full-inventory scheduling scans | Open | `NetworkSession::refresh` scans process inventory and repeats upon finishing a process. Activation-scaled benchmark and controlled optimization not yet accepted. |
| H5 Expensive closure analysis | Partial | Compiled downstream adjacency removes repeated whole-edge scans, but request closure continues inspecting queued packets; missing measured stress and complexity envelope. |
| H6 No native performance baseline | Open | `docs/fbp-measurements.md` is a source-size comparison, not a runtime benchmark. Need repeatable throughput / p50/p95 / allocation measurements and environment metadata. |
| H7 Request isolation not complete | Open | CorrelatedAll and per-request session workaround exist, but multi-request shared-session fairness, cancellation isolation, and policy guarantees not fully proved. |
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
| Q5 Concurrency testing not exhaustive | Open — improved evidence | Deterministic kernel concurrency regressions now include cross-thread cycle and driver lease, but there is no exhaustive state-space/lock-order model. |
| Q6 Diagnostics intentionally minimal | Open | Diagnostics still intentionally minimal; richer actionable deadlock/readiness explanations remain to build. |
| Q7 Error reporting uneven | Open | Error handling still mixes strings and typed outcomes; code lease/source preservation needs a public API pass. |
| Q8 Final reports stale | In progress | This branch replaces outdated `fbp-report.md` and ledger with an exact-stack evidence map; must pass its own final-head CI before closing. |

## Acceptance and nonclaims

- **M0–M3:** primitive/composite component model, native compiler and
  host-authorized declarative composition substantively implemented.
- **M4:** persistent runtime and cancellation substantially implemented;
  H3b lifecycle/host destruction, H4/H6 cost and H7 shared-session fairness open.
- **M5–M6:** native Kernel/StorageGraph integration, legacy-engine cutover
  and deterministic agent example substantially implemented.
- **M7:** **not complete**. No universal concurrent lifecycle proof,
  benchmark-driven performance acceptance, retirement policy, or final
  agent-product acceptance. Passing Rust/Nix tests is a gate, not M7 itself.

### Evidence gates and provenance

| Scope | Commit | Evidence |
| --- | --- | --- |
| Baseline native FBP branch | `89e00d06498979a2f231dfc8ca780eba44140d27` | Historic branch tip, *not* the terminal/driver fix |
| H2, same-thread H3b and terminal driver | `72d56e413321b58204280b8b6b1d29d6aa5e3f1a` | [CI 43185486539](https://github.com/matthis-k/phenix-ai/actions/runs/43185486539), all six jobs green |
| Cross-thread session/drain cycle, single-driver lease | `fe09a68da755a8a435a2df4927b47db0e3cc73eb` | [CI 43197663375](https://github.com/matthis-k/phenix-ai/actions/runs/43197663375), all six jobs green |
| Refreshed ledger/report and new benchmark harness | This branch | Requires a distinct six-job CI on **its** final head |

For H1 strict FIFO vs `CorrelatedAll`, distinguish *opt-in matching*
from default FIFO behavior. For C3, do not silently revoke archival work
in the name of a security fix. For C7, in-process native code has the
host's OS privileges. For H3b, the specific session/drain two-thread cycle
is handled; unrelated user locks and `KernelHost::drop` are not.

### Next independent iterations

1. Finish Q8 refreshed report verification, introduce H6 reproducible
   per-event/per-topology performance harness; only promote H6 when
   actual measured results and environment are recorded.
2. Define `KernelHost::drop` self-reentrancy and direct drain vs driver
   ownership policy, then test the chosen semantics (H3b/Q5).
3. Reproduce H7 shared-session starvation/fairness patterns with a
   bounded reference model, separate from per-request session workaround.
4. Establish H4 scheduling complexity/throughput baseline before altering
   `refresh()`. Then address H5 residual closure scans and Q2/Q3.
5. Obtain explicit decisions on C3/C6/C7, and review A1–A6/Q1/Q4/Q6/Q7
   against real agent-loop use cases.

The earlier file was dated 2026-11-11 and contained many
`CI pending` entries despite later green exact-head runs. Those entries
are superseded by this reconciled table; do not carry forward its obsolete
verification text.
