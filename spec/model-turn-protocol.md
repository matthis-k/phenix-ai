# Model turn protocol

status: specification-only

## Goal

Define one provider-neutral model contract for role-aware conversation, tool calls, tool results, streaming, capabilities, and usage.

The agent loop must not parse provider-specific JSON or infer tool calls from text.

The protocol reports what a backend can enforce and what work a turn consumed. Higher-level usage policy may choose requested resources, but provider adapters do not own that policy.

## Request

A model turn contains:

- selected model target or routing reference
- ordered instructions
- ordered conversation messages
- available tool definitions
- provider-neutral generation options
- requested reasoning/effort control when supported
- logical execution/attempt identity and optional usage-plan revision/reference

Prompt assembly produces instructions and context separately. The model request preserves that distinction instead of flattening every input into one byte string.

The usage-plan reference is attribution metadata. It does not authorize tools, enlarge budgets, or let the provider adapter reinterpret context/delegation/retry policy.

## Messages

Each message has an explicit role and ordered content parts.

Required roles:

- `user`
- `assistant`
- `tool`

Instructions are carried outside conversation history. Provider adapters map them to the strongest supported instruction role.

Content parts must support text and opaque binary media references without requiring every provider to support every part type. Unsupported required content fails before provider invocation.

## Tool definitions

A tool definition contains:

- stable callable id
- human-readable description
- structural input schema
- structural output schema when known
- concurrency declaration

Tool arguments and tool results use `PhenixValue`. JSON conversion belongs in provider adapters or external protocol adapters.

The model protocol does not grant tool authority. It only describes tools visible to the model. Invocation still passes through execution authority and tool policy.

## Response

A completed model turn contains ordered assistant output items. Each item is one of:

- assistant content
- tool call

A tool call contains:

- provider-neutral call id
- callable id
- structural arguments

Call ids are opaque within the session. The agent loop returns tool results against the exact call id.

The response also contains:

- finish reason
- token or provider usage when available
- effective reasoning/effort behavior when the adapter can report or derive it
- provider metadata isolated from portable fields

## Finish reasons

Portable reasons:

- `complete`
- `tool_calls`
- `length`
- `content_filter`
- `cancelled`
- `provider_error`

Provider-specific reasons may be retained in metadata but do not replace the portable reason.

A response with tool calls uses `tool_calls` even if it also contains assistant text.

## Tool result turn

Each tool result contains:

- call id
- callable id
- structural result or typed failure

Tool failures are data returned to the model unless execution policy requires the whole agent run to fail. A tool panic or transport failure is never represented as successful tool output.

The next model turn receives tool results in the same order as the originating tool calls. Parallel execution must not reorder the conversation record.

## Streaming

Streaming is an optional transport behavior over the same logical turn.

Portable events:

1. assistant content delta
2. tool call started
3. tool argument delta when the provider exposes one
4. tool call completed
5. usage update
6. turn completed

Consumers may ignore deltas and use the final assembled response. Streaming must not change final semantics.

## Retained history

The session record needed for replay and compaction stores the provider-neutral logical turns, not raw provider requests.

Retained entries preserve:

- role
- content
- tool call ids and arguments
- tool results
- finish reason
- usage
- requested/effective reasoning behavior when known
- logical execution/attempt and optional usage-plan identity
- exact context and instruction revision references used to build the turn

Provider metadata may be retained separately for diagnostics. Replay correctness must not depend on it.

## Validation

Validate the request before provider invocation:

- tool ids are unique in one request
- content parts are supported by the selected provider target
- required reasoning/effort control is supported when the caller marks it mandatory

Validate assistant tool calls before tool invocation:

- each tool call references a declared tool
- tool arguments satisfy the consumer tool schema

Validate tool results before the next provider invocation:

- each tool result references a pending call exactly once

Structural mismatches return errors and emit the existing mismatch event path. They do not panic.

## Provider adapters

### Effective capabilities

A model provider performs inference. An agent backend may also own a conversation,
tools, and an internal agent loop. These are separate capabilities. An ACP transport
or a persistent session alone does not prove prompt, tool, reasoning-effort, or
budget control.

Resolve a typed capability snapshot for the selected deployment and adapter
generation. Effective capabilities are the intersection of model support, adapter
support, and execution policy. Unknown support cannot satisfy a hard requirement.
Capability changes invalidate prepared requests before dispatch.

Extend the existing backend/model contracts rather than adding another backend
registry. The snapshot distinguishes these semantic modes:

| Concern | Modes |
| --- | --- |
| Context control | replaceable portable turns; append-only session with explicit reset/replay; opaque managed session |
| Tool control | host-mediated allow-listed calls; isolated backend tools with equivalent enforceable authority; uncontrolled tools |
| Reasoning/effort control | explicit bounded levels/budget; fixed provider/model behavior; unavailable/unknown control |
| Usage visibility | complete normalized usage; partial usage; unavailable |
| Capacity | known deployment limits; configured conservative limits; unknown |
| Optional operations | isolated inference, structured output, streaming, cancellation, exact source resolution, deferred schemas, native compaction, provider cache support |

Required task content, tool authority, fixed constraints, and mandatory effort control
must be enforceable. Reject an incompatible backend before dispatch. Optional
efficiency operations may degrade to the portable or target-default path and emit a
stable reason.

For replaceable turns, Phenix sends the current complete projection. For append-only
sessions, the adapter tracks the acknowledged projection revision and sends only a
valid delta. A rewrite requires acknowledged reset/replay. Sending a full projection
as another user message is not a context replacement.

Opaque managed sessions may receive a bounded task packet and return observations.
Their internal context, reasoning, and tool bytes are outside Phenix accounting.
Report that coverage as partial; do not claim complete prompt enforcement, reasoning
budget enforcement, internal compaction, or replay from a packet alone. Tasks
requiring those guarantees need a backend that exposes them. Unknown-cost opaque
calls cannot satisfy an enforced total cost cap.

An opaque backend used as a child must support a fresh isolated session and an
enforceable attenuated authority boundary. Host tool mediation does not restrict
separate backend-owned tools. Reject the child when those tools cannot be disabled
or isolated under the same authority.

### Optional provider state

Provider cache handles and native compaction payloads are adapter-owned accelerators.
Their compatibility key includes deployment, adapter version, configuration and
authority generation, and covered projection revision. Drop incompatible handles
when switching targets. Reconstruct from portable durable turns and exact sources.
An unchanged covered prefix may reuse its handle with an appended suffix when the
adapter supports that operation; a new whole-projection revision alone is not a
cache miss.

Use native compaction only when its adapter declares the covered range, preserves
mandatory context and tool-call validity, and retains a portable recovery path.
An opaque payload does not become a portable checkpoint or long-term memory.

### Usage and retries

Usage fields carry availability and provenance: reported, estimated, or unavailable.
Unavailable is not zero. Adapters normalize whether cached input is included in
input totals and reasoning is included in output totals. Aggregation must not add
overlapping fields. Preserve provider counters separately when conversion is lossy.

Portable accounting should distinguish when available:

```text
fresh input
cache read
cache write
output
reasoning
tool-definition input
latency
```

Record execution, parent execution, logical turn, attempt, task kind, target, and
optional usage-plan identity/revision. Streaming usage updates replace cumulative
counters for the same attempt; they are not added as independent turns. Record
failed, cancelled, and unknown-outcome attempts, including helper, delegated, retry,
and verification calls. Unknown cost remains unknown in aggregate reports.

Higher layers may attach a bounded purpose/causal attribution to an attempt, such as
primary work, retry, helper, delegation, verification, or reacquisition after prior
context/tool reduction. The adapter reports usage; it does not infer that cause.
When causal attribution is unknown, preserve the work as ordinary unattributed usage
rather than treating it as savings.

Requested and effective reasoning/effort behavior remain separate. A backend with
fixed effort records that fact. It must not report the requested level as enforced.

Persist request identity before dispatch. A transport disconnect after possible
acceptance has an unknown outcome. Reconcile by adapter request identity when
supported; otherwise expose the uncertainty and apply explicit retry policy.
Do not replay tool side effects or promise exactly-once external inference.

### Adapter conformance

Deterministic fixtures must exercise portable turns, append-only reset/replay, and
opaque managed sessions. Prove capability rejection, requested/effective reasoning
control reporting, partial usage, tool authority, target switching, incompatible
opaque-state discard, and unknown dispatch outcomes. An optimization-only capability
missing from a fixture must preserve ordinary work.

### Mapping ownership

A provider adapter owns:

- mapping portable roles to provider roles
- tool schema conversion
- provider request serialization
- provider response parsing
- streaming event assembly
- portable finish reason mapping
- provider-specific metadata
- translation of supported reasoning/effort controls
- normalization of reported provider usage

The core agent loop owns none of those mappings.

## Non-goals

- Define provider authentication.
- Define usage-planning or model-routing policy.
- Define tool scheduling policy.
- Define compaction summarization.
- Infer reacquisition causality inside provider adapters.
- Preserve raw provider payloads as the authoritative session history.
- Require every provider to support every content type or effort control.

## Implementation order

1. Replace byte-only model input and output with the typed turn contract.
2. Add effective capability reporting, including context/tool/reasoning control and usage visibility.
3. Adapt native providers and the basic model fixture.
4. Persist portable model/tool history plus attempt and usage-plan attribution in sessions.
5. Wire the agent loop to tool calls and tool results.
6. Normalize per-attempt usage with reported/estimated/unavailable provenance.
7. Add compaction against retained portable history.