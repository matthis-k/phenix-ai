//! Host composition of explicitly selected tools, resources and a model.
//! Both profiles use the same local loop; P2 adds ordinary environment routes.

use crate::{
    AgentContracts, AgentLoop, CONTRACTS_PLUGIN, LOOP_PLUGIN,
    environment::{EnvironmentRoutes, environment_grants, environment_tool_import},
    loop_grants,
    model::{
        context::{ModelContext, Skill},
        selection::ModelSelection,
    },
    tools::{SelectedTool, Tool, select_tools, tool_port},
};
use phenix_graph::{ValidatedResourceSet, compose::Footprint, runtime::RequestFailurePolicy};
use phenix_kernel::{BuildOptions, CallbackError, Key, OfferedComponent, host::KernelHost};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroUsize,
};

const MODEL: &str = "harness-model";
const ROUTES: &str = "harness-environments";

/// Host-selected tool declarations and grants. Tool code cannot select its
/// own capabilities. Empty environments reject every named call.
#[derive(Default)]
pub struct Environment {
    tools: Vec<(Tool, ValidatedResourceSet)>,
    plugins: Vec<ToolPlugin>,
}

struct ToolPlugin {
    owner: String,
    instance: Box<dyn PluginInstance>,
    selected: BTreeMap<String, ValidatedResourceSet>,
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

    /// Select ordinary named tool offers from an independent plugin.
    /// Preserve the original owner and placement; only selected components
    /// get write/resource grants. Other graph contributions need their own
    /// explicit policy and are not silently authorized.
    pub fn with_plugin(
        mut self,
        owner: impl Into<String>,
        instance: impl PluginInstance + 'static,
        selected: BTreeMap<String, ValidatedResourceSet>,
    ) -> Self {
        self.plugins.push(ToolPlugin {
            owner: owner.into(),
            instance: Box::new(instance),
            selected,
        });
        self
    }
}

/// Explicit host selection of an independent plugin and its trusted policy.
/// The policy callback receives selected model metadata, not a runtime handle.
/// Candidate validation still enforces every exact import and edit grant.
type ExtensionPolicy =
    Box<dyn FnOnce(&mut BuildOptions, &OfferedComponent) -> Result<(), CallbackError>>;

pub struct Extension {
    owner: String,
    instance: Box<dyn PluginInstance>,
    policy: ExtensionPolicy,
}

impl Extension {
    pub fn new(
        owner: impl Into<String>,
        instance: impl PluginInstance + 'static,
        policy: impl FnOnce(&mut BuildOptions, &OfferedComponent) -> Result<(), CallbackError> + 'static,
    ) -> Self {
        Self {
            owner: owner.into(),
            instance: Box::new(instance),
            policy: Box::new(policy),
        }
    }
}

/// Host policy independent of how plugins are loaded. A tool plugin, a skill
/// plugin, or a general extension can contribute selected model-visible skills.
#[derive(Default)]
pub struct Composition {
    pub extensions: Vec<Extension>,
    pub skills: BTreeSet<Key>,
}

struct ExtensionInstance(Box<dyn PluginInstance>);

impl PluginInstance for ExtensionInstance {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        self.0.contribute(registrar)
    }
}

/// Compose the explicitly selected tools and supply their native metadata to
/// the selected model factory. No model or tool is privileged. Resolve the
/// single model offer from the selected revision; ambiguity is an error.
/// Provider-specific JSON schema and transport remain the model plugin's job.
pub fn compose<M: PluginInstance + 'static>(
    make_model: impl FnOnce(&ModelContext) -> M,
    local: Environment,
    mut additional: BTreeMap<String, Environment>,
    composition: Composition,
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
    let mut extension_stages = Vec::new();
    for extension in composition.extensions {
        let key = catalog.register(&extension.owner, Ok)?;
        extension_stages.push((
            key,
            ExtensionInstance(extension.instance),
            extension.policy,
        ));
    }
    let mut options = BuildOptions::default();
    options.execution.max_requests = NonZeroUsize::new(1).unwrap();
    options.execution.capture_capacity = NonZeroUsize::new(1).unwrap();
    options.execution.request_failure_policy = RequestFailurePolicy::RestartAffectedProcess;
    options.imports.allow = Some(Default::default());
    let mut providers = Vec::new();
    let mut placements = BTreeMap::new();
    for (environment, mut configured) in additional {
        // Inline declarations are ordinary plugins too, not a second
        // registration path or a group that erases independent provenance.
        for (tool, resources) in configured.tools {
            let name = tool.name().to_owned();
            let path = format!("tool-{}-{environment}-{}", environment.len(), tool.path());
            let owner = format!("inline-{}-{environment}-{name}", environment.len());
            configured.plugins.push(ToolPlugin {
                owner,
                instance: Box::new(tool.placement(path)),
                selected: BTreeMap::from([(name, resources)]),
            });
        }
        placements.insert(environment.clone(), BTreeMap::new());
        for plugin in configured.plugins {
            let key = catalog.register(&plugin.owner, Ok)?;
            providers.push((
                environment.clone(),
                key,
                ExtensionInstance(plugin.instance),
                plugin.selected,
            ));
        }
    }
    let mut host = KernelHost::new(catalog.instance());
    let mut stages = vec![host.plugins().prepare_load(&contracts, ())?];
    let mut context = ModelContext::default();
    for (environment, key, instance, grants) in providers {
        let stage = host.plugins().prepare_load(&key, instance)?;
        let snapshot = stage
            .candidate()
            .ok_or_else(|| std::io::Error::other("selected tool plugin has no snapshot"))?;
        let mut discovered = SelectedTool::discover([snapshot])?;
        let selected = placements
            .get_mut(&environment)
            .expect("configured environment");
        for (name, resources) in grants {
            let tool = discovered.remove(&name).ok_or_else(|| {
                std::io::Error::other(format!("selected tool offer missing: {name}"))
            })?;
            let path = tool.path.clone();
            if selected.insert(name, tool).is_some() {
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
        }
        stages.push(stage);
    }
    let mut tool_placements = BTreeMap::new();
    for (environment, selected) in placements {
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
        context
            .tools
            .insert(environment.clone(), SelectedTool::catalog(&selected)?);
        tool_placements.insert(
            environment,
            selected
                .into_iter()
                .map(|(name, tool)| (name, tool.path))
                .collect::<BTreeMap<_, _>>(),
        );
    }

    // Stage all extensions before constructing the model. Skill selection
    // is independent of the plugin's other roles or contribution types.
    let mut extension_policies = Vec::new();
    for (key, instance, policy) in extension_stages {
        stages.push(host.plugins().prepare_load(&key, instance)?);
        extension_policies.push(policy);
    }
    let snapshots = host.plugins().candidate_snapshots(&stages)?;
    for key in composition.skills {
        let skill = snapshots
            .iter()
            .find(|snapshot| snapshot.name() == key.owner)
            .and_then(|snapshot| snapshot.get::<Skill>(&key.id))
            .ok_or_else(|| {
                std::io::Error::other(format!(
                    "selected skill missing: {}/{}",
                    key.owner, key.id
                ))
            })?;
        context.skills.insert(key, (*skill).clone());
    }
    let model = make_model(&context);
    let model_stage = host.plugins().prepare_load(&model_key, model)?;
    let snapshot = model_stage
        .candidate()
        .ok_or_else(|| std::io::Error::other("selected model revision has no snapshot"))?;
    let model_offer = ModelSelection::default().select([snapshot])?.component;
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
        .insert(model_import, model_offer.offer.clone());
    options
        .composition
        .entry(model_offer.producer.clone())
        .or_default()
        .grants
        .insert(Footprint::Component(model_offer.path.clone()));

    let local = tool_placements
        .remove("local")
        .expect("local environment inserted");
    options
        .composition
        .entry(Key::new(LOOP_PLUGIN, "main"))
        .or_default()
        .grants
        .extend(loop_grants(&model_offer.path, &local));
    stages.push(
        host.plugins()
            .prepare_load(&agent, AgentLoop::new(local.into_keys()))?,
    );
    stages.push(model_stage);
    if !tool_placements.is_empty() {
        options
            .composition
            .entry(Key::new(ROUTES, "main"))
            .or_default()
            .grants
            .extend(environment_grants(&tool_placements));
        stages.push(
            host.plugins().prepare_load(
                &routes_key,
                EnvironmentRoutes {
                    environments: tool_placements
                        .into_iter()
                        .map(|(environment, tools)| (environment, tools.into_keys().collect()))
                        .collect(),
                },
            )?,
        );
    }
    for policy in extension_policies {
        policy(&mut options, &model_offer)?;
    }
    let candidate = host.prepare(stages, options, "agent-harness", |_| Ok(()))?;
    host.publish(candidate)?;
    Ok(host)
}
