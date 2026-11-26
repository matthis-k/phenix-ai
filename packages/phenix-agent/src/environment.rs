//! Optional application-level environment routing. The base loop and tool
//! providers need no knowledge of the extension.

use super::{
    CONTRACTS_PLUGIN, LOOP_PLUGIN, TOOL_CONTRACT, TOOL_FEEDBACK, TOOL_INVOCATION, TOOL_ROUTER,
    ToolInvocation, ToolResult, UNAVAILABLE, capacity, environment_port, port, tool_router,
};
use phenix_graph::{
    compose::{ConnectionKey, Edit, Footprint, NetworkPatch},
    network::ConnectionDef,
};
use phenix_kernel::{CandidateArtifacts, Contribution, Key};
use phenix_plugin::{Failure, PluginInstance, Registrar};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

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
            .prepare(move |context| {
                let router = tool_router(
                    environments.clone(),
                    std::convert::identity::<ToolInvocation>,
                )?;
                let mut edits = vec![
                    Edit::Add {
                        path: ROUTER.into(),
                        definition: Arc::new(router),
                    },
                    Edit::Disconnect(ConnectionKey::new(
                        port(TOOL_INVOCATION, "output"),
                        port(TOOL_ROUTER, "input"),
                    )),
                    Edit::Connect(ConnectionDef {
                        from: port(TOOL_INVOCATION, "output"),
                        to: port(ROUTER, "input"),
                        capacity: capacity(),
                    }),
                    Edit::Connect(ConnectionDef {
                        from: port(ROUTER, UNAVAILABLE),
                        to: port(TOOL_ROUTER, "input"),
                        capacity: capacity(),
                    }),
                ];
                let mut feedback = BTreeSet::new();
                for environment in &environments {
                    let output = environment_port(environment);
                    let tool = context.bind_import(&output)?;
                    let input = tool.input::<ToolInvocation>("input")?;
                    let result = tool.output::<ToolResult>("output")?;
                    edits.push(Edit::Connect(ConnectionDef {
                        from: port(ROUTER, &output),
                        to: port(tool.path(), input.name()),
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

/// Exact host-approved edits. Additional providers have distinct placements
/// from the base tool; a conflicting duplicate feedback edge is rejected.
pub fn environment_grants(environments: &BTreeMap<String, String>) -> Vec<Footprint> {
    let mut grants = vec![
        Footprint::Component(ROUTER.into()),
        Footprint::Connection(ConnectionKey::new(
            port(TOOL_INVOCATION, "output"),
            port(TOOL_ROUTER, "input"),
        )),
        Footprint::Connection(ConnectionKey::new(
            port(TOOL_INVOCATION, "output"),
            port(ROUTER, "input"),
        )),
        Footprint::Connection(ConnectionKey::new(
            port(ROUTER, UNAVAILABLE),
            port(TOOL_ROUTER, "input"),
        )),
    ];
    for (environment, tool) in environments {
        grants.push(Footprint::Connection(ConnectionKey::new(
            port(ROUTER, &environment_port(environment)),
            port(tool, "input"),
        )));
        grants.push(Footprint::Connection(ConnectionKey::new(
            port(tool, "output"),
            port(TOOL_FEEDBACK, "input"),
        )));
    }
    grants
}
