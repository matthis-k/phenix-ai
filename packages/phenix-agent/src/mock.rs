//! Deterministic provider plugins and a host policy for the mock harness.
//! No network, credentials, shell, external effects, or durable history.

use crate::{
    AgentEvent, BasicLoop, LOOP_PLUGIN, MODEL_CONTRACT, ModelInput, TOOL_CONTRACT, ToolCall,
    loop_grants, model_contract, tool_contract,
};
use phenix_graph::{
    ComponentDef, Failure as GraphFailure, Process, ProcessContext, ProcessEmission, Step, Value,
    compose::{Edit, Footprint, NetworkPatch},
    process::LocalProcessDispatcher,
};
use phenix_kernel::{BuildOptions, CallbackError, CompositionPolicy, Contribution, Key, host::KernelHost};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::{collections::VecDeque, num::NonZeroUsize, sync::Arc};

pub const MODEL_PLUGIN: &str = "mock-model";
pub const TOOL_PLUGIN: &str = "mock-tool";
pub const MODEL_PATH: &str = "model";
pub const TOOL_PATH: &str = "tool";

#[derive(Clone, Debug)]
pub struct MockModel {
    pub label: String,
    pub use_tool: bool,
}

impl Default for MockModel {
    fn default() -> Self {
        Self {
            label: "mock".into(),
            use_tool: true,
        }
    }
}

struct ModelProcess {
    config: MockModel,
    pending: VecDeque<ProcessEmission>,
    active: bool,
}

impl Process for ModelProcess {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, GraphFailure> {
        if let Some(emission) = self.pending.pop_front() {
            return Ok(Step::Emit(emission));
        }
        if self.active {
            self.active = false;
            return Ok(Step::Complete);
        }
        let Some(value) = context.take("input") else {
            return Ok(if context.is_eos("input") {
                Step::Finished
            } else {
                Step::Complete
            });
        };
        let input = value.borrow::<ModelInput>().ok_or_else(|| {
            GraphFailure::application_message("model expected a typed input")
        })?;
        self.active = true;
        match input {
            ModelInput::Prompt(prompt) if self.config.use_tool => {
                let call = ToolCall {
                    prompt: prompt.clone(),
                };
                self.pending.push_back(ProcessEmission::new(
                    "events",
                    Value::local(AgentEvent::ToolRequested(call.clone())),
                ));
                self.pending.push_back(ProcessEmission::new("tool", Value::local(call)));
            }
            ModelInput::Prompt(prompt) => self.answer(prompt),
            ModelInput::ToolResult { output, .. } => self.answer(output),
        }
        Ok(Step::Emit(self.pending.pop_front().expect("one or more emissions")))
    }
}

impl ModelProcess {
    fn answer(&mut self, text: &str) {
        for event in [
            AgentEvent::Delta(format!("{}: ", self.config.label)),
            AgentEvent::Delta(text.into()),
            AgentEvent::Completed,
        ] {
            self.pending.push_back(ProcessEmission::new("events", Value::local(event)));
        }
    }
}

impl PluginInstance for MockModel {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        let contract = model_contract();
        let config = self.clone();
        let definition = ComponentDef::primitive(
            contract.required().clone(),
            Arc::new(move || ModelProcess {
                config: config.clone(),
                pending: VecDeque::new(),
                active: false,
            }),
        )
        .claim(&contract)
        .expect("mock model uses the canonical interface");
        Contribution::new(
            "main",
            NetworkPatch::new([Edit::Add {
                path: MODEL_PATH.into(),
                definition: Arc::new(definition),
            }]),
        )
        .offer("model", MODEL_PATH, Key::new(LOOP_PLUGIN, MODEL_CONTRACT))
        .register(registrar)
    }
}

#[derive(Clone, Debug)]
pub struct MockTool {
    pub prefix: String,
}

impl Default for MockTool {
    fn default() -> Self {
        Self {
            prefix: "lookup".into(),
        }
    }
}

impl PluginInstance for MockTool {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        let prefix = self.prefix.clone();
        let definition = ComponentDef::map(move |call: ToolCall| ModelInput::ToolResult {
            output: format!("{prefix}({})", call.prompt),
            call,
        })
        .claim(&tool_contract())
        .expect("mock tool uses the canonical interface");
        Contribution::new(
            "main",
            NetworkPatch::new([Edit::Add {
                path: TOOL_PATH.into(),
                definition: Arc::new(definition),
            }]),
        )
        .offer("tool", TOOL_PATH, Key::new(LOOP_PLUGIN, TOOL_CONTRACT))
        .register(registrar)
    }
}

/// The host approves only the known provider placements and loop wiring.
/// One admitted request at a time avoids claiming independent process state.
pub fn options() -> BuildOptions {
    let mut options = BuildOptions::default();
    options.execution.max_requests = NonZeroUsize::new(1).unwrap();
    options.execution.capture_capacity = NonZeroUsize::new(1).unwrap();
    options.imports.allow = Some(
        [
            (Key::new(LOOP_PLUGIN, "model"), Key::new(MODEL_PLUGIN, "model")),
            (Key::new(LOOP_PLUGIN, "tool"), Key::new(TOOL_PLUGIN, "tool")),
        ]
        .into(),
    );
    for (owner, path) in [(MODEL_PLUGIN, MODEL_PATH), (TOOL_PLUGIN, TOOL_PATH)] {
        options.composition.insert(
            Key::new(owner, "main"),
            CompositionPolicy {
                grants: [Footprint::Component(path.into())].into(),
                ..Default::default()
            },
        );
    }
    options.composition.insert(
        Key::new(LOOP_PLUGIN, "main"),
        CompositionPolicy {
            grants: loop_grants(MODEL_PATH, TOOL_PATH).into(),
            ..Default::default()
        },
    );
    options
}

pub fn host(model: MockModel, tool: MockTool) -> Result<KernelHost, CallbackError> {
    host_with_options(model, tool, options())
}

pub fn host_with_options(
    model: MockModel,
    tool: MockTool,
    options: BuildOptions,
) -> Result<KernelHost, CallbackError> {
    let mut catalog = PluginCatalog::new();
    let loop_key = catalog.register(LOOP_PLUGIN, |_: ()| Ok(BasicLoop))?;
    let model_key = catalog.register(MODEL_PLUGIN, |model: MockModel| Ok(model))?;
    let tool_key = catalog.register(TOOL_PLUGIN, |tool: MockTool| Ok(tool))?;
    let mut host = KernelHost::new(catalog.instance());
    let stages = vec![
        host.plugins().prepare_load(&loop_key, ())?,
        host.plugins().prepare_load(&model_key, model)?,
        host.plugins().prepare_load(&tool_key, tool)?,
    ];
    let candidate = host.prepare(
        stages,
        options,
        Arc::new(LocalProcessDispatcher),
        "mock-agent",
        |_| Ok(()),
    )?;
    host.publish(candidate)?;
    Ok(host)
}
