//! Optional named-tool selection inside the original local execution route.
//! Environment selection is orthogonal: it may wrap this route, but does
//! not need to know the names, implementations or grants of local tools.

use crate::{
    CONTRACTS_PLUGIN, LOOP_PLUGIN, TOOL_CONTRACT, TOOL_FEEDBACK, TOOL_ROUTER, ToolInvocation,
    ToolOutcome, ToolResult, UNAVAILABLE, capacity, invocation_router, port,
};
use phenix_graph::{
    ComponentDef, ContractValue, Schema,
    compose::{ConnectionKey, Edit, Footprint, NetworkPatch},
};
use phenix_kernel::{BuildOptions, Contribution, Key, component_offers};
use phenix_plugin::{Code, Failure, PluginInstance, PluginSnapshot, Registrar};
use std::collections::{BTreeMap, BTreeSet};

/// A tool's structured argument or result. The codec and complete schema are
/// shared with Graph and Storage; this is not a model-provider JSON format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolValue {
    schema: Schema,
    bytes: Vec<u8>,
}

impl ToolValue {
    pub fn new<T: ContractValue>(value: &T) -> Self {
        Self {
            schema: T::schema(),
            bytes: value.encode(),
        }
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn decode<T: ContractValue>(&self) -> Result<T, phenix_graph::Failure> {
        if self.schema != T::schema() {
            return Err(phenix_graph::Failure::application_message(
                "tool value schema mismatch",
            ));
        }
        T::decode(&self.bytes)
    }

    pub fn as_text(&self) -> Option<&str> {
        if self.schema == String::schema() {
            std::str::from_utf8(&self.bytes).ok()
        } else {
            None
        }
    }
}

impl From<String> for ToolValue {
    fn from(value: String) -> Self {
        Self::new(&value)
    }
}

impl From<&str> for ToolValue {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}

/// Model-facing semantic metadata, independent of graph placement. The schema
/// describes the native wire contract, not yet a provider's JSON Schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub arguments: Schema,
    pub result: Schema,
}

impl phenix_plugin::Contribution for ToolDescriptor {
    const KIND: &'static str = "agent.tool";
    fn id(&self) -> &str {
        &self.name
    }
}

/// A named tool with a derived argument/result contract and ordinary component
/// registration. Complex tools use the same descriptor and offer mechanism.
pub struct Tool {
    descriptor: ToolDescriptor,
    placement: String,
    definition: ComponentDef,
}

impl Tool {
    pub fn new<A: ContractValue, R: ContractValue>(
        name: impl Into<String>,
        execute: impl Fn(A) -> Result<R, String> + Send + Sync + 'static,
    ) -> Self {
        let name = name.into();
        let descriptor = ToolDescriptor {
            name: name.clone(),
            description: String::new(),
            arguments: A::schema(),
            result: R::schema(),
        };
        let definition = ComponentDef::map(move |invocation: ToolInvocation| {
            let result = invocation
                .call()
                .arguments
                .decode::<A>()
                .map_err(|error| error.summary().to_owned())
                .and_then(&execute);
            let outcome = match result {
                Ok(output) => ToolOutcome::Succeeded {
                    output: ToolValue::new(&output),
                },
                Err(reason) => ToolOutcome::Failed {
                    output: String::new().into(),
                    reason,
                },
            };
            invocation.complete(outcome)
        });
        Self::from_component(descriptor, definition)
    }

    pub fn from_component(descriptor: ToolDescriptor, definition: ComponentDef) -> Self {
        Self {
            placement: descriptor.name.clone(),
            descriptor,
            definition,
        }
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.descriptor.description = description.into();
        self
    }

    pub fn placement(mut self, placement: impl Into<String>) -> Self {
        self.placement = placement.into();
        self
    }

    /// The descriptor, claim and offer enter the same atomic plugin bundle.
    /// Selecting and granting this contribution is a separate host decision.
    pub fn register(self, registrar: &mut Registrar) -> Result<(), Failure> {
        let mut bundle = Registrar::new(registrar.owner())?;
        Contribution::offered_component(
            &self.descriptor.name,
            &self.placement,
            &self.descriptor.name,
            Key::new(CONTRACTS_PLUGIN, TOOL_CONTRACT),
            &crate::tool_contract(),
            self.definition,
        )?
        .register(&mut bundle)?;
        bundle.add(self.descriptor)?;
        registrar.merge(bundle.finish())
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
    pub descriptor: Option<ToolDescriptor>,
}

impl SelectedTool {
    /// Discover named tools from snapshots chosen by the host. The offer's
    /// identity supplies its name; the declaration supplies its placement.
    /// Duplicate names must be resolved by the host, never by discovery order.
    /// This operation supplies metadata only, not authorization.
    pub fn discover(
        snapshots: impl IntoIterator<Item = PluginSnapshot>,
    ) -> Result<BTreeMap<String, Self>, Failure> {
        let contract = Key::new(CONTRACTS_PLUGIN, TOOL_CONTRACT);
        let mut tools = BTreeMap::new();
        for snapshot in snapshots {
            for offer in component_offers(&snapshot) {
                if offer.contract != contract {
                    continue;
                }
                let name = offer.offer.id.clone();
                let descriptor = snapshot
                    .get::<ToolDescriptor>(&name)
                    .map(|value| (*value).clone());
                if tools
                    .insert(
                        name.clone(),
                        Self {
                            offer: offer.offer,
                            path: offer.path,
                            descriptor,
                        },
                    )
                    .is_some()
                {
                    return Err(Failure::new(
                        Code::DescriptorInvalid,
                        format!("ambiguous tool name: {name}"),
                    ));
                }
            }
        }
        Ok(tools)
    }
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
                            output: String::new().into(),
                            reason: "tool unavailable".into(),
                        })
                    },
                )?;
                let mut edits = vec![
                    Edit::add(ROUTER, router),
                    Edit::disconnect((TOOL_ROUTER, "env-local"), (base_path, base_input.name())),
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

    struct ExampleTool;

    impl PluginInstance for ExampleTool {
        fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
            Tool::new("echo", |text: String| Ok(text)).register(registrar)
        }
    }

    #[test]
    fn discovery_reports_ambiguity_and_does_not_grant_provider_writes() {
        let mut catalog = phenix_plugin::PluginCatalog::new();
        let a = catalog.register("a", |_: ()| Ok(ExampleTool)).unwrap();
        let b = catalog.register("b", |_: ()| Ok(ExampleTool)).unwrap();
        let plugins = catalog.instance();
        let first = plugins.prepare_load(&a, ()).unwrap();
        let second = plugins.prepare_load(&b, ()).unwrap();
        let a = first.candidate().unwrap();
        let b = second.candidate().unwrap();
        assert!(SelectedTool::discover([a.clone(), b]).is_err());

        let tools = SelectedTool::discover([a]).unwrap();
        assert_eq!(tools["echo"].offer, Key::new("a", "echo"));
        assert_eq!(tools["echo"].path, "echo");
        assert_eq!(
            tools["echo"].descriptor.as_ref().unwrap().arguments,
            String::schema()
        );
        let mut options = BuildOptions::default();
        let denied = (Key::new("routes", "tool-echo"), Key::new("a", "echo"));
        options.imports.deny.insert(denied.clone());
        LocalTools::configure(&mut options, "routes", "echo", tools).unwrap();
        assert!(options.imports.deny.contains(&denied));
        assert!(!options.composition.contains_key(&Key::new("a", "echo")));
    }

    #[test]
    fn invalid_component_is_rejected_before_any_registration() {
        let mut registrar = Registrar::new("tool-author").unwrap();
        let error = Tool::from_component(
            ToolDescriptor {
                name: "bad".into(),
                description: String::new(),
                arguments: String::schema(),
                result: String::schema(),
            },
            ComponentDef::map(|text: String| text),
        )
        .register(&mut registrar)
        .unwrap_err();
        assert_eq!(error.code, Code::SchemaRejected);
        assert_eq!(registrar.finish().keys().count(), 0);
    }

    #[test]
    fn duplicate_tool_registration_is_atomic() {
        let mut registrar = Registrar::new("tool-author").unwrap();
        Tool::new("first", |text: String| Ok(text))
            .register(&mut registrar)
            .unwrap();
        assert!(
            Tool::new("first", |text: String| Ok(text))
                .register(&mut registrar)
                .is_err()
        );
        Tool::new("second", |text: String| Ok(text))
            .register(&mut registrar)
            .unwrap();
        assert_eq!(registrar.finish().keys().count(), 6);
    }
}
