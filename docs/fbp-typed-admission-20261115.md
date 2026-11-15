# Typed Admission Using Existing Input Keys

Status: implemented and independently reviewed; exact-head hosted acceptance pending.
Based on correctness-verified `90591a12`.

## Actual Problem And Constraints

`inject_local<T>(name, payload)` does not relate T to the destination name. The
caller can pair an i64 input name with a String and discover the mistake only at
runtime. Authoring already has `InputKey<T>` with local/encoded mode, but admission
does not consume it. Typed keys are names, not owner/generation/permission proofs;
actual exported ingress, type/schema, request validity, closure and credit remain
runtime authorities. No private component input becomes injectable through a key.

Preserve one injection implementation, atomic fanout, scoped metadata, bounded
request lanes, rejected-value ownership, code leases and nonblocking Kernel access.

## Alternatives And Selected Extension

- **Unchanged:** minimal API, but leaves typed authoring disconnected from admission.
- **Name-only typed wrapper:** fixes Rust payload pairing but would incorrectly
  turn encoded keys into local values. Rejected.
- **Selected:** reuse `InputKey<T>` for `inject_key(key, payload)` and scoped
  `inject_request_key(request, key, payload)`. An existing key converts T into the
  canonical Value using its local or encoded mode; both methods delegate to existing
  injection paths. Kernel/Generation forward ordinary typed admission and producers
  get a typed `InputEvent` constructor. No endpoint hierarchy or second scheduler.
- **Generation-bound checked handle:** can detect stale topology at an earlier
  boundary, but is a larger ownership concept than required for typed-name/payload
  ergonomics. Do not introduce it under the guise of this small correction.
- **External schemas/builders:** no new crate or serialization format is needed;
  existing `ContractValue`, `Value`, typed keys and error envelopes supply the work.

`InputKey::value(payload)` is the shared mode-aware conversion needed by session,
Kernel and producer boundaries. Local payloads need not implement Clone or a codec;
encoded keys use the existing canonical `ContractValue` envelope. Output keys cannot
be passed and payload mismatches fail Rust type checking. Dynamic string/Value
admission remains necessary for runtime-discovered ports; it is not a compatibility
shim or a separate execution implementation.

Conversion precedes the existing admission checks, just as callers currently create
Value before injecting it. It may invoke the supplied encoded codec; no purity,
rollback, panic containment or zero-codec-on-rejection guarantee is synthesized.
Failures return the constructed Value through the existing error type, not a
promised original unencoded T. Envelope leases and accepted packet leases retain
their existing contract; borrowed-data cloning remains the caller's responsibility.

## Critique And Acceptance Gate

Independent review approved reuse of existing keys, conditional on preserving
encoded conversion without adding codec bounds to local payloads. The converter
is captured where `T: ContractValue` is known. Kernel conversion occurs before
session acquisition and delegates to its existing raw inject method, never through
an outer `with_session` wrapper that would change callback ordering/error ownership.
Typed keys name public export aliases, not private component paths or topology
identities. Explicit envelope leases remain available through
`key.value(payload).with_lease(...)` followed by raw admission; no lease is inferred
from a key. Ordinary Kernel/Generation forwarding is in scope, while scoped
forwarding uses the existing session API rather than adding another outer error
protocol.

Independent review challenges local/wire identity, encoding and validation
ordering, returned ownership, forged/stale/foreign keys and requests, callback
effects, leases, Kernel contention and declaration authority. Acceptance includes
move-only local payloads, encoded canonical schemas, both queue modes, request
metadata, Full/closed/unknown/mismatched ingress, foreign/sealed/released requests,
and real typed producer/host execution. Rustdoc compile-fail cases reject wrong
payload and wrong key direction. Existing dynamic admission remains the actual
transport authority; no unbounded data cache or public test-only inspector is added.

## Implementation And Independent Review

The exact selected surface is implemented without a new handle hierarchy, error
envelope or injection implementation. Independent source review found no defects
and judged the bounded original A6 payload/admission-ergonomics acceptance complete,
subject to final exact-head hosted acceptance. Keys deliberately remain typed
names; generation-bound authority and wider physical-path exposure belong to A5.

Native tests cover move-only local values, canonical wire schemas and interoperability,
wrong mode/type/schema, unknown/private inputs, atomic fanout, Full/closed/terminal
rejection, both request policies and authenticated capture metadata, foreign/sealed/
released IDs including lane reuse, real KernelHost/Generation and producer execution,
conversion/re-entry/contention ordering, retained-event retry and accepted/error
leases. Compile-fail Rustdoc rejects wrong payload/direction for ordinary, scoped
and producer methods. Existing admission, request, cancellation and lease regressions
remain in force. This is a closure candidate for A6, not an accepted score increase
until its committed head passes the required CI.

## Other Easy Independent Corrections

The same batch repairs source traversal for existing Network/Compose/Prepare/Request
error envelopes and preserves typed NetworkError through derived authoring/binding.
Genuine string errors remain leaves rather than reconstructed causes. Real failure
paths in graph and StorageGraph `error_sources` tests verify context and downcasts.
This closes a Q7 provenance subproblem, not its remaining string-based process and
runtime failure protocol.

`request_blocked_queues(id)` supplies a passive authenticated request-specific view
of existing publication waits, sharing the session-wide blocker calculation. Tests
cover full captures/internal queues, attribution behind another request's buffered
capture, relief, invalid IDs, unscoped exclusion and no callback/deadline mutation.
It resolves a Q6 attribution/diagnostic gap, not general Custom readiness explanations
or static liveness. Both independent corrections require final same-head CI too.
