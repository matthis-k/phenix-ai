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
    runtime::{PolicyDeclaration, Readiness, ReadinessPolicy, ReadinessSnapshot},
};
use phenix_kernel::{CandidateArtifacts, ContractDefinition, Contribution, Key};
use phenix_plugin::{Failure, PluginInstance, Registrar};
use std::{collections::BTreeSet, num::NonZeroUsize, sync::Arc};

pub mod environment;
pub mod mock;
pub mod tools;

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
    pub name: String,
    pub prompt: String,
    pub environment: String,
}

/// One local invocation, distinct even when two calls have identical arguments.
/// Cloning preserves identity; constructing another invocation never does.
/// This is not a durable identifier or proof that an external effect occurred.
#[derive(Clone, Debug)]
pub struct ToolInvocation(Arc<ToolCall>);

impl ToolInvocation {
    pub fn new(call: ToolCall) -> Self {
        Self(Arc::new(call))
    }

    pub fn call(&self) -> &ToolCall {
        &self.0
    }

    pub fn complete(self, outcome: ToolOutcome) -> ToolResult {
        ToolResult {
            invocation: self,
            outcome,
        }
    }
}

impl PartialEq for ToolInvocation {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for ToolInvocation {}

/// Tool feedback binds an outcome to the dispatched invocation, not a user prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolResult {
    invocation: ToolInvocation,
    outcome: ToolOutcome,
}

impl ToolResult {
    pub fn invocation(&self) -> &ToolInvocation {
        &self.invocation
    }

    pub fn outcome(&self) -> &ToolOutcome {
        &self.outcome
    }
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
pub const PROMPT_CONTRACT: &str = "prompt";
pub const PROMPT: &str = "prompt";
pub const EVENTS: &str = "events";
const PROMPT_ADAPTER: &str = "agent-prompt";
const TOOL_INVOCATION: &str = "agent-tool-invocation";
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
                [PortDef::local::<ToolInvocation>("input")],
                [PortDef::local::<ToolResult>("output")],
            )
            .unwrap(),
        ),
    )
    .unwrap()
}

/// Typed prompt ingress seam; frontends attach before model-input construction.
pub fn prompt_contract() -> Contract {
    Contract::new(
        "agent/prompt@1",
        Arc::new(
            Interface::new(
                [PortDef::local::<String>("input")],
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
        })?;
        registrar.add(ContractDefinition {
            id: PROMPT_CONTRACT.into(),
            contract: prompt_contract(),
        })
    }
}

/// Standalone local agent loop. Additional routing is an ordinary extension.
pub struct AgentLoop;

struct ToolRouter<T> {
    routes: BTreeSet<String>,
    select: fn(&ToolCall) -> &str,
    output_port: fn(&str) -> String,
    fallback: fn(ToolInvocation) -> T,
    emitted: bool,
}

// The same router mechanism is usable with either a failure or a forwarding
// fallback. The fallback port's type follows its actual behavior.
fn tool_router<T: Send + Sync + 'static>(
    environments: BTreeSet<String>,
    fallback: fn(ToolInvocation) -> T,
) -> Result<ComponentDef, phenix_graph::InterfaceError> {
    invocation_router(
        environments,
        |call| &call.environment,
        environment_port,
        fallback,
    )
}

fn invocation_router<T: Send + Sync + 'static>(
    routes: BTreeSet<String>,
    select: fn(&ToolCall) -> &str,
    output_port: fn(&str) -> String,
    fallback: fn(ToolInvocation) -> T,
) -> Result<ComponentDef, phenix_graph::InterfaceError> {
    let interface = Interface::new(
        [PortDef::local::<ToolInvocation>("input")],
        routes
            .iter()
            .map(|name| PortDef::local::<ToolInvocation>(output_port(name)))
            .chain([PortDef::local::<T>(UNAVAILABLE)]),
    )?;
    Ok(ComponentDef::primitive(
        Arc::new(interface),
        Arc::new(move || ToolRouter {
            routes: routes.clone(),
            select,
            output_port,
            fallback,
            emitted: false,
        }),
    ))
}

impl<T: Send + Sync + 'static> Process for ToolRouter<T> {
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
        let invocation = value
            .borrow::<ToolInvocation>()
            .ok_or_else(|| GraphFailure::application_message("router expected a tool invocation"))?
            .clone();
        let route = (self.select)(invocation.call());
        let (output, value) = if self.routes.contains(route) {
            ((self.output_port)(route), Value::local(invocation))
        } else {
            (
                UNAVAILABLE.into(),
                Value::local((self.fallback)(invocation)),
            )
        };
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new(output, value)))
    }
}

/// Match feedback against the exact invocation within the request before it
/// reaches the model. The sequential loop expects feedback in dispatch order.
struct ToolFeedbackFactory;

impl ProcessFactory for ToolFeedbackFactory {
    fn spawn(&self) -> Box<dyn Process> {
        Box::new(ToolFeedback { emitted: false })
    }

    fn readiness(&self) -> Readiness {
        Readiness::from_policy(ToolFeedbackReadiness)
    }
}

/// Dispatch publishes the expectation and routed invocation atomically, so a
/// result without an expectation is unsolicited, not an incomplete valid join.
/// Leave expectations queued until a result arrives.
struct ToolFeedbackReadiness;

impl ReadinessPolicy for ToolFeedbackReadiness {
    fn declaration(&self) -> PolicyDeclaration {
        PolicyDeclaration {
            rule: None,
            correlated: true,
        }
    }

    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        let available = |name: &str| {
            snapshot
                .heads
                .iter()
                .any(|head| head.port == name && head.available)
        };
        if !available("input") {
            return vec![];
        }
        if available("expected") {
            vec!["expected".into(), "input".into()]
        } else {
            vec!["input".into()]
        }
    }

    fn notification_driven(&self) -> bool {
        true
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
                let expected = expected.borrow::<ToolInvocation>().ok_or_else(|| {
                    GraphFailure::application_message("feedback expected a tool invocation")
                })?;
                let result = result.borrow::<ToolResult>().ok_or_else(|| {
                    GraphFailure::application_message("feedback expected a tool result")
                })?;
                if expected != result.invocation() {
                    return Err(GraphFailure::application_message(
                        "tool feedback does not match dispatched invocation",
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
            (None, Some(_)) => Err(GraphFailure::application_message(
                "unsolicited tool feedback",
            )),
            (Some(_), None) => Err(GraphFailure::application_message("unpaired tool feedback")),
        }
    }
}

impl PluginInstance for AgentLoop {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        Contribution::new("main", NetworkPatch::new([]))
            .import("model", Key::new(CONTRACTS_PLUGIN, MODEL_CONTRACT))
            .import("env-local", Key::new(CONTRACTS_PLUGIN, TOOL_CONTRACT))
            .offer(
                "prompt",
                PROMPT_ADAPTER,
                Key::new(CONTRACTS_PLUGIN, PROMPT_CONTRACT),
            )
            .prepare(|context| {
                let model = context.bind_import("model")?;
                let model_input = model.input::<ModelInput>("input")?;
                let model_tool = model.output::<ToolCall>("tool")?;
                let events = model.output::<AgentEvent>("events")?;
                let router = tool_router(["local".into()].into(), |invocation| {
                    invocation.complete(ToolOutcome::Failed {
                        output: String::new(),
                        reason: "environment unavailable".into(),
                    })
                })?;
                let tool = context.bind_import("env-local")?;
                let tool_input = tool.input::<ToolInvocation>("input")?;
                let result = tool.output::<ToolResult>("output")?;
                let edits = vec![
                    Edit::Add {
                        path: TOOL_INVOCATION.into(),
                        definition: Arc::new(ComponentDef::map(ToolInvocation::new)),
                    },
                    context.connect_bound_output(
                        &model_tool,
                        port(TOOL_INVOCATION, "input"),
                        capacity(),
                    )?,
                    Edit::Add {
                        path: TOOL_ROUTER.into(),
                        definition: Arc::new(router),
                    },
                    Edit::Connect(ConnectionDef {
                        from: port(TOOL_INVOCATION, "output"),
                        to: port(TOOL_ROUTER, "input"),
                        capacity: capacity(),
                    }),
                    Edit::Add {
                        path: TOOL_FEEDBACK.into(),
                        definition: Arc::new(ComponentDef::primitive(
                            Arc::new(Interface::new(
                                [
                                    PortDef::local::<ToolInvocation>("expected"),
                                    PortDef::local::<ToolResult>("input"),
                                ],
                                [PortDef::local::<ModelInput>("output")],
                            )?),
                            Arc::new(ToolFeedbackFactory),
                        )),
                    },
                    Edit::Connect(ConnectionDef {
                        from: port(TOOL_INVOCATION, "output"),
                        to: port(TOOL_FEEDBACK, "expected"),
                        capacity: capacity(),
                    }),
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
                        definition: Arc::new(
                            ComponentDef::map(ModelInput::Prompt)
                                .claim(&prompt_contract())
                                .expect("prompt adapter implements its declared contract"),
                        ),
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
                    Edit::Connect(ConnectionDef {
                        from: port(TOOL_ROUTER, "env-local"),
                        to: port(tool.path(), tool_input.name()),
                        capacity: capacity(),
                    }),
                    context.connect_bound_output(
                        &result,
                        port(TOOL_FEEDBACK, "input"),
                        capacity(),
                    )?,
                ];
                Ok(CandidateArtifacts {
                    patch: Some(NetworkPatch::new(edits)),
                    ..Default::default()
                })
            })
            .register(registrar)
    }
}

/// Exact host grants for the configured graph; names are not authority by themselves.
pub fn loop_grants(model: &str, tool: &str) -> Vec<Footprint> {
    vec![
        Footprint::Component(TOOL_INVOCATION.into()),
        Footprint::Connection(ConnectionKey::new(
            port(model, "tool"),
            port(TOOL_INVOCATION, "input"),
        )),
        Footprint::Component(TOOL_ROUTER.into()),
        Footprint::Connection(ConnectionKey::new(
            port(TOOL_INVOCATION, "output"),
            port(TOOL_ROUTER, "input"),
        )),
        Footprint::Component(TOOL_FEEDBACK.into()),
        Footprint::Connection(ConnectionKey::new(
            port(TOOL_INVOCATION, "output"),
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
        Footprint::Connection(ConnectionKey::new(
            port(TOOL_ROUTER, "env-local"),
            port(tool, "input"),
        )),
        Footprint::Connection(ConnectionKey::new(
            port(tool, "output"),
            port(TOOL_FEEDBACK, "input"),
        )),
    ]
}

/// Check the complete shape and claim when defining a provider component.
pub fn claimed(
    definition: ComponentDef,
    contract: &Contract,
) -> Result<Arc<ComponentDef>, NetworkError> {
    Ok(Arc::new(definition.claim(contract)?))
}
