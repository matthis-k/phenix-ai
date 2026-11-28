//! Optional environment routing. Each environment uses the same named-tool
//! dispatch as the local loop. The extension never edits local tool selection.

use super::{
    LOOP_PLUGIN, TOOL_INVOCATION, TOOL_ROUTER, UNAVAILABLE, capacity, environment_port, tool_router,
    tools::{bind_tools, declare_tools, named_edges, named_grants, named_router},
};
use phenix_graph::compose::{Edit, Footprint, NetworkPatch};
use phenix_kernel::Contribution;
use phenix_plugin::{Code, Failure, PluginInstance, Registrar};
use std::collections::{BTreeMap, BTreeSet};

const ROUTER: &str = "agent-environment-router";

fn router_path(environment: &str) -> String {
    format!("agent-env-{environment}-tools")
}

/// Length-prefix the environment so arbitrary tool and environment names cannot
/// alias an import merely by containing the separator.
pub fn environment_tool_import(environment: &str, name: &str) -> String {
    format!("env-{}-{environment}-tool-{name}", environment.len())
}

/// Additional environment -> selected tool names. Empty sets are valid and
/// reject every call with a tool-unavailable result.
pub struct EnvironmentRoutes {
    pub environments: BTreeMap<String, BTreeSet<String>>,
}

impl PluginInstance for EnvironmentRoutes {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        if self.environments.contains_key("local") {
            return Err(Failure::new(
                Code::DescriptorInvalid,
                "local is owned by the loop",
            ));
        }
        let environments = self.environments.clone();
        let mut contribution = Contribution::new("main", NetworkPatch::new([]))
            .after([phenix_kernel::Key::new(LOOP_PLUGIN, "main")]);
        for (environment, names) in &environments {
            contribution = declare_tools(contribution, names, |name| {
                environment_tool_import(environment, name)
            });
        }
        contribution
            .prepare_patch(move |context| {
                let router = tool_router(
                    environments.keys().cloned().collect(),
                    std::convert::identity,
                )?;
                let mut edits = vec![Edit::add(ROUTER, router)];
                edits.extend(route_edges(environments.keys()));
                for (environment, names) in &environments {
                    let path = router_path(environment);
                    let placements = bind_tools(context, names, |name| {
                        environment_tool_import(environment, name)
                    })?;
                    edits.push(Edit::add(&path, named_router(names.clone())?));
                    edits.extend(named_edges(&path, &placements));
                }
                Ok(NetworkPatch::new(edits))
            })
            .register(registrar)
    }
}

fn route_edges<'a>(environments: impl Iterator<Item = &'a String>) -> Vec<Edit> {
    let mut edits = vec![
        Edit::disconnect((TOOL_INVOCATION, "output"), (TOOL_ROUTER, "input")),
        Edit::connect((TOOL_INVOCATION, "output"), (ROUTER, "input"), capacity()),
        Edit::connect((ROUTER, UNAVAILABLE), (TOOL_ROUTER, "input"), capacity()),
    ];
    for environment in environments {
        edits.push(Edit::connect(
            (ROUTER, environment_port(environment)),
            (router_path(environment), "input"),
            capacity(),
        ));
    }
    edits
}

/// Exact required writes for the host's selected environments and placements.
pub fn environment_grants(
    environments: &BTreeMap<String, BTreeMap<String, String>>,
) -> Vec<Footprint> {
    let mut grants = vec![Footprint::Component(ROUTER.into())];
    grants.extend(NetworkPatch::new(route_edges(environments.keys())).footprints());
    for (environment, tools) in environments {
        grants.extend(named_grants(&router_path(environment), tools));
    }
    grants
}
