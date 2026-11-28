//! Provider-side model adaptation. Model backends supply decisions and deltas;
//! the graph owns the feedback cycle and dispatches tools.

use crate::{AgentEvent, CONTRACTS_PLUGIN, MODEL_CONTRACT, ModelInput, ToolCall, model_contract};
use phenix_graph::{
    ComponentDef, Failure as GraphFailure, Process, ProcessContext, ProcessEmission, Step, Value,
};
use phenix_kernel::{Contribution, Key};
use phenix_plugin::{Failure, PluginInstance, Registrar};
use std::{collections::BTreeSet, sync::Arc};

pub mod command;
pub mod fake;

/// Advertised metadata, not authorization. Capability names are open;
/// independent providers may add capabilities without changing Graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelProfile {
    pub provider: String,
    pub model: String,
    pub capabilities: BTreeSet<String>,
    pub context_tokens: Option<u64>,
}

impl ModelProfile {
    pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
            capabilities: BTreeSet::new(),
            context_tokens: None,
        }
    }

    pub fn capability(mut self, name: impl Into<String>) -> Self {
        self.capabilities.insert(name.into());
        self
    }

    pub fn context_tokens(mut self, tokens: u64) -> Self {
        self.context_tokens = Some(tokens);
        self
    }
}

impl phenix_plugin::Contribution for ModelProfile {
    const KIND: &'static str = "agent.model-profile";

    fn id(&self) -> &str {
        "model"
    }
}

/// A response ends with exactly one terminal item: a tool call or completion.
/// The adapter drops the stream at that item; any remaining items are unused.
pub enum ModelOutput {
    /// Report the profile supplied to this response, not a provider assertion.
    Info,
    Delta(String),
    Tool(ToolCall),
    Complete,
}

pub type ModelStream = Box<dyn Iterator<Item = Result<ModelOutput, GraphFailure>> + Send>;

/// Provider-specific state lives in the backend, not in the graph library.
/// This immediate adapter is cooperative between items; implementations
/// which block inside respond/next block their executing worker.
pub trait ModelBackend: Send + 'static {
    fn respond(
        &mut self,
        input: ModelInput,
        profile: &ModelProfile,
    ) -> Result<ModelStream, GraphFailure>;
}

/// Ordinary model provider plugin. Construction and registration derive the
/// canonical ports, contract claim and event/tool fanout.
pub struct Model {
    path: String,
    profile: ModelProfile,
    definition: ComponentDef,
}

impl Model {
    pub fn new<B: ModelBackend>(
        path: impl Into<String>,
        profile: ModelProfile,
        factory: impl Fn() -> B + Send + Sync + 'static,
    ) -> Self {
        let runtime_profile = profile.clone();
        let definition = ComponentDef::primitive(
            model_contract().required().clone(),
            Arc::new(move || ModelProcess {
                backend: factory(),
                profile: runtime_profile.clone(),
                state: State::Idle,
            }),
        );
        Self {
            path: path.into(),
            profile,
            definition,
        }
    }
}

impl PluginInstance for Model {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        let mut bundle = Registrar::new(registrar.owner())?;
        Contribution::offered_component(
            "main",
            &self.path,
            "model",
            Key::new(CONTRACTS_PLUGIN, MODEL_CONTRACT),
            &model_contract(),
            self.definition.clone(),
        )?
        .register(&mut bundle)?;
        bundle.add(self.profile.clone())?;
        registrar.merge(bundle.finish())
    }
}

enum State {
    Idle,
    Streaming(ModelStream),
    Dispatch(ToolCall),
    Done,
}

struct ModelProcess<B> {
    backend: B,
    profile: ModelProfile,
    state: State,
}

impl<B: ModelBackend> Process for ModelProcess<B> {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, GraphFailure> {
        if matches!(self.state, State::Idle) {
            let Some(value) = context.take("input") else {
                return Ok(if context.is_eos("input") {
                    Step::Finished
                } else {
                    Step::Complete
                });
            };
            let input = value
                .borrow::<ModelInput>()
                .ok_or_else(|| GraphFailure::application_message("model expected typed input"))?
                .clone();
            self.state = State::Streaming(self.backend.respond(input, &self.profile)?);
        }
        match std::mem::replace(&mut self.state, State::Idle) {
            State::Idle => unreachable!("initialized above"),
            State::Streaming(mut stream) => {
                let next = stream.next().ok_or_else(|| {
                    GraphFailure::application_message("model stream ended without terminal output")
                })??;
                match next {
                    ModelOutput::Info => {
                        self.state = State::Streaming(stream);
                        Ok(event(AgentEvent::ModelInfo(self.profile.clone())))
                    }
                    ModelOutput::Delta(delta) => {
                        self.state = State::Streaming(stream);
                        Ok(event(AgentEvent::Delta(delta)))
                    }
                    ModelOutput::Tool(call) => {
                        self.state = State::Dispatch(call.clone());
                        Ok(event(AgentEvent::ToolRequested(call)))
                    }
                    ModelOutput::Complete => {
                        self.state = State::Done;
                        Ok(event(AgentEvent::Completed))
                    }
                }
            }
            State::Dispatch(call) => {
                self.state = State::Done;
                Ok(Step::Emit(ProcessEmission::new("tool", Value::local(call))))
            }
            State::Done => Ok(Step::Complete),
        }
    }
}

fn event(event: AgentEvent) -> Step {
    Step::Emit(ProcessEmission::new("events", Value::local(event)))
}
