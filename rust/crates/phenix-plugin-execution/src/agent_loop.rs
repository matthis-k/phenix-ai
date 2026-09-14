use crate::{execution_component_id, AgentLoopInterface};
use phenix_core::{
    Bytes, CallableId, ComponentInterface, ModelToolCall, ModelToolDescriptor, PluginContext,
    PluginHost, PluginInstance, RoutingProfileId, SdkClient, ServiceId,
};
use phenix_sdk::{ModelCommand, ModelResponse, ModelRoutingInterface};
use serde::{Deserialize, Serialize};

pub const AGENT_LOOP_SERVICE: &str = "phenix.agent-loop@1";
pub const DEFAULT_MAX_PARALLEL_TOOL_CALLS: u32 = 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentLoopPolicy {
    max_parallel_tool_calls: u32,
}

impl AgentLoopPolicy {
    #[must_use]
    pub const fn max_parallel_tool_calls(self) -> u32 {
        self.max_parallel_tool_calls
    }
}

impl Default for AgentLoopPolicy {
    fn default() -> Self {
        Self {
            max_parallel_tool_calls: DEFAULT_MAX_PARALLEL_TOOL_CALLS,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentLoopCommand {
    Run {
        profile_id: RoutingProfileId,
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentLoopResponse {
    Completed {
        output: Bytes,
        tool_calls: Vec<ModelToolCall>,
        usage: AgentLoopUsage,
    },
}

#[must_use]
pub fn agent_loop_service() -> ServiceId {
    ServiceId::parse(AGENT_LOOP_SERVICE).expect("static agent loop service id is valid")
}

pub(crate) fn agent_loop_factory() -> Box<dyn PluginInstance> {
    Box::new(AgentLoopPlugin)
}

struct AgentLoopSdk<'host, 'runtime> {
    models: SdkClient<'host, 'runtime, ModelRoutingInterface>,
}

type AgentLoopContext<'host, 'runtime> =
    PluginContext<'host, 'runtime, AgentLoopSdk<'host, 'runtime>>;

fn context<'host, 'runtime>(
    host: &'host PluginHost<'runtime>,
) -> AgentLoopContext<'host, 'runtime> {
    PluginContext::new(
        host,
        AgentLoopSdk {
            models: SdkClient::new(host, execution_component_id()),
        },
        (),
        (),
    )
}

struct AgentLoopPlugin;

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
        invoke_agent_loop(service, input, host)
    }
}

pub(crate) fn invoke_agent_loop(
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
    let response = handle(&context, command)?;
    context
        .kernel
        .encode_value(&response)
        .map_err(|error| error.to_string())
}

fn handle(
    context: &AgentLoopContext<'_, '_>,
    command: AgentLoopCommand,
) -> Result<AgentLoopResponse, String> {
    match command {
        AgentLoopCommand::Run {
            profile_id,
            callable_id,
            input,
            tools,
        } => {
            let response = context
                .sdk
                .models
                .invoke_projected(&ModelCommand::Invoke {
                    profile_id,
                    callable_id,
                    input,
                    tools,
                })
                .map_err(|error| error.to_string())?;
            let ModelResponse::Inference { response, .. } = response else {
                return Err("model routing returned a non-inference response to invoke".into());
            };
            Ok(AgentLoopResponse::Completed {
                output: response.output,
                tool_calls: response.tool_calls.clone(),
                usage: AgentLoopUsage {
                    model_calls: 1,
                    tool_calls: u32::try_from(response.tool_calls.len())
                        .map_err(|_| "model returned too many tool calls".to_owned())?,
                },
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_parallel_tool_cap_is_bounded() {
        assert_eq!(
            AgentLoopPolicy::default().max_parallel_tool_calls(),
            DEFAULT_MAX_PARALLEL_TOOL_CALLS
        );
        assert_eq!(DEFAULT_MAX_PARALLEL_TOOL_CALLS, 10);
    }
}
