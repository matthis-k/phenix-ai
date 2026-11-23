# A2 Explicit Adapted Replacement

Base: local A1 `923ccd6376b4cd8e3980b61ae26ad78c08e9c6a9`.
Branch/worktree: `feat/fbp-a2-adapted-replacement-20261123`,
`/tmp/opencode/fbp-a2-adapted-replacement-20261123`. A1 is not edited.
This is a bounded A2 improvement, not full A2 acceptance or a percentage claim.

## Phase 1: Requirements

Exact interfaces and explicit semantic claims are accepted invariants. Do not
weaken `Interface::check_shape`, port equality, contract catalogs, frozen private
obligations, C7, or H1. Two connected changes are useful without new authority:

- Preserve the canonical shape error on builder and root/nested composer
  replacement, including endpoint, reason, expected/actual identity, full path,
  phase and the existing error source chain. Remove the obsolete opaque variant.
- Execute a provider upgrade in a real context/model/tool/feedback/stop network.
  V2 has renamed ports, materially different wire encoding and an extra output.
  An ordinary composite facade explicitly translates both directions and consumes
  the extra output. Only exact host-granted `Edit::Replace` is needed.

The upgrade seam must genuinely be stable. An old primitive's public promise is
appropriate; replacing a composite that freezes private implementation contracts
is not a license to delete them. Negative tests must retain that refusal.

## Phase 2: Relation And Tradeoffs

Let E be the old public interface, P2 the new provider, I the input converter,
Ot and Of the tool/final converters, and U the explicit usage consumer. The
application authors F2 = compose(I, P2, Ot, Of, U) with public interface E.
Replacement requires `shape(E, interface(F2))` by exact canonical equality and
all old explicit claims and inherited requirements. Neither `shape(E, P2)` nor
`claim(E, P2)` follows from the existence of conversion code.

The behavioral adaptation relation is application-owned: supported legacy
contexts map to V2 dialogue frames, invocation frames map to legacy tool calls,
completion frames map to legacy answers, and every usage event is consumed.
Unsupported values fail the activation; they are not reinterpreted or dropped.
Both tool feedback and final-stop paths must retain request identity. Provider
labels may change, but one ordinary turn must perform exactly one tool effect
and two model decisions; a direct stop performs no tool effect. This is finite
workload evidence for the claim, not a compiler proof of arbitrary semantics.

| Alternative | Decision / cost |
| --- | --- |
| Global width/subtyping or implicit conversion | Reject: changes accepted exact contracts and hides policy |
| Separate, explicit direct width-satisfaction relation | Outstanding A2 design/acceptance decision, not implemented or waived by this work |
| New replacement wrapper/API | Reject: ordinary `NetworkBuilder` and `adapt_wire` already express the relation |
| Facade with explicit translations and usage sink | Select: extra stages, queue slots and codec work, but visible and testable policy |
| Delete old private requirements to accept V2 | Reject: use a genuinely stable seam instead |

Shape failures use Authoring phase during builder replacement and declarative
replay, before candidate compilation. Defensive composite-body validation uses
Compiled phase, matching adjacent validation. Contracts remain compiler-owned
in composer replay; no duplicate claim checker is added there.

## Phase 3: Pre-Edit Critique

Read the existing independent
[nested-replacement critique](fbp-nested-replacement-critique-20261111.md).
No task/subagent tool or installed `opencode` executable is available for a new
independent critique. The following is standalone reasoning, not independent
review or approval; the integrating owner will independently review the result.

After the initial implementation, the user relayed an independent review finding
one P3 source issue (nested compiled body-interface diagnostics lost ancestors)
and verification gaps for detached leases, in-flight old requests, failure after
a real tool effect, and obligation deduplication. Those findings were accepted.
The focused follow-up below has not itself received independent re-review.

- Compare old expected shape against new actual shape, not the reverse. Cover
  missing, extra, wrong-direction and wrong-type failures, both input and output.
- Deep replay must qualify the entire requested path, not only the final pair.
  Failed replacement must preserve published handles, registry and old sessions.
- Shape identity does not infer a public semantic claim. Same contract ID with
  a different definition must still fail, as must removed private obligations.
- A facade that exposes a wider boundary is not adapted replacement. Extra usage
  must connect to a real sink, with no discard option or hidden runtime policy.
- Conversion must change actual bytes/structure and execute on both branches.
  Failure must not fabricate replies or duplicate tools. Side effects preceding
  a later conversion failure are not transactional and must be disclosed.
- Capacity-one queues alone are not backpressure evidence. Deliberately withhold
  consumption, observe blocked progress, then drain and check exact effect counts.
- Revoke must restore the original provider for new sessions while retained V1
  and adapted sessions keep their own topology and behavior under both policies.

## Phase 4: Implementation And Evidence

### Changes

The production diff is limited to five existing graph files:
`src/network.rs`, `src/network/authoring.rs`, `src/network/validation.rs`,
`src/compose/replay.rs`, and `src/network/compiler.rs`. The four equality/refusal
sites now use the unchanged canonical exact checker. Nested errors qualify all ancestors, and only the
failure path constructs that diagnostic string. `NetworkError::InterfaceMismatch`
is removed, not deprecated or aliased. `NetworkError::Interface` and
`ComposeError::Network` already expose concrete sources; no new error wrapper is
needed. `Compiler::expand` qualifies local interface errors with its existing
prefix via the established `qualify` helper. Validation still owns local checks;
no new path authority or qualification of unrelated error variants is added.
The internal malformed-body regression now calls `compile()` at root and two
nested depths, requiring `outer/inner/broken` at the deepest level, rather than
only calling the local validator. Public constructors cannot create this mismatch.

Latest-only consumers were migrated in graph `fbp_components.rs`,
`fbp_nested_replacement.rs`, `fbp_authoring_equivalence.rs`, and Kernel
`composer_nested_extension.rs`. The Kernel regression now asserts the full leaf
path, Authoring phase, missing output name/reason and expected/actual presence.
No module responsibilities, dependencies, runtime code or claim rules changed.

The one new integration file is graph `tests/fbp_a2_adapted_replacement.rs`:

| Test | Evidence |
| --- | --- |
| Canonical diagnostics matrix | Missing, extra, wrong-direction and incompatible wire type, separately for input/output; builder plus root, one-level and two-level nested replay; entire canonical error including expected/actual; typed source chain; retained handles and unchanged active registry |
| Versioned feedback provider | Exact grant at `agent/loop/provider`, with both ancestor opt-ins; V1 primitive public promise to ordinary V2 composite facade; full eleven-emission trace including real feedback and both converters; direct stop; failed dependent update atomicity; revoke restores definition key and edges; retained V1 and V2 sessions continue |
| Bounded capture backpressure | Both queue policies, capacity-one capture/edges/ingress and one admitted request; two sequential turns in a deliberately multi-output scope; actual pending publication and blocked-queue evidence; three blocked retries; explicit drain; exactly four model decisions, two tools and four consumed usage events |
| Conversion failure | Input, tool-output and final-output refusal; no forged capture/tool effect; exact preceding usage/decision counts; original input-conversion `io::Error` source; failed request; separate healthy session still executes |
| Failure after feedback | `fail-after-feedback` produces a valid tool call first, corrupts only the completion after real observation feedback, and fails final conversion with exactly two decisions, one tool effect and two consumed usage events; no reply or repeated effects across three subsequent drives |
| In-flight old request | Pause an already-dispatched tool job before its effect, reject a dependent replacement, revoke the upgrade, drop the composer and old capture handle, then finish that original authenticated V2 request exactly once; no effects repeated during idle retries |
| Detached facade lease | Attach a drop sentinel using existing `CompiledNetwork::with_lease`; success capture, input-conversion failure and post-feedback final-conversion failure survive revoke; composer, compiled graph and session are dropped; only the detached packet or failure retains the host code/resource lease, which drops exactly once when that final owner drops |
| Rejection invariants | Direct renamed V2 gives MissingPort; wider facade gives ExtraPort; exact facade without public claim fails Compiled membership; contradictory same-ID private definition fails catalog validation; exact public shape/claim cannot erase old `provider/core` private obligation; unchanged generations still execute |

V1 is UTF-8 wire text. V2 is a different named/revisioned schema with a version
byte and branch-kind byte before text, renamed dialogue/invocation/completion
ports, and an additional integer usage port. The input adapter creates a kind-0
frame; tool/final adapters require kind 1/2 respectively and re-encode legacy
text. Every usage event traverses a capacity-one edge into an explicit validating
sink. There is no discard option, implicit coercion, topology-edit wrapper or
privileged agent API. A1 support was inspected but is local-String-specific;
rewriting it to introduce these wire protocols would unnecessarily disturb A1/M8.
Both existing agent suites are rerun unchanged instead.

All successful single-turn scopes expect exactly one authenticated reply. The
backpressure test deliberately uses `begin_request(None)` because it produces
two replies in one scope; it does not relax single-reply validation. Separate
requests have separate capture lanes under BoundedLanes. Failed SharedFifo
scopes reject scoped reads as stale; failed BoundedLanes scopes retain their
existing terminal-drain permission. Tests reflect those existing rules rather
than changing H1 or session semantics.

### Verification

Executed in this worktree with the following prefix for Cargo checks:

```sh
nix develop -c env CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/tmp/opencode/fbp-a2-target-20261123
```

Exact final follow-up test commands after that prefix:

```sh
cargo test --locked -p phenix-graph --lib --test fbp_a2_adapted_replacement --test fbp_obligation_dedup --test fbp_declaration_leases --test fbp_components --test fbp_authoring_equivalence --test fbp_nested_replacement --test fbp_wire_adapters --test fbp_contracts --test fbp_a1_nested_agent_loop --test fbp_m8_agent_loop -p phenix-kernel --test composer_nested_extension --test contract_catalog_consistency --test callback_errors --test generations --test checked_nested_exports --test code_leases --quiet
cargo test --locked -p phenix-graph --no-default-features --lib --test fbp_a2_adapted_replacement --test fbp_obligation_dedup --test fbp_declaration_leases --test fbp_components --test fbp_authoring_equivalence --test fbp_nested_replacement --test fbp_wire_adapters --test fbp_contracts --test fbp_a1_nested_agent_loop --test fbp_m8_agent_loop --quiet
cargo test --locked -p phenix-kernel --no-default-features --lib --test composer_nested_extension --test contract_catalog_consistency --test callback_errors --test generations --test checked_nested_exports --test code_leases --quiet
```

- Default: 194 passed, comprising 61 graph unit tests, 74 graph integrations,
  37 Kernel unit tests and 22 Kernel integrations.
- Isolated graph no-default: 134 passed, comprising 61 unit and 73 integration
  tests. The new seven-test file is not derive-gated; all seven execute here.
- Kernel no-default: 59 passed. Kernel declares no features and its graph
  dependency still enables graph defaults; this does not replace isolation above.

All of these final checks passed (same prefix, except formatting needs no target
directory):

```sh
cargo clippy --locked -p phenix-graph -p phenix-kernel --all-targets -- -D warnings
cargo clippy --locked -p phenix-graph --no-default-features --all-targets -- -D warnings
cargo clippy --locked -p phenix-kernel --no-default-features --all-targets -- -D warnings
```

`nix develop -c env CARGO_BUILD_JOBS=2 cargo fmt --all --check` and
`git diff --check` also pass. The initial new-fixture runs failed on
test assumptions about duplicate registration IDs, per-lane capture capacity,
exact-one capture expectations and terminal drain. Those were corrected in the
fixture only; no runtime/claim rule was altered to make them pass.
An initial follow-up sentinel experiment also assumed an input envelope's lease
would survive the custom provider's pending-output state. That is not the
adapter guarantee: the native provider authors fresh pending wire values. The
focused facade sentinel regression instead tests the documented host executable
lease on captures/failures after all other owners drop. It does not claim arbitrary
input-lease propagation through application-owned provider state. Existing
`fbp_wire_adapters` input-lease coverage is rerun unchanged; no core lease change
was made. All three `fbp_obligation_dedup` tests execute in both graph feature
configurations; normalization was not modified.

### Acceptance Limits

Full A2's original direct width-relation requirement remains an explicit
acceptance/design decision; this facade approach does not implement that relation
or close all A2 scope. Exact checker semantics must not be silently relaxed to
obtain it. Independent re-review of this follow-up and exact-head CI remain
outstanding.
No full-workspace tests, doctests, hosted CI or performance measurements were run.

The deterministic provider and tool are real ordinary processes in an executable
feedback cycle, not network LLM/tool integrations. The context stage is stateless;
this does not claim durable memory, state migration, arbitrary concurrent-request
fairness, static liveness, sandboxing or rollback of already-executed effects.
Each drive has a 128-owned-step test budget. With one admitted request, the seven
connection slots, one ingress slot and one capture slot form a nine-slot scoped
queued-packet envelope, not the whole runtime allocation. Pending emissions,
runtime reservations, unscoped queues and arbitrary payload bytes are separate
costs. C7 and H1 are unchanged.

The starting workspace's untracked `composer_private_extension.rs` was not read
or modified. The A1 ref remains `923ccd6376b4cd8e3980b61ae26ad78c08e9c6a9`.
No staging, commit, push or A1 branch edits were performed.
