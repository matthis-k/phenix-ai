//! Conceptual ACP adapter: typed in-process messages, not ACP wire types.
//! The host admits a prompt into a request scope and correlates its captures.
//! Transport, protocol session IDs, cancellation messages and negotiation are
//! deliberately absent. No model or environment knows this adapter exists.

use crate::{
    AgentEvent, CONTRACTS_PLUGIN, MODEL_CONTRACT, PROMPT_CONTRACT, capacity, model_contract,
    prompt_contract,
};
use phenix_graph::{
    ComponentDef, InputKey, NetworkError, OutputKey,
    compose::{Footprint, NetworkPatch, PatchBuilder, PatchComponent},
};
use phenix_kernel::{Contribution, ImportSpec};
use phenix_plugin::{Failure, PluginInstance, Registrar};

pub const PLUGIN: &str = "mock-acp";
pub const INPUT: &str = "acp-prompt";
pub const OUTPUT: &str = "acp-message";
const DECODE: &str = "acp-decode";
const ENCODE: &str = "acp-encode";
const PROMPT_IMPORT: ImportSpec = ImportSpec::new("prompt", CONTRACTS_PLUGIN, PROMPT_CONTRACT);
const MODEL_IMPORT: ImportSpec = ImportSpec::new("model", CONTRACTS_PLUGIN, MODEL_CONTRACT);

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

fn translate(event: AgentEvent) -> Option<Message> {
    Some(match event {
        AgentEvent::ModelInfo(_) => return None,
        AgentEvent::Delta(text) => Message::AgentMessageChunk(text),
        AgentEvent::ToolRequested(call) => Message::ToolCall {
            title: call.arguments.as_text().unwrap_or(&call.name).to_owned(),
            environment: call.environment,
        },
        AgentEvent::Completed => Message::EndTurn,
    })
}

pub struct MockAcp;

impl PluginInstance for MockAcp {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        Contribution::dynamic("main", |context| {
            let prompt = PROMPT_IMPORT.bind(context)?;
            let model = MODEL_IMPORT.bind(context)?;
            Ok(adapter_patch(prompt.path(), model.path())?)
        })
        .require(&PROMPT_IMPORT)
        .require(&MODEL_IMPORT)
        .register(registrar)
    }
}

fn adapter_patch(prompt: &str, model: &str) -> Result<NetworkPatch, NetworkError> {
    let mut patch = PatchBuilder::new(capacity());
    let prompt = PatchComponent::at(prompt, prompt_contract().required().clone());
    let model = PatchComponent::at(model, model_contract().required().clone());
    let decode = patch.map(DECODE, |request: PromptRequest| request.text);
    let encode = patch.add(ENCODE, ComponentDef::filter_map(translate));
    patch.connect(&decode.output, &prompt.input(InputKey::new("input"))?)?;
    patch.connect(
        &model.output(OutputKey::<AgentEvent>::new("events"))?,
        &encode.input(InputKey::new("input"))?,
    )?;
    patch.expose_input(INPUT, &decode.input);
    patch.expose_output(OUTPUT, &encode.output(OutputKey::<Message>::new("output"))?);
    Ok(patch.finish())
}

/// The trusted host computes required writes for its chosen placements.
/// It never accepts plugin-supplied grants.
pub fn grants(prompt: &str, model: &str) -> Vec<Footprint> {
    adapter_patch(prompt, model)
        .expect("canonical ACP adapter")
        .footprints()
        .into_iter()
        .collect()
}
