//! Optional named-tool selection inside the original local execution route.
//! Environment selection is orthogonal: it may wrap this route, but does
//! not need to know the names, implementations or grants of local tools.

use crate::{
    CONTRACTS_PLUGIN, LOOP_PLUGIN, TOOL_CONTRACT, TOOL_FEEDBACK, TOOL_ROUTER, ToolInvocation,
    ToolOutcome, ToolResult, UNAVAILABLE, capacity, invocation_router, port,
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

const ROUTER: &str = "agent-local-tool-router";

fn tool_port(name: &str) -> String {
    format!("tool-{name}")
}

/// Select independently offered tools by name. Keep the original local
/// provider as one named tool and retain its feedback connection.
/// Each additional implementation fulfills the same tool contract.
pub struct LocalTools {
    pub base: String,
    pub additional: BTreeSet<String>,
}

impl PluginInstance for LocalTools {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        let base = self.base.clone();
        let mut names = self.additional.clone();
        names.insert(base.clone());
        let mut contribution =
            Contribution::new("main", NetworkPatch::new([])).after([Key::new(LOOP_PLUGIN, "main")]);
        for name in &names {
            contribution =
                contribution.import(tool_port(name), Key::new(CONTRACTS_PLUGIN, TOOL_CONTRACT));
        }
        contribution
            .prepare(move |context| {
                let base_tool = context.bind_import(&tool_port(&base))?;
                let base_path = base_tool.path();
                let base_input = base_tool.input::<ToolInvocation>("input")?;
                let router = invocation_router(
                    names.clone(),
                    |call| &call.name,
                    tool_port,
                    |invocation| {
                        invocation.complete(ToolOutcome::Failed {
                            output: String::new(),
                            reason: "tool unavailable".into(),
                        })
                    },
                )?;
                let mut edits = vec![
                    Edit::Add {
                        path: ROUTER.into(),
                        definition: Arc::new(router),
                    },
                    Edit::Disconnect(ConnectionKey::new(
                        port(TOOL_ROUTER, "env-local"),
                        port(base_path, base_input.name()),
                    )),
                    Edit::Connect(ConnectionDef {
                        from: port(TOOL_ROUTER, "env-local"),
                        to: port(ROUTER, "input"),
                        capacity: capacity(),
                    }),
                    Edit::Connect(ConnectionDef {
                        from: port(ROUTER, UNAVAILABLE),
                        to: port(TOOL_FEEDBACK, "input"),
                        capacity: capacity(),
                    }),
                ];
                let mut feedback = BTreeSet::from([base_path.to_owned()]);
                for name in &names {
                    let output = tool_port(name);
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

/// Exact host grants for a selected base provider and named tool placements.
/// These do not grant any authority to the tools themselves.
pub fn local_tool_grants(base_path: &str, tools: &BTreeMap<String, String>) -> Vec<Footprint> {
    let mut grants = vec![
        Footprint::Component(ROUTER.into()),
        Footprint::Connection(ConnectionKey::new(
            port(TOOL_ROUTER, "env-local"),
            port(base_path, "input"),
        )),
        Footprint::Connection(ConnectionKey::new(
            port(TOOL_ROUTER, "env-local"),
            port(ROUTER, "input"),
        )),
        Footprint::Connection(ConnectionKey::new(
            port(ROUTER, UNAVAILABLE),
            port(TOOL_FEEDBACK, "input"),
        )),
    ];
    for (name, path) in tools {
        grants.push(Footprint::Connection(ConnectionKey::new(
            port(ROUTER, &tool_port(name)),
            port(path, "input"),
        )));
        if path != base_path {
            grants.push(Footprint::Connection(ConnectionKey::new(
                port(path, "output"),
                port(TOOL_FEEDBACK, "input"),
            )));
        }
    }
    grants
}
