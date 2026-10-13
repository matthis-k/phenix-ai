# Basic pending-model execution slice

B5 keeps the single BasicInput submit operation and one graph-triggered agent.
The Basic agent starts the selected Model import with the SDK's `Host::begin`.
An immediate reply follows the same continuation as a pending reply. The
graph's existing ticket and readiness pump own scheduling and cancellation.

The agent persists session history and pending tool IDs before executing
external tools. A model reply is committed only after its pending import
settles. Streaming text already appended by the Model provider must not be
emitted twice. The agent checks the current run's journal text since the model
started and appends only the unreported suffix. Inconsistent progress rejects
the reply rather than returning a duplicate or forged final answer.

Cancellation revokes the root and its child before more tool effects.
The native agent owns no I/O thread. A child transport's physical lease
continues draining through the kernel even after the agent cancels.

The product controller uses `begin_action_scoped` and `GraphExecution::pump`.
Synchronous submission remains available to other graph users. Terminal/JSONL
and ACP live-input cancellation are separate follow-up work, not accepted by
this slice.

Acceptance requires the scripted model's existing multi-tool, session,
failure and authority gates, plus pending HTTP model streaming and a
same-root cancellation fixture. Check CI at the exact branch head.