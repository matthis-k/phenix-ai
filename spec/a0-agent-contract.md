# A0 basic agent preflight contract

Status: frozen before implementation. Base: M1 S1b commit `4d3ccb6c7c5279988f95178942c2107381d259f4`. This gate exercises ordinary M0 native services. A1 remains dependent on S5.

## Ownership and protocol

The public SDK-only fixture workspace owns four native implementations: agent, scripted model, alternate model and text tool. A shared Rust contract crate owns typed operations and generated descriptors. The kernel remains unchanged.

`Run` accepts a prompt and caller-selected maximum model turns. Zero means unlimited. The agent owns turn progression and request-local feedback. It returns an answer plus model-turn and tool-call counts.

`Complete` accepts the original prompt, one-based turn, the available tool name, and typed feedback from the preceding tool call. Feedback carries status, call ID, text and domain error. The first request carries empty feedback. This preflight is a single-tool, single-call-per-turn profile, without full conversation history.

A model reply is either final text with empty call ID/tool, or a tool call with nonempty unique call ID, exact advertised tool name and text input. The agent rejects unknown tools, duplicate call IDs and contradictory final replies before tool dispatch. Empty final text is valid.

The agent records the tool response before asking the model again. A tool domain failure becomes typed error feedback, so the model can recover. Kernel failures propagate with their original code. Model domain failures end the run. A configured turn limit rejects another model call after the permitted count. The agent never invents a timeout or default turn cap.

The text tool counts every admitted call and returns uppercase text, or a typed failure for the explicit failure fixture. An audit operation exposes the count. The alternate model implements the same Complete contract and returns a distinct answer. Composition selects the implementation.

## Acceptance

| Case | Required evidence |
| --- | --- |
| Model -> tool -> model | Final answer uses the actual uppercase tool response; exact counts |
| Repeated tools | Twenty tool calls followed by a final answer with no configured limit |
| Domain failures | Tool failure reaches the next model request; model failure remains typed |
| Invalid replies | Unknown tool, duplicate call ID and contradictory final reply reject before another tool call |
| Authority | Denied model/tool import preserves Denied; denied tool does not increment its counter |
| Scope | Sequential roots retain independent feedback and turn numbers |
| Composition | Rebinding Complete selects the alternate provider; reordered artifacts preserve inspection |
| Product | Real CLI invocation and offline inspection pass against packaged plugins |
| Distribution | Plugins build from SDK-only source; production kernel closure stays free of agent plugins |
| Regression | All sixteen accepted M0/M1 gates pass in full exact-head CI |

`spec/a0-agent-matrix.md` owns file-level progress and evidence. The gate does not claim asynchronous model I/O, streaming, cancellation during synchronous calls, dynamic catalogs, persistent sessions, ValueRef state, reactive scheduling, skills, memory, compaction, ACP or a live model provider. Those require their own plugin contracts and acceptance.
