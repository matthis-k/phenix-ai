//! Typed adaptation of the standard agent topology to generic kernel execution.
//!
//! The kernel decides which node to execute. This module translates the shared
//! agent contracts into node requests, branch outcomes, and caller-owned state.
//! It does not implement context, model invocation, tools, or session logic.

use phenix_core::{
    Bytes, ModelToolCall, PhenixValue, Project, RootExecutionHandle, WorkflowRunError,
    WorkflowRunReport,
};
use phenix_sdk::{
    AgentLoopCommand, AgentLoopResponse, AgentLoopUsage, AgentToolBatchRequest,
    AgentToolBatchResponse, AgentTurnState, AgentTurnStepRequest, AgentTurnStepResponse,
    validate_model_tool_calls,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU64,
};

struct TurnRun {
    state: AgentTurnState,
    pending: Option<(Bytes, Vec<ModelToolCall>)>,
    terminal: Option<AgentLoopResponse>,
}

impl TurnRun {
    fn from_command(command: AgentLoopCommand) -> Self {
        let AgentLoopCommand::Run {
            execution_id,
            session_id,
            parent_attempt_id,
            callable_id,
            input,
            tools,
        } = command;
        Self {
            state: AgentTurnState {
                execution_id,
                session_id,
                parent_attempt_id,
                callable_id,
                input,
                tools,
                continuation: Vec::new(),
                seen_tool_call_ids: Vec::new(),
                observations: BTreeMap::new(),
                usage: AgentLoopUsage {
                    model_calls: 0,
                    tool_calls: 0,
                },
            },
            pending: None,
            terminal: None,
        }
    }

    fn validate_initial(&self) -> Result<(), String> {
        phenix_sdk::validate_initial_tools(&self.state.tools)
    }

    fn finish(self, report: WorkflowRunReport) -> Result<AgentLoopResponse, String> {
        let terminal = self
            .terminal
            .ok_or_else(|| "agent topology ended without a terminal response".to_owned())?;
        let expected = match &terminal {
            AgentLoopResponse::Completed { .. } => "final",
            AgentLoopResponse::Cancelled { .. } => "cancelled",
            AgentLoopResponse::Failed { .. } => "failed",
        };
        if report.final_outcome != expected {
            return Err(format!(
                "agent topology ended on {}, expected {expected} for the terminal response",
                report.final_outcome
            ));
        }
        Ok(terminal)
    }

    fn prepare(&mut self, node: &str) -> Result<Vec<u8>, String> {
        // Terminal results are final. A malformed or substituted topology
        // must not invoke another provider after a terminal response and
        // silently return the earlier result.
        if self.terminal.is_some() {
            return Err("agent topology invoked a node after terminal response".into());
        }
        let value = match node {
            "turn" => {
                if self.pending.is_some() {
                    return Err("agent topology skipped an admitted tool batch".into());
                }
                PhenixValue::from(&AgentTurnStepRequest {
                    state: self.state.clone(),
                })
            }
            "tool_batch" => {
                let (assistant_output, tool_calls) = self.pending.as_ref().ok_or_else(|| {
                    "agent topology entered tool batch without tool calls".to_owned()
                })?;
                PhenixValue::from(&AgentToolBatchRequest {
                    state: self.state.clone(),
                    assistant_output: assistant_output.clone(),
                    tool_calls: tool_calls.clone(),
                })
            }
            other => return Err(format!("undeclared agent workflow node: {other}")),
        };
        serde_json::to_vec(&value).map_err(|error| error.to_string())
    }

    fn check_state(&self, next: &AgentTurnState) -> Result<(), String> {
        if next.execution_id != self.state.execution_id
            || next.session_id != self.state.session_id
            || next.parent_attempt_id != self.state.parent_attempt_id
            || next.callable_id != self.state.callable_id
        {
            return Err("agent node changed immutable execution identity".into());
        }
        if next.input != self.state.input {
            return Err("agent node changed original request input".into());
        }
        let previous_ids: BTreeSet<_> = self.state.seen_tool_call_ids.iter().collect();
        let next_ids: BTreeSet<_> = next.seen_tool_call_ids.iter().collect();
        if previous_ids.len() != self.state.seen_tool_call_ids.len()
            || next_ids.len() != next.seen_tool_call_ids.len()
            || !previous_ids.is_subset(&next_ids)
        {
            return Err("agent node removed or duplicated recorded tool-call ids".into());
        }
        if next.usage.model_calls < self.state.usage.model_calls
            || next.usage.tool_calls < self.state.usage.tool_calls
        {
            return Err("agent node regressed execution usage counters".into());
        }
        Ok(())
    }

    /// A successful batch must record the exact effects admitted by the model.
    /// It may add compatible tools and observations, but cannot insert fictitious
    /// history or leave a tool result out of the continuation.
    fn check_completed_batch(&self, next: &AgentTurnState) -> Result<(), String> {
        let (assistant_output, calls) = self
            .pending
            .as_ref()
            .ok_or_else(|| "tool-batch completion without a pending batch".to_owned())?;
        let expected_len = self
            .state
            .continuation
            .len()
            .checked_add(1)
            .ok_or_else(|| "tool continuation length overflowed".to_owned())?;
        if next.continuation.len() != expected_len
            || !next.continuation.starts_with(&self.state.continuation)
        {
            return Err("tool-batch provider rewrote or omitted continuation history".into());
        }
        let recorded = &next.continuation[expected_len - 1];
        if recorded.assistant_output != *assistant_output
            || recorded.tool_calls != *calls
            || recorded.tool_results.len() != calls.len()
            || !recorded
                .tool_results
                .iter()
                .zip(calls)
                .all(|(result, call)| {
                    result.call_id == call.call_id && result.callable_id == call.callable_id
                })
        {
            return Err("tool-batch provider returned mismatched tool results".into());
        }
        let completed = u32::try_from(calls.len())
            .map_err(|_| "tool batch exceeds representable call count".to_owned())?;
        if self.state.usage.tool_calls.checked_add(completed) != Some(next.usage.tool_calls) {
            return Err("tool-batch provider reported inconsistent tool usage".into());
        }
        if self
            .state
            .tools
            .iter()
            .any(|tool| !next.tools.contains(tool))
        {
            return Err("tool-batch provider removed or replaced an active tool".into());
        }
        phenix_sdk::validate_initial_tools(&next.tools)?;
        self.check_batch_observations(next, calls, completed)?;
        Ok(())
    }

    /// A cancelled batch can already have performed some tool effects, but it
    /// cannot synthesize a completed model continuation. Partial observations
    /// and usage are limited to the pending calls.
    fn check_cancelled_batch(&self, next: &AgentTurnState) -> Result<(), String> {
        let (_, calls) = self
            .pending
            .as_ref()
            .ok_or_else(|| "cancelled tool batch without pending calls".to_owned())?;
        if next.continuation != self.state.continuation {
            return Err("cancelled tool batch fabricated continuation history".into());
        }
        let completed = next
            .usage
            .tool_calls
            .checked_sub(self.state.usage.tool_calls)
            .ok_or_else(|| "cancelled tool batch regressed tool usage".to_owned())?;
        let admitted = u32::try_from(calls.len())
            .map_err(|_| "tool batch exceeds representable call count".to_owned())?;
        if completed > admitted {
            return Err("cancelled tool batch reported unadmitted tool usage".into());
        }
        self.check_batch_observations(next, calls, completed)?;
        if completed == 0 && next.tools != self.state.tools {
            return Err("cancelled tool batch changed tools without completed calls".into());
        }
        if self
            .state
            .tools
            .iter()
            .any(|tool| !next.tools.contains(tool))
        {
            return Err("cancelled tool batch removed or replaced an active tool".into());
        }
        phenix_sdk::validate_initial_tools(&next.tools)?;
        Ok(())
    }

    /// Tool execution may update observations for the pending callables only.
    /// A replacement provider must preserve unrelated observations verbatim.
    fn check_batch_observations(
        &self,
        next: &AgentTurnState,
        calls: &[ModelToolCall],
        completed: u32,
    ) -> Result<(), String> {
        let completed_prefix = usize::try_from(completed)
            .map_err(|_| "completed tool count cannot index this platform".to_owned())?;
        let mut changes = 0_u32;
        for (callable, before) in &self.state.observations {
            match next.observations.get(callable) {
                None => return Err("tool-batch provider removed recorded observations".into()),
                Some(after)
                    if before != after
                        && !calls.iter().any(|call| &call.callable_id == callable) =>
                {
                    return Err("tool-batch provider changed unrelated observations".into());
                }
                _ => {}
            }
        }
        for (callable, after) in &next.observations {
            if self.state.observations.get(callable) == Some(after) {
                continue;
            }
            if !calls
                .iter()
                .any(|call| &call.callable_id == callable && call.call_id == after.occurrence_id)
            {
                return Err("tool-batch observation has no matching admitted call".into());
            }
            changes = changes
                .checked_add(1)
                .ok_or_else(|| "tool-batch observation count overflowed".to_owned())?;
            if changes > completed {
                return Err("tool-batch observations exceed completed tool usage".into());
            }
            if !calls
                .iter()
                .take(completed_prefix)
                .any(|call| &call.callable_id == callable && call.call_id == after.occurrence_id)
            {
                return Err("tool-batch observation came from an uncompleted call".into());
            }
            after
                .validate()
                .map_err(|error| format!("tool-batch observation failed validation: {error:?}"))?;
        }
        Ok(())
    }

    fn project(&mut self, node: &str, output: &[u8]) -> Result<String, String> {
        let value: PhenixValue =
            serde_json::from_slice(output).map_err(|error| error.to_string())?;
        match node {
            "turn" => {
                let response = AgentTurnStepResponse::try_from(Project(&value))
                    .map_err(|error| format!("invalid turn-step response: {error:?}"))?;
                let outcome = response.workflow_outcome().to_owned();
                let increment: u32 = match &response {
                    AgentTurnStepResponse::ToolCalls { .. }
                    | AgentTurnStepResponse::Final { .. } => 1,
                    AgentTurnStepResponse::Cancelled { .. } => 0,
                    AgentTurnStepResponse::Failed { failure, .. } => match failure {
                        phenix_sdk::AgentLoopFailure::ModelTurnLimitExceeded { .. } => 0,
                        phenix_sdk::AgentLoopFailure::ToolCallLimitExceeded { .. } => 1,
                    },
                };
                let expected_model_calls = self
                    .state
                    .usage
                    .model_calls
                    .checked_add(increment)
                    .ok_or_else(|| "turn-step model-call usage overflowed".to_owned())?;
                match &response {
                    AgentTurnStepResponse::ToolCalls { state, .. }
                    | AgentTurnStepResponse::Final { state, .. }
                    | AgentTurnStepResponse::Cancelled { state }
                    | AgentTurnStepResponse::Failed { state, .. } => {
                        self.check_state(state)?;
                        if state.usage.model_calls != expected_model_calls {
                            return Err("turn-step reported inconsistent model usage".into());
                        }
                        // A model turn can report new call identities, but tool
                        // execution state is owned by the tool-batch provider.
                        // Do not accept fabricated tool output or catalog edits.
                        if state.continuation != self.state.continuation
                            || state.observations != self.state.observations
                            || state.tools != self.state.tools
                            || state.usage.tool_calls != self.state.usage.tool_calls
                        {
                            return Err("turn-step changed tool-execution-owned state".into());
                        }
                    }
                }
                match response {
                    AgentTurnStepResponse::ToolCalls {
                        state,
                        assistant_output,
                        tool_calls,
                    } => {
                        if tool_calls.is_empty() {
                            return Err("turn-step returned tool_calls with an empty batch".into());
                        }
                        // A substituted turn provider must satisfy the same global
                        // call-identity invariant as the Basic provider. Otherwise
                        // a reused id could execute an effect twice on a later turn.
                        let mut expected: BTreeSet<_> =
                            self.state.seen_tool_call_ids.iter().cloned().collect();
                        if expected.len() != self.state.seen_tool_call_ids.len() {
                            return Err(
                                "agent state contains duplicate recorded tool-call ids".into()
                            );
                        }
                        validate_model_tool_calls(&tool_calls, &mut expected)?;
                        let reported: BTreeSet<_> =
                            state.seen_tool_call_ids.iter().cloned().collect();
                        if reported.len() != state.seen_tool_call_ids.len() || reported != expected
                        {
                            return Err(
                                "turn-step tool-call ids disagree with execution history".into()
                            );
                        }
                        // Seen IDs form a canonical sorted set, not an ordered
                        // execution transcript. Basic nodes use BTreeSet here.
                        // Validate the representation as well as membership.
                        if state.seen_tool_call_ids != expected.into_iter().collect::<Vec<_>>() {
                            return Err("turn-step reordered admitted tool-call identities".into());
                        }
                        self.state = state;
                        self.pending = Some((assistant_output, tool_calls));
                    }
                    AgentTurnStepResponse::Final { state, output } => {
                        if state.seen_tool_call_ids != self.state.seen_tool_call_ids {
                            return Err("final turn-step invented tool-call history".into());
                        }
                        self.terminal = Some(AgentLoopResponse::Completed {
                            output,
                            usage: state.usage.clone(),
                        });
                        self.state = state;
                    }
                    AgentTurnStepResponse::Cancelled { state } => {
                        if state.seen_tool_call_ids != self.state.seen_tool_call_ids {
                            return Err("cancelled turn-step changed tool-call history".into());
                        }
                        self.terminal = Some(AgentLoopResponse::Cancelled {
                            usage: state.usage.clone(),
                        });
                        self.state = state;
                    }
                    AgentTurnStepResponse::Failed { state, failure } => {
                        match &failure {
                            phenix_sdk::AgentLoopFailure::ModelTurnLimitExceeded { limit } => {
                                // A declared positive limit must have been reached.
                                // No model invocation occurred. No new call IDs
                                // may enter the execution history.
                                if *limit == 0 || state.usage.model_calls < *limit {
                                    return Err("pre-model failure triggered before limit".into());
                                }
                                if state.seen_tool_call_ids != self.state.seen_tool_call_ids {
                                    return Err(
                                        "pre-model failure invented tool-call history".into()
                                    );
                                }
                            }
                            phenix_sdk::AgentLoopFailure::ToolCallLimitExceeded {
                                limit,
                                actual,
                            } => {
                                // The Basic provider records all validated model
                                // tool-call IDs before rejecting the oversized batch.
                                let previous: BTreeSet<_> =
                                    self.state.seen_tool_call_ids.iter().cloned().collect();
                                let recorded: BTreeSet<_> =
                                    state.seen_tool_call_ids.iter().cloned().collect();
                                let newly_seen = recorded.len().checked_sub(previous.len());
                                if actual <= limit
                                    || recorded.len() != state.seen_tool_call_ids.len()
                                    || state.seen_tool_call_ids
                                        != recorded.into_iter().collect::<Vec<_>>()
                                    || newly_seen != usize::try_from(*actual).ok()
                                {
                                    return Err(
                                        "post-model limit failure reported inconsistent tool-call history"
                                            .into(),
                                    );
                                }
                            }
                        }
                        self.terminal = Some(AgentLoopResponse::Failed {
                            failure,
                            usage: state.usage.clone(),
                        });
                        self.state = state;
                    }
                }
                Ok(outcome)
            }
            "tool_batch" => {
                let response = AgentToolBatchResponse::try_from(Project(&value))
                    .map_err(|error| format!("invalid tool-batch response: {error:?}"))?;
                let outcome = response.workflow_outcome().to_owned();
                match &response {
                    AgentToolBatchResponse::Continue { state }
                    | AgentToolBatchResponse::Cancelled { state } => {
                        self.check_state(state)?;
                        let reported: BTreeSet<_> = state.seen_tool_call_ids.iter().collect();
                        let recorded: BTreeSet<_> = self.state.seen_tool_call_ids.iter().collect();
                        if reported != recorded
                            || state.seen_tool_call_ids != self.state.seen_tool_call_ids
                        {
                            return Err(
                                "tool-batch provider changed model tool-call history".into()
                            );
                        }
                        if state.usage.model_calls != self.state.usage.model_calls {
                            return Err("tool-batch provider changed model usage".into());
                        }
                    }
                }
                match response {
                    AgentToolBatchResponse::Continue { state } => {
                        self.check_completed_batch(&state)?;
                        self.state = state;
                        self.pending = None;
                    }
                    AgentToolBatchResponse::Cancelled { state } => {
                        self.check_cancelled_batch(&state)?;
                        self.terminal = Some(AgentLoopResponse::Cancelled {
                            usage: state.usage.clone(),
                        });
                        self.state = state;
                        self.pending = None;
                    }
                }
                Ok(outcome)
            }
            other => Err(format!("undeclared agent workflow node: {other}")),
        }
    }
}

/// Execute the standard declared agent loop with any compatible node providers.
///
/// All node calls use the root's resolved import handles, authority and Layers.
/// The topology is never rebuilt or replaced by this adapter. A transport or
/// projection error stops execution; it cannot replay a partially executed tool.
///
/// No implicit step cap, timeout, retry or model-turn limit is introduced.
pub fn run_agent_workflow(
    root: &RootExecutionHandle,
    command: AgentLoopCommand,
    cancelled: impl FnMut() -> bool,
    step_limit: Option<NonZeroU64>,
) -> Result<AgentLoopResponse, String> {
    run_agent_workflow_with_dispatch(root, command, cancelled, step_limit, false)
}

/// Opt in to the native pending Core scheduler without changing the agent
/// topology, domain adapter or resolved provider contracts.
pub fn run_agent_workflow_pending(
    root: &RootExecutionHandle,
    command: AgentLoopCommand,
    cancelled: impl FnMut() -> bool,
    step_limit: Option<NonZeroU64>,
) -> Result<AgentLoopResponse, String> {
    run_agent_workflow_with_dispatch(root, command, cancelled, step_limit, true)
}

fn run_agent_workflow_with_dispatch(
    root: &RootExecutionHandle,
    command: AgentLoopCommand,
    cancelled: impl FnMut() -> bool,
    step_limit: Option<NonZeroU64>,
    native_pending: bool,
) -> Result<AgentLoopResponse, String> {
    let mut run = TurnRun::from_command(command);
    // Provider substitution must not bypass the entry contract enforced by
    // the Basic node. Reject ambiguous tool identities before dispatch.
    run.validate_initial()?;
    let owner = phenix_core::ComponentId::parse(super::AGENT_TOPOLOGY_PLUGIN)
        .expect("static agent topology component id");
    let result = if native_pending {
        root.execute_workflow_pending(
            (&owner, "agent.turn"),
            &mut run,
            |node, _, run| run.prepare(node),
            |node, _, output, run| run.project(node, output),
            cancelled,
            step_limit,
        )
    } else {
        root.execute_workflow(
            (&owner, "agent.turn"),
            &mut run,
            |node, _, run| run.prepare(node),
            |node, _, output, run| run.project(node, output),
            cancelled,
            step_limit,
        )
    };
    match result {
        Ok(report) => run.finish(report),
        Err(WorkflowRunError::Cancelled { .. }) => Ok(AgentLoopResponse::Cancelled {
            usage: run.state.usage,
        }),
        Err(error) => Err(format!("agent workflow execution failed: {error:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_initial_tool_descriptors_fail_before_dispatch() {
        use phenix_core::{CallableId, ModelToolDescriptor, PhenixSchema};

        let descriptor = ModelToolDescriptor {
            id: CallableId::parse("fixture.tool").unwrap(),
            description: "fixture tool".into(),
            input_schema: PhenixSchema::Any,
            output_schema: PhenixSchema::Any,
        };
        let run = TurnRun::from_command(AgentLoopCommand::Run {
            execution_id: "duplicate-input".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: None,
            input: Bytes::from(b"hello".to_vec()),
            tools: vec![descriptor.clone(), descriptor],
        });
        assert!(
            run.validate_initial()
                .unwrap_err()
                .contains("duplicate tool descriptor")
        );
    }

    #[test]
    fn terminal_results_cannot_resume_or_be_reported_as_another_exit() {
        let completed_run = || {
            let mut run = TurnRun::from_command(AgentLoopCommand::Run {
                execution_id: "terminal-only".into(),
                session_id: None,
                parent_attempt_id: None,
                callable_id: None,
                input: Bytes::from(b"hello".to_vec()),
                tools: Vec::new(),
            });
            let mut state = run.state.clone();
            state.usage.model_calls = 1;
            let response = AgentTurnStepResponse::Final {
                state,
                output: Bytes::from(b"done".to_vec()),
            };
            let encoded = serde_json::to_vec(&PhenixValue::from(&response)).unwrap();
            assert_eq!(run.project("turn", &encoded).unwrap(), "final");
            run
        };

        let mut completed = completed_run();
        // A broken topology must not execute another provider after the
        // adapter has observed a completed turn.
        for next in ["turn", "tool_batch"] {
            assert!(
                completed
                    .prepare(next)
                    .unwrap_err()
                    .contains("after terminal response")
            );
        }
        assert_eq!(completed.state.usage.model_calls, 1);
        assert!(matches!(
            completed.finish(WorkflowRunReport {
                last_node: "turn".into(),
                final_outcome: "final".into(),
                executed_nodes: 1,
            }),
            Ok(AgentLoopResponse::Completed { .. })
        ));
        assert!(
            completed_run()
                .finish(WorkflowRunReport {
                    last_node: "turn".into(),
                    final_outcome: "cancelled".into(),
                    executed_nodes: 1,
                })
                .unwrap_err()
                .contains("expected final")
        );
    }

    #[test]
    fn tool_batch_is_admitted_only_after_a_typed_turn_result() {
        use phenix_core::CallableId;

        let mut run = TurnRun::from_command(AgentLoopCommand::Run {
            execution_id: "fixture-tools".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: None,
            input: Bytes::from(b"hello".to_vec()),
            tools: Vec::new(),
        });
        assert!(run.prepare("tool_batch").is_err());
        let call = ModelToolCall {
            call_id: "call-1".into(),
            callable_id: CallableId::parse("fixture.tool").unwrap(),
            input: PhenixValue::Unit,
        };
        let mut after_model = run.state.clone();
        after_model.usage.model_calls = 1;
        after_model.seen_tool_call_ids = vec!["call-1".into()];
        let turn = AgentTurnStepResponse::ToolCalls {
            state: after_model.clone(),
            assistant_output: Bytes::from(b"invoke tool".to_vec()),
            tool_calls: vec![call.clone()],
        };
        let encoded = serde_json::to_vec(&PhenixValue::from(&turn)).unwrap();
        assert_eq!(run.project("turn", &encoded).unwrap(), "tool_calls");
        assert!(
            run.prepare("turn")
                .unwrap_err()
                .contains("skipped an admitted tool batch"),
            "a substituted topology must not admit another model turn before tool effects settle"
        );
        let batch = run.prepare("tool_batch").unwrap();
        let decoded: PhenixValue = serde_json::from_slice(&batch).unwrap();
        let request = AgentToolBatchRequest::try_from(Project(&decoded)).unwrap();
        assert_eq!(request.tool_calls, vec![call.clone()]);
        assert_eq!(request.state.usage.model_calls, 1);

        let mut after_tools = after_model;
        after_tools.usage.tool_calls = 1;
        after_tools.continuation.push(phenix_core::ModelToolTurn {
            assistant_output: Bytes::from(b"invoke tool".to_vec()),
            tool_calls: vec![call.clone()],
            tool_results: vec![phenix_core::ModelToolResult {
                call_id: call.call_id,
                callable_id: call.callable_id,
                output: PhenixValue::Unit,
                is_error: false,
            }],
        });
        let result = AgentToolBatchResponse::Continue {
            state: after_tools.clone(),
        };
        let encoded = serde_json::to_vec(&PhenixValue::from(&result)).unwrap();
        assert_eq!(run.project("tool_batch", &encoded).unwrap(), "continue");
        assert!(run.pending.is_none());
        let new_turn = run.prepare("turn").unwrap();
        let decoded: PhenixValue = serde_json::from_slice(&new_turn).unwrap();
        let request = AgentTurnStepRequest::try_from(Project(&decoded)).unwrap();
        assert_eq!(request.state.usage.tool_calls, 1);
    }

    #[test]
    fn a_node_cannot_change_immutable_execution_identity() {
        let mut run = TurnRun::from_command(AgentLoopCommand::Run {
            execution_id: "original-execution".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: None,
            input: Bytes::from(b"hello".to_vec()),
            tools: Vec::new(),
        });
        let mut forged = run.state.clone();
        forged.execution_id = "another-execution".into();
        let response = AgentTurnStepResponse::Final {
            state: forged,
            output: Bytes::from(b"untrusted".to_vec()),
        };
        let bytes = serde_json::to_vec(&PhenixValue::from(&response)).unwrap();
        assert!(
            run.project("turn", &bytes)
                .unwrap_err()
                .contains("immutable execution identity")
        );
        assert_eq!(run.state.execution_id, "original-execution");
        assert!(run.terminal.is_none());
    }

    #[test]
    fn a_tool_batch_cannot_reset_usage_after_a_model_turn() {
        use phenix_core::CallableId;

        let mut run = TurnRun::from_command(AgentLoopCommand::Run {
            execution_id: "usage-test".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: None,
            input: Bytes::from(b"hello".to_vec()),
            tools: Vec::new(),
        });
        let mut after_model = run.state.clone();
        after_model.usage.model_calls = 1;
        after_model.seen_tool_call_ids = vec!["call-1".into()];
        let turn = AgentTurnStepResponse::ToolCalls {
            state: after_model,
            assistant_output: Bytes::from(b"tool please".to_vec()),
            tool_calls: vec![ModelToolCall {
                call_id: "call-1".into(),
                callable_id: CallableId::parse("fixture.tool").unwrap(),
                input: PhenixValue::Unit,
            }],
        };
        let bytes = serde_json::to_vec(&PhenixValue::from(&turn)).unwrap();
        assert_eq!(run.project("turn", &bytes).unwrap(), "tool_calls");

        let reverted = AgentToolBatchResponse::Continue {
            state: AgentTurnState {
                usage: AgentLoopUsage {
                    model_calls: 0,
                    tool_calls: 0,
                },
                ..run.state.clone()
            },
        };
        let bytes = serde_json::to_vec(&PhenixValue::from(&reverted)).unwrap();
        assert!(
            run.project("tool_batch", &bytes)
                .unwrap_err()
                .contains("regressed execution usage")
        );
        assert_eq!(run.state.usage.model_calls, 1);
        assert!(run.pending.is_some());
    }

    #[test]
    fn a_replacement_turn_provider_cannot_reuse_a_tool_call_id() {
        use phenix_core::CallableId;

        let mut run = TurnRun::from_command(AgentLoopCommand::Run {
            execution_id: "reused-id".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: None,
            input: Bytes::from(b"hello".to_vec()),
            tools: Vec::new(),
        });
        run.state.seen_tool_call_ids = vec!["already-executed".into()];
        run.state.usage.model_calls = 1;
        let mut after = run.state.clone();
        after.usage.model_calls = 2;
        let response = AgentTurnStepResponse::ToolCalls {
            state: after,
            assistant_output: Bytes::from(b"tool please".to_vec()),
            tool_calls: vec![ModelToolCall {
                call_id: "already-executed".into(),
                callable_id: CallableId::parse("fixture.tool").unwrap(),
                input: PhenixValue::Unit,
            }],
        };
        let bytes = serde_json::to_vec(&PhenixValue::from(&response)).unwrap();
        assert!(
            run.project("turn", &bytes)
                .unwrap_err()
                .contains("duplicate tool call id")
        );
        assert_eq!(run.state.usage.model_calls, 1);
        assert!(run.pending.is_none());
    }

    #[test]
    fn a_replacement_turn_provider_must_record_exact_new_call_ids() {
        use phenix_core::CallableId;

        let mut run = TurnRun::from_command(AgentLoopCommand::Run {
            execution_id: "missing-provenance".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: None,
            input: Bytes::from(b"hello".to_vec()),
            tools: Vec::new(),
        });
        let mut after = run.state.clone();
        after.usage.model_calls = 1;
        let response = AgentTurnStepResponse::ToolCalls {
            state: after,
            assistant_output: Bytes::from(b"tool please".to_vec()),
            tool_calls: vec![ModelToolCall {
                call_id: "new-call".into(),
                callable_id: CallableId::parse("fixture.tool").unwrap(),
                input: PhenixValue::Unit,
            }],
        };
        let bytes = serde_json::to_vec(&PhenixValue::from(&response)).unwrap();
        assert!(
            run.project("turn", &bytes)
                .unwrap_err()
                .contains("execution history")
        );
        assert!(run.pending.is_none());
    }

    #[test]
    fn a_replacement_turn_provider_cannot_reorder_new_call_id_history() {
        use phenix_core::CallableId;

        let mut run = TurnRun::from_command(AgentLoopCommand::Run {
            execution_id: "ordered-model-history".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: None,
            input: Bytes::from(b"hello".to_vec()),
            tools: Vec::new(),
        });
        let calls = ["first", "second"]
            .into_iter()
            .map(|id| ModelToolCall {
                call_id: id.into(),
                callable_id: CallableId::parse("fixture.tool").unwrap(),
                input: PhenixValue::Unit,
            })
            .collect::<Vec<_>>();
        let mut forged = run.state.clone();
        forged.usage.model_calls = 1;
        forged.seen_tool_call_ids = vec!["second".into(), "first".into()];
        let response = AgentTurnStepResponse::ToolCalls {
            state: forged,
            assistant_output: Bytes::from(b"tools".to_vec()),
            tool_calls: calls,
        };
        let bytes = serde_json::to_vec(&PhenixValue::from(&response)).unwrap();
        assert!(
            run.project("turn", &bytes)
                .unwrap_err()
                .contains("reordered admitted tool-call identities")
        );
        assert!(run.pending.is_none());
        assert!(run.state.seen_tool_call_ids.is_empty());
    }

    #[test]
    fn a_replacement_tool_provider_cannot_reorder_existing_call_ids() {
        let mut run = TurnRun::from_command(AgentLoopCommand::Run {
            execution_id: "ordered-tool-history".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: None,
            input: Bytes::from(b"hello".to_vec()),
            tools: Vec::new(),
        });
        run.state.seen_tool_call_ids = vec!["first".into(), "second".into()];
        let mut forged = run.state.clone();
        forged.seen_tool_call_ids.reverse();
        let response = AgentToolBatchResponse::Continue { state: forged };
        let bytes = serde_json::to_vec(&PhenixValue::from(&response)).unwrap();
        assert!(
            run.project("tool_batch", &bytes)
                .unwrap_err()
                .contains("model tool-call history")
        );
        assert_eq!(run.state.seen_tool_call_ids, ["first", "second"]);
    }

    #[test]
    fn a_cancelled_tool_batch_cannot_forge_continuation_or_completed_usage() {
        use phenix_core::{CallableId, ModelToolTurn};

        let mut run = TurnRun::from_command(AgentLoopCommand::Run {
            execution_id: "cancelled-batch".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: None,
            input: Bytes::from(b"hello".to_vec()),
            tools: Vec::new(),
        });
        let call = ModelToolCall {
            call_id: "call-1".into(),
            callable_id: CallableId::parse("fixture.tool").unwrap(),
            input: PhenixValue::Unit,
        };
        run.state.seen_tool_call_ids.push(call.call_id.clone());
        run.state.usage.model_calls = 1;
        run.pending = Some((Bytes::from(b"tool".to_vec()), vec![call]));

        let mut forged = run.state.clone();
        forged.continuation.push(ModelToolTurn {
            assistant_output: Bytes::from(b"fabricated".to_vec()),
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
        });
        let bytes = serde_json::to_vec(&PhenixValue::from(&AgentToolBatchResponse::Cancelled {
            state: forged,
        }))
        .unwrap();
        assert!(
            run.project("tool_batch", &bytes)
                .unwrap_err()
                .contains("fabricated continuation")
        );
        assert!(run.terminal.is_none());
        assert!(run.pending.is_some());

        let mut forged = run.state.clone();
        forged.usage.tool_calls = 2;
        let bytes = serde_json::to_vec(&PhenixValue::from(&AgentToolBatchResponse::Cancelled {
            state: forged,
        }))
        .unwrap();
        assert!(
            run.project("tool_batch", &bytes)
                .unwrap_err()
                .contains("unadmitted tool usage")
        );
        assert!(run.terminal.is_none());

        let mut inserted_without_execution = run.state.clone();
        inserted_without_execution
            .tools
            .push(phenix_core::ModelToolDescriptor {
                id: CallableId::parse("fixture.inserted").unwrap(),
                description: "tool that has not been activated".into(),
                input_schema: phenix_core::PhenixSchema::Any,
                output_schema: phenix_core::PhenixSchema::Any,
            });
        let bytes = serde_json::to_vec(&PhenixValue::from(&AgentToolBatchResponse::Cancelled {
            state: inserted_without_execution,
        }))
        .unwrap();
        assert!(
            run.project("tool_batch", &bytes)
                .unwrap_err()
                .contains("without completed calls")
        );
        assert!(run.pending.is_some());

        let mut partial = run.state.clone();
        partial.usage.tool_calls = 1;
        let bytes = serde_json::to_vec(&PhenixValue::from(&AgentToolBatchResponse::Cancelled {
            state: partial,
        }))
        .unwrap();
        assert_eq!(run.project("tool_batch", &bytes).unwrap(), "cancelled");
        assert!(run.pending.is_none());
        assert!(matches!(
            run.terminal,
            Some(AgentLoopResponse::Cancelled { .. })
        ));
    }

    #[test]
    fn a_cancelled_tool_batch_cannot_delete_unrelated_observations() {
        use phenix_core::{ArtifactRevision, CallableId};
        use phenix_sdk::{
            ToolObservation, ToolObservationExactSource, ToolObservationInvalidation,
            ToolObservationStatus,
        };

        let mut run = TurnRun::from_command(AgentLoopCommand::Run {
            execution_id: "cancelled-observations".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: None,
            input: Bytes::from(b"hello".to_vec()),
            tools: Vec::new(),
        });
        let previous = CallableId::parse("fixture.previous").unwrap();
        run.state.observations.insert(
            previous.clone(),
            ToolObservation {
                occurrence_id: "previous-call".into(),
                status: ToolObservationStatus::Succeeded,
                model_view: PhenixValue::Unit,
                model_view_bytes: 0,
                model_view_complete: true,
                content_identity: ArtifactRevision::from_content(b"previous"),
                exact_source: ToolObservationExactSource::Unavailable {
                    reason: "test fixture".into(),
                },
                source_revision: None,
                invalidation: ToolObservationInvalidation::Volatile,
            },
        );
        let call = ModelToolCall {
            call_id: "current-call".into(),
            callable_id: CallableId::parse("fixture.current").unwrap(),
            input: PhenixValue::Unit,
        };
        run.state.seen_tool_call_ids.push(call.call_id.clone());
        run.state.usage.model_calls = 1;
        run.pending = Some((Bytes::from(b"tool".to_vec()), vec![call]));

        let mut forged = run.state.clone();
        forged.observations.remove(&previous);
        let response = AgentToolBatchResponse::Cancelled { state: forged };
        let bytes = serde_json::to_vec(&PhenixValue::from(&response)).unwrap();
        assert!(
            run.project("tool_batch", &bytes)
                .unwrap_err()
                .contains("removed recorded observations")
        );
        assert!(run.pending.is_some());
        assert!(run.terminal.is_none());
        assert!(run.state.observations.contains_key(&previous));

        let current = CallableId::parse("fixture.current").unwrap();
        let original = run.state.observations.get(&previous).unwrap().clone();
        let mut forged = run.state.clone();
        forged.observations.insert(
            current.clone(),
            ToolObservation {
                occurrence_id: "unadmitted-call".into(),
                ..original.clone()
            },
        );
        let response = AgentToolBatchResponse::Cancelled { state: forged };
        let bytes = serde_json::to_vec(&PhenixValue::from(&response)).unwrap();
        assert!(
            run.project("tool_batch", &bytes)
                .unwrap_err()
                .contains("no matching admitted call")
        );
        assert!(run.terminal.is_none());

        // A partial batch cannot claim the second call's observation when
        // only the first call has completed.
        run.state.seen_tool_call_ids.push("second-call".into());
        run.pending.as_mut().unwrap().1.push(ModelToolCall {
            call_id: "second-call".into(),
            callable_id: CallableId::parse("fixture.second").unwrap(),
            input: PhenixValue::Unit,
        });
        let mut future_observation = run.state.clone();
        future_observation.usage.tool_calls = 1;
        future_observation.observations.insert(
            CallableId::parse("fixture.second").unwrap(),
            ToolObservation {
                occurrence_id: "second-call".into(),
                ..original.clone()
            },
        );
        let bytes = serde_json::to_vec(&PhenixValue::from(&AgentToolBatchResponse::Cancelled {
            state: future_observation,
        }))
        .unwrap();
        assert!(
            run.project("tool_batch", &bytes)
                .unwrap_err()
                .contains("uncompleted call")
        );

        // A provider cannot invent observations when it reports no tool effects.
        let mut no_usage = run.state.clone();
        no_usage.observations.insert(
            current.clone(),
            ToolObservation {
                occurrence_id: "current-call".into(),
                ..original.clone()
            },
        );
        let bytes = serde_json::to_vec(&PhenixValue::from(&AgentToolBatchResponse::Cancelled {
            state: no_usage,
        }))
        .unwrap();
        assert!(
            run.project("tool_batch", &bytes)
                .unwrap_err()
                .contains("exceed completed tool usage")
        );

        // An observation with the pending call's provenance is admissible,
        // including when the batch terminates before writing a continuation.
        let mut partial = run.state.clone();
        partial.usage.tool_calls = 1;
        partial.observations.insert(
            current,
            ToolObservation {
                occurrence_id: "current-call".into(),
                ..original
            },
        );
        let bytes = serde_json::to_vec(&PhenixValue::from(&AgentToolBatchResponse::Cancelled {
            state: partial,
        }))
        .unwrap();
        assert_eq!(run.project("tool_batch", &bytes).unwrap(), "cancelled");
        assert!(run.pending.is_none());
    }

    #[test]
    fn a_cancelled_turn_cannot_forge_tool_call_history() {
        let mut run = TurnRun::from_command(AgentLoopCommand::Run {
            execution_id: "cancelled-history".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: None,
            input: Bytes::from(b"hello".to_vec()),
            tools: Vec::new(),
        });
        let mut forged = run.state.clone();
        forged.seen_tool_call_ids.push("never-executed".into());
        let response = AgentTurnStepResponse::Cancelled { state: forged };
        let bytes = serde_json::to_vec(&PhenixValue::from(&response)).unwrap();
        assert!(
            run.project("turn", &bytes)
                .unwrap_err()
                .contains("cancelled turn-step changed tool-call history")
        );
        assert!(run.terminal.is_none());
    }

    #[test]
    fn a_replacement_tool_provider_cannot_discard_recorded_call_ids() {
        let mut run = TurnRun::from_command(AgentLoopCommand::Run {
            execution_id: "history-check".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: None,
            input: Bytes::from(b"hello".to_vec()),
            tools: Vec::new(),
        });
        run.state.seen_tool_call_ids = vec!["already-admitted".into()];
        let mut tampered = run.state.clone();
        tampered.seen_tool_call_ids.clear();
        let response = AgentToolBatchResponse::Continue { state: tampered };
        let bytes = serde_json::to_vec(&PhenixValue::from(&response)).unwrap();
        assert!(
            run.project("tool_batch", &bytes)
                .unwrap_err()
                .contains("recorded tool-call ids")
        );
        assert_eq!(run.state.seen_tool_call_ids, ["already-admitted"]);
    }

    #[test]
    fn a_replacement_tool_provider_cannot_forge_model_usage_or_call_ids() {
        let mut run = TurnRun::from_command(AgentLoopCommand::Run {
            execution_id: "tool-boundary".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: None,
            input: Bytes::from(b"hello".to_vec()),
            tools: Vec::new(),
        });
        let mut tampered = run.state.clone();
        tampered.seen_tool_call_ids.push("not-from-model".into());
        let response = AgentToolBatchResponse::Continue { state: tampered };
        let bytes = serde_json::to_vec(&PhenixValue::from(&response)).unwrap();
        assert!(
            run.project("tool_batch", &bytes)
                .unwrap_err()
                .contains("model tool-call history")
        );

        let mut tampered = run.state.clone();
        tampered.usage.model_calls = 1;
        let response = AgentToolBatchResponse::Continue { state: tampered };
        let bytes = serde_json::to_vec(&PhenixValue::from(&response)).unwrap();
        assert!(
            run.project("tool_batch", &bytes)
                .unwrap_err()
                .contains("model usage")
        );
    }

    #[test]
    fn a_replacement_tool_provider_must_complete_the_exact_pending_batch() {
        use phenix_core::{CallableId, ModelToolResult, ModelToolTurn};

        let mut run = TurnRun::from_command(AgentLoopCommand::Run {
            execution_id: "batch-effects".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: None,
            input: Bytes::from(b"hello".to_vec()),
            tools: Vec::new(),
        });
        let call = ModelToolCall {
            call_id: "call-1".into(),
            callable_id: CallableId::parse("fixture.tool").unwrap(),
            input: PhenixValue::Unit,
        };
        run.state.seen_tool_call_ids.push(call.call_id.clone());
        run.state.usage.model_calls = 1;
        let assistant_output = Bytes::from(b"call fixture.tool".to_vec());
        run.pending = Some((assistant_output.clone(), vec![call.clone()]));

        let response = AgentToolBatchResponse::Continue {
            state: run.state.clone(),
        };
        let bytes = serde_json::to_vec(&PhenixValue::from(&response)).unwrap();
        assert!(
            run.project("tool_batch", &bytes)
                .unwrap_err()
                .contains("continuation history")
        );

        let mut forged = run.state.clone();
        forged.usage.tool_calls = 1;
        forged.continuation.push(ModelToolTurn {
            assistant_output,
            tool_calls: vec![call],
            tool_results: vec![ModelToolResult {
                call_id: "different-call".into(),
                callable_id: CallableId::parse("fixture.tool").unwrap(),
                output: PhenixValue::Unit,
                is_error: false,
            }],
        });
        let response = AgentToolBatchResponse::Continue { state: forged };
        let bytes = serde_json::to_vec(&PhenixValue::from(&response)).unwrap();
        assert!(
            run.project("tool_batch", &bytes)
                .unwrap_err()
                .contains("mismatched tool results")
        );
        assert!(run.pending.is_some());
    }

    #[test]
    fn a_replacement_turn_provider_cannot_forge_completed_tools() {
        let mut run = TurnRun::from_command(AgentLoopCommand::Run {
            execution_id: "model-boundary".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: None,
            input: Bytes::from(b"hello".to_vec()),
            tools: Vec::new(),
        });
        let mut tampered = run.state.clone();
        tampered.usage.model_calls = 1;
        tampered.usage.tool_calls = 1;
        let response = AgentTurnStepResponse::Final {
            state: tampered,
            output: Bytes::from(b"fabricated".to_vec()),
        };
        let bytes = serde_json::to_vec(&PhenixValue::from(&response)).unwrap();
        assert!(
            run.project("turn", &bytes)
                .unwrap_err()
                .contains("tool-execution-owned state")
        );
        assert!(run.terminal.is_none());
    }

    #[test]
    fn a_final_turn_cannot_add_unexecuted_tool_call_ids() {
        let mut run = TurnRun::from_command(AgentLoopCommand::Run {
            execution_id: "final-boundary".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: None,
            input: Bytes::from(b"hello".to_vec()),
            tools: Vec::new(),
        });
        let mut tampered = run.state.clone();
        tampered.seen_tool_call_ids = vec!["phantom-call".into()];
        tampered.usage.model_calls = 1;
        let response = AgentTurnStepResponse::Final {
            state: tampered,
            output: Bytes::from(b"finished".to_vec()),
        };
        let bytes = serde_json::to_vec(&PhenixValue::from(&response)).unwrap();
        assert!(
            run.project("turn", &bytes)
                .unwrap_err()
                .contains("invented tool-call history")
        );
        assert!(run.terminal.is_none());
    }

    #[test]
    fn a_replacement_provider_cannot_rewrite_original_input() {
        let mut run = TurnRun::from_command(AgentLoopCommand::Run {
            execution_id: "immutable-input".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: None,
            input: Bytes::from(b"original".to_vec()),
            tools: Vec::new(),
        });
        let mut after = run.state.clone();
        after.input = Bytes::from(b"substituted".to_vec());
        let response = AgentTurnStepResponse::Final {
            state: after,
            output: Bytes::from(b"finished".to_vec()),
        };
        let bytes = serde_json::to_vec(&PhenixValue::from(&response)).unwrap();
        assert!(
            run.project("turn", &bytes)
                .unwrap_err()
                .contains("original request input")
        );
        assert!(run.terminal.is_none());
    }

    #[test]
    fn replacement_turn_cannot_forge_model_usage_or_failure_classification() {
        use phenix_sdk::AgentLoopFailure;

        let mut run = TurnRun::from_command(AgentLoopCommand::Run {
            execution_id: "model-usage-boundary".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: None,
            input: Bytes::from(b"hello".to_vec()),
            tools: Vec::new(),
        });

        let mut forged = run.state.clone();
        forged.usage.model_calls = 3;
        let bytes = serde_json::to_vec(&PhenixValue::from(&AgentTurnStepResponse::Final {
            state: forged,
            output: Bytes::from(b"done".to_vec()),
        }))
        .unwrap();
        assert!(
            run.project("turn", &bytes)
                .unwrap_err()
                .contains("inconsistent model usage")
        );
        assert!(run.terminal.is_none());

        let mut forged = run.state.clone();
        forged.usage.model_calls = 1;
        let bytes = serde_json::to_vec(&PhenixValue::from(&AgentTurnStepResponse::Failed {
            state: forged,
            failure: AgentLoopFailure::ModelTurnLimitExceeded { limit: 1 },
        }))
        .unwrap();
        assert!(
            run.project("turn", &bytes)
                .unwrap_err()
                .contains("inconsistent model usage")
        );
        assert!(run.terminal.is_none());
    }

    #[test]
    fn typed_turn_failures_preserve_exact_model_usage() {
        use phenix_sdk::AgentLoopFailure;

        let command = AgentLoopCommand::Run {
            execution_id: "failure-usage".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: None,
            input: Bytes::from(b"hello".to_vec()),
            tools: Vec::new(),
        };
        let mut before_model = TurnRun::from_command(command.clone());
        before_model.state.usage.model_calls = 1;
        let bytes = serde_json::to_vec(&PhenixValue::from(&AgentTurnStepResponse::Failed {
            state: before_model.state.clone(),
            failure: AgentLoopFailure::ModelTurnLimitExceeded { limit: 1 },
        }))
        .unwrap();
        assert_eq!(before_model.project("turn", &bytes).unwrap(), "failed");
        assert!(matches!(
            before_model.terminal,
            Some(AgentLoopResponse::Failed { .. })
        ));
        assert_eq!(before_model.state.usage.model_calls, 1);

        let mut after_model = TurnRun::from_command(command);
        let mut state = after_model.state.clone();
        state.usage.model_calls = 1;
        state.seen_tool_call_ids = vec!["call-a".into(), "call-b".into()];
        let bytes = serde_json::to_vec(&PhenixValue::from(&AgentTurnStepResponse::Failed {
            state,
            failure: AgentLoopFailure::ToolCallLimitExceeded {
                limit: 1,
                actual: 2,
            },
        }))
        .unwrap();
        assert_eq!(after_model.project("turn", &bytes).unwrap(), "failed");
        assert!(matches!(
            after_model.terminal,
            Some(AgentLoopResponse::Failed { .. })
        ));
        assert_eq!(after_model.state.usage.model_calls, 1);
    }

    #[test]
    fn terminal_limit_failures_cannot_forge_history_or_call_counts() {
        use phenix_sdk::AgentLoopFailure;

        let mut run = TurnRun::from_command(AgentLoopCommand::Run {
            execution_id: "failed-history-check".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: None,
            input: Bytes::from(b"hello".to_vec()),
            tools: Vec::new(),
        });

        let bytes = serde_json::to_vec(&PhenixValue::from(&AgentTurnStepResponse::Failed {
            state: run.state.clone(),
            failure: AgentLoopFailure::ModelTurnLimitExceeded { limit: 1 },
        }))
        .unwrap();
        assert!(
            run.project("turn", &bytes)
                .unwrap_err()
                .contains("before limit")
        );
        assert!(run.terminal.is_none());

        run.state.usage.model_calls = 1;
        let mut pre_model = run.state.clone();
        pre_model.seen_tool_call_ids.push("imaginary-call".into());
        let bytes = serde_json::to_vec(&PhenixValue::from(&AgentTurnStepResponse::Failed {
            state: pre_model,
            failure: AgentLoopFailure::ModelTurnLimitExceeded { limit: 1 },
        }))
        .unwrap();
        assert!(
            run.project("turn", &bytes)
                .unwrap_err()
                .contains("pre-model failure invented tool-call history")
        );
        assert!(run.terminal.is_none());

        // The next subcase starts a fresh model-turn accounting baseline.
        run.state.usage.model_calls = 0;
        let mut post_model = run.state.clone();
        post_model.usage.model_calls = 1;
        post_model.seen_tool_call_ids = vec!["call-a".into(), "call-b".into()];
        let bytes = serde_json::to_vec(&PhenixValue::from(&AgentTurnStepResponse::Failed {
            state: post_model,
            failure: AgentLoopFailure::ToolCallLimitExceeded {
                limit: 1,
                actual: 3,
            },
        }))
        .unwrap();
        assert!(
            run.project("turn", &bytes)
                .unwrap_err()
                .contains("inconsistent tool-call history")
        );
        assert!(run.terminal.is_none());
    }

    #[test]
    fn contracts_drive_branching_and_state_transfer() {
        let mut run = TurnRun::from_command(AgentLoopCommand::Run {
            execution_id: "fixture-1".into(),
            session_id: None,
            parent_attempt_id: None,
            callable_id: None,
            input: Bytes::from(b"hello".to_vec()),
            tools: Vec::new(),
        });
        let decoded: PhenixValue = serde_json::from_slice(&run.prepare("turn").unwrap()).unwrap();
        let request = AgentTurnStepRequest::try_from(Project(&decoded)).unwrap();
        assert_eq!(request.state.execution_id, "fixture-1");
        let mut completed = run.state.clone();
        completed.usage.model_calls = 1;
        let response = AgentTurnStepResponse::Final {
            state: completed,
            output: Bytes::from(b"finished".to_vec()),
        };
        let encoded = serde_json::to_vec(&PhenixValue::from(&response)).unwrap();
        assert_eq!(run.project("turn", &encoded).unwrap(), "final");
        assert!(matches!(
            run.terminal,
            Some(AgentLoopResponse::Completed { .. })
        ));
    }
}
