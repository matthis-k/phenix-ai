//! One atomic publication of packaged execution definitions and routing profiles.
use super::*;
use phenix_sdk::{ModelCommand, ModelResponse, RoutingProfile};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(super) struct Ownership {
    pub(super) agents: BTreeMap<CallableId, AgentDefinition>,
    pub(super) orchestrations: BTreeMap<CallableId, OrchestrationDefinition>,
    pub(super) active_agents: BTreeSet<CallableId>,
    pub(super) active_orchestrations: BTreeSet<CallableId>,
}

fn reconcile<T: Clone + Eq>(
    records: &mut BTreeMap<CallableId, T>,
    owned: &mut BTreeMap<CallableId, T>,
    desired: BTreeMap<CallableId, T>,
) -> Result<BTreeSet<CallableId>, String> {
    for (id, expected) in owned.iter() {
        if records.get(id) != Some(expected) {
            return Err(format!("packaged callable changed outside configuration: {id}"));
        }
    }
    for (id, definition) in &desired {
        if !owned.contains_key(id) && records.get(id).is_some_and(|existing| existing != definition) {
            return Err(format!("foreign callable identity is immutable: {id}"));
        }
    }
    let active = desired.keys().cloned().collect();
    for (id, definition) in desired {
        owned.insert(id.clone(), definition.clone());
        records.insert(id, definition);
    }
    Ok(active)
}

fn unique<T>(values: Vec<T>, id: impl Fn(&T) -> &CallableId) -> Result<BTreeMap<CallableId, T>, String> {
    let mut result = BTreeMap::new();
    for value in values {
        let key = id(&value).clone();
        if result.insert(key.clone(), value).is_some() {
            return Err(format!("duplicate packaged callable: {key}"));
        }
    }
    Ok(result)
}

pub(super) fn configure(
    context: &ExecutionConfigurationContext<'_, '_>,
    agents: Vec<AgentDefinition>,
    orchestrations: Vec<OrchestrationDefinition>,
    profiles: Vec<RoutingProfile>,
) -> Result<ExecutionConfigurationResponse, String> {
    let agents = unique(agents, AgentDefinition::id)?;
    let orchestrations = unique(orchestrations, OrchestrationDefinition::id)?;
    if agents.keys().any(|id| orchestrations.contains_key(id)) {
        return Err("agent and orchestration IDs overlap".into());
    }
    let (old, mut state) = read_state(context)?;
    state.packaged.active_agents = reconcile(&mut state.agents, &mut state.packaged.agents, agents)?;
    let available_agents = state.agents.iter()
        .filter(|(id, _)| !state.packaged.agents.contains_key(*id) || state.packaged.active_agents.contains(*id))
        .map(|(id, agent)| (id.clone(), agent.clone())).collect();
    for orchestration in orchestrations.values() {
        validate_orchestration(orchestration, &available_agents)?;
    }
    state.packaged.active_orchestrations = reconcile(&mut state.orchestrations, &mut state.packaged.orchestrations, orchestrations)?;
    let prepared: ModelResponse = context.sdk.invoke_projected(&ModelCommand::PreparePackagedProfiles { profiles })
        .map_err(|error| error.to_string())?;
    let ModelResponse::PreparedProfiles { mutation, profiles } = prepared else {
        return Err("routing service returned the wrong configuration response".into());
    };
    let execution = context.kernel.prepare_durable_transaction(&execution_configuration_namespace(), &[
        TransactionOp::AssertValue { key: STATE_KEY.into(), expected: old },
        TransactionOp::Put { key: STATE_KEY.into(), value: serde_json::to_vec(&state).map_err(|error| error.to_string())? },
    ]).map_err(|error| error.to_string())?;
    context.kernel.transact_prepared(&[mutation, execution]).map_err(|error| error.to_string())?;
    Ok(ExecutionConfigurationResponse::Configured { profiles })
}
