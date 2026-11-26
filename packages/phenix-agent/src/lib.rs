//! Application-level agent contracts and a basic feedback network.
//!
//! The model and tool are independently selected contributions. The loop owns
//! only wiring; the model owns response decisions and the tool owns execution.
//! These local Rust messages are not a provider or protocol wire format.

use phenix_graph::{
    ComponentDef, Contract, Interface, NetworkError, PortDef,
    compose::{ConnectionKey, Edit, Footprint, NetworkPatch},
    network::PortPath,
};
use phenix_kernel::{CandidateArtifacts, ContractDefinition, Contribution, Key};
use phenix_plugin::{Failure, PluginInstance, Registrar};
use std::{num::NonZeroUsize, sync::Arc};

pub mod mock;

/// A turn starts with a prompt and can continue with tool feedback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelInput {
    Prompt(String),
    ToolResult { call: ToolCall, output: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCall {
    pub prompt: String,
}

/// Ordered model events on the public capture. Completion is explicit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentEvent {
    ToolRequested(ToolCall),
    Delta(String),
    Completed,
}

pub const CONTRACTS_PLUGIN: &str = "agent-contracts";
pub const LOOP_PLUGIN: &str = "agent-loop";
pub const MODEL_CONTRACT: &str = "model";
pub const TOOL_CONTRACT: &str = "tool";
pub const PROMPT: &str = "prompt";
pub const EVENTS: &str = "events";

fn capacity() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}

fn port(process: &str, name: &str) -> PortPath {
    PortPath {
        process: process.into(),
        port: name.into(),
    }
}

pub fn model_contract() -> Contract {
    Contract::new(
        "agent/model@1",
        Arc::new(
            Interface::new(
                [PortDef::local::<ModelInput>("input")],
                [
                    PortDef::local::<ToolCall>("tool"),
                    PortDef::local::<AgentEvent>("events"),
                ],
            )
            .unwrap(),
        ),
    )
    .unwrap()
}

pub fn tool_contract() -> Contract {
    Contract::new(
        "agent/tool@1",
        Arc::new(
            Interface::new(
                [PortDef::local::<ToolCall>("input")],
                [PortDef::local::<ModelInput>("output")],
            )
            .unwrap(),
        ),
    )
    .unwrap()
}

/// Shared application contracts are available independently of any loop preset.
pub struct AgentContracts;

impl PluginInstance for AgentContracts {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        registrar.add(ContractDefinition {
            id: MODEL_CONTRACT.into(),
            contract: model_contract(),
        })?;
        registrar.add(ContractDefinition {
            id: TOOL_CONTRACT.into(),
            contract: tool_contract(),
        })
    }
}

/// A preset is an ordinary plugin; it needs no runtime privileges.
pub struct BasicLoop;

impl PluginInstance for BasicLoop {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        Contribution::new("main", NetworkPatch::new([]))
            .import("model", Key::new(CONTRACTS_PLUGIN, MODEL_CONTRACT))
            .import("tool", Key::new(CONTRACTS_PLUGIN, TOOL_CONTRACT))
            .prepare(|context| {
                let model = context.bind_import("model")?;
                let tool = context.bind_import("tool")?;
                let model_input = model.input::<ModelInput>("input")?;
                let tool_input = tool.input::<ToolCall>("input")?;
                let model_tool = model.output::<ToolCall>("tool")?;
                let result = tool.output::<ModelInput>("output")?;
                let events = model.output::<AgentEvent>("events")?;
                Ok(CandidateArtifacts {
                    patch: Some(NetworkPatch::new([
                        context.connect_bound_output(
                            &model_tool,
                            port(tool.path(), tool_input.name()),
                            capacity(),
                        )?,
                        context.connect_bound_output(
                            &result,
                            port(model.path(), model_input.name()),
                            capacity(),
                        )?,
                        context.expose_bound_input(PROMPT, &[model_input], capacity())?,
                        context.expose_bound_output(EVENTS, &events)?,
                    ])),
                    ..Default::default()
                })
            })
            .register(registrar)
    }
}

/// Exact host policy for the preset's wiring, independent of plugin payloads.
pub fn loop_grants(model: &str, tool: &str) -> [Footprint; 4] {
    [
        Footprint::Connection(ConnectionKey::new(port(model, "tool"), port(tool, "input"))),
        Footprint::Connection(ConnectionKey::new(port(tool, "output"), port(model, "input"))),
        Footprint::InputExport(PROMPT.into()),
        Footprint::OutputExport(EVENTS.into()),
    ]
}

/// Check the complete shape and claim when defining a provider component.
pub fn claimed(
    definition: ComponentDef,
    contract: &Contract,
) -> Result<Arc<ComponentDef>, NetworkError> {
    Ok(Arc::new(definition.claim(contract)?))
}
