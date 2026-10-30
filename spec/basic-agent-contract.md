# Basic agent contracts and product

The Basic product uses the accepted M1 declarations, scoped actions, dispatcher
and pending tickets. Its entry is the discovered `phenix.basic.submit` action.
`PromptSubmitted` triggers a pending BasicInput operation contributed by the
agent plugin. The host pumps generic graph work and observes output contracts.

## Contracts and plugins

`phenix-basic-contracts` defines Model, ToolCatalog, ToolExecute, Skills, Context,
Session, EventJournal, Storage, BasicInput, BasicOutput and Frontend. All request,
response and error schemas derive from portable Rust types. Its independently
built definition package owns the prompt event/action. A shared Blob type uses
the existing canonical byte value, with no extra encoding or wire version.

Session identity maps to one authenticated `phenix.session.<id>` resource.
Every session-scoped provider checks the current host resource before effects.
Imports remain explicitly selected. Catalog discovery grants no tool execution.

The SQLite storage plugin owns durable records and atomic revision-checked
transactions. The journal assigns ordered per-session sequence numbers and
persists every semantic event. Observer offsets are independent. Session replay
restores full messages, assistant call IDs, tool results, usage and run status.
A started tool without a durable result is indeterminate after restart; only
explicit reconciliation supplies a result. A checkpoint is a replay cache.

Tools, skills and context are independent providers. The initial native tools
are uppercase, length and a typed failure fixture. Skills/context persist through
Storage. Catalog cursors bind query and revision. Tool requests carry session,
call ID, selected tool and JSON arguments.

The scripted and HTTP model providers implement the same full-history contract.
It carries assistant calls, tool results, text, tool schemas, context, skills and
usage. HTTP credentials and endpoint/model settings remain provider configuration.
Worker progress reaches the journal only through authenticated poll callbacks.
Workers retain no ordinary host scope. Cancellation stops new effects and drains
existing workers through the kernel's existing ticket tree.

The agent imports these providers. It records a prompt, loads full history,
starts the model through a pending import, validates call IDs, records tools
before effects, appends typed results and continues model turns. It publishes
partial and terminal output through the journal. All execution limits and HTTP
timeouts are caller-selected options; omitted options impose no execution limit.
Agent cancellation cleanup performs no revoked writes. The frontend records
authorized session cancellation through a separate same-root control operation.

Terminal and JSONL frontend plugins decode input and render semantic output.
The product host admits discovered actions, pumps them, forwards output and
cancellation, and owns no model/tool/session logic. ACP uses the same contracts
in its own frontend milestone. Logging and introspection remain independent
optional plugins outside this Basic implementation.

## Acceptance

The incremental `phenix-b5-agent` gate also exercises a partial Graph V2 native
migration: the selected terminal frontend decodes a prompt, an explicit V2
Source/Flow invokes the existing Basic submit operation through ABI 3, and the
frontend renders journaled answer/failure output. The first prompt opens its
session automatically; the caller supplies the existing session resource scope,
not an authentication or manual session-open workflow. The gate checks typed
model failure, single terminal completion, idempotent pumping and cancellation
before dispatch with no journal effect. A B5-only `controlled-pending` build of
the scripted Model additionally holds a real native child and worker FD using
channel handshakes. Private fixture control (`PHENIX_SCRIPTED_MODEL_CONTROL`:
hold/fail/drain) exercises polled kernel failure and in-flight cancellation:
Draining retains leases and blocks shutdown; completion follows worker join;
late success produces no answer or revoked journal write. Kernel failure remains
a kernel failure, with one reconciled failure event and a Failed session.
No public Model request option or kernel agent semantics are introduced. The
normal packaged scripted Model remains synchronous. The gate repackages offline
graph metadata for acceptance only. The Basic loop remains plugin-owned; this
does not establish packaged runtime-host V2 support, graph-decomposed model/tool
turns, frontend cancellation recording/recovery, or full P1 parity.

Public-SDK builds must prove typed definitions and native providers without
private kernel sources. Product tests cover full history, multiple call IDs,
tool error recovery, provider substitution, skills/context, usage, partial
output, caller limits, cancellation, two observer cursors, restart continuation,
indeterminate tools, transaction conflicts and cross-session zero effects.
Terminal/JSONL use the same composed agent. Exact-head Nix shipping and packaged
product evidence remain required. Partial implementations keep their roadmap
rows unchecked until their gates pass.
