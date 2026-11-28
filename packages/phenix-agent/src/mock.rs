//! Deterministic provider plugins and a host policy for the mock harness.
//! No network, credentials, shell, external effects, or durable history.

use crate::{
    AgentContracts, AgentLoop, CONTRACTS_PLUGIN, LOOP_PLUGIN, ModelInput, TOOL_CONTRACT, ToolCall,
    ToolInvocation, ToolOutcome, loop_grants,
    model::{Model, ModelBackend, ModelOutput, ModelProfile, ModelStream},
    tool_contract,
};
use phenix_graph::{
    ComponentDef, Failure as GraphFailure,
    compose::{Edit, Footprint, NetworkPatch},
    runtime::RequestFailurePolicy,
};
use phenix_kernel::{
    BuildOptions, CallbackError, CompositionPolicy, Contribution, Key, host::KernelHost,
};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::{collections::BTreeMap, num::NonZeroUsize, sync::Arc};

pub mod acp;

pub const MODEL_PLUGIN: &str = "mock-model";
pub const TOOL_PLUGIN: &str = "mock-tool";
pub const MODEL_PATH: &str = "model";
pub const TOOL_PATH: &str = "tool";

#[derive(Clone, Debug)]
pub struct MockModel {
    pub label: String,
    pub use_tool: bool,
    pub environment: String,
    pub environment_by_prompt: BTreeMap<String, String>,
    pub tool_by_prompt: BTreeMap<String, String>,
    pub arguments_by_prompt: BTreeMap<String, crate::ToolValue>,
    pub available_tools: BTreeMap<String, crate::tools::ToolDescriptor>,
    /// Deterministic continuation after a successful tool result. Keys are
    /// the completed call's prompt; the value is the next model tool call.
    pub follow_up_by_prompt: BTreeMap<String, ToolCall>,
}

impl Default for MockModel {
    fn default() -> Self {
        Self {
            label: "mock".into(),
            use_tool: true,
            environment: "local".into(),
            environment_by_prompt: BTreeMap::new(),
            tool_by_prompt: BTreeMap::new(),
            arguments_by_prompt: BTreeMap::new(),
            available_tools: BTreeMap::new(),
            follow_up_by_prompt: BTreeMap::new(),
        }
    }
}

impl ModelBackend for MockModel {
    fn respond(
        &mut self,
        input: ModelInput,
        _: &ModelProfile,
    ) -> Result<ModelStream, GraphFailure> {
        let response = match input {
            ModelInput::Prompt(prompt) if prompt == "list-tools" => {
                let names = self
                    .available_tools
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                self.answer(&names)
            }
            ModelInput::Prompt(prompt) if self.use_tool => {
                let call = ToolCall {
                    name: self
                        .tool_by_prompt
                        .get(&prompt)
                        .cloned()
                        .unwrap_or_else(|| "lookup".into()),
                    arguments: self
                        .arguments_by_prompt
                        .get(&prompt)
                        .cloned()
                        .unwrap_or_else(|| prompt.clone().into()),
                    environment: self
                        .environment_by_prompt
                        .get(&prompt)
                        .unwrap_or(&self.environment)
                        .clone(),
                };
                outputs([ModelOutput::Tool(call)])
            }
            ModelInput::Prompt(prompt) => self.answer(&prompt),
            ModelInput::ToolResult(result) => match result.outcome() {
                ToolOutcome::Succeeded { output } => {
                    let next = result
                        .invocation()
                        .call()
                        .arguments
                        .as_text()
                        .and_then(|text| self.follow_up_by_prompt.get(text))
                        .cloned();
                    if let Some(next) = next {
                        outputs([ModelOutput::Tool(next)])
                    } else {
                        self.answer(output.as_text().unwrap_or("structured tool result"))
                    }
                }
                ToolOutcome::Failed { output, reason } => self.answer(&format!(
                    "tool failed: {reason}: {}",
                    output.as_text().unwrap_or("structured tool result")
                )),
            },
        };
        Ok(response)
    }
}

fn outputs<const N: usize>(items: [ModelOutput; N]) -> ModelStream {
    Box::new(items.into_iter().map(Ok))
}

impl MockModel {
    fn answer(&self, text: &str) -> ModelStream {
        outputs([
            ModelOutput::Delta(format!("{}: ", self.label)),
            ModelOutput::Delta(text.into()),
            ModelOutput::Complete,
        ])
    }
}

impl PluginInstance for MockModel {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        let config = self.clone();
        Model::new(
            MODEL_PATH,
            ModelProfile::new("mock", &self.label)
                .capability("text")
                .capability("tools"),
            move || config.clone(),
        )
        .contribute(registrar)
    }
}

#[derive(Clone, Debug)]
pub struct MockTool {
    pub prefix: String,
    pub path: String,
}

impl Default for MockTool {
    fn default() -> Self {
        Self {
            prefix: "lookup".into(),
            path: TOOL_PATH.into(),
        }
    }
}

impl PluginInstance for MockTool {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        let prefix = self.prefix.clone();
        let definition = ComponentDef::map(move |invocation: ToolInvocation| {
            let outcome = match invocation.call().arguments.as_text() {
                Some(text) => ToolOutcome::Succeeded {
                    output: format!("{prefix}({text})").into(),
                },
                None => ToolOutcome::Failed {
                    output: String::new().into(),
                    reason: "expected text arguments".into(),
                },
            };
            invocation.complete(outcome)
        })
        .claim(&tool_contract())
        .expect("mock tool uses the canonical interface");
        Contribution::new(
            "main",
            NetworkPatch::new([Edit::Add {
                path: self.path.clone(),
                definition: Arc::new(definition),
            }]),
        )
        .offer(
            "tool",
            &self.path,
            Key::new(CONTRACTS_PLUGIN, TOOL_CONTRACT),
        )
        .register(registrar)
    }
}

/// The host approves only the known provider placements and loop wiring.
/// One admitted request at a time avoids claiming independent process state.
/// Stateless mock processes can be recreated on request failure or cancellation.
pub fn options() -> BuildOptions {
    let mut options = BuildOptions::default();
    options.execution.max_requests = NonZeroUsize::new(1).unwrap();
    options.execution.request_failure_policy = RequestFailurePolicy::RestartAffectedProcess;
    options.execution.capture_capacity = NonZeroUsize::new(1).unwrap();
    options.imports.allow = Some(
        [
            (
                Key::new(LOOP_PLUGIN, "model"),
                Key::new(MODEL_PLUGIN, "model"),
            ),
            (
                Key::new(LOOP_PLUGIN, "tool-lookup"),
                Key::new(TOOL_PLUGIN, "tool"),
            ),
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
            grants: loop_grants(MODEL_PATH, &[("lookup".into(), TOOL_PATH.into())].into())
                .into_iter()
                .collect(),
            ..Default::default()
        },
    );
    options
}

pub fn host(model: MockModel, tool: MockTool) -> Result<KernelHost, CallbackError> {
    host_with_options(model, tool, options())
}

/// Compose independently authored providers under the same host-selected roles.
/// Providers must offer the canonical contracts and stay within host grants.
pub fn host_with_options(
    model: impl PluginInstance + 'static,
    tool: impl PluginInstance + 'static,
    options: BuildOptions,
) -> Result<KernelHost, CallbackError> {
    compose_mock(model, tool, options, BTreeMap::new())
}

/// Compose a mock loop with additional independently selected tool environments.
/// The local provider, model and base loop are unchanged. The fixture supplies
/// exact grants for these configured mock placements, not for arbitrary plugins.
pub fn host_with_environments(
    model: MockModel,
    environments: BTreeMap<String, MockTool>,
) -> Result<KernelHost, CallbackError> {
    compose_mock(model, MockTool::default(), options(), environments)
}

fn compose_mock(
    model: impl PluginInstance + 'static,
    tool: impl PluginInstance + 'static,
    mut options: BuildOptions,
    environments: BTreeMap<String, MockTool>,
) -> Result<KernelHost, CallbackError> {
    use crate::environment::{EnvironmentRoutes, environment_grants, environment_tool_import};

    const ROUTES_PLUGIN: &str = "mock-environment-routes";
    let mut catalog = PluginCatalog::new();
    let contracts_key = catalog.register(CONTRACTS_PLUGIN, |_: ()| Ok(AgentContracts))?;
    let loop_key = catalog.register(LOOP_PLUGIN, Ok)?;
    let model_key = catalog.register(MODEL_PLUGIN, Ok)?;
    let tool_key = catalog.register(TOOL_PLUGIN, Ok)?;
    let mut providers = Vec::new();
    let mut placements = BTreeMap::new();
    for (environment, provider) in environments {
        if environment == "local" {
            return Err(std::io::Error::other("local is already provided by the base loop").into());
        }
        let owner = format!("mock-env-{environment}");
        let key = catalog.register(&owner, Ok)?;
        options
            .imports
            .allow
            .as_mut()
            .expect("mock import policy")
            .insert((
                Key::new(
                    ROUTES_PLUGIN,
                    environment_tool_import(&environment, "lookup"),
                ),
                Key::new(&owner, "tool"),
            ));
        options.composition.insert(
            Key::new(&owner, "main"),
            CompositionPolicy {
                grants: [Footprint::Component(provider.path.clone())].into(),
                ..Default::default()
            },
        );
        placements.insert(
            environment,
            BTreeMap::from([("lookup".into(), provider.path.clone())]),
        );
        providers.push((key, provider));
    }
    let routes_key = if placements.is_empty() {
        None
    } else {
        Some(catalog.register(ROUTES_PLUGIN, Ok)?)
    };
    let mut host = KernelHost::new(catalog.instance());
    let mut stages = vec![
        host.plugins().prepare_load(&contracts_key, ())?,
        host.plugins()
            .prepare_load(&loop_key, AgentLoop::new(["lookup"]))?,
        host.plugins().prepare_load(&model_key, model)?,
        host.plugins().prepare_load(&tool_key, tool)?,
    ];
    for (key, provider) in providers {
        stages.push(host.plugins().prepare_load(&key, provider)?);
    }
    if let Some(key) = routes_key {
        options.composition.insert(
            Key::new(ROUTES_PLUGIN, "main"),
            CompositionPolicy {
                grants: environment_grants(&placements).into_iter().collect(),
                ..Default::default()
            },
        );
        stages.push(
            host.plugins().prepare_load(
                &key,
                EnvironmentRoutes {
                    environments: placements
                        .into_iter()
                        .map(|(environment, tools)| (environment, tools.into_keys().collect()))
                        .collect(),
                },
            )?,
        );
    }
    let candidate = host.prepare(stages, options, "mock-agent", |_| Ok(()))?;
    host.publish(candidate)?;
    Ok(host)
}
