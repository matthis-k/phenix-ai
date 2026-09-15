use phenix_core::{
    Authority, CapabilityId, PluginContext, PluginInstance, PluginManifest, ResourceNamespace,
    ServiceId, TransactionOp,
};
use phenix_sdk::{
    DecisionRecord, HistoryEntry, HistoryKind, ObjectiveRecord, PlanRecord, PlanStep,
    PlanningCommand, PlanningInterface, PlanningResponse, StaticPluginDefinition, PLANNING_SERVICE,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const PLANNING_NAMESPACE: &str = "phenix.planning.state";
const PERSISTENCE_SCHEMA: &str = "kernel.persistence.schema";
const PERSISTENCE_READ: &str = "kernel.persistence.read";
const PERSISTENCE_WRITE: &str = "kernel.persistence.write";
const OBJECTIVE_INDEX: &str = "index/objectives";
const PLAN_INDEX: &str = "index/plans";
const DECISION_INDEX: &str = "index/decisions";

type PlanningContext<'host, 'runtime> = PluginContext<'host, 'runtime, ()>;

struct PlanningStore;

#[phenix_sdk::resource(schema = 1)]
impl PlanningStore {}

#[phenix_sdk::component]
struct Api;

#[phenix_sdk::component]
impl Api {
    #[phenix(
        export(PlanningInterface),
        terminal,
        priority = 100,
        authority = persistence_authority()
    )]
    fn handle(
        &self,
        context: &phenix_sdk::PluginContext<'_, '_, ()>,
        command: PlanningCommand,
    ) -> Result<PlanningResponse, String> {
        handle(context, command)
    }
}

#[phenix_sdk::plugin(id = "phenix.planning", authority = persistence_authority())]
pub struct Plugin {
    #[phenix(component, id = "phenix.planning")]
    api: Api,

    #[phenix(resource, id = "phenix.planning.state")]
    _state: phenix_sdk::Durable<PlanningStore>,
}

#[must_use]
pub fn planning_manifest() -> PluginManifest {
    Plugin::manifest()
}

#[must_use]
pub fn planning_factory() -> Box<dyn PluginInstance> {
    phenix_sdk::StaticPluginComponentDispatch::into_plugin_instance(Plugin {
        api: Api,
        _state: phenix_sdk::Durable::new(),
    })
}

#[must_use]
pub fn planning_durable_schema_registrations() -> Vec<phenix_core::DurableSchemaRegistration> {
    <Plugin as phenix_sdk::StaticPluginResources>::durable_schema_registrations(
        &planning_manifest().id,
    )
}

#[must_use]
pub fn planning_service() -> ServiceId {
    ServiceId::parse(PLANNING_SERVICE).expect("static service id is valid")
}

fn planning_namespace() -> ResourceNamespace {
    ResourceNamespace::parse(PLANNING_NAMESPACE).expect("static namespace is valid")
}

fn persistence_authority() -> Authority {
    Authority::new([
        capability(PERSISTENCE_SCHEMA),
        capability(PERSISTENCE_READ),
        capability(PERSISTENCE_WRITE),
    ])
}

fn capability(value: &str) -> CapabilityId {
    CapabilityId::parse(value).expect("static capability is valid")
}

fn handle(
    context: &PlanningContext<'_, '_>,
    command: PlanningCommand,
) -> Result<PlanningResponse, String> {
    match command {
        PlanningCommand::CreateObjective { id, title, parent } => Ok(PlanningResponse::Objective {
            objective: Some(create_objective(context, id, title, parent)?),
        }),
        PlanningCommand::CreatePlan {
            id,
            objective_id,
            goal,
            steps,
        } => Ok(PlanningResponse::Plan {
            plan: Some(create_plan(context, id, objective_id, goal, steps)?),
        }),
        PlanningCommand::RecordDecision {
            id,
            objective_id,
            statement,
            rationale,
            dependencies,
            supersedes,
        } => Ok(PlanningResponse::Decision {
            decision: Some(record_decision(
                context,
                id,
                objective_id,
                statement,
                rationale,
                dependencies,
                supersedes,
            )?),
        }),
        PlanningCommand::GetObjective { id } => Ok(PlanningResponse::Objective {
            objective: read_record(context, &objective_key(&id))?,
        }),
        PlanningCommand::GetPlan { id } => Ok(PlanningResponse::Plan {
            plan: read_record(context, &plan_key(&id))?,
        }),
        PlanningCommand::GetDecision { id } => Ok(PlanningResponse::Decision {
            decision: read_record(context, &decision_key(&id))?,
        }),
        PlanningCommand::SearchHistory {
            objective_id,
            query,
        } => Ok(PlanningResponse::History {
            entries: search_history(context, objective_id.as_deref(), &query)?,
        }),
    }
}

fn create_objective(
    context: &PlanningContext<'_, '_>,
    id: String,
    title: String,
    parent: Option<String>,
) -> Result<ObjectiveRecord, String> {
    validate_identity("objective id", &id)?;
    validate_text("objective title", &title)?;
    if let Some(parent_id) = &parent {
        validate_identity("parent objective id", parent_id)?;
        require_objective(context, parent_id)?;
        if parent_id == &id {
            return Err("objective cannot parent itself".into());
        }
    }
    let record = ObjectiveRecord { id, title, parent };
    insert_record(
        context,
        OBJECTIVE_INDEX,
        &objective_key(&record.id),
        &record,
    )?;
    Ok(record)
}

fn create_plan(
    context: &PlanningContext<'_, '_>,
    id: String,
    objective_id: String,
    goal: String,
    mut steps: Vec<PlanStep>,
) -> Result<PlanRecord, String> {
    validate_identity("plan id", &id)?;
    validate_identity("plan objective id", &objective_id)?;
    validate_text("plan goal", &goal)?;
    require_objective(context, &objective_id)?;
    normalize_and_validate_steps(&mut steps)?;
    let record = PlanRecord {
        id,
        objective_id,
        goal,
        steps,
    };
    insert_record(context, PLAN_INDEX, &plan_key(&record.id), &record)?;
    Ok(record)
}

fn record_decision(
    context: &PlanningContext<'_, '_>,
    id: String,
    objective_id: String,
    statement: String,
    rationale: String,
    mut dependencies: Vec<String>,
    supersedes: Option<String>,
) -> Result<DecisionRecord, String> {
    validate_identity("decision id", &id)?;
    validate_identity("decision objective id", &objective_id)?;
    validate_text("decision statement", &statement)?;
    validate_text("decision rationale", &rationale)?;
    require_objective(context, &objective_id)?;
    dependencies.sort();
    dependencies.dedup();
    for dependency in &dependencies {
        validate_identity("decision dependency", dependency)?;
        let record: DecisionRecord = read_record(context, &decision_key(dependency))?
            .ok_or_else(|| format!("unknown decision dependency: {dependency}"))?;
        if record.objective_id != objective_id {
            return Err(format!(
                "decision dependency {dependency} belongs to objective {} instead of {objective_id}",
                record.objective_id
            ));
        }
    }
    if dependencies.iter().any(|dependency| dependency == &id) {
        return Err("decision cannot depend on itself".into());
    }
    if let Some(prior) = &supersedes {
        validate_identity("superseded decision", prior)?;
        let prior_record: DecisionRecord = read_record(context, &decision_key(prior))?
            .ok_or_else(|| format!("unknown superseded decision: {prior}"))?;
        if prior_record.objective_id != objective_id {
            return Err("superseded decision must belong to the same objective".into());
        }
        if prior == &id {
            return Err("decision cannot supersede itself".into());
        }
    }
    let existing = load_decisions(context)?;
    let mut graph: BTreeMap<String, Vec<String>> = existing
        .into_iter()
        .map(|record| (record.id, record.dependencies))
        .collect();
    graph.insert(id.clone(), dependencies.clone());
    ensure_acyclic(&graph, "decision dependency")?;

    let record = DecisionRecord {
        id,
        objective_id,
        statement,
        rationale,
        dependencies,
        supersedes,
    };
    insert_record(context, DECISION_INDEX, &decision_key(&record.id), &record)?;
    Ok(record)
}

fn normalize_and_validate_steps(steps: &mut [PlanStep]) -> Result<(), String> {
    let mut ids = BTreeSet::new();
    for step in steps.iter_mut() {
        validate_identity("plan step id", &step.id)?;
        validate_text("plan step description", &step.description)?;
        if !ids.insert(step.id.clone()) {
            return Err(format!("duplicate plan step: {}", step.id));
        }
        step.dependencies.sort();
        step.dependencies.dedup();
    }
    let graph: BTreeMap<String, Vec<String>> = steps
        .iter()
        .map(|step| {
            for dependency in &step.dependencies {
                if !ids.contains(dependency) {
                    return Err(format!(
                        "plan step {} depends on unknown step {dependency}",
                        step.id
                    ));
                }
                if dependency == &step.id {
                    return Err(format!("plan step {} cannot depend on itself", step.id));
                }
            }
            Ok((step.id.clone(), step.dependencies.clone()))
        })
        .collect::<Result<_, String>>()?;
    ensure_acyclic(&graph, "plan step dependency")
}

fn ensure_acyclic(graph: &BTreeMap<String, Vec<String>>, label: &str) -> Result<(), String> {
    fn visit(
        node: &str,
        graph: &BTreeMap<String, Vec<String>>,
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
    ) -> bool {
        if visited.contains(node) {
            return true;
        }
        if !visiting.insert(node.to_owned()) {
            return false;
        }
        if let Some(dependencies) = graph.get(node) {
            for dependency in dependencies {
                if !visit(dependency, graph, visiting, visited) {
                    return false;
                }
            }
        }
        visiting.remove(node);
        visited.insert(node.to_owned());
        true
    }

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for node in graph.keys() {
        if !visit(node, graph, &mut visiting, &mut visited) {
            return Err(format!("{label} cycle detected"));
        }
    }
    Ok(())
}

fn require_objective(
    context: &PlanningContext<'_, '_>,
    id: &str,
) -> Result<ObjectiveRecord, String> {
    read_record(context, &objective_key(id))?.ok_or_else(|| format!("unknown objective: {id}"))
}

fn search_history(
    context: &PlanningContext<'_, '_>,
    objective_id: Option<&str>,
    query: &str,
) -> Result<Vec<HistoryEntry>, String> {
    if let Some(id) = objective_id {
        validate_identity("history objective id", id)?;
        require_objective(context, id)?;
    }
    let query = query.trim().to_lowercase();
    let mut entries = Vec::new();
    for objective in load_records::<ObjectiveRecord>(context, OBJECTIVE_INDEX, objective_key)? {
        if objective_id.is_some_and(|scope| scope != objective.id) {
            continue;
        }
        if matches_query(&query, [objective.title.as_str()]) {
            entries.push(HistoryEntry {
                kind: HistoryKind::Objective,
                id: objective.id.clone(),
                objective_id: objective.id,
                summary: objective.title,
            });
        }
    }
    for plan in load_records::<PlanRecord>(context, PLAN_INDEX, plan_key)? {
        if objective_id.is_some_and(|scope| scope != plan.objective_id) {
            continue;
        }
        if matches_query(&query, [plan.goal.as_str()]) {
            entries.push(HistoryEntry {
                kind: HistoryKind::Plan,
                id: plan.id,
                objective_id: plan.objective_id,
                summary: plan.goal,
            });
        }
    }
    for decision in load_decisions(context)? {
        if objective_id.is_some_and(|scope| scope != decision.objective_id) {
            continue;
        }
        if matches_query(
            &query,
            [decision.statement.as_str(), decision.rationale.as_str()],
        ) {
            entries.push(HistoryEntry {
                kind: HistoryKind::Decision,
                id: decision.id,
                objective_id: decision.objective_id,
                summary: decision.statement,
            });
        }
    }
    entries.sort_by(|left, right| {
        left.objective_id
            .cmp(&right.objective_id)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(entries)
}

fn matches_query<'a>(query: &str, values: impl IntoIterator<Item = &'a str>) -> bool {
    query.is_empty()
        || values
            .into_iter()
            .any(|value| value.to_lowercase().contains(query))
}

fn insert_record<T: Serialize>(
    context: &PlanningContext<'_, '_>,
    index_key: &str,
    record_key: &str,
    record: &T,
) -> Result<(), String> {
    let old_index = read_raw(context, index_key)?;
    let mut ids = decode_index(old_index.as_deref())?;
    let id = record_key
        .rsplit('/')
        .next()
        .ok_or_else(|| format!("invalid durable record key: {record_key}"))?
        .to_owned();
    if ids.iter().any(|existing| existing == &id) || read_raw(context, record_key)?.is_some() {
        return Err(format!("immutable record already exists: {id}"));
    }
    ids.push(id);
    ids.sort();
    context
        .kernel
        .transact_durable(
            &planning_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: record_key.into(),
                    expected: None,
                },
                TransactionOp::AssertValue {
                    key: index_key.into(),
                    expected: old_index,
                },
                TransactionOp::Put {
                    key: record_key.into(),
                    value: serde_json::to_vec(record).map_err(|error| error.to_string())?,
                },
                TransactionOp::Put {
                    key: index_key.into(),
                    value: serde_json::to_vec(&ids).map_err(|error| error.to_string())?,
                },
            ],
        )
        .map_err(|error| error.to_string())
}

fn load_decisions(context: &PlanningContext<'_, '_>) -> Result<Vec<DecisionRecord>, String> {
    load_records(context, DECISION_INDEX, decision_key)
}

fn load_records<T: for<'de> Deserialize<'de>>(
    context: &PlanningContext<'_, '_>,
    index_key: &str,
    key: fn(&str) -> String,
) -> Result<Vec<T>, String> {
    decode_index(read_raw(context, index_key)?.as_deref())?
        .into_iter()
        .map(|id| {
            read_record(context, &key(&id))?
                .ok_or_else(|| format!("missing durable record from {index_key}: {id}"))
        })
        .collect()
}

fn read_record<T: for<'de> Deserialize<'de>>(
    context: &PlanningContext<'_, '_>,
    key: &str,
) -> Result<Option<T>, String> {
    read_raw(context, key)?
        .map(|value| serde_json::from_slice(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn read_raw(context: &PlanningContext<'_, '_>, key: &str) -> Result<Option<Vec<u8>>, String> {
    context
        .kernel
        .read_durable(&planning_namespace(), key)
        .map_err(|error| error.to_string())
}

fn decode_index(value: Option<&[u8]>) -> Result<Vec<String>, String> {
    value
        .map(|value| serde_json::from_slice(value).map_err(|error| error.to_string()))
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn objective_key(id: &str) -> String {
    format!("objective/{id}")
}

fn plan_key(id: &str) -> String {
    format!("plan/{id}")
}

fn decision_key(id: &str) -> String {
    format!("decision/{id}")
}

fn validate_identity(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.contains('/') {
        Err(format!(
            "{label} must be non-empty and must not contain '/'"
        ))
    } else {
        Ok(())
    }
}

fn validate_text(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{label} must not be empty"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning_component_manifest;
    use phenix_core::{Kernel, LocalPersistence, ResolvedHarness, ResolvedHarnessActivation};
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_db(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "phenix-{name}-{}-{nonce}.sqlite",
            std::process::id()
        ))
    }

    fn kernel_with(path: &PathBuf) -> Kernel {
        let manifest = planning_manifest();
        let plugin = manifest.id.clone();
        let resolved = ResolvedHarness::resolve_with_durable_schemas(
            [manifest],
            [planning_component_manifest()],
            planning_durable_schema_registrations(),
            [],
            &planning_manifest().maximum_authority,
        )
        .unwrap();
        let persistence = LocalPersistence::open(path).unwrap();
        let mut kernel = Kernel::with_persistence(resolved.kernel_config().clone(), persistence);
        kernel.activate_resolved_harness(&resolved).unwrap();
        kernel
            .register_embedded_factory(plugin, planning_factory)
            .unwrap();
        kernel.activate_all().unwrap();
        kernel
    }

    fn invoke(kernel: &mut Kernel, command: PlanningCommand) -> Result<PlanningResponse, String> {
        let input = serde_json::to_vec(&phenix_core::PhenixValue::from(&command)).unwrap();
        let component = planning_component_manifest();
        let output = kernel
            .invoke_component(
                &component.id,
                &planning_service(),
                &input,
                &planning_manifest().maximum_authority,
                &component.owner,
            )
            .map_err(|error| error.to_string())?;
        let output: phenix_core::PhenixValue =
            serde_json::from_slice(&output).map_err(|error| error.to_string())?;
        match phenix_core::InvocationOutcome::from_transport_value(output) {
            phenix_core::InvocationOutcome::Success(value) => {
                value.project().map_err(|error| error.to_string())
            }
            phenix_core::InvocationOutcome::DomainError(error) => {
                let message: String = error.project().map_err(|error| error.to_string())?;
                Err(message)
            }
        }
    }

    #[test]
    fn objectives_plans_and_decisions_restore_from_plugin_owned_state() {
        let path = temp_db("planning-restore");
        {
            let mut kernel = kernel_with(&path);
            invoke(
                &mut kernel,
                PlanningCommand::CreateObjective {
                    id: "objective-1".into(),
                    title: "Ship plugin suite".into(),
                    parent: None,
                },
            )
            .unwrap();
            invoke(
                &mut kernel,
                PlanningCommand::CreatePlan {
                    id: "plan-1".into(),
                    objective_id: "objective-1".into(),
                    goal: "Move durable planning state behind plugins".into(),
                    steps: vec![
                        PlanStep {
                            id: "persist".into(),
                            description: "Persist state".into(),
                            dependencies: vec![],
                        },
                        PlanStep {
                            id: "verify".into(),
                            description: "Verify restore".into(),
                            dependencies: vec!["persist".into()],
                        },
                    ],
                },
            )
            .unwrap();
            invoke(
                &mut kernel,
                PlanningCommand::RecordDecision {
                    id: "decision-1".into(),
                    objective_id: "objective-1".into(),
                    statement: "Use plugin-owned schema".into(),
                    rationale: "Kernel must not interpret planning rows".into(),
                    dependencies: vec![],
                    supersedes: None,
                },
            )
            .unwrap();
        }

        let mut restored = kernel_with(&path);
        assert!(matches!(
            invoke(
                &mut restored,
                PlanningCommand::GetPlan {
                    id: "plan-1".into()
                }
            )
            .unwrap(),
            PlanningResponse::Plan { plan: Some(_) }
        ));
        let search = invoke(
            &mut restored,
            PlanningCommand::SearchHistory {
                objective_id: Some("objective-1".into()),
                query: "plugin".into(),
            },
        )
        .unwrap();
        match search {
            PlanningResponse::History { entries } => assert!(entries.len() >= 2),
            other => panic!("unexpected response: {other:?}"),
        }
        let _ = fs::remove_file(path);
    }

    #[test]
    fn invalid_plan_and_decision_dependencies_are_rejected() {
        let path = temp_db("planning-dag");
        let mut kernel = kernel_with(&path);
        invoke(
            &mut kernel,
            PlanningCommand::CreateObjective {
                id: "objective-1".into(),
                title: "Validate DAGs".into(),
                parent: None,
            },
        )
        .unwrap();
        let cycle = invoke(
            &mut kernel,
            PlanningCommand::CreatePlan {
                id: "plan-cycle".into(),
                objective_id: "objective-1".into(),
                goal: "Reject cycle".into(),
                steps: vec![
                    PlanStep {
                        id: "a".into(),
                        description: "a".into(),
                        dependencies: vec!["b".into()],
                    },
                    PlanStep {
                        id: "b".into(),
                        description: "b".into(),
                        dependencies: vec!["a".into()],
                    },
                ],
            },
        );
        assert!(cycle.unwrap_err().contains("cycle"));

        let missing = invoke(
            &mut kernel,
            PlanningCommand::RecordDecision {
                id: "decision-2".into(),
                objective_id: "objective-1".into(),
                statement: "Depends on missing decision".into(),
                rationale: "must fail".into(),
                dependencies: vec!["missing".into()],
                supersedes: None,
            },
        );
        assert!(missing.unwrap_err().contains("unknown decision dependency"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn decisions_are_immutable_and_supersession_is_explicit() {
        let path = temp_db("planning-decisions");
        let mut kernel = kernel_with(&path);
        invoke(
            &mut kernel,
            PlanningCommand::CreateObjective {
                id: "objective-1".into(),
                title: "Decision history".into(),
                parent: None,
            },
        )
        .unwrap();
        let first = PlanningCommand::RecordDecision {
            id: "decision-1".into(),
            objective_id: "objective-1".into(),
            statement: "First choice".into(),
            rationale: "initial evidence".into(),
            dependencies: vec![],
            supersedes: None,
        };
        invoke(&mut kernel, first.clone()).unwrap();
        assert!(invoke(&mut kernel, first)
            .unwrap_err()
            .contains("already exists"));
        let replacement = invoke(
            &mut kernel,
            PlanningCommand::RecordDecision {
                id: "decision-2".into(),
                objective_id: "objective-1".into(),
                statement: "Replacement choice".into(),
                rationale: "new evidence".into(),
                dependencies: vec!["decision-1".into()],
                supersedes: Some("decision-1".into()),
            },
        )
        .unwrap();
        match replacement {
            PlanningResponse::Decision {
                decision: Some(decision),
            } => {
                assert_eq!(decision.supersedes.as_deref(), Some("decision-1"));
            }
            other => panic!("unexpected response: {other:?}"),
        }
        let _ = fs::remove_file(path);
    }
}
