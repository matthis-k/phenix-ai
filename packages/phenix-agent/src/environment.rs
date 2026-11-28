//! Optional application-level environment routing. The base loop and tool
//! providers need no knowledge of the extension.

use super::{
    CONTRACTS_PLUGIN, LOOP_PLUGIN, TOOL_CONTRACT, TOOL_FEEDBACK, TOOL_INVOCATION, TOOL_ROUTER,
    ToolInvocation, ToolResult, UNAVAILABLE, capacity, environment_port, tool_router,
};
use phenix_graph::compose::{Edit, Footprint, NetworkPatch};
use phenix_kernel::{Contribution, Key};
use phenix_plugin::{Failure, PluginInstance, Registrar};
use std::collections::{BTreeMap, BTreeSet};

const ROUTER: &str = "agent-environment-router";

/// Route selected environments through independently resolved tool providers.
/// Unmatched invocations continue down the original local route. This is the
/// routing slice, not an implementation of remote execution or tool context.
pub struct EnvironmentRoutes {
    pub environments: BTreeSet<String>,
}

impl PluginInstance for EnvironmentRoutes {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        let environments = self.environments.clone();
        let mut contribution =
            Contribution::new("main", NetworkPatch::new([])).after([Key::new(LOOP_PLUGIN, "main")]);
        for environment in &environments {
            contribution = contribution.import(
                environment_port(environment),
                Key::new(CONTRACTS_PLUGIN, TOOL_CONTRACT),
            );
        }
        contribution
            .prepare_patch(move |context| {
                let router = tool_router(
                    environments.clone(),
                    std::convert::identity::<ToolInvocation>,
                )?;
                let mut placements = BTreeMap::new();
                for environment in &environments {
                    let tool = context.bind_import(&environment_port(environment))?;
                    tool.input::<ToolInvocation>("input")?;
                    tool.output::<ToolResult>("output")?;
                    placements.insert(environment.clone(), tool.path().to_owned());
                }
                let mut edits = vec![Edit::add(ROUTER, router)];
                edits.extend(route_edges(&placements));
                Ok(NetworkPatch::new(edits))
            })
            .register(registrar)
    }
}

// Both candidate preparation and trusted host planning use these declarations.
fn route_edges(environments: &BTreeMap<String, String>) -> Vec<Edit> {
    let mut edits = vec![
        Edit::disconnect((TOOL_INVOCATION, "output"), (TOOL_ROUTER, "input")),
        Edit::connect((TOOL_INVOCATION, "output"), (ROUTER, "input"), capacity()),
        Edit::connect((ROUTER, UNAVAILABLE), (TOOL_ROUTER, "input"), capacity()),
    ];
    let mut feedback = BTreeSet::new();
    for (environment, path) in environments {
        edits.push(Edit::connect(
            (ROUTER, environment_port(environment)),
            (path, "input"),
            capacity(),
        ));
        if feedback.insert(path) {
            edits.push(Edit::connect(
                (path, "output"),
                (TOOL_FEEDBACK, "input"),
                capacity(),
            ));
        }
    }
    edits
}

/// Exact host-approved edits. Additional providers have distinct placements
/// from the base tool; a conflicting duplicate feedback edge is rejected.
pub fn environment_grants(environments: &BTreeMap<String, String>) -> Vec<Footprint> {
    let mut grants = vec![Footprint::Component(ROUTER.into())];
    grants.extend(NetworkPatch::new(route_edges(environments)).footprints());
    grants
}
