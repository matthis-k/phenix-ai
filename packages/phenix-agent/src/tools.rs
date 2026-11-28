//! Optional named-tool selection inside the original local execution route.
//! Environment selection is orthogonal: it may wrap this route, but does
//! not need to know the names, implementations or grants of local tools.

use crate::{
    CONTRACTS_PLUGIN, LOOP_PLUGIN, TOOL_CONTRACT, TOOL_FEEDBACK, TOOL_ROUTER, ToolInvocation,
    ToolOutcome, ToolResult, UNAVAILABLE, capacity, invocation_router, port,
};
use phenix_graph::{
    ComponentDef,
    compose::{ConnectionKey, Edit, Footprint, NetworkPatch},
};
use phenix_kernel::{BuildOptions, Contribution, Key};
use phenix_plugin::{Code, Failure, PluginInstance, Registrar};
use std::collections::{BTreeMap, BTreeSet};

/// A named synchronous tool using the current mock text argument contract.
/// Registration derives the component, claim, offer and invocation correlation.
/// A domain error is returned as tool feedback rather than a scheduler failure.
/// Complex tools can register their own component under the same contract.
pub struct Tool {
    name: String,
    definition: ComponentDef,
}

impl Tool {
    pub fn new(
        name: impl Into<String>,
        execute: impl Fn(&str) -> Result<String, String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            definition: ComponentDef::map(move |invocation: ToolInvocation| {
                let outcome = match execute(&invocation.call().prompt) {
                    Ok(output) => ToolOutcome::Succeeded { output },
                    Err(reason) => ToolOutcome::Failed {
                        output: String::new(),
                        reason,
                    },
                };
                invocation.complete(outcome)
            }),
        }
    }

    /// Advanced tools can supply an ordinary graph component instead.
    /// The same checked offer and host authorization apply.
    pub fn from_component(name: impl Into<String>, definition: ComponentDef) -> Self {
        Self {
            name: name.into(),
            definition,
        }
    }

    /// Use the tool's name as its contribution, placement and offer identity.
    /// Selecting and granting this contribution is a separate host decision.
    pub fn register(self, registrar: &mut Registrar) -> Result<(), Failure> {
        Contribution::offered_component(
            &self.name,
            &self.name,
            &self.name,
            Key::new(CONTRACTS_PLUGIN, TOOL_CONTRACT),
            &crate::tool_contract(),
            self.definition,
        )?
        .register(registrar)
    }
}

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

/// A host-selected provider, not a claim of write authority by the tool.
pub struct SelectedTool {
    pub offer: Key,
    pub path: String,
}

impl LocalTools {
    /// Derive dispatch imports and exact routing grants from one trusted
    /// selection. Existing deny rules still win. Provider component grants
    /// remain separate; this only authorizes the local routing contribution.
    pub fn configure(
        options: &mut BuildOptions,
        owner: &str,
        base: &str,
        providers: BTreeMap<String, SelectedTool>,
    ) -> Result<Self, Failure> {
        let Some(base_tool) = providers.get(base) else {
            return Err(Failure::new(Code::DescriptorInvalid, "missing base tool"));
        };
        let placements = providers
            .iter()
            .map(|(name, provider)| (name.clone(), provider.path.clone()))
            .collect();
        let grants = local_tool_grants(&base_tool.path, &placements);
        let additional = providers
            .keys()
            .filter(|name| name.as_str() != base)
            .cloned()
            .collect();
        for (name, provider) in providers {
            let import = Key::new(owner, tool_port(&name));
            if let Some(allow) = &mut options.imports.allow {
                allow.insert((import.clone(), provider.offer.clone()));
            }
            options.imports.selections.insert(import, provider.offer);
        }
        options
            .composition
            .entry(Key::new(owner, "main"))
            .or_default()
            .grants
            .extend(grants);
        Ok(Self {
            base: base.into(),
            additional,
        })
    }
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
            .prepare_patch(move |context| {
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
                    Edit::add(ROUTER, router),
                    Edit::disconnect(
                        (TOOL_ROUTER, "env-local"),
                        (base_path, base_input.name()),
                    ),
                    Edit::connect((TOOL_ROUTER, "env-local"), (ROUTER, "input"), capacity()),
                    Edit::connect((ROUTER, UNAVAILABLE), (TOOL_FEEDBACK, "input"), capacity()),
                ];
                let mut feedback = BTreeSet::from([base_path.to_owned()]);
                for name in &names {
                    let output = tool_port(name);
                    let tool = context.bind_import(&output)?;
                    let input = tool.input::<ToolInvocation>("input")?;
                    let result = tool.output::<ToolResult>("output")?;
                    edits.push(context.connect_bound_input(
                        (ROUTER, output.as_str()),
                        &input,
                        capacity(),
                    )?);
                    if feedback.insert(tool.path().to_owned()) {
                        edits.push(context.connect_bound_output(
                            &result,
                            port(TOOL_FEEDBACK, "input"),
                            capacity(),
                        )?);
                    }
                }
                Ok(NetworkPatch::new(edits))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_component_is_rejected_before_any_registration() {
        let mut registrar = Registrar::new("tool-author").unwrap();
        let error = Tool::from_component("bad", ComponentDef::map(|text: String| text))
            .register(&mut registrar)
            .unwrap_err();
        assert_eq!(error.code, Code::SchemaRejected);
        assert_eq!(registrar.finish().keys().count(), 0);
    }

    #[test]
    fn duplicate_tool_registration_is_atomic() {
        let mut registrar = Registrar::new("tool-author").unwrap();
        Tool::new("first", |text| Ok(text.into()))
            .register(&mut registrar)
            .unwrap();
        assert!(
            Tool::new("first", |text| Ok(text.into()))
                .register(&mut registrar)
                .is_err()
        );
        Tool::new("second", |text| Ok(text.into()))
            .register(&mut registrar)
            .unwrap();
        assert_eq!(registrar.finish().keys().count(), 4);
    }
}
