//! Provider-side model adaptation. Model backends supply decisions and deltas;
//! the graph owns the feedback cycle and dispatches tools.

use crate::{AgentEvent, CONTRACTS_PLUGIN, MODEL_CONTRACT, ModelInput, ToolCall, model_contract};
use phenix_graph::{
    ComponentDef, Failure as GraphFailure, Process, ProcessContext, ProcessEmission, Step, Value,
};
use phenix_kernel::{Contribution, Key};
use phenix_plugin::{Failure, PluginInstance, Registrar};
use std::sync::Arc;

pub mod command;

/// A response ends with exactly one terminal item: a tool call or completion.
/// The adapter drops the stream at that item; any remaining items are unused.
pub enum ModelOutput {
    Delta(String),
    Tool(ToolCall),
    Complete,
}

pub type ModelStream = Box<dyn Iterator<Item = Result<ModelOutput, GraphFailure>> + Send>;

/// Provider-specific state lives in the backend, not in the graph library.
/// This immediate adapter is cooperative between items; implementations
/// which block inside respond/next block their executing worker.
pub trait ModelBackend: Send + 'static {
    fn respond(&mut self, input: ModelInput) -> Result<ModelStream, GraphFailure>;
}

/// Ordinary model provider plugin. Construction and registration derive the
/// canonical ports, contract claim and event/tool fanout.
pub struct Model {
    path: String,
    definition: ComponentDef,
}

impl Model {
    pub fn new<B: ModelBackend>(
        path: impl Into<String>,
        factory: impl Fn() -> B + Send + Sync + 'static,
    ) -> Self {
        let definition = ComponentDef::primitive(
            model_contract().required().clone(),
            Arc::new(move || ModelProcess {
                backend: factory(),
                state: State::Idle,
            }),
        );
        Self {
            path: path.into(),
            definition,
        }
    }
}

impl PluginInstance for Model {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        Contribution::offered_component(
            "main",
            &self.path,
            "model",
            Key::new(CONTRACTS_PLUGIN, MODEL_CONTRACT),
            &model_contract(),
            self.definition.clone(),
        )?
        .register(registrar)
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
            self.state = State::Streaming(self.backend.respond(input)?);
        }
        match std::mem::replace(&mut self.state, State::Idle) {
            State::Idle => unreachable!("initialized above"),
            State::Streaming(mut stream) => {
                let next = stream.next().ok_or_else(|| {
                    GraphFailure::application_message("model stream ended without terminal output")
                })??;
                match next {
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
