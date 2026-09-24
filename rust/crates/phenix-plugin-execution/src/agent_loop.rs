use crate::{
    agent_loop_component_id, AgentLoopControlInterface, AgentLoopInterface,
    AgentLoopProgressInterface, AgentToolExecutionInterface,
};
use phenix_core::{
    Authority, Bytes, CallableId, ComponentInterface, ModelToolCall, ModelToolDescriptor,
    ModelToolResult, ModelToolTurn, PluginContext, PluginExecution,
    PluginHost, PluginId, PluginInstance, PluginManifest, SdkClient, ServiceContribution,
    ServiceId, ServiceRole, SessionId,
};
use phenix_sdk::{
    DefaultInvocationCommand, DefaultInvocationInterface, InvocationRequest, StepRunnerResponse,
};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

pub const AGENT_LOOP_PLUGIN: &str = "phenix.agent-loop";
pub const AGENT_LOOP_SERVICE: &str = "phenix.agent-loop@1";
pub const AGENT_TOOL_EXECUTION_SERVICE: &str = "phenix.agent-tool-execution@1";
pub const AGENT_LOOP_PROGRESS_SERVICE: &str = "phenix.agent-loop-progress@1";
pub const AGENT_LOOP_CONTROL_SERVICE: &str = "phenix.agent-loop-control@1";
pub const DEFAULT_MAX_MODEL_TURNS: u32 = 16;
pub const DEFAULT_MAX_TOOL_CALLS_PER_TURN: u32 = 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentLoopPolicy {
    max_model_turns: NonZeroU32,
    max_tool_calls_per_turn: NonZeroU32,
}

impl AgentLoopPolicy {
    #[must_use]
    pub const fn new(max_model_turns: NonZeroU32, max_tool_calls_per_turn: NonZeroU32) -> Self {
        Self {
            max_model_turns,
            max_tool_calls_per_turn,
        }
    }

    #[must_use]
    pub const fn max_model_turns(self) -> NonZeroU32 {
        self.max_model_turns
    }

    #[must_use]
    pub const fn max_tool_calls_per_turn(self) -> NonZeroU32 {
        self.max_tool_calls_per_turn
    }
}

impl Default for AgentLoopPolicy {
    fn default() -> Self {
        Self::new(
            NonZeroU32::new(DEFAULT_MAX_MODEL_TURNS).expect("default model-turn limit is non-zero"),
            NonZeroU32::new(DEFAULT_MAX_TOOL_CALLS_PER_TURN)
                .expect("default per-turn tool-call limit is non-zero"),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentLoopCommand {
    Run {
        execution_id: String,
        session_id: Option<SessionId>,
        parent_attempt_id: Option<String>,
        callable_id: Option<CallableId>,
        input: Bytes,
        #[serde(default)]
        tools: Vec<ModelToolDescriptor>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct AgentLoopUsage {
    pub model_calls: u32,
    pub tool_calls: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "failure", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentLoopFailure {
    ModelTurnLimitExceeded { limit: u32 },
    ToolCallLimitExceeded { limit: u32, actual: u32 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentLoopResponse {
    Completed {
        output: Bytes,
        usage: AgentLoopUsage,
    },
    Cancelled {
        usage: AgentLoopUsage,
    },
    Failed {
        failure: AgentLoopFailure,
        usage: AgentLoopUsage,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct AgentLoopControlRequest {
    pub execution_id: String,
    pub session_id: Option<SessionId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentLoopControlResponse {
    Continue,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct AgentToolExecutionRequest {
    pub execution_id: String,
    pub session_id: Option<SessionId>,
    pub call: ModelToolCall,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentToolExecutionResponse {
    Completed { result: ModelToolResult },
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "progress", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentLoopProgress {
    ToolCall { call: ModelToolCall },
    ToolResult { result: ModelToolResult },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct AgentLoopProgressRecord {
    pub execution_id: String,
    pub session_id: Option<SessionId>,
    pub progress: AgentLoopProgress,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentLoopProgressResponse {
    Recorded,
}

#[must_use]
pub fn agent_loop_service() -> ServiceId {
    ServiceId::parse(AGENT_LOOP_SERVICE).expect("static agent loop service id is valid")
}

#[must_use]
pub fn agent_tool_execution_service() -> ServiceId {
    ServiceId::parse(AGENT_TOOL_EXECUTION_SERVICE)
        .expect("static agent tool execution service id is valid")
}

#[must_use]
pub fn agent_loop_progress_service() -> ServiceId {
    ServiceId::parse(AGENT_LOOP_PROGRESS_SERVICE)
        .expect("static agent loop progress service id is valid")
}

#[must_use]
pub fn agent_loop_control_service() -> ServiceId {
    ServiceId::parse(AGENT_LOOP_CONTROL_SERVICE)
        .expect("static agent loop control service id is valid")
}

#[must_use]
pub fn agent_loop_manifest(maximum_authority: Authority) -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(AGENT_LOOP_PLUGIN).expect("static agent loop plugin id is valid"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: ServiceRole::Terminal,
            service: agent_loop_service(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority,
    }
}

#[must_use]
pub fn agent_loop_factory() -> Box<dyn PluginInstance> {
    agent_loop_factory_with_policy(AgentLoopPolicy::default())
}

#[must_use]
pub fn agent_loop_factory_with_policy(policy: AgentLoopPolicy) -> Box<dyn PluginInstance> {
    Box::new(AgentLoopPlugin { policy })
}

struct AgentLoopSdk<'host, 'runtime> {
    invocation: SdkClient<'host, 'runtime, DefaultInvocationInterface>,
    control: SdkClient<'host, 'runtime, AgentLoopControlInterface>,
    tools: SdkClient<'host, 'runtime, AgentToolExecutionInterface>,
    progress: SdkClient<'host, 'runtime, AgentLoopProgressInterface>,
}

type AgentLoopContext<'host, 'runtime> =
    PluginContext<'host, 'runtime, AgentLoopSdk<'host, 'runtime>>;

fn context<'host, 'runtime>(
    host: &'host PluginHost<'runtime>,
) -> AgentLoopContext<'host, 'runtime> {
    PluginContext::new(
        host,
        AgentLoopSdk {
            invocation: SdkClient::new(host, agent_loop_component_id()),
            control: SdkClient::new(host, agent_loop_component_id()),
            tools: SdkClient::new(host, agent_loop_component_id()),
            progress: SdkClient::new(host, agent_loop_component_id()),
        },
        (),
        (),
    )
}

struct AgentLoopPlugin {
    policy: AgentLoopPolicy,
}

impl PluginInstance for AgentLoopPlugin {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &agent_loop_service() {
            return Err(format!("unsupported agent loop service: {service}"));
        }
        let context = context(host);
        let interface = AgentLoopInterface::interface_id();
        let command = context
            .kernel
            .decode_projected::<AgentLoopCommand>(&interface, input)
            .map_err(|error| error.to_string())?;
        let response = handle(&context, self.policy, command)?;
        context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string())
    }
}

fn handle(
    context: &AgentLoopContext<'_, '_>,
    policy: AgentLoopPolicy,
    command: AgentLoopCommand,
) -> Result<AgentLoopResponse, String> {
    match command {
        AgentLoopCommand::Run {
            execution_id,
            session_id,
            parent_attempt_id,
            callable_id,
            input,
            tools,
        } => run(
            context,
            policy,
            execution_id,
            session_id,
            parent_attempt_id,
            callable_id,
            input,
            tools,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    context: &AgentLoopContext<'_, '_>,
    policy: AgentLoopPolicy,
    execution_id: String,
    session_id: Option<SessionId>,
    parent_attempt_id: Option<String>,
    callable_id: Option<CallableId>,
    input: Bytes,
    tools: Vec<ModelToolDescriptor>,
) -> Result<AgentLoopResponse, String> {
    let mut continuation = Vec::<ModelToolTurn>::new();
    let mut usage = AgentLoopUsage {
        model_calls: 0,
        tool_calls: 0,
    };

    for _ in 0..policy.max_model_turns().get() {
        if context
            .kernel
            .cancellation_token()
            .is_some_and(|token| token.is_cancelled())
        {
            return Ok(AgentLoopResponse::Cancelled { usage });
        }
        let control: AgentLoopControlResponse = context
            .sdk
            .control
            .invoke_projected(&AgentLoopControlRequest {
                execution_id: execution_id.clone(),
                session_id: session_id.clone(),
            })
            .map_err(|error| error.to_string())?;
        if matches!(control, AgentLoopControlResponse::Cancelled) {
            return Ok(AgentLoopResponse::Cancelled { usage });
        }

        let response = context
            .sdk
            .invocation
            .invoke_projected(&DefaultInvocationCommand::Invoke {
                request: InvocationRequest {
                    execution_id: execution_id.clone(),
                    session_id: session_id.clone(),
                    parent_attempt_id: parent_attempt_id.clone(),
                    callable_id: callable_id.clone(),
                    input: input.clone(),
                    tools: tools.clone(),
                    continuation: continuation.clone(),
                },
            })
            .map_err(|error| error.to_string())?;
        usage.model_calls = usage
            .model_calls
            .checked_add(1)
            .ok_or_else(|| "agent loop model-call usage overflowed".to_owned())?;

        let StepRunnerResponse::Completed {
            output, tool_calls, ..
        } = response;
        if tool_calls.is_empty() {
            return Ok(AgentLoopResponse::Completed { output, usage });
        }

        let actual = u32::try_from(tool_calls.len())
            .map_err(|_| "model returned too many tool calls to represent".to_owned())?;
        let limit = policy.max_tool_calls_per_turn().get();
        if actual > limit {
            return Ok(AgentLoopResponse::Failed {
                failure: AgentLoopFailure::ToolCallLimitExceeded { limit, actual },
                usage,
            });
        }

        let mut tool_results = Vec::with_capacity(tool_calls.len());
        for call in &tool_calls {
            emit_progress(
                context,
                AgentLoopProgressRecord {
                    execution_id: execution_id.clone(),
                    session_id: session_id.clone(),
                    progress: AgentLoopProgress::ToolCall { call: call.clone() },
                },
            )?;

            let response: AgentToolExecutionResponse = context
                .sdk
                .tools
                .invoke_projected(&AgentToolExecutionRequest {
                    execution_id: execution_id.clone(),
                    session_id: session_id.clone(),
                    call: call.clone(),
                })
                .map_err(|error| error.to_string())?;

            let result = match response {
                AgentToolExecutionResponse::Completed { result } => result,
                AgentToolExecutionResponse::Cancelled => {
                    return Ok(AgentLoopResponse::Cancelled { usage });
                }
            };
            if result.call_id != call.call_id || result.callable_id != call.callable_id {
                return Err(format!(
                    "tool executor changed call identity for {}",
                    call.call_id
                ));
            }
            usage.tool_calls = usage
                .tool_calls
                .checked_add(1)
                .ok_or_else(|| "agent loop tool-call usage overflowed".to_owned())?;
            emit_progress(
                context,
                AgentLoopProgressRecord {
                    execution_id: execution_id.clone(),
                    session_id: session_id.clone(),
                    progress: AgentLoopProgress::ToolResult {
                        result: result.clone(),
                    },
                },
            )?;
            tool_results.push(result);
        }

        continuation.push(ModelToolTurn {
            assistant_output: output,
            tool_calls,
            tool_results,
        });
    }

    Ok(AgentLoopResponse::Failed {
        failure: AgentLoopFailure::ModelTurnLimitExceeded {
            limit: policy.max_model_turns().get(),
        },
        usage,
    })
}

fn emit_progress(
    context: &AgentLoopContext<'_, '_>,
    record: AgentLoopProgressRecord,
) -> Result<(), String> {
    let response: AgentLoopProgressResponse = context
        .sdk
        .progress
        .invoke_projected(&record)
        .map_err(|error| error.to_string())?;
    match response {
        AgentLoopProgressResponse::Recorded => Ok(()),
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_progression_policy_preserves_existing_limits() {
        let policy = AgentLoopPolicy::default();
        assert_eq!(policy.max_model_turns().get(), DEFAULT_MAX_MODEL_TURNS);
        assert_eq!(
            policy.max_tool_calls_per_turn().get(),
            DEFAULT_MAX_TOOL_CALLS_PER_TURN
        );
        assert_eq!(DEFAULT_MAX_MODEL_TURNS, 16);
        assert_eq!(DEFAULT_MAX_TOOL_CALLS_PER_TURN, 10);
    }
}
