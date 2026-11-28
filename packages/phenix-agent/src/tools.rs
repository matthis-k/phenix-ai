//! Typed tool registration and shared named dispatch for each environment.

use crate::{
    CONTRACTS_PLUGIN, TOOL_CONTRACT, TOOL_FEEDBACK, ToolInvocation, ToolOutcome, ToolResult,
    UNAVAILABLE, capacity, invocation_router,
};
use phenix_graph::{
    ComponentDef, ContractValue, Schema,
    compose::{Edit, Footprint, NetworkPatch},
};
use phenix_kernel::{
    BuildOptions, CallbackError, CandidateContext, Contribution, Key, component_offers,
};
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

// A schema-bearing value is data, not graph or plugin authority. Persist the
// complete descriptor, not just its name or a hash.
#[derive(ContractValue)]
#[contract(schema = "agent.tool-value")]
struct StoredToolValue {
    name: String,
    revision: u32,
    structure: String,
    codec: String,
    bytes: Vec<u8>,
}

impl ContractValue for ToolValue {
    fn schema() -> Schema {
        StoredToolValue::schema()
    }

    fn encode(&self) -> Vec<u8> {
        StoredToolValue {
            name: self.schema.name.clone(),
            revision: self.schema.revision,
            structure: self.schema.structure.clone(),
            codec: self.schema.codec.clone(),
            bytes: self.bytes.clone(),
        }
        .encode()
    }

    fn decode(bytes: &[u8]) -> Result<Self, phenix_graph::Failure> {
        let stored = StoredToolValue::decode(bytes)?;
        Ok(Self {
            schema: Schema {
                name: stored.name,
                revision: stored.revision,
                structure: stored.structure,
                codec: stored.codec,
            },
            bytes: stored.bytes,
        })
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
#[derive(Clone)]
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
        Self::stateful(name, || (), move |_, args| execute(args))
    }

    /// Initialize process-private state once per spawn. Successful requests
    /// reuse that state; cancellation/restart may reconstruct it. This helper
    /// is synchronous and makes no durability or rollback promises.
    pub fn stateful<A: ContractValue, R: ContractValue, S: Send + 'static>(
        name: impl Into<String>,
        initialize: impl Fn() -> S + Send + Sync + 'static,
        execute: impl Fn(&mut S, A) -> Result<R, String> + Send + Sync + 'static,
    ) -> Self {
        Self::contextual(name, initialize, move |state, _, args| execute(state, args))
    }

    /// Borrow a host-granted, typed resource for each call. Missing grants
    /// become ordinary tool failures, without calling the implementation.
    pub fn with_resource<A, R, C>(
        name: impl Into<String>,
        resource: &'static str,
        execute: impl Fn(&C, A) -> Result<R, String> + Send + Sync + 'static,
    ) -> Self
    where
        A: ContractValue,
        R: ContractValue,
        C: Send + Sync + 'static,
    {
        Self::contextual(
            name,
            || (),
            move |_, context, args| {
                let capability = context.resource::<C>(resource).map_err(|e| e.to_string())?;
                execute(capability, args)
            },
        )
    }

    fn contextual<A: ContractValue, R: ContractValue, S: Send + 'static>(
        name: impl Into<String>,
        initialize: impl Fn() -> S + Send + Sync + 'static,
        execute: impl Fn(&mut S, &phenix_graph::ProcessContext, A) -> Result<R, String>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        let name = name.into();
        let descriptor = ToolDescriptor {
            name: name.clone(),
            description: String::new(),
            arguments: A::schema(),
            result: R::schema(),
        };
        let definition = ComponentDef::map_context_state(
            initialize,
            move |state, context, invocation: ToolInvocation| {
                let result = invocation
                    .call()
                    .arguments
                    .decode::<A>()
                    .map_err(|error| error.summary().to_owned())
                    .and_then(|args| execute(state, context, args));
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
            },
        );
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

    pub fn name(&self) -> &str {
        &self.descriptor.name
    }

    pub fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    pub fn path(&self) -> &str {
        &self.placement
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

impl PluginInstance for Tool {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        self.clone().register(registrar)
    }
}

pub fn tool_port(name: &str) -> String {
    format!("tool-{name}")
}

/// A host-selected provider, not a claim of write authority by the tool.
pub struct SelectedTool {
    pub offer: Key,
    pub producer: Key,
    pub path: String,
    pub descriptor: Option<ToolDescriptor>,
}

impl SelectedTool {
    /// Required component write for an explicitly selected tool placement.
    /// This is a permission request, not an approval of plugin-supplied edits.
    pub fn required_component(&self) -> (Key, Footprint) {
        (
            self.producer.clone(),
            Footprint::Component(self.path.clone()),
        )
    }

    /// Native descriptors for the host's selected set, using selected aliases.
    /// This is not a proof of authorization; candidate construction must pass
    /// before a model can execute with this configuration.
    pub fn catalog(
        selected: &BTreeMap<String, Self>,
    ) -> Result<BTreeMap<String, ToolDescriptor>, Failure> {
        selected
            .iter()
            .map(|(name, tool)| {
                let mut descriptor = tool.descriptor.clone().ok_or_else(|| {
                    Failure::new(
                        Code::MissingBinding,
                        format!("tool {name} has no descriptor"),
                    )
                })?;
                descriptor.name = name.clone();
                Ok((name.clone(), descriptor))
            })
            .collect()
    }

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
                            producer: offer.producer,
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

/// Derive import selections from a host-selected set. This does not grant
/// provider writes or resources and never removes deny rules.
pub fn select_tools(
    options: &mut BuildOptions,
    owner: &str,
    providers: &BTreeMap<String, SelectedTool>,
    import_id: impl Fn(&str) -> String,
) {
    for (name, provider) in providers {
        let import = Key::new(owner, import_id(name));
        if let Some(allow) = &mut options.imports.allow {
            allow.insert((import.clone(), provider.offer.clone()));
        }
        options
            .imports
            .selections
            .insert(import, provider.offer.clone());
    }
}

pub(crate) fn declare_tools(
    mut contribution: Contribution,
    names: &BTreeSet<String>,
    import_id: impl Fn(&str) -> String,
) -> Contribution {
    for name in names {
        contribution =
            contribution.import(import_id(name), Key::new(CONTRACTS_PLUGIN, TOOL_CONTRACT));
    }
    contribution
}

pub(crate) fn bind_tools(
    context: &CandidateContext<'_>,
    names: &BTreeSet<String>,
    import_id: impl Fn(&str) -> String,
) -> Result<BTreeMap<String, String>, CallbackError> {
    let mut placements = BTreeMap::new();
    for name in names {
        let tool = context.bind_import(&import_id(name))?;
        tool.input::<ToolInvocation>("input")?;
        tool.output::<ToolResult>("output")?;
        placements.insert(name.clone(), tool.path().to_owned());
    }
    Ok(placements)
}

pub(crate) fn named_router(
    names: BTreeSet<String>,
) -> Result<ComponentDef, phenix_graph::InterfaceError> {
    invocation_router(
        names,
        |call| &call.name,
        tool_port,
        |invocation| {
            invocation.complete(ToolOutcome::Failed {
                output: String::new().into(),
                reason: "tool unavailable".into(),
            })
        },
    )
}

/// Common routing declaration for local and additional environments.
pub(crate) fn named_edges(router: &str, tools: &BTreeMap<String, String>) -> Vec<Edit> {
    let mut edits = vec![Edit::connect(
        (router, UNAVAILABLE),
        (TOOL_FEEDBACK, "input"),
        capacity(),
    )];
    let mut feedback = BTreeSet::new();
    for (name, path) in tools {
        edits.push(Edit::connect(
            (router, tool_port(name)),
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

pub(crate) fn named_grants(router: &str, tools: &BTreeMap<String, String>) -> Vec<Footprint> {
    let mut grants = vec![Footprint::Component(router.into())];
    grants.extend(NetworkPatch::new(named_edges(router, tools)).footprints());
    grants
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ExampleTool;

    impl PluginInstance for ExampleTool {
        fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
            Tool::new("echo", Ok::<String, String>).register(registrar)
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
        select_tools(&mut options, "routes", &tools, tool_port);
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
    fn typed_results_round_trip_and_reject_other_schemas() {
        let value = ToolValue::new(&42_i64);
        assert_eq!(value.decode::<i64>().unwrap(), 42);
        assert!(value.decode::<String>().is_err());
        assert!(value.as_text().is_none());
    }

    #[test]
    fn duplicate_tool_registration_is_atomic() {
        let mut registrar = Registrar::new("tool-author").unwrap();
        Tool::new("first", Ok::<String, String>)
            .register(&mut registrar)
            .unwrap();
        assert!(
            Tool::new("first", Ok::<String, String>)
                .register(&mut registrar)
                .is_err()
        );
        Tool::new("second", Ok::<String, String>)
            .register(&mut registrar)
            .unwrap();
        assert_eq!(registrar.finish().keys().count(), 6);
    }
}
