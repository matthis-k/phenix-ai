//! Host composition of explicitly selected tools, resources and a model.
//! Both profiles use the same local loop; P2 adds ordinary environment routes.

use crate::{
    AgentContracts, AgentLoop, CONTRACTS_PLUGIN, LOOP_PLUGIN, MODEL_CONTRACT,
    environment::{EnvironmentRoutes, environment_grants, environment_tool_import},
    loop_grants,
    tools::{SelectedTool, Tool, ToolDescriptor, select_tools, tool_port},
};
use phenix_graph::{ValidatedResourceSet, compose::Footprint, runtime::RequestFailurePolicy};
use phenix_kernel::{BuildOptions, CallbackError, Key, component_offers, host::KernelHost};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::{collections::BTreeMap, num::NonZeroUsize};

const MODEL: &str = "harness-model";
const ROUTES: &str = "harness-environments";

/// A native model catalog of the host's selected tools, grouped by environment.
pub type ToolCatalog = BTreeMap<String, BTreeMap<String, ToolDescriptor>>;

/// Host-selected tool declarations and grants. Tool code cannot select its
/// own capabilities. Empty environments reject every named call.
#[derive(Default)]
pub struct Environment {
    tools: Vec<(Tool, ValidatedResourceSet)>,
}

impl Environment {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_tool(self, tool: Tool) -> Self {
        self.with_resources(tool, ValidatedResourceSet::default())
    }

    pub fn with_resources(mut self, tool: Tool, resources: ValidatedResourceSet) -> Self {
        self.tools.push((tool, resources));
        self
    }
}

struct ToolGroup(Vec<Tool>);

impl PluginInstance for ToolGroup {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        for tool in &self.0 {
            tool.clone().register(registrar)?;
        }
        Ok(())
    }
}

/// Compose the explicitly selected tools and supply their native metadata to
/// the selected model factory. No model or tool is privileged. Resolve the
/// single model offer from the selected revision; ambiguity is an error.
/// Provider-specific JSON schema and transport remain the model plugin's job.
pub fn compose<M: PluginInstance + 'static>(
    make_model: impl FnOnce(&ToolCatalog) -> M,
    local: Environment,
    mut additional: BTreeMap<String, Environment>,
) -> Result<KernelHost, CallbackError> {
    if additional.contains_key("local") {
        return Err(std::io::Error::other("local environment is already selected").into());
    }
    additional.insert("local".into(), local);
    let mut catalog = PluginCatalog::new();
    let contracts = catalog.register(CONTRACTS_PLUGIN, |_: ()| Ok(AgentContracts))?;
    let agent = catalog.register(LOOP_PLUGIN, Ok)?;
    let model_key = catalog.register(MODEL, Ok)?;
    let routes_key = catalog.register(ROUTES, Ok)?;
    let mut options = BuildOptions::default();
    options.execution.max_requests = NonZeroUsize::new(1).unwrap();
    options.execution.capture_capacity = NonZeroUsize::new(1).unwrap();
    options.execution.request_failure_policy = RequestFailurePolicy::RestartAffectedProcess;
    options.imports.allow = Some(Default::default());
    let mut providers = Vec::new();
    let mut placements = BTreeMap::new();
    let mut tool_catalog = ToolCatalog::new();
    for (environment, configured) in additional {
        let owner = format!("harness-env-{environment}");
        let key = catalog.register(&owner, Ok)?;
        let mut selected = BTreeMap::new();
        let mut group = Vec::new();
        for (tool, resources) in configured.tools {
            let name = tool.name().to_owned();
            // A tool's placement is relative to the host's environment.
            let path = format!("tool-{}-{environment}-{}", environment.len(), tool.path());
            let tool = tool.placement(&path);
            let offer = Key::new(&owner, &name);
            let selection = SelectedTool {
                offer: offer.clone(),
                producer: offer,
                path: path.clone(),
                descriptor: Some(tool.descriptor().clone()),
            };
            if selected.insert(name, selection).is_some() {
                return Err(std::io::Error::other("duplicate tool name in environment").into());
            }
            if options
                .execution
                .resources
                .insert(path, resources)
                .is_some()
            {
                return Err(std::io::Error::other("duplicate tool placement").into());
            }
            group.push(tool);
        }
        for tool in selected.values() {
            let (producer, footprint) = tool.required_component();
            options
                .composition
                .entry(producer)
                .or_default()
                .grants
                .insert(footprint);
        }
        if environment == "local" {
            select_tools(&mut options, LOOP_PLUGIN, &selected, tool_port);
        } else {
            select_tools(&mut options, ROUTES, &selected, |name| {
                environment_tool_import(&environment, name)
            });
        }
        tool_catalog.insert(environment.clone(), SelectedTool::catalog(&selected)?);
        placements.insert(
            environment,
            selected
                .into_iter()
                .map(|(name, tool)| (name, tool.path))
                .collect::<BTreeMap<_, _>>(),
        );
        providers.push((key, ToolGroup(group)));
    }
    let model = make_model(&tool_catalog);
    let mut host = KernelHost::new(catalog.instance());
    let model_stage = host.plugins().prepare_load(&model_key, model)?;
    let snapshot = model_stage
        .candidate()
        .ok_or_else(|| std::io::Error::other("selected model revision has no snapshot"))?;
    let model_contract = Key::new(CONTRACTS_PLUGIN, MODEL_CONTRACT);
    let mut offers = component_offers(&snapshot)
        .into_iter()
        .filter(|offer| offer.contract == model_contract);
    let model_offer = offers
        .next()
        .ok_or_else(|| std::io::Error::other("selected model offers no model contract"))?;
    if offers.next().is_some() {
        return Err(std::io::Error::other("selected model has ambiguous offers").into());
    }
    let model_import = Key::new(LOOP_PLUGIN, "model");
    options
        .imports
        .allow
        .as_mut()
        .expect("explicit host policy")
        .insert((model_import.clone(), model_offer.offer.clone()));
    options
        .imports
        .selections
        .insert(model_import, model_offer.offer);
    options
        .composition
        .entry(model_offer.producer)
        .or_default()
        .grants
        .insert(Footprint::Component(model_offer.path.clone()));

    let local = placements
        .remove("local")
        .expect("local environment inserted");
    options
        .composition
        .entry(Key::new(LOOP_PLUGIN, "main"))
        .or_default()
        .grants
        .extend(loop_grants(&model_offer.path, &local));
    let mut stages = vec![
        host.plugins().prepare_load(&contracts, ())?,
        host.plugins()
            .prepare_load(&agent, AgentLoop::new(local.into_keys()))?,
        model_stage,
    ];
    for (key, group) in providers {
        stages.push(host.plugins().prepare_load(&key, group)?);
    }
    if !placements.is_empty() {
        options
            .composition
            .entry(Key::new(ROUTES, "main"))
            .or_default()
            .grants
            .extend(environment_grants(&placements));
        stages.push(
            host.plugins().prepare_load(
                &routes_key,
                EnvironmentRoutes {
                    environments: placements
                        .into_iter()
                        .map(|(environment, tools)| (environment, tools.into_keys().collect()))
                        .collect(),
                },
            )?,
        );
    }
    let candidate = host.prepare(stages, options, "agent-harness", |_| Ok(()))?;
    host.publish(candidate)?;
    Ok(host)
}
