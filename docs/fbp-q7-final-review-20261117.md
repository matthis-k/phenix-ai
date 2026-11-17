# Q7 Final Review And Erasure Inventory

## Status

**Original Q7 is scoped verified at
`6f6e751b36f77273da8799900f665bc026447418`: 26.0% -> 100.0%, +74.0 pp.**
Independent whole-slice reviews accepted the full migration after the fixes below.
All local assembled gates passed, including the immutable Nix snapshot and actual
committed-head `nix flake check`. All fifteen exact-head hosted workflows completed
SUCCESS, confirmed via `gh`; the complete URL table appears below.

Authoritative counts are **19 scoped verified / 6 partial / 7 open / 1 approved
exclusion**, leaving **13 in-scope findings**. The subjective unweighted issue mean
is **75.4% (2489 / 33)**, previously 73.2% (2415 / 33), not product completion.
C7's historical 12.0% remains unchanged with no sandbox credit. All prior Q1
acceptance, including the broader Kernel root residual, remains intact.

The [typed-failures checkpoint](fbp-typed-failures-20261116.md) retains historical
phase-1 and intermediate evidence. Its current top status and this note supersede
old core-only restrictions and stale remaining-migration assertions. This
record supplies the exact-head acceptance evidence for the [authoritative scorecard](fbp-issue-progress-estimates-20261113.md).
It supersedes earlier Q7 OPEN/pending assertions, including the explicitly
historical gate at the end. This evidence-only docs revision requires its own
checks; the accepted implementation's checks do not certify this revision or a
newer implementation. No commit or push is part of this documentation update.

## Boundary Inventory

The current implementation is a native signature migration, not a String
compatibility layer. The inventory distinguishes preserved causes from deliberate
presentation and genuine message-only leaves; it is not a claim that every String
in the workspace is an error-erasure bug. Acceptance covers the original full Q7
process/Value/guards/helpers/codecs/macros/adapters/handoff/runtime/request/session/
Kernel/source DATA/query scope, plus framework typed reasons and plugin lifetime
errors, rather than narrowing acceptance to an envelope core.

| Boundary | Current representation and preservation |
| --- | --- |
| Graph failure core | `Failure` owns a cached host summary, typed classification/context, original concrete non-Clone cause, ordered cleanup handles, and known leases. COW enrichment preserves cause pointer and origin identity. `source()` borrows the actual cause rather than a formatted replacement. |
| Process/guards/helpers/jobs/handoff | Step, source/map/sink helper and guard failures carry `Failure`; caught callback panics carry typed phase data. First failure remains primary, reverse cleanup appends typed secondaries, and accepted-job identity enriches diagnostics without inventing unknown IDs. Managed cleanup contains the virtual disposer boundary too. |
| Value/input/codec/macros | Fallible access and reconstruction use typed failures. Exact-local `Failure::from_value` retains known envelope leases recursively on success and rejection, without retaining the Value/payload Arc. Standard and generated/portable codecs preserve typed field/tag/offset/length data and actual conversion sources. |
| Adapters/connections | Preparation preserves original adapter/connection causes plus payload/lease cleanup faults. Errors and successful local adapter outputs pin the exact adapter definition; portable host-owned bytes need no extra adapter pin. Partial fanout cleanup does not replace the initiating failure. |
| Runtime/request/session | Native slots retain original failure handles, typed readiness/runtime faults, and cleanup metadata. Late cleanup synchronizes matching request/session origins across COW rather than replacing an unrelated primary. Recovery faults retain known retiring request/ticket identity. |
| Interface/network/composition | Interface reasons own typed invalid/conflicting data instead of a message-only reason. `NetworkError::IncompletePublicBoundary` is a distinct framework sentinel; the Interface display arm forwards the human-readable cause and its source stays inspectable. Compilation versus Interface precedence is preserved. |
| Kernel | Typed access/configuration/resolution/driver errors retain their data and session failures. `MissingUnit` retains the original `NetworkError` source. `lease_failure` applies the existing execution lease recursively to nested cleanup, without pinning a candidate snapshot. |
| StorageGraph source DATA | The source error port/projection carries local `Failure` DATA, not wire text. Read/decode failures retain `StorageError`, phase, and immutable read/codec configuration rather than the process/context. |
| StorageGraph query | Execution failures are reused; other initiating non-Clone derived errors move once into the source chain. Finalization selects the enriched matching origin or retains a new/unrelated termination failure separately. Cleanup inventory stays typed; `RequestInterrupted` remains outermost. |
| Plugin registry/binding/lifecycle | Public `RegistryFault`, `BindingFault`, and `LifecycleFault` sources preserve identities, expected/actual types or revisions, and rejected operation data. Binding inspection retains the actual `NetworkError`; callback failures retain phase/context and original leased sources. The plugin `Failure` is its own protocol, not Graph `Failure`. |
| Adjacent storage causes | `CodecCause` retains actual external codec errors without a Graph dependency. Backend-opening errors retain the actual callback cause and exact selected contribution/provider/code lifetime. Runtime-directory I/O retains the original OS error, path, and phase without relaxing directory validation. |
| Deliberate presentation/leaves | Cached summaries and Q6 observations are passive host data. `Failure::application_message`, plugin application message leaves, and genuine storage `Codec(String)` leaves need no invented source. Typed framework faults must not be replaced by these leaves. |

## Final Findings

Six additional P2 findings were reproduced in final reviews and are now fixed:

1. `from_value` retained envelope leases only at the parent: enrichment now reaches
   independently escapable nested secondary handles.
2. A virtual Process disposer could unwind unchecked: the managed boundary now
   contains that call while its known code/resources remain live.
3. A discarded emission lease snapshot could panic on its final release without
   being reported: managed release now retains the typed cleanup diagnostic.
4. Kernel `lease_failure` left nested secondaries without the execution lease:
   recursive public lease enrichment now covers those handles.
5. Late cleanup could diverge across request/session COW records: matching current
   origins now receive the enriched cleanup inventory, without mutating escaped
   clones or conflating unrelated failures.
6. Invalid-emission secondaries lacked correct attribution: cleanup now carries
   the actual known emission/accepted-job metadata, leaving unknown identity unset.

Public `with_lease` recursively enriches existing cleanup handles, deduplicating
known leases per record. `with_cleanup` recursively inherits known parent leases
onto the newly attached secondary tree. Neither operation changes earlier escaped
clones of the parent or secondary. `same_origin` identifies shared construction
origin through COW; it is not equality of metadata, lease completeness, or authority.

The framework sentinel is now `NetworkError::IncompletePublicBoundary`, the
network Interface display forwards the human cause, and Kernel `MissingUnit`
retains the original network source. Typed claim regressions preserve actual
Compilation versus Interface precedence rather than changing validation order to
fit an assertion. An independent final fix reviewer found only two test expectation
mistakes, now corrected, and no additional production issues in that review.

The plugin review covered all requested manager conditions, including binding
locator/origin failures and lifecycle foreign catalog/set, registration/load state,
revision exhaustion, duplicate/stale stages, candidate revision, and missing
candidate conditions. Reported evidence is **38 plugin tests plus strict Clippy
passed**. Those focused results alone were not whole-Q7 acceptance. Subsequent
independent whole-slice reviews accepted the original full protocol and its
assembled integration after the fixes, with no remaining blocking findings.
The full local gates and exact-head hosted results below complete that acceptance;
neither a core-only review nor green CI alone is the closure rationale.

## Constraints And Alternatives

- Preserve a concrete cause once behind shared ownership instead of requiring
  arbitrary errors to be Clone/Eq or flattening them into strings. Host-authored
  cached summaries avoid invoking external formatting during passive observation;
  callers explicitly traversing/formatting a borrowed source can execute its code.
- Use recursive per-handle lease ownership instead of relying on a parent's
  lifetime. Retain exact known input/adapter/offer/execution/configuration owners,
  not whole candidate snapshots, scheduler snapshots, Values, or process contexts.
  This avoids extra payload ownership and broad retention cycles.
- Preserve a first primary and ordered typed cleanup secondaries instead of
  replacing the primary with a cleanup message. Managed disposal reports panics
  actually caught; an outer catch around a swallowing ordinary Drop cannot do so.
- Keep typed reasons and framework sentinel variants rather than text sentinels,
  fake arbitrary-error equality, or broad legacy conversions. Boxing Interface
  reasons preserves data without enlarging unboxed error returns unnecessarily.
- Preserve query wrappers and actual validation precedence instead of weakening
  typed assertions or reordering validation to obtain a preferred error shape.
  Origin matching is diagnostic correlation, not execution authority.
- Keep the benchmark-only baseline API selection in harness cfg rather than
  introducing a production String/Failure shim or changing the historical baseline.

## Limits

- Lease completeness is known-owner, per-handle, and nonretroactive. Unknown
  external leases cannot be inferred; previously escaped clones do not gain later
  leases or cleanup. Application owners must supply required leases before escape.
- This is trusted native-code lifetime/diagnostic handling, not a sandbox or an
  abort-recovery mechanism. `panic=abort`, aborting destructors/hooks, and arbitrary
  double-panic behavior are outside the contract. The bounded cleanup policy can
  intentionally forget a second panic payload rather than recurse into destruction.
- Ordinary Drop contains supported unwinds but cannot retroactively report a
  cleanup failure. Managed disposal reports only panics actually caught by that
  operation; another shared owner's later final destruction is not predicted.
- Q6 remains passive host-only cached presentation: no external formatter/source
  callback or executable failure ownership is added to observations. No Q6 scope
  expansion, Job/Completion cloning, or new Graph-to-storage dependency is claimed.
- Focused reviews/tests do not prove every interleaving or erase the historical
  query-review limitation: the unrelated-terminal test uses a real timeout rather
  than forcing expiry at the exact instant inside finalizer `fail_request`.
- Green benchmark workflows below establish successful execution, not performance
  acceptance or a regression-budget result. Successful release compilation is not
  a performance measurement. H4/H5/H6 scores and M7/M8 acceptance remain unchanged.

## Verification Evidence

The assembled migration passed these commands under `nix develop --command`,
using its own worktree target directory:

```bash
cargo test --locked --offline --workspace --no-fail-fast --quiet
cargo test --locked --offline --workspace --no-default-features --no-fail-fast --quiet
cargo test --locked --offline -p phenix-graph --no-default-features --no-fail-fast --quiet
cargo test --locked --offline --workspace --doc
cargo test --locked --offline --workspace --no-default-features --doc
cargo test --locked --offline -p phenix-graph --no-default-features --doc
cargo check --locked --offline --workspace --all-targets
cargo clippy --locked --offline --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

The isolated Graph command supplies real derive-free coverage, independent of
workspace feature unification. Positive portable compiler fixtures compiled and
executed; the required-schema negative fixture checks E0046 rather than accepting
stale-signature or missing-macro failures. Existing intentionally ignored examples
and zero-test feature-gated targets receive no additional coverage credit.

Focused regression evidence includes Graph failure core (12), failure protocol
(15 default / 14 no-default; the generated codec case requires derive), and the
runtime group (207 default / 206 no-default), including library, terminal cleanup,
deferred timeout, in-flight recovery, request/session isolation, request lanes,
owned steps, connections and streams. StorageGraph query cleanup (2) and typed
failures (2) passed, alongside derived/source/host coverage. These are overlapping
target/group counts, not an additive workspace total. Tests inspect original
non-Clone cause pointer identity, typed reasons and sources, ordered cleanup and exact known metadata,
recursive per-handle leases and unchanged escaped clones, and passive repeated Q6
presentation with zero external formatter/source callbacks. The full workspace and
isolated-feature gates above, not just these focused tests, support acceptance.

The benchmark adaptation passed independent review. The three
existing EOS-endpoint, targeted-wake, and closure workflows use the same harness
source for baseline and candidate, with explicit
`fbp_benchmark_baseline_string_error` cfg selecting only the baseline String API;
candidate uses Failure. Baseline remains
`516e0883875d5ec483e3595d50b77b3d5e8979c4`. Artifacts record API selection and
`RUSTFLAGS`, including the explicit check-cfg declaration. Production has no shim.
Candidate and exact-baseline release builds, strict scoped Clippy, and example
tests passed. Sources were byte-identical across revisions. Neither build/review
success nor the now-successful workflow execution is performance acceptance.

The current flake adds default doctests to the per-package test helper, explicit
Graph-plus-plugin-plus-derive doctests, and workspace no-default doctests alongside isolated
Graph no-default docs. It adds no new workflow. Source inspection confirms gate
wiring; the complete Nix snapshot check also passed these commands.

Before commit, the assembled migration tree included untracked sources. The local
Nix snapshot used `builtins.path` to include them, with `.git` and `target` excluded;
the ordinary Git-filtered flake view could omit required new files. The
source-capture expression was:

```nix
builtins.path {
  path = /tmp/opencode/fbp-q7-typed-failures-7c611912;
  name = "fbp-q7-assembled-source";
  filter = path: type:
    !(builtins.elem (builtins.baseNameOf path) [ ".git" "target" ]);
}
```

Snapshot `/nix/store/cbqspiik1gij7zk12fw7fmidwz2w8nxy-source` includes the new
failure/error modules and tests. All ten checks passed with:

```bash
nix flake check path:/nix/store/cbqspiik1gij7zk12fw7fmidwz2w8nxy-source --no-update-lock-file --print-build-logs --max-jobs 2
```

The first invocation hit the terminal's ten-minute timeout while builds were
still running; repeating the same immutable snapshot command completed with
`all checks passed!`. This is local source-snapshot evidence, not hosted CI.

## Exact-Head Acceptance

Accepted implementation SHA: **`6f6e751b36f77273da8799900f665bc026447418`**.
The actual committed-head `nix flake check` passed in addition to the immutable
assembled-source snapshot above. The snapshot is not substituted for this
committed-head check. Local results here record the completed migration gates,
not a rerun or acceptance of this new evidence-only docs revision.

`gh api` queried the repository's runs filtered by that exact `head_sha`, and
`gh run view 44059873792` separately confirmed its head and jobs. Each of the
following fifteen workflows is completed with conclusion **SUCCESS** on that SHA:

| Workflow | Exact-Head Run URL | Conclusion |
| --- | --- | --- |
| Flake CI | [44059873792](https://github.com/matthis-k/phenix-ai/actions/runs/44059873792) | SUCCESS |
| Q5 session ownership Loom | [44059872702](https://github.com/matthis-k/phenix-ai/actions/runs/44059872702) | SUCCESS |
| Q5 actual wake Loom | [44059872936](https://github.com/matthis-k/phenix-ai/actions/runs/44059872936) | SUCCESS |
| Q5 native notifier retirement concurrency | [44059873083](https://github.com/matthis-k/phenix-ai/actions/runs/44059873083) | SUCCESS |
| Q5 in-flight worker recovery | [44059872727](https://github.com/matthis-k/phenix-ai/actions/runs/44059872727) | SUCCESS |
| Native runtime benchmark | [44059872761](https://github.com/matthis-k/phenix-ai/actions/runs/44059872761) | SUCCESS |
| H4 EOS endpoint paired benchmark | [44059872694](https://github.com/matthis-k/phenix-ai/actions/runs/44059872694) | SUCCESS |
| H4 scoped optional poll paired benchmark | [44059872720](https://github.com/matthis-k/phenix-ai/actions/runs/44059872720) | SUCCESS |
| H5 scoped closure paired benchmark | [44059872764](https://github.com/matthis-k/phenix-ai/actions/runs/44059872764) | SUCCESS |
| H5 request index paired benchmark | [44059872704](https://github.com/matthis-k/phenix-ai/actions/runs/44059872704) | SUCCESS |
| H4 targeted wake paired benchmark | [44059872736](https://github.com/matthis-k/phenix-ai/actions/runs/44059872736) | SUCCESS |
| H4 notified Custom readiness benchmark | [44059873077](https://github.com/matthis-k/phenix-ai/actions/runs/44059873077) | SUCCESS |
| H4 live request sparse paired benchmark | [44059873061](https://github.com/matthis-k/phenix-ai/actions/runs/44059873061) | SUCCESS |
| H6 agent allocation events | [44059873094](https://github.com/matthis-k/phenix-ai/actions/runs/44059873094) | SUCCESS |
| H4 mixed agent acceptance benchmark | [44059872750](https://github.com/matthis-k/phenix-ai/actions/runs/44059872750) | SUCCESS |

All **six Flake jobs** succeeded: exact-head inventory evaluation, shared Nix
build/cache writer, check shard-00, shard-01, shard-02, and the Flake matrix evidence
aggregate. No skipped shard or pending aggregate is counted as a pass.

## Numerical Reconciliation

Only Q7 changes: open -> scoped verified, 26.0% -> 100.0%, +74.0 pp.
Thus 18 -> **19 verified**, 6 -> **6 partial**, 8 -> **7 open**, and 1 -> **1
approved exclusion**; 19 + 6 + 7 + 1 = 33. Remaining partial/open findings move
14 -> **13** (6 + 7). The score sum moves 2415 -> **2489**, and 2489 / 33 =
75.4242...%, rounded to **75.4%**. This is a subjective issue mean, not product
completion. C7 stays 12.0%, +0.0 pp; Q1 and every other score/delta are unchanged.
Historical rows and narrative retain their original stage's values and are
superseded for current status, not rewritten into newer-head evidence.

## Historical Exact-Head Gate

The following checkpoint predates acceptance at `6f6e751b36f77273da8799900f665bc026447418`
and is preserved as history only. Its pending status and counts are superseded by
the current acceptance above.

**PENDING.** No exact committed migration head or matching CI result is recorded
here. Local working-tree
results, earlier CI, another head, and separately passing agent selections cannot
substitute for the exact committed assembled head.

Before a future closure decision, record the final assembled local gate results,
the committed SHA containing all required new files, independent benchmark-review
outcome, and CI run/check URLs tied to that exact SHA. Until then Q7 remains OPEN,
all authoritative counts stay unchanged, and there is no score or closure claim.
