//! Application-level agent contracts and a basic feedback network.
//!
//! The model and tool are independently selected contributions. The loop owns
//! only wiring; the model owns response decisions and the tool owns execution.
//! These local Rust messages are not a provider or protocol wire format.

use phenix_graph::{
    ComponentDef, Contract, Failure as GraphFailure, Interface, NetworkError, PortDef, Process,
    ProcessContext, ProcessEmission, ProcessFactory, Step, Value,
    compose::{ConnectionKey, Edit, Footprint, NetworkPatch},
    network::{ConnectionDef, PortPath},
    runtime::Readiness,
};
use phenix_kernel::{CandidateArtifacts, ContractDefinition, Contribution, Key};
use phenix_plugin::{Failure, PluginInstance, Registrar};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroUsize,
    sync::Arc,
};

pub mod mock;

/// A turn starts with a prompt and can continue with tool feedback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelInput {
    Prompt(String),
    ToolResult(ToolResult),
}

/// A tool's domain outcome is data for the model, not a scheduler failure.
/// Failed executions may still carry output; a reason is mandatory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolOutcome {
    Succeeded { output: String },
    Failed { output: String, reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCall {
    pub prompt: String,
    pub environment: String,
}

/// Tool feedback contains only a call and its outcome, never a user prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolResult {
    pub call: ToolCall,
    pub outcome: ToolOutcome,
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
const PROMPT_ADAPTER: &str = "agent-prompt";
const TOOL_ROUTER: &str = "agent-tool-router";
const TOOL_FEEDBACK: &str = "agent-tool-feedback";
const UNAVAILABLE: &str = "unavailable";

fn environment_port(environment: &str) -> String {
    format!("env-{environment}")
}

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
                [PortDef::local::<ToolResult>("output")],
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

/// One loop mechanism; the default is a single local environment.
/// Host import policy authorizes the provider for every configured environment.
pub struct AgentLoop {
    pub environments: BTreeSet<String>,
}

impl Default for AgentLoop {
    fn default() -> Self {
        Self {
            environments: ["local".into()].into(),
        }
    }
}

struct ToolRouter {
    environments: BTreeSet<String>,
    emitted: bool,
}

impl Process for ToolRouter {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, GraphFailure> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        let Some(value) = context.take("input") else {
            return Ok(if context.is_eos("input") {
                Step::Finished
            } else {
                Step::Complete
            });
        };
        let call = value
            .borrow::<ToolCall>()
            .ok_or_else(|| GraphFailure::application_message("router expected a tool call"))?
            .clone();
        let (output, value) = if self.environments.contains(&call.environment) {
            (environment_port(&call.environment), Value::local(call))
        } else {
            (
                UNAVAILABLE.into(),
                Value::local(ToolResult {
                    call,
                    outcome: ToolOutcome::Failed {
                        output: String::new(),
                        reason: "environment unavailable".into(),
                    },
                }),
            )
        };
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new(output, value)))
    }
}

/// Match feedback against the original call within the request before it reaches
/// the model. This checks the call data, not effect evidence or unique call IDs.
/// The current sequential loop expects feedback in call order.
struct ToolFeedbackFactory;

impl ProcessFactory for ToolFeedbackFactory {
    fn spawn(&self) -> Box<dyn Process> {
        Box::new(ToolFeedback { emitted: false })
    }

    fn readiness(&self) -> Readiness {
        Readiness::CORRELATED_ALL
    }
}

struct ToolFeedback {
    emitted: bool,
}

impl Process for ToolFeedback {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, GraphFailure> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        match (context.take("expected"), context.take("input")) {
            (Some(expected), Some(result)) => {
                let expected = expected.borrow::<ToolCall>().ok_or_else(|| {
                    GraphFailure::application_message("feedback expected a tool call")
                })?;
                let result = result.borrow::<ToolResult>().ok_or_else(|| {
                    GraphFailure::application_message("feedback expected a tool result")
                })?;
                if expected != &result.call {
                    return Err(GraphFailure::application_message(
                        "tool feedback does not match dispatched call",
                    ));
                }
                self.emitted = true;
                Ok(Step::Emit(ProcessEmission::new(
                    "output",
                    Value::local(ModelInput::ToolResult(result.clone())),
                )))
            }
            (None, None) => Ok(if context.is_eos("expected") && context.is_eos("input") {
                Step::Finished
            } else {
                Step::Complete
            }),
            _ => Err(GraphFailure::application_message("unpaired tool feedback")),
        }
    }
}

impl PluginInstance for AgentLoop {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        let environments = self.environments.clone();
        let mut contribution = Contribution::new("main", NetworkPatch::new([]))
            .import("model", Key::new(CONTRACTS_PLUGIN, MODEL_CONTRACT));
        for environment in &environments {
            contribution = contribution.import(
                environment_port(environment),
                Key::new(CONTRACTS_PLUGIN, TOOL_CONTRACT),
            );
        }
        contribution
            .prepare(move |context| {
                let model = context.bind_import("model")?;
                let model_input = model.input::<ModelInput>("input")?;
                let model_tool = model.output::<ToolCall>("tool")?;
                let events = model.output::<AgentEvent>("events")?;
                let interface = Interface::new(
                    [PortDef::local::<ToolCall>("input")],
                    environments
                        .iter()
                        .map(|name| PortDef::local::<ToolCall>(environment_port(name)))
                        .chain([PortDef::local::<ToolResult>(UNAVAILABLE)]),
                )?;
                let routes = environments.clone();
                let router = ComponentDef::primitive(
                    Arc::new(interface),
                    Arc::new(move || ToolRouter {
                        environments: routes.clone(),
                        emitted: false,
                    }),
                );
                let mut edits = vec![
                    Edit::Add {
                        path: TOOL_ROUTER.into(),
                        definition: Arc::new(router),
                    },
                    context.connect_bound_output(
                        &model_tool,
                        port(TOOL_ROUTER, "input"),
                        capacity(),
                    )?,
                    Edit::Add {
                        path: TOOL_FEEDBACK.into(),
                        definition: Arc::new(ComponentDef::primitive(
                            Arc::new(Interface::new(
                                [
                                    PortDef::local::<ToolCall>("expected"),
                                    PortDef::local::<ToolResult>("input"),
                                ],
                                [PortDef::local::<ModelInput>("output")],
                            )?),
                            Arc::new(ToolFeedbackFactory),
                        )),
                    },
                    context.connect_bound_output(
                        &model_tool,
                        port(TOOL_FEEDBACK, "expected"),
                        capacity(),
                    )?,
                    Edit::Connect(ConnectionDef {
                        from: port(TOOL_ROUTER, UNAVAILABLE),
                        to: port(TOOL_FEEDBACK, "input"),
                        capacity: capacity(),
                    }),
                    Edit::Connect(ConnectionDef {
                        from: port(TOOL_FEEDBACK, "output"),
                        to: port(model.path(), model_input.name()),
                        capacity: capacity(),
                    }),
                    Edit::Add {
                        path: PROMPT_ADAPTER.into(),
                        definition: Arc::new(ComponentDef::map(ModelInput::Prompt)),
                    },
                    Edit::Connect(ConnectionDef {
                        from: port(PROMPT_ADAPTER, "output"),
                        to: port(model.path(), model_input.name()),
                        capacity: capacity(),
                    }),
                    Edit::ExposeInput {
                        name: PROMPT.into(),
                        targets: vec![port(PROMPT_ADAPTER, "input")],
                        capacity: capacity(),
                    },
                    context.expose_bound_output(EVENTS, &events)?,
                ];
                let mut feedback = BTreeSet::new();
                for environment in &environments {
                    let output = environment_port(environment);
                    let tool = context.bind_import(&output)?;
                    let tool_input = tool.input::<ToolCall>("input")?;
                    let result = tool.output::<ToolResult>("output")?;
                    edits.push(Edit::Connect(ConnectionDef {
                        from: port(TOOL_ROUTER, &output),
                        to: port(tool.path(), tool_input.name()),
                        capacity: capacity(),
                    }));
                    if feedback.insert(tool.path().to_owned()) {
                        edits.push(context.connect_bound_output(
                            &result,
                            port(TOOL_FEEDBACK, "input"),
                            capacity(),
                        )?);
                    }
                }
                Ok(CandidateArtifacts {
                    patch: Some(NetworkPatch::new(edits)),
                    ..Default::default()
                })
            })
            .register(registrar)
    }
}

/// Exact host grants for the configured graph; names are not authority by themselves.
pub fn loop_grants(model: &str, environments: &BTreeMap<String, String>) -> Vec<Footprint> {
    let mut grants = vec![
        Footprint::Component(TOOL_ROUTER.into()),
        Footprint::Connection(ConnectionKey::new(
            port(model, "tool"),
            port(TOOL_ROUTER, "input"),
        )),
        Footprint::Component(TOOL_FEEDBACK.into()),
        Footprint::Connection(ConnectionKey::new(
            port(model, "tool"),
            port(TOOL_FEEDBACK, "expected"),
        )),
        Footprint::Connection(ConnectionKey::new(
            port(TOOL_ROUTER, UNAVAILABLE),
            port(TOOL_FEEDBACK, "input"),
        )),
        Footprint::Connection(ConnectionKey::new(
            port(TOOL_FEEDBACK, "output"),
            port(model, "input"),
        )),
        Footprint::Component(PROMPT_ADAPTER.into()),
        Footprint::Connection(ConnectionKey::new(
            port(PROMPT_ADAPTER, "output"),
            port(model, "input"),
        )),
        Footprint::InputExport(PROMPT.into()),
        Footprint::OutputExport(EVENTS.into()),
    ];
    for (environment, tool) in environments {
        grants.push(Footprint::Connection(ConnectionKey::new(
            port(TOOL_ROUTER, &environment_port(environment)),
            port(tool, "input"),
        )));
        grants.push(Footprint::Connection(ConnectionKey::new(
            port(tool, "output"),
            port(TOOL_FEEDBACK, "input"),
        )));
    }
    grants
}

/// Check the complete shape and claim when defining a provider component.
pub fn claimed(
    definition: ComponentDef,
    contract: &Contract,
) -> Result<Arc<ComponentDef>, NetworkError> {
    Ok(Arc::new(definition.claim(contract)?))
}
