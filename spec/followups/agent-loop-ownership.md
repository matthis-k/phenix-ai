---
status: implementing
source: kernel-runtime-mechanism-audit-2026-09-23
base_sha: fb0a0e54e80d4f51edd15e641ed576931694a11b
---

# Agent loop progression ownership

## Goal

Make `phenix.agent-loop` own the agent run loop.

At the audited head, the agent-loop plugin performs one model invocation and returns tool calls. `phenix-harness::application::run_agent_execution` owns the real loop, continuation history, hard-coded turn limit, hard-coded tool-call limit, tool execution, and progress emission.

That split gives the agent-loop plugin a policy type without authority over the behavior it claims to configure.

After this PR the application submits one run. The agent loop owns model progression until completion, cancellation, or a typed terminal failure.

## Current path

```text
Application::run_agent_execution
  -> for up to 16 turns
     -> AgentLoopCommand::Run
        -> AgentLoopPlugin
           -> DefaultInvocationCommand::Invoke
           -> return output + tool_calls
     -> application checks tool_calls <= 10
     -> application executes tools
     -> application builds ModelToolTurn continuation
  -> final output
```

`AgentLoopPolicy::max_parallel_tool_calls` is currently separate from the application check that enforces the same value.

## Target path

```text
Application
  -> AgentLoopCommand::Run once
     -> AgentLoop
        -> invoke model
        -> validate returned tool calls
        -> execute admitted tool calls
        -> append continuation
        -> repeat under AgentLoopPolicy
     -> AgentLoopResponse::Completed
  -> application records final result
```

The application remains the client/session transport boundary. The agent loop does not gain direct ACP, Neovim, or frontend knowledge.

## Policy

Extend `AgentLoopPolicy` into the canonical progression policy:

```rust
pub struct AgentLoopPolicy {
    max_model_turns: NonZeroU32,
    max_parallel_tool_calls: NonZeroU32,
}
```

Default values preserve current behavior:

- `max_model_turns = 16`
- `max_parallel_tool_calls = 10`

The root usage budget remains authoritative. The turn limit is a second deterministic guard against a provider repeatedly returning tool calls without terminal output.

Delete the matching numeric checks from `application.rs`.

## Tool execution boundary

Do not move client transport or permission UI into the agent-loop plugin.

Introduce one typed execution dependency that represents an already admitted model tool call:

```text
ToolExecution::Execute {
  session_id,
  execution_id,
  call
}
  -> ToolExecutionResult {
       call_id,
       callable_id,
       output,
       is_error
     }
```

The concrete application-side implementation keeps the current split:

- runtime-owned model tools execute through the runtime path
- client-provided tools execute through the existing admitted client tool path
- permission handling stays at the application/client boundary
- cancellation remains observable before and after tool execution

The agent loop sees one result contract. It does not branch on ACP versus runtime transport.

Use an existing typed component/service pattern in the repository. Do not introduce a callback trait that bypasses kernel dispatch if an ordinary component interface can express the same dependency.

## Progress boundary

Progress is output from the run, not control of the loop.

Define a small typed progress sink or progress service for these existing events:

- tool call started
- tool result
- tool failure

The application owns the adapter from that generic progress record to `ExecutionWorkerEvent` and the client protocol.

Do not make the agent-loop plugin import `ExecutionChange`, `SdkApplicationService`, ACP types, or the application worker channel.

## Continuation ownership

`ModelToolTurn` construction moves into the agent loop.

For each model turn with tool calls:

1. validate the returned calls against the advertised tool descriptors
2. reject more calls than policy permits before executing any call
3. emit tool-call progress
4. execute admitted calls
5. emit terminal result or failure progress
6. build one `ModelToolTurn`
7. append it to the continuation sent to the next model invocation

The application no longer owns `Vec<ModelToolTurn>`.

## Tool scheduling

Keep tool execution sequential in this PR.

Rename the current misleading `max_parallel_tool_calls` policy to `max_tool_calls_per_turn`. The default remains 10.

The agent loop validates the complete returned call set before running the first tool, then executes calls in model order and records results in the same order.

Do not add a scheduler abstraction in this PR. Parallel tool execution is a separate optimization and requires explicit concurrency semantics for client permission callbacks, capability dispatch, cancellation, progress ordering, and runtime tools.

## Error semantics

Preserve the current distinction between a tool failure and a run failure.

- ordinary tool failure becomes `ModelToolResult { is_error: true }` and the next model turn can inspect it
- cancellation terminates the run
- malformed or unavailable tool calls become deterministic tool failures unless the existing contract classifies them as invalid run input
- model invocation failure keeps its typed invocation failure
- exceeding the turn limit returns one typed loop-limit failure
- exceeding the per-turn call limit fails before any tool side effect

Do not stringify typed application or invocation failures earlier than the existing public boundary requires.

## Files and mechanical changes

Primary files:

- `rust/crates/phenix-plugin-execution/src/agent_loop.rs`
- `rust/crates/phenix-plugin-execution/src/agent_loop_regression.rs`
- `rust/crates/phenix-harness/src/application.rs`
- the existing SDK contract module that owns model tool call/result values
- the application interface adapter that already owns admitted client tools

Mechanical sequence:

1. Add the typed tool execution and progress contracts.
2. Implement the application-owned adapter using the current normalization, permission, runtime-tool, and client-tool functions.
3. Move tool descriptor validation into the agent-loop path or a shared contract helper with one owner.
4. Move continuation construction into `AgentLoopPlugin`.
5. Move the 16-turn guard and 10-call guard into `AgentLoopPolicy`.
6. Change one `AgentLoopCommand::Run` to represent a complete run.
7. Reduce `run_agent_execution` to one encoded invocation plus final response conversion.
8. Delete the application loop, application-owned continuation vector, and duplicated numeric guards.

## Invariants from earlier PRs

Preserve #567:

```text
phenix.agent-loop -> phenix.step-runner -> phenix.execution
```

Do not move execution state, budgets, attempts, or transactions back into the agent-loop plugin.

Preserve #563:

- central invocation owns model preparation and accounting
- helper work cannot bypass root budgets
- routing decisions remain pinned to concrete model targets
- provider preflight remains before dispatch

Preserve client tool admission:

- the model cannot invoke a client tool that was not advertised and admitted
- permission decisions remain client/application policy
- the agent loop receives no raw frontend transport capability

## Tests

Add a deterministic run fixture that proves:

- one application call can complete after two model turns and one tool turn
- the application invokes `phenix.agent-loop@1` once
- continuation seen by model turn two contains the exact first output, calls, and results
- 17th model turn fails at the loop boundary
- 11 returned calls fail before any tool executes with the default policy
- cancellation between turns stops before another model invocation
- cancellation during a tool terminates the run
- one tool failure is returned to the next model turn as `is_error = true`
- runtime and client-provided tools use the same agent-loop result contract
- progress ordering remains call then result/failure for each call
- existing execution budget accounting remains unchanged

## Deletion target

Delete the agent progression loop from `phenix-harness/src/application.rs`.

After this PR there is one owner for:

- model turn limit
- tool-call limit
- continuation
- tool-turn progression
- agent-loop usage accumulation

That owner is `phenix.agent-loop`.

## Non-goals

- no new execution-state owner
- no routing redesign
- no memory/context policy change
- no client protocol change
- no agent definition/configuration redesign
- no delegation policy change

## Acceptance criteria

- [ ] Application invokes the agent loop once per root run.
- [ ] `AgentLoopPolicy` controls every agent progression limit.
- [ ] Application contains no model/tool turn loop and no continuation vector.
- [ ] Agent loop has no ACP, Neovim, worker-channel, or frontend transport dependency.
- [ ] Client permission handling stays at the application boundary.
- [ ] #567 causal ownership remains intact.
- [ ] Existing usage and execution accounting remain correct across multi-turn runs.
- [ ] Source, Rust, Clippy, Product, Integration, Docs, and Maintenance checks pass at exact head.

## Follow-up relation

This slice removes application-layer orchestration duplication. It should land before call-scope cleanup so the later runtime context work only has one agent progression path to support.
