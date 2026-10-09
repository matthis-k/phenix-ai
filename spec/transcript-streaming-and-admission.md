# Transcript event requirements for rich clients

status: partial
consumers: phenix-ai.nvim transcript redesign

## Why

The application transcript currently publishes `SessionChange::Message`, `TextDelta`, execution state, tool start/result, and progress. Model adapters expose reasoning deltas, but the application transcript drops them. Tools publish a terminal result rather than bounded stdout/stderr increments. Clients cannot render actual live thinking or a running shell's output from this stream.

The UI must render observed events, not guess thinking or output from a final response.

## Reasoning stream

Add an optional ordered event in `phenix.application@1`:

```text
SessionChange::ReasoningDelta {
  execution_id: String,
  text: String
}
```

The active generation emits it when the provider exposes reasoning content that is permitted for display. Providers that expose hidden internal reasoning or only reasoning token counts must not synthesize text. Empty or unavailable content yields no thinking node.

- Preserve session sequence ordering with text deltas and tool events.
- Keep a stable execution ID for incremental rendering and replay.
- Treat reasoning content as sensitive: apply the existing content-storage and retention policy; support redaction and bounded transcript retention.
- ACP emits `AgentThoughtChunk` when permitted by client/protocol support. The descriptor-owned extension remains the fallback for clients without that ACP feature.
- Keep reasoning distinct from final assistant text. Partial reasoning does not become a user message.
- Do not log raw reasoning into diagnostic logs by default.

## Live tool output

Add an optional ordered execution event:

```text
ExecutionChange::ToolOutputDelta {
  call_id: String,
  stream: Stdout | Stderr,
  text: String
}
```

Emits bounded, ordered text segments for commands and other tools that support streaming. `ToolCall`, `ToolResult`, and `ToolFailed` remain the lifecycle events. Final output is authoritative and may overlap streaming chunks; replay clients must not duplicate it.

- The owner of process execution emits stdout/stderr chunks as they arrive, before the final tool result.
- Preserve output order within each stream, content type and byte bounds, and cancellation/disconnect behavior. Do not imply cross-stream ordering without timestamps or a common sequence.
- Unsupported tools keep the current start/result lifecycle.
- ACP tool-call updates contain partial content where the protocol can represent it, with extension fallback for typed tool deltas.
- The frontend may change display modes locally: one-line summary, human-readable command/output, complete protocol input/result. Raw protocol data is still retrievable after concealment.
- The terminal tool outcome stays authoritative; delta output must not turn errors into success.

## Prompt admission and follow-up queue

The legacy `prompt` operation responds only after execution completion. The optional `prompt-admit@1` operation instead returns a `PromptAdmission` receipt after the user message has entered the durable session journal and before model completion. The journal entry is `MessageAdmitted { message, item_id, revision, execution_id }`: receipt identity and user content are committed together, not inferred from matching text.

The admission key is `(session_id, item_id, revision)`. Exact duplicate requests replay the persisted receipt without starting another execution. Reusing a key with different content is a conflict. A client may remove a queued buffer after the matching receipt, but must wait for the terminal `ExecutionChange::State` journal event before sending the next queued turn.

A prompt now atomically appends its initial message and acquires an exclusive claim for that session journal stream. Two independent workers sharing persistence cannot both admit fresh prompts for the same stream. The durable claim value is a random 256-bit secret held by the owning application worker, separate from the journal-visible execution ID. A second worker cannot release the claim using a public receipt or transcript. The terminal state and claim release are committed in one transaction; only then is the terminal event delivered. This retains the terminal event as the next-turn readiness boundary without a split-release window. The legacy prompt path acquires the same claim. The selected declarative agent route uses that same admission and release path. Its transport regression retries one queue item concurrently, checks one receipt and journal admission, rejects changed content under the same key, and counts exactly one provider invocation for the admitted item.

Model input construction must reflect the journal committed before the new prompt's claim. If another worker advanced the session after the local projection was read, the admitted journal sequence reveals the gap. The application then rebuilds model input from the repaired durable projection, excluding the message just admitted to avoid submitting that message twice. This applies to both receipt-based and legacy prompt paths.

**Crash recovery is still a merge blocker.** If the owning process dies after admission and before the terminal transaction, its durable claim remains held. Replaying the original key returns its receipt without restarting unknown side effects; a fresh key must fail closed. An orphan cannot be identified solely by the absence of an entry in one worker's local `active` map. Recovery must prove the previous owner cannot still execute (for example, through explicitly fenced process ownership), record the interrupted terminal state and release the claim atomically. No automatic timeout-based claim stealing is permitted.

The client maintains strict FIFO and focus/write soft locks. Busy or disconnected submissions remain recoverable with their original identity. Steering is a separate input lane, not equivalent to submitting another turn while one runs; this operation does not implement steering. Older runtimes keep the legacy blocking prompt behavior.

For compatibility, the original requirement for a distinct event is retained below as a potential explicit notification shape. The implemented typed operation returns the durable journal sequence directly, and typed session updates carry the same request metadata.

```text
PromptAdmission {
  request_id,
  session_id,
  journal_sequence,
  status: admitted | rejected
}
```

The request ID is per submitted prompt. The frontend correlates admission to its own draft revision, clears only that draft after admission, and keeps edits typed after submission.

Follow-up queuing may live in the client initially. It must retain each immutable prompt and attachments independently, present queued items and allow removal before dispatch, and submit sequentially per session. An upstream prompt queue is optional, but if implemented it must preserve FIFO and session identity with stable IDs.

A user should be able to submit another draft while a prior response runs. Failed or disconnected sends leave unsent drafts recoverable. A disconnected/restarted client must not mistake a stale admission for the current draft.

## Compatibility

These are additive event variants; clients that do not support them continue with message, tool lifecycle and final response events. Versioned descriptor/binding fixtures and ACP translations must be updated together. Unknown variants must use an agreed extension policy rather than causing client failure.

Do not add UI-specific Rust code to the kernel or model providers. Keep events generic enough for any client. UI presentation remains client-owned.

## Acceptance tests

- Reasoning provider emits permitted deltas; ACP and descriptor clients receive the same ordered text; hidden reasoning providers emit none.
- Reasoning and final response render as separate nodes, with exact replay after reconnect.
- A shell with incremental stdout/stderr produces updates before termination; final output is not duplicated.
- Tool error, cancellation, stderr-only output and disconnect retain terminal states and error data.
- Admission arrives after the user message is persisted and before model completion; rejected prompts keep their drafts.
- A worker with stale session history observes another worker's completed messages after claiming the next prompt, without duplicating its own admitted user message in model input.
- Rapid sends, duplicate IDs, two sessions and two frontends do not mix queues or acknowledgements.
- A disconnected client does not consume stale action or admission events.
- Existing descriptor snapshot and generated bindings agree after adding event variants.

This PR is a dependency specification for the phenix-ai.nvim redesign. A passing documentation check does not satisfy these behavioral requirements.
