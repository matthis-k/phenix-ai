use phenix_core::{
    CallableId, CapabilityId, ComponentInterface, DurableSchema, InterfaceId, PhenixSchema,
    PhenixValue, PluginContext, PluginHost, PluginInstance, ResourceNamespace, SdkClient,
    ServiceId, TransactionOp, TypeKind, ValueCodec, ValueError,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{btree_map::Entry, BTreeMap, BTreeSet},
    fmt::Display,
};

pub const EXECUTION_CONFIGURATION_SERVICE: &str = "phenix.execution.configuration@1";
const EXECUTION_CONFIGURATION_NAMESPACE: &str = "phenix.execution.configuration";
const STATE_KEY: &str = "state";

mod packaged;

type ExecutionConfigurationContext<'host, 'runtime> =
    PluginContext<'host, 'runtime, SdkClient<'host, 'runtime, phenix_sdk::ModelRoutingInterface>>;

fn context<'host, 'runtime>(
    host: &'host PluginHost<'runtime>,
) -> ExecutionConfigurationContext<'host, 'runtime> {
    PluginContext::new(
        host,
        SdkClient::new(host, crate::component::execution_component_id()),
        (),
        (),
    )
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String")]
struct NonEmptyText(String);

impl NonEmptyText {
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for NonEmptyText {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.trim().is_empty() {
            return Err("text must not be empty");
        }
        Ok(Self(value))
    }
}

impl ValueCodec for NonEmptyText {
    fn phenix_type() -> PhenixSchema {
        PhenixSchema::String
    }

    fn to_value(&self) -> PhenixValue {
        PhenixValue::String(self.0.clone())
    }

    fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        match value {
            PhenixValue::String(value) => value
                .clone()
                .try_into()
                .map_err(|error: &'static str| ValueError::InvalidValue(error.into())),
            _ => Err(ValueError::TypeMismatch {
                expected: TypeKind::String,
                actual: value.kind(),
            }),
        }
    }
}

macro_rules! string_value_codec {
    ($type:ty, $variant:path, $value:literal, $error:literal) => {
        impl ValueCodec for $type {
            fn phenix_type() -> PhenixSchema {
                PhenixSchema::String
            }

            fn to_value(&self) -> PhenixValue {
                PhenixValue::String($value.into())
            }

            fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
                match value {
                    PhenixValue::String(value) if value == $value => Ok($variant),
                    PhenixValue::String(_) => Err(ValueError::InvalidValue($error.into())),
                    _ => Err(ValueError::TypeMismatch {
                        expected: TypeKind::String,
                        actual: value.kind(),
                    }),
                }
            }
        }
    };
}

#[derive(
    Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
pub struct CallablePolicy {
    #[serde(default)]
    pub requires_permission: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
struct CallableDefinition {
    id: CallableId,
    description: NonEmptyText,
    input_schema: PhenixSchema,
    output_schema: PhenixSchema,
    #[serde(default)]
    capabilities: BTreeSet<CapabilityId>,
    #[serde(default)]
    policy: CallablePolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AgentKind {
    Agent,
}

string_value_codec!(AgentKind, AgentKind::Agent, "agent", "expected agent kind");

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct AgentDefinition {
    #[serde(flatten)]
    callable: CallableDefinition,
    kind: AgentKind,
}

impl AgentDefinition {
    pub fn id(&self) -> &CallableId {
        &self.callable.id
    }

    pub fn description(&self) -> &str {
        self.callable.description.as_str()
    }

    pub fn input_schema(&self) -> &PhenixSchema {
        &self.callable.input_schema
    }

    pub fn output_schema(&self) -> &PhenixSchema {
        &self.callable.output_schema
    }

    pub fn capabilities(&self) -> &BTreeSet<CapabilityId> {
        &self.callable.capabilities
    }

    pub fn policy(&self) -> &CallablePolicy {
        &self.callable.policy
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OrchestrationKind {
    Orchestration,
}

string_value_codec!(
    OrchestrationKind,
    OrchestrationKind::Orchestration,
    "orchestration",
    "expected orchestration kind"
);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
struct OrchestrationDescriptor {
    #[serde(flatten)]
    callable: CallableDefinition,
    kind: OrchestrationKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct OrchestrationNode {
    callable: CallableId,
    objective: NonEmptyText,
}

impl OrchestrationNode {
    pub fn callable(&self) -> &CallableId {
        &self.callable
    }

    pub fn objective(&self) -> &str {
        self.objective.as_str()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Vec<OrchestrationNode>")]
struct NonEmptyNodes(Vec<OrchestrationNode>);

impl NonEmptyNodes {
    fn as_slice(&self) -> &[OrchestrationNode] {
        &self.0
    }
}

impl TryFrom<Vec<OrchestrationNode>> for NonEmptyNodes {
    type Error = &'static str;

    fn try_from(nodes: Vec<OrchestrationNode>) -> Result<Self, Self::Error> {
        if nodes.is_empty() {
            return Err("orchestration must contain at least one node");
        }
        Ok(Self(nodes))
    }
}

impl ValueCodec for NonEmptyNodes {
    fn phenix_type() -> PhenixSchema {
        <Vec<OrchestrationNode> as ValueCodec>::phenix_type()
    }

    fn to_value(&self) -> PhenixValue {
        self.0.to_value()
    }

    fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        Vec::<OrchestrationNode>::from_value(value)?
            .try_into()
            .map_err(|error: &'static str| ValueError::InvalidValue(error.into()))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OrchestrationPolicy {
    Sequential,
}

string_value_codec!(
    OrchestrationPolicy,
    OrchestrationPolicy::Sequential,
    "sequential",
    "expected sequential orchestration policy"
);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct OrchestrationDefinition {
    descriptor: OrchestrationDescriptor,
    policy: OrchestrationPolicy,
    nodes: NonEmptyNodes,
}

impl OrchestrationDefinition {
    pub fn id(&self) -> &CallableId {
        &self.descriptor.callable.id
    }

    pub fn nodes(&self) -> &[OrchestrationNode] {
        self.nodes.as_slice()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum ExecutionConfigurationCommand {
    ConfigurePackaged {
        agents: Vec<AgentDefinition>,
        orchestrations: Vec<OrchestrationDefinition>,
        profiles: Vec<phenix_sdk::RoutingProfile>,
    },
    RegisterAgent {
        agent: AgentDefinition,
    },
    GetAgent {
        id: CallableId,
    },
    ListAgents,
    RegisterOrchestration {
        orchestration: OrchestrationDefinition,
    },
    GetOrchestration {
        id: CallableId,
    },
    ListOrchestrations,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case")]
pub enum ExecutionConfigurationResponse {
    Configured {
        profiles: Vec<phenix_sdk::RoutingProfile>,
    },
    Agent {
        agent: Option<AgentDefinition>,
    },
    Agents {
        agents: Vec<AgentDefinition>,
    },
    Orchestration {
        orchestration: Option<OrchestrationDefinition>,
    },
    Orchestrations {
        orchestrations: Vec<OrchestrationDefinition>,
    },
}

pub struct ExecutionConfigurationInterface;

impl ComponentInterface for ExecutionConfigurationInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(EXECUTION_CONFIGURATION_SERVICE)
            .expect("static execution configuration interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<
            ExecutionConfigurationCommand,
            ExecutionConfigurationResponse,
        >()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
struct ExecutionConfigurationState {
    agents: BTreeMap<CallableId, AgentDefinition>,
    orchestrations: BTreeMap<CallableId, OrchestrationDefinition>,
    #[serde(default)]
    packaged: packaged::Ownership,
}

pub(crate) fn execution_configuration_namespace() -> ResourceNamespace {
    ResourceNamespace::parse(EXECUTION_CONFIGURATION_NAMESPACE)
        .expect("static execution configuration namespace is valid")
}

#[must_use]
pub fn execution_configuration_service() -> ServiceId {
    ServiceId::parse(EXECUTION_CONFIGURATION_SERVICE)
        .expect("static execution configuration service is valid")
}

pub(crate) fn configuration_factory() -> Box<dyn PluginInstance> {
    Box::new(ExecutionConfigurationPlugin)
}

struct ExecutionConfigurationPlugin;

impl PluginInstance for ExecutionConfigurationPlugin {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        context(host)
            .kernel
            .register_durable_schema(&DurableSchema::new(execution_configuration_namespace(), 1))
            .map_err(|error| error.to_string())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &execution_configuration_service() {
            return Err(format!(
                "unsupported execution configuration service: {service}"
            ));
        }
        let context = context(host);
        let interface = ExecutionConfigurationInterface::interface_id();
        let command = context
            .kernel
            .decode_projected::<ExecutionConfigurationCommand>(&interface, input)
            .map_err(|error| error.to_string())?;
        let response = execute(&context, command)?;
        context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string())
    }
}

fn execute(
    context: &ExecutionConfigurationContext<'_, '_>,
    command: ExecutionConfigurationCommand,
) -> Result<ExecutionConfigurationResponse, String> {
    match command {
        ExecutionConfigurationCommand::ConfigurePackaged {
            agents,
            orchestrations,
            profiles,
        } => packaged::configure(context, agents, orchestrations, profiles),
        ExecutionConfigurationCommand::GetAgent { id } => {
            let (_, state) = read_state(context)?;
            Ok(ExecutionConfigurationResponse::Agent {
                agent: state.agents.get(&id).cloned(),
            })
        }
        ExecutionConfigurationCommand::ListAgents => {
            let (_, state) = read_state(context)?;
            Ok(ExecutionConfigurationResponse::Agents {
                agents: state
                    .agents
                    .into_iter()
                    .filter(|(id, _)| {
                        !state.packaged.agents.contains_key(id)
                            || state.packaged.active_agents.contains(id)
                    })
                    .map(|(_, agent)| agent)
                    .collect(),
            })
        }
        ExecutionConfigurationCommand::GetOrchestration { id } => {
            let (_, state) = read_state(context)?;
            Ok(ExecutionConfigurationResponse::Orchestration {
                orchestration: state.orchestrations.get(&id).cloned(),
            })
        }
        ExecutionConfigurationCommand::ListOrchestrations => {
            let (_, state) = read_state(context)?;
            Ok(ExecutionConfigurationResponse::Orchestrations {
                orchestrations: state
                    .orchestrations
                    .into_iter()
                    .filter(|(id, _)| {
                        !state.packaged.orchestrations.contains_key(id)
                            || state.packaged.active_orchestrations.contains(id)
                    })
                    .map(|(_, orchestration)| orchestration)
                    .collect(),
            })
        }
        ExecutionConfigurationCommand::RegisterAgent { agent } => mutate_state(context, |state| {
            insert_immutable(
                &mut state.agents,
                agent.id().clone(),
                agent.clone(),
                "agent",
            )?;
            Ok(ExecutionConfigurationResponse::Agent { agent: Some(agent) })
        }),
        ExecutionConfigurationCommand::RegisterOrchestration { orchestration } => {
            mutate_state(context, |state| {
                validate_orchestration(&orchestration, &state.agents)?;
                insert_immutable(
                    &mut state.orchestrations,
                    orchestration.id().clone(),
                    orchestration.clone(),
                    "orchestration",
                )?;
                Ok(ExecutionConfigurationResponse::Orchestration {
                    orchestration: Some(orchestration),
                })
            })
        }
    }
}

fn insert_immutable<K, T>(
    records: &mut BTreeMap<K, T>,
    id: K,
    value: T,
    label: &str,
) -> Result<(), String>
where
    K: Ord + Display,
    T: Eq,
{
    match records.entry(id) {
        Entry::Vacant(entry) => {
            entry.insert(value);
            Ok(())
        }
        Entry::Occupied(entry) if entry.get() == &value => Ok(()),
        Entry::Occupied(entry) => Err(format!("{label} identity is immutable: {}", entry.key())),
    }
}

fn validate_orchestration(
    orchestration: &OrchestrationDefinition,
    agents: &BTreeMap<CallableId, AgentDefinition>,
) -> Result<(), String> {
    for node in orchestration.nodes() {
        if !agents.contains_key(node.callable()) {
            return Err(format!(
                "orchestration {} references unknown agent: {}",
                orchestration.id(),
                node.callable()
            ));
        }
    }
    Ok(())
}

fn mutate_state<F>(
    context: &ExecutionConfigurationContext<'_, '_>,
    mutation: F,
) -> Result<ExecutionConfigurationResponse, String>
where
    F: FnOnce(&mut ExecutionConfigurationState) -> Result<ExecutionConfigurationResponse, String>,
{
    let (old, mut state) = read_state(context)?;
    let response = mutation(&mut state)?;
    context
        .kernel
        .transact_durable(
            &execution_configuration_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: STATE_KEY.into(),
                    expected: old,
                },
                TransactionOp::Put {
                    key: STATE_KEY.into(),
                    value: serde_json::to_vec(&state).map_err(|error| error.to_string())?,
                },
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(response)
}

fn read_state(
    context: &ExecutionConfigurationContext<'_, '_>,
) -> Result<(Option<Vec<u8>>, ExecutionConfigurationState), String> {
    let old = context
        .kernel
        .read_durable(&execution_configuration_namespace(), STATE_KEY)
        .map_err(|error| error.to_string())?;
    let state = old
        .as_deref()
        .map(|bytes| serde_json::from_slice(bytes).map_err(|error| error.to_string()))
        .transpose()?
        .unwrap_or_default();
    Ok((old, state))
}
