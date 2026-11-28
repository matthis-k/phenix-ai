//! Host composition for the first real single-tool-per-environment slice.
//! The model remains an independently supplied plugin. Both profiles use
//! the same agent loop; extra environments are ordinary graph contributions.

use crate::{
    AgentContracts, AgentLoop, CONTRACTS_PLUGIN, LOOP_PLUGIN,
    environment::{EnvironmentRoutes, environment_grants},
    loop_grants,
    tools::Tool,
};
use phenix_graph::{ValidatedResourceSet, compose::Footprint, runtime::RequestFailurePolicy};
use phenix_kernel::{BuildOptions, CallbackError, Key, host::KernelHost};
use phenix_plugin::{PluginCatalog, PluginInstance};
use std::{collections::BTreeMap, num::NonZeroUsize};

const MODEL: &str = "harness-model";
const LOCAL: &str = "harness-local";
const ROUTES: &str = "harness-environments";

/// Host-selected tool and capabilities; no tool may choose its own grants.
pub struct Environment {
    pub tool: Tool,
    pub resources: ValidatedResourceSet,
}

/// The first real harness accepts one explicitly selected tool per environment.
/// Additional named tools can still be composed with LocalTools. This helper
/// intentionally does not infer trust from arbitrary plugin discovery.
/// The caller supplies a model offering the model contract from "main"/"model".
pub fn compose(
    model: impl PluginInstance + 'static,
    model_path: &str,
    local: Environment,
    additional: BTreeMap<String, Environment>,
) -> Result<KernelHost, CallbackError> {
    if additional.contains_key("local") {
        return Err(std::io::Error::other("local environment is already selected").into());
    }
    let mut catalog = PluginCatalog::new();
    let contracts = catalog.register(CONTRACTS_PLUGIN, |_: ()| Ok(AgentContracts))?;
    let agent = catalog.register(LOOP_PLUGIN, Ok)?;
    let model_key = catalog.register(MODEL, Ok)?;
    let local_key = catalog.register(LOCAL, Ok)?;
    let routes_key = catalog.register(ROUTES, Ok)?;
    let mut options = BuildOptions::default();
    options.execution.max_requests = NonZeroUsize::new(1).unwrap();
    options.execution.capture_capacity = NonZeroUsize::new(1).unwrap();
    options.execution.request_failure_policy = RequestFailurePolicy::RestartAffectedProcess;
    options.imports.allow = Some(Default::default());
    let local_path = local.tool.path().to_owned();
    select(
        &mut options,
        Key::new(LOOP_PLUGIN, "model"),
        Key::new(MODEL, "model"),
        Key::new(MODEL, "main"),
        model_path,
    );
    select(
        &mut options,
        Key::new(LOOP_PLUGIN, "env-local"),
        Key::new(LOCAL, local.tool.name()),
        Key::new(LOCAL, local.tool.name()),
        &local_path,
    );
    options.execution.resources.insert(local_path.clone(), local.resources);
    options
        .composition
        .entry(Key::new(LOOP_PLUGIN, "main"))
        .or_default()
        .grants
        .extend(loop_grants(model_path, &local_path));

    let mut providers = Vec::new();
    let mut placements = BTreeMap::new();
    for (environment, provider) in additional {
        let owner = format!("harness-env-{environment}");
        let key = catalog.register(&owner, Ok)?;
        let path = provider.tool.path().to_owned();
        select(
            &mut options,
            Key::new(ROUTES, format!("env-{environment}")),
            Key::new(&owner, provider.tool.name()),
            Key::new(&owner, provider.tool.name()),
            &path,
        );
        if options.execution.resources.insert(path.clone(), provider.resources).is_some() {
            return Err(std::io::Error::other("duplicate environment placement").into());
        }
        placements.insert(environment, path);
        providers.push((key, provider.tool));
    }
    let mut host = KernelHost::new(catalog.instance());
    let mut stages = vec![
        host.plugins().prepare_load(&contracts, ())?,
        host.plugins().prepare_load(&agent, AgentLoop)?,
        host.plugins().prepare_load(&model_key, model)?,
        host.plugins().prepare_load(&local_key, local.tool)?,
    ];
    for (key, tool) in providers {
        stages.push(host.plugins().prepare_load(&key, tool)?);
    }
    if !placements.is_empty() {
        options
            .composition
            .entry(Key::new(ROUTES, "main"))
            .or_default()
            .grants
            .extend(environment_grants(&placements));
        stages.push(host.plugins().prepare_load(
            &routes_key,
            EnvironmentRoutes {
                environments: placements.into_keys().collect(),
            },
        )?);
    }
    let candidate = host.prepare(stages, options, "agent-harness", |_| Ok(()))?;
    host.publish(candidate)?;
    Ok(host)
}

fn select(
    options: &mut BuildOptions,
    import: Key,
    offer: Key,
    producer: Key,
    path: &str,
) {
    options
        .imports
        .allow
        .as_mut()
        .expect("explicit host selection policy")
        .insert((import.clone(), offer.clone()));
    options.imports.selections.insert(import, offer);
    options
        .composition
        .entry(producer)
        .or_default()
        .grants
        .insert(Footprint::Component(path.into()));
}
