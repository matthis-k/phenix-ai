//! Conceptual ACP adapter: typed in-process messages, not ACP wire types.
//! The host admits a prompt into a request scope and correlates its captures.
//! Transport, protocol session IDs, cancellation messages and negotiation are
//! deliberately absent. No model or environment knows this adapter exists.

use crate::{AgentEvent, CONTRACTS_PLUGIN, MODEL_CONTRACT, PROMPT_CONTRACT, capacity, port};
use phenix_graph::{
    ComponentDef, InputKey, OutputKey,
    compose::{ConnectionKey, Edit, Footprint, NetworkPatch},
};
use phenix_kernel::{Contribution, Key};
use phenix_plugin::{Failure, PluginInstance, Registrar};

pub const PLUGIN: &str = "mock-acp";
pub const INPUT: &str = "acp-prompt";
pub const OUTPUT: &str = "acp-message";
const DECODE: &str = "acp-decode";
const ENCODE: &str = "acp-encode";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptRequest {
    pub text: String,
}

/// A simplified protocol event or terminal response, not a graph event wrapper.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    AgentMessageChunk(String),
    ToolCall { title: String, environment: String },
    EndTurn,
}

fn translate(event: AgentEvent) -> Message {
    match event {
        AgentEvent::Delta(text) => Message::AgentMessageChunk(text),
        AgentEvent::ToolRequested(call) => Message::ToolCall {
            title: call.prompt,
            environment: call.environment,
        },
        AgentEvent::Completed => Message::EndTurn,
    }
}

pub struct MockAcp;

impl PluginInstance for MockAcp {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        Contribution::new("main", NetworkPatch::new([]))
            .import("prompt", Key::new(CONTRACTS_PLUGIN, PROMPT_CONTRACT))
            .import("model", Key::new(CONTRACTS_PLUGIN, MODEL_CONTRACT))
            .prepare_patch(|context| {
                let input = context.import_input("prompt", InputKey::<String>::new("input"))?;
                let events =
                    context.import_output("model", OutputKey::<AgentEvent>::new("events"))?;
                Ok(NetworkPatch::new([
                    Edit::add(
                        DECODE,
                        ComponentDef::map(|request: PromptRequest| request.text),
                    ),
                    Edit::add(ENCODE, ComponentDef::map(translate)),
                    context.connect_bound_input((DECODE, "output"), &input, capacity())?,
                    context.connect_bound_output(&events, (ENCODE, "input"), capacity())?,
                    Edit::expose_input(INPUT, [(DECODE, "input")], capacity()),
                    Edit::expose_output(OUTPUT, (ENCODE, "output")),
                ]))
            })
            .register(registrar)
    }
}

/// Explicit host approval for these selected provider placements.
pub fn grants(prompt: &str, model: &str) -> Vec<Footprint> {
    vec![
        Footprint::Component(DECODE.into()),
        Footprint::Component(ENCODE.into()),
        Footprint::Connection(ConnectionKey::new(
            port(DECODE, "output"),
            port(prompt, "input"),
        )),
        Footprint::Connection(ConnectionKey::new(
            port(model, "events"),
            port(ENCODE, "input"),
        )),
        Footprint::InputExport(INPUT.into()),
        Footprint::OutputExport(OUTPUT.into()),
    ]
}
