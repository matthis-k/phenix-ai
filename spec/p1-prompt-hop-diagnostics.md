# Prompt hop diagnostics (P1)

This gate tests a **single submitted message** through observable boundaries,
using ordinary production interfaces and a deterministic scripted Model.
The trace is not a replacement for real external-provider integration.

## The handoffs

| Stage | Evidence | How to attribute failure |
| --- | --- | --- |
| ACP transport | `initialize`, `session/new`, `session/prompt` replies | No JSON-RPC response means framing/transport/controller issue |
| Frontend decode / projection | ACP `user_message_chunk` | Wrong/missing user chunk means frontend decode/render or notification |
| Graph input admission | durable `PromptAccepted` | No event means decode, action admission, graph dispatch, session or initial storage failed |
| Model scheduling | durable `ModelStarted` | No event after acceptance means agent discovery/prepare failed |
| Model execution | script Model returns `mock:<probe>`; journal `Answer` contains exactly this reply | No matching reply after ModelStarted means Model invocation, return, continuation or checkpoint failed |
| Streaming/reconciliation | durable `TextDelta` equals Model output | Wrong/missing delta means Model streaming or agent reconciliation |
| Frontend output | ACP `agent_message_chunk` equals journal reply | Journal correct but notification wrong means frontend render, ACP sender or client display |
| Tool continuation | two `ToolStarted/ToolFinished` pairs and final Model answer | Inspect Tool execution, result history and second Model invocation |
| Failure boundary | `PromptAccepted`, `ModelStarted`, `Failure`, no `Answer` | Confirms Model-domain failure is not reinterpreted as success |
| Recovery | successful following prompt | Confirms a failed Model turn did not poison session/graph |

**Caveat:** `ModelStarted` is recorded *before* the agent invokes `Model::complete`.
It means admission reached the Model request boundary, **not** that a network
request was attempted or a provider accepted it. Likewise ACP `end_turn`
does not by itself prove a coherent assistant answer. The trace asserts the
observed output as well.

The `p1-prompt-hop-jsonl` gate asserts the journal and Model receipt; the
`p1-prompt-hop-acp` gate sends a real ACP prompt and subsequently reads the
same session's persisted journal with the independent JSONL observer.
No HTTP, OAuth or production secrets are involved. The harness records only
fixed synthetic test probe data and redacted stage names, not user transcripts.

When diagnosing **Neovim specifically**, first compare the runtime-side
ACP `session/update` notifications against the user-visible buffer. If the
runtime's final assistant chunk is correct, the failure is on the Neovim
transport/update/render side. If the journal contains `PromptAccepted`
and `ModelStarted` but not `Answer`, inspect the actual selected Model
provider, its protocol, credential resolution, and transport response.
A deterministic scripted Model passing does not establish real OAuth,
entitlement or OpenAI HTTP connectivity.

## Existing model selector violation

`crates/phenix-basic-runtime/src/acp_sdk.rs::current_model_id`
falls back to `installed_model_provider()` without an effective selected
provider model. The `model_options` implementation then publishes that
plugin identity as a UI select option. On an HTTP preset this exposes
`phenix.model.http`, a transport implementation, not a selectable model.
This is a controller/presentation issue separate from prompt delivery.
Do not assert that the HTML/ACP menu corresponds to provider discovery
until selection validates an actual `ProviderModel` instance.

## Running

```sh
nix build -L .#checks.x86_64-linux.p1-prompt-hop-jsonl
nix build -L .#checks.x86_64-linux.p1-prompt-hop-acp
```

Each check generates `trace.json` in its Nix output. An assertion failure
prints its stage name and fails the derivation. This is an offline regression
gate. Run actual live-provider diagnostics separately, with masked credentials
and no token or full prompt logging.
