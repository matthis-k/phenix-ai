//! Optional developer observer. Only observes ordinary authorized outputs.
//! No capture, scheduler hook, provider dependency, or prompt/result content
//! is required. Sinks run synchronously and participate in backpressure;
//! enable only for trusted development diagnostics.

use crate::{
    AgentEvent, CONTRACTS_PLUGIN, FEEDBACK_CONTRACT, LOOP_PLUGIN, MODEL_CONTRACT, ModelInput,
    PROMPT_CONTRACT, ToolCall, ToolOutcome, capacity, model::ModelProfile,
};
use phenix_graph::{
    ComponentDef,
    compose::{Edit, NetworkPatch},
};
use phenix_kernel::{BuildOptions, Contribution, ImportSpec, Key};
use phenix_plugin::{Failure, PluginInstance, Registrar};
use std::sync::Arc;

const PROMPT: ImportSpec = ImportSpec::new("prompt", CONTRACTS_PLUGIN, PROMPT_CONTRACT);
const MODEL: ImportSpec = ImportSpec::new("model", CONTRACTS_PLUGIN, MODEL_CONTRACT);
const FEEDBACK: ImportSpec = ImportSpec::new("feedback", CONTRACTS_PLUGIN, FEEDBACK_CONTRACT);

/// Typed runtime evidence. Payloads, credentials, and error messages are
/// deliberately absent; names and schemas are provider/tool metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TraceRecord {
    Prompt {
        bytes: usize,
    },
    Model(ModelProfile),
    Requested {
        name: String,
        environment: String,
    },
    Dispatch {
        name: String,
        environment: String,
        argument_schema: String,
    },
    Feedback {
        name: String,
        environment: String,
        succeeded: bool,
        result_schema: String,
    },
    Delta {
        bytes: usize,
    },
    Completed,
}

#[derive(Clone)]
pub struct Diagnostics {
    prefix: String,
    record: Arc<dyn Fn(TraceRecord) + Send + Sync>,
}

impl Diagnostics {
    pub fn new(
        prefix: impl Into<String>,
        record: impl Fn(TraceRecord) + Send + Sync + 'static,
    ) -> Self {
        Self {
            prefix: prefix.into(),
            record: Arc::new(record),
        }
    }

    fn patch(&self, prompt: &str, model: &str, feedback: &str) -> NetworkPatch {
        let record = self.record.clone();
        let prompt_sink = format!("{}-prompt", self.prefix);
        let model_sink = format!("{}-events", self.prefix);
        let dispatch_sink = format!("{}-dispatch", self.prefix);
        let feedback_sink = format!("{}-feedback", self.prefix);
        let observe_prompt = record.clone();
        let observe_model = record.clone();
        let observe_dispatch = record.clone();
        NetworkPatch::new([
            Edit::add(
                &prompt_sink,
                ComponentDef::sink(move |input: &ModelInput| {
                    if let ModelInput::Prompt(text) = input {
                        observe_prompt(TraceRecord::Prompt { bytes: text.len() });
                    }
                }),
            ),
            Edit::connect((prompt, "output"), (&prompt_sink, "input"), capacity()),
            Edit::add(
                &model_sink,
                ComponentDef::sink(move |event: &AgentEvent| {
                    observe_model(match event {
                        AgentEvent::ModelInfo(profile) => TraceRecord::Model(profile.clone()),
                        AgentEvent::ToolRequested(call) => TraceRecord::Requested {
                            name: call.name.clone(),
                            environment: call.environment.clone(),
                        },
                        AgentEvent::Delta(text) => TraceRecord::Delta { bytes: text.len() },
                        AgentEvent::Completed => TraceRecord::Completed,
                    });
                }),
            ),
            Edit::connect((model, "events"), (&model_sink, "input"), capacity()),
            Edit::add(
                &dispatch_sink,
                ComponentDef::sink(move |call: &ToolCall| {
                    observe_dispatch(TraceRecord::Dispatch {
                        name: call.name.clone(),
                        environment: call.environment.clone(),
                        argument_schema: call.arguments.schema().name.clone(),
                    });
                }),
            ),
            Edit::connect((model, "tool"), (&dispatch_sink, "input"), capacity()),
            Edit::add(
                &feedback_sink,
                ComponentDef::sink(move |input: &ModelInput| {
                    if let ModelInput::ToolResult(result) = input {
                        let call = result.invocation().call();
                        let (succeeded, output) = match result.outcome() {
                            ToolOutcome::Succeeded { output } => (true, output),
                            ToolOutcome::Failed { output, .. } => (false, output),
                        };
                        record(TraceRecord::Feedback {
                            name: call.name.clone(),
                            environment: call.environment.clone(),
                            succeeded,
                            result_schema: output.schema().name.clone(),
                        });
                    }
                }),
            ),
            Edit::connect((feedback, "output"), (&feedback_sink, "input"), capacity()),
        ])
    }

    /// Trusted host opt-in: select existing offers and grant exactly the
    /// observation edges and sink placements. No broad inspection grant.
    pub fn configure(
        &self,
        options: &mut BuildOptions,
        owner: &str,
        model_offer: Key,
        model_path: &str,
    ) {
        for (import, offer) in [
            ("prompt", Key::new(LOOP_PLUGIN, "prompt")),
            ("model", model_offer),
            ("feedback", Key::new(LOOP_PLUGIN, "feedback")),
        ] {
            let import = Key::new(owner, import);
            if let Some(allow) = &mut options.imports.allow {
                allow.insert((import.clone(), offer.clone()));
            }
            options.imports.selections.insert(import, offer);
        }
        options
            .composition
            .entry(Key::new(owner, "main"))
            .or_default()
            .grants
            .extend(
                self.patch(crate::PROMPT_ADAPTER, model_path, crate::TOOL_FEEDBACK)
                    .footprints(),
            );
    }
}

impl PluginInstance for Diagnostics {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        let config = self.clone();
        Contribution::dynamic("main", move |context| {
            let prompt = PROMPT.bind(context)?;
            let model = MODEL.bind(context)?;
            let feedback = FEEDBACK.bind(context)?;
            Ok(config.patch(prompt.path(), model.path(), feedback.path()))
        })
        .require(&PROMPT)
        .require(&MODEL)
        .require(&FEEDBACK)
        .register(registrar)
    }
}
