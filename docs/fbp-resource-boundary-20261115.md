# A3 Graph Resource Boundary

Status: implemented and independently reviewed bounded whole-finding closure
candidate for A3's graph-local store coupling. Full workspace/default/no-default,
formatting and strict Clippy pass locally; exact-head hosted CI remains an
acceptance gate. This note does not change the campaign scorecard or certify
any other finding, milestone, or concurrent change.

## Defect And Constraints

Graph previously owned `resource::Store`, wrapped arbitrary values in `RwLock`,
and implemented read/write permissions through `ResourceGrant` and context
callbacks. This mixed graph scheduling with storage and authority policy, even
though the independent Storage library already owns its own authorization and
resource lifecycle.

The bounded correction removes that store and its reexport, context `read/write`,
and grant `read/write` constructors entirely. No compatibility aliases, new
crate, Storage dependency, GraphStorageDependency abstraction, lock wrapper,
permissions engine, or production StorageGraph/Kernel edits belong to this slice.
Existing Storage handles, authorization, identities, transactions, persistence,
and adapters remain outside Graph and unchanged by this implementation. The three
behavioral graph-local resource consumers and the empty resource configuration
in admission-preflight tests are migrated to the validated set.

## Selected Contract

- `ResourceGrant::new(key, capability)` stores exactly the supplied `Any + Send +
  Sync` type behind a shared erased owner. `C` and `Arc<C>` remain different types.
- `ProcessContext::resource<C>(key) -> Result<&C, ResourceError>` only resolves a
  grant and downcasts it. It invokes no capability operation or user callback.
- `ResourceError` contains only `Duplicate`, `NotGranted`, and `WrongType`, each
  retaining its resource key. There is no storage/access-mode error vocabulary.
- `ValidatedResourceSet` has a private vector. Its consuming `try_new` rejects
  duplicate keys irrespective of capability type or registration order. A raw
  vector cannot be installed in a context or `SessionOptions`.
- Validation occurs before use, so a rejected set cannot reach factories,
  readiness callbacks, or resource operations. Consuming rejected grants can
  execute destructors; this is not a side-effect-free prepare guarantee.
- Default/empty sets are valid. Cloning shares the validated grants without
  revalidation or custom callbacks. Duplicate keys in different physical process
  sets are valid and do not establish resource identity or implicit sharing.
- Session options and retained runtime process resources use the same validated
  type. Existing automatic source, request source, and reservation construction
  already clone the process set; no separate source callback or repeated
  validation is introduced. Input/wake continuations and dispatch rejection retain
  their existing context. Recovery retains process resources and external effects.
- The application/host chooses authority through opaque facade types. The compiled
  example puts a private mutable handle inside an application module. Its reader
  exposes no writer, inner handle, or conversion; its writer supports read and
  write. Callers outside that module are compile-checked against all three reader
  upgrade attempts. Graph downcast failure is not an application permission engine.

## Alternatives

Keeping or renaming Store, moving its locks behind context callbacks, or adding
read/write flags preserves the coupling and was rejected. A Graph-specific storage
trait/dependency would unnecessarily constrain external capabilities and duplicate
Storage's API. A public unchecked resource vector would require repeated startup
validation and allow ambiguous context lookup. A map alone could silently replace
duplicates; the validated vector instead preserves an explicit rejection contract.
The selected design uses existing Rust type erasure and ownership, with no new
production dependency.

## Critique Corrections

Context-only cleanup is insufficient: `RuntimeProcess` keeps another resource-set
owner after an activation completes, and a host may keep a cloned set even longer.
A consuming duplicate rejection also destroys its input; dropping a raw vector
of two hostile payloads during unwinding can otherwise abort. Both concerns must
be addressed at the grant/set ownership boundary rather than only at job teardown.

Each grant has an `Option<Arc<dyn Any + Send + Sync>>` plus appended leases. Its
private cleanup first takes and individually catches destruction of the capability.
All of that grant's leases remain owned while the caught panic payload is disposed,
including a separately caught panic from that payload's destructor. Only then are
the leases dropped, one at a time under separate catches. Secondary panic payloads
that also panic on destruction use the existing bounded-disposal helper, which
forgets that secondary payload rather than recursively dropping it.

Grant `Drop` invokes this cleanup and cannot report its result. Consuming managed
disposal invokes the same cleanup but returns the actual panic boolean. Set cleanup
drains grants individually, accumulating those booleans; its ordinary `Drop` and
managed disposal share that implementation. Duplicate validation wraps the input
in the set before examining keys, so its error path uses the same safe cleanup.
Managed context cleanup accumulates the resource set's actual result, not a catch
around a now-infallible outer grant drop. Completion/cancellation therefore retain
their existing destructor-failure diagnostics.

This is bounded unwind containment, not a promise to contain all in-process aborts.
`panic=abort`, a panic hook that aborts, non-yielding callbacks, allocation failure,
or double panics inside a single application destructor remain outside the
guarantee. Each grant preserves its own attached leases, not leases for unrelated
grants. Hosts must attach the required leases before sharing grants, and escaped
application-owned capability clones must preserve their own code lifetime. Graph
does not sandbox native code, revoke captured authority, or roll back effects.

## Verification

`fbp_resources` covers exact `C` versus `Arc<C>` downcasts, borrowed/shared identity,
absent/wrong-type errors, zero operations on lookup, same/different-type duplicates
in both orders before callbacks, independent same-key process/session identities,
auto/request sources, rejection retries, yield/staged-input continuations, and
recovery without resetting external state.

Destructor probes use two capability-specific leases independent of any job or
session lease. They assert both leases are alive during payload destruction and
during a panicking panic-payload destructor. Cases include normal/direct grant
drop, outer unwinding, individually panicking leases, mixed-type duplicate rejection
in both orders, managed completion/cancellation diagnostics, runtime-set final-owner
drop after context release, and an externally retained set outliving the session.
The preexisting timeout regression retains both hostile resource destructors with
distinct keys and its exact-job lease checks.

Local results on 2026-11-15:

| Check | Result |
| --- | --- |
| Default features: library plus `integration_regressions`, `fbp_streams`, `fbp_deferred_timeout`, `fbp_resources`, `fbp_owned_steps`, `fbp_declaration_leases`, `fbp_inflight_worker_recovery`, `fbp_terminal_transitions` | 150 passed |
| No default features: library plus the three original consumer suites, resources, and owned steps | 140 passed |
| Graph doctests, including the new capability example | 26 passed |
| `cargo run -p phenix-graph --locked --example resource_capabilities` | Passed |
| Package-scoped Clippy: library, all examples, original consumer suites, resources, and owned steps, with `-D warnings` | Passed through `nix develop --command cargo clippy` |
| Owned Rust files: rustfmt with `skip_children=true` | Passed through the Nix development environment; no workspace-wide formatting |
| `git diff --check` | Passed |
| Full locked workspace/default and no-default-feature tests | Passed on assembled sources after admission-preflight migration |
| Workspace format check and strict all-target Clippy | Passed on assembled sources |

Integration checks compile the exact facade example source from outside the
application module, accepting ordinary read/write use and rejecting reader
`write`, private-handle access, and reader-to-writer conversion with the expected
Rust diagnostics. All original tests remain; both hostile timeout resource probes
remain under distinct keys.

## Remaining Acceptance

The integration owner migrated the extra admission-preflight constructor to an
empty validated set. Package-wide tests now run as part of the passing full
workspace gates. No compatibility adapter bypasses the type boundary.

Independent adversarial source review and a separate assembled lifecycle/resource/
wake integration review found no actionable defects. Both support the original
bounded A3 closure candidate, conditional on exact committed-head hosted acceptance.
Hosted exact-head CI remains unrun and unclaimed. Arbitrary native-code honesty,
revocation, abort containment, and rollback of external effects are not acceptance
claims of this boundary correction.
