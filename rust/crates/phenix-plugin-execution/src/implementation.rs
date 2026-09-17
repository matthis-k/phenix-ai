use phenix_core::{
    Authority, CapabilityId, ComponentInterface, DurableSchema, PluginContext, PluginExecution,
    PluginHost, PluginId, PluginInstance, PluginManifest, ResourceNamespace, ServiceContribution,
    ServiceId, TransactionOp,
};
use phenix_sdk::{
    execution_service, CallableRecord, ExecutionAuthority, ExecutionCommand, ExecutionInterface,
    ExecutionRecord, ExecutionResponse, ExecutionState, WorkerTaskRecord, WorkerTaskState,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const EXECUTION_PLUGIN: &str = "phenix.execution";
const EXECUTION_NAMESPACE: &str = "phenix.execution.state";
const PERSISTENCE_SCHEMA: &str = "kernel.persistence.schema";
const PERSISTENCE_READ: &str = "kernel.persistence.read";
const PERSISTENCE_WRITE: &str = "kernel.persistence.write";
const STATE_KEY: &str = "state";

type ExecutionContext<'host, 'runtime> = PluginContext<'host, 'runtime, ()>;

fn context<'host, 'runtime>(
    host: &'host PluginHost<'runtime>,
) -> ExecutionContext<'host, 'runtime> {
    PluginContext::new(host, (), (), ())
}

fn parse_execution_authority(value: &ExecutionAuthority) -> Result<Authority, String> {
    value
        .capabilities
        .iter()
        .map(|value| CapabilityId::parse(value).map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()
        .map(Authority::new)
}

fn execution_authority_from(authority: &Authority) -> ExecutionAuthority {
    ExecutionAuthority::new(
        authority
            .capabilities()
            .map(|capability| capability.as_str().to_owned()),
    )
}

fn attenuate_execution_authority(
    authority: &ExecutionAuthority,
    requested: &ExecutionAuthority,
) -> Result<ExecutionAuthority, String> {
    Ok(execution_authority_from(
        &parse_execution_authority(authority)?.attenuate(&parse_execution_authority(requested)?),
    ))
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
struct ExecutionProjection {
    executions: BTreeMap<String, ExecutionRecord>,
    callables: BTreeMap<String, CallableRecord>,
    tasks: BTreeMap<String, WorkerTaskRecord>,
}

#[must_use]
pub fn execution_manifest(maximum_authority: Authority) -> PluginManifest {
    let persistence = Authority::new([
        capability(PERSISTENCE_SCHEMA),
        capability(PERSISTENCE_READ),
        capability(PERSISTENCE_WRITE),
    ]);
    let maximum_authority = Authority::new(
        maximum_authority
            .capabilities()
            .cloned()
            .chain(persistence.capabilities().cloned()),
    );
    PluginManifest {
        id: PluginId::parse(EXECUTION_PLUGIN).expect("static plugin id is valid"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: phenix_core::ServiceRole::Terminal,
            service: execution_service(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        resource_namespaces: vec![execution_namespace()],
        maximum_authority,
    }
}

#[must_use]
pub fn execution_factory() -> Box<dyn PluginInstance> {
    Box::new(ExecutionPlugin)
}

fn execution_namespace() -> ResourceNamespace {
    ResourceNamespace::parse(EXECUTION_NAMESPACE).expect("static namespace is valid")
}

fn capability(value: &str) -> CapabilityId {
    CapabilityId::parse(value).expect("static capability is valid")
}

struct ExecutionPlugin;

impl PluginInstance for ExecutionPlugin {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        context(host)
            .kernel
            .register_durable_schema(&DurableSchema::new(execution_namespace(), 1))
            .map_err(|error| error.to_string())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &execution_service() {
            return Err(format!("unsupported execution service: {service}"));
        }
        let context = context(host);
        let interface = ExecutionInterface::interface_id();
        let command = context
            .kernel
            .decode_projected::<ExecutionCommand>(&interface, input)
            .map_err(|error| error.to_string())?;
        let response = execute(&context, command)?;
        context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string())
    }
}

fn execute(
    context: &ExecutionContext<'_, '_>,
    command: ExecutionCommand,
) -> Result<ExecutionResponse, String> {
    match command {
        ExecutionCommand::GetExecution { id } => {
            let (_, state) = read_state(context)?;
            Ok(ExecutionResponse::ExecutionLookup {
                execution: state.executions.get(&id).cloned(),
            })
        }
        ExecutionCommand::GetTask { id } => {
            let (_, state) = read_state(context)?;
            Ok(ExecutionResponse::TaskLookup {
                task: state.tasks.get(&id).cloned(),
            })
        }
        ExecutionCommand::InvokeCallable {
            execution_id,
            callable_id,
            input,
        } => invoke_callable(context, &execution_id, &callable_id, &input),
        other => mutate_state(context, |state| mutate(context, other, state)),
    }
}

fn mutate(
    context: &ExecutionContext<'_, '_>,
    command: ExecutionCommand,
    state: &mut ExecutionProjection,
) -> Result<ExecutionResponse, String> {
    match command {
        ExecutionCommand::CreateExecution {
            id,
            requested_authority,
        } => {
            validate_identity("execution id", &id)?;
            ensure_new_execution(state, &id)?;
            let requested = parse_execution_authority(&requested_authority)?;
            let effective = execution_authority_from(&context.call.authority.attenuate(&requested));
            let graph_generation = context
                .call
                .graph_generation
                .ok_or_else(|| "execution requires an active graph generation".to_owned())?
                .as_str()
                .to_owned();
            let execution = ExecutionRecord {
                id: id.clone(),
                parent_execution: None,
                graph_generation,
                authority: effective,
                state: ExecutionState::Active,
            };
            state.executions.insert(id, execution.clone());
            Ok(ExecutionResponse::Execution { execution })
        }
        ExecutionCommand::DelegateExecution {
            parent_execution,
            id,
            requested_authority,
        } => {
            validate_identity("execution id", &id)?;
            ensure_new_execution(state, &id)?;
            let parent = active_execution(state, &parent_execution)?.clone();
            let requested = parse_execution_authority(&requested_authority)?;
            let caller_limited =
                execution_authority_from(&context.call.authority.attenuate(&requested));
            let authority = attenuate_execution_authority(&parent.authority, &caller_limited)?;
            let execution = ExecutionRecord {
                id: id.clone(),
                parent_execution: Some(parent_execution),
                graph_generation: parent.graph_generation.clone(),
                authority,
                state: ExecutionState::Active,
            };
            state.executions.insert(id, execution.clone());
            Ok(ExecutionResponse::Execution { execution })
        }
        ExecutionCommand::FinishExecution { id, success } => {
            let execution = state
                .executions
                .get_mut(&id)
                .ok_or_else(|| format!("unknown execution: {id}"))?;
            if execution.state != ExecutionState::Active {
                return Err(format!("execution is not active: {id}"));
            }
            execution.state = if success {
                ExecutionState::Completed
            } else {
                ExecutionState::Failed
            };
            Ok(ExecutionResponse::Execution {
                execution: execution.clone(),
            })
        }
        ExecutionCommand::RegisterCallable {
            id,
            service,
            required_authority,
        } => {
            validate_identity("callable id", &id)?;
            ServiceId::parse(&service).map_err(|error| error.to_string())?;
            parse_execution_authority(&required_authority)?;
            let callable = CallableRecord {
                id: id.clone(),
                service,
                required_authority,
            };
            if let Some(existing) = state.callables.get(&id) {
                if existing != &callable {
                    return Err(format!("callable identity is immutable: {id}"));
                }
                return Ok(ExecutionResponse::Callable {
                    callable: existing.clone(),
                });
            }
            state.callables.insert(id, callable.clone());
            Ok(ExecutionResponse::Callable { callable })
        }
        ExecutionCommand::CreateTask {
            id,
            parent_execution,
            description,
            depends_on,
            requested_authority,
        } => {
            validate_identity("worker task id", &id)?;
            validate_identity("worker task description", &description)?;
            if state.tasks.contains_key(&id) {
                return Err(format!("worker task already exists: {id}"));
            }
            let parent = active_execution(state, &parent_execution)?;
            for dependency in &depends_on {
                if !state.tasks.contains_key(dependency) {
                    return Err(format!("unknown worker task dependency: {dependency}"));
                }
            }
            // Dependencies must already exist, while task identities and dependency sets are
            // immutable after insertion. Adding this fresh task therefore cannot create a cycle.
            let requested = parse_execution_authority(&requested_authority)?;
            let caller_limited =
                execution_authority_from(&context.call.authority.attenuate(&requested));
            let delegated_authority =
                attenuate_execution_authority(&parent.authority, &caller_limited)?;
            let task = WorkerTaskRecord {
                id: id.clone(),
                parent_execution,
                graph_generation: parent.graph_generation.clone(),
                description,
                depends_on,
                delegated_authority,
                state: WorkerTaskState::Pending,
            };
            state.tasks.insert(id, task.clone());
            Ok(ExecutionResponse::Task { task })
        }
        ExecutionCommand::RunnableTasks => {
            let task_ids = runnable_tasks(state);
            Ok(ExecutionResponse::RunnableTasks { task_ids })
        }
        ExecutionCommand::StartTask {
            task_id,
            execution_id,
        } => {
            let task = state
                .tasks
                .get(&task_id)
                .ok_or_else(|| format!("unknown worker task: {task_id}"))?
                .clone();
            if task.state != WorkerTaskState::Pending {
                return Err(format!("worker task is not pending: {task_id}"));
            }
            if !task.depends_on.iter().all(|dependency| {
                matches!(
                    state.tasks.get(dependency).map(|task| &task.state),
                    Some(WorkerTaskState::Completed { .. })
                )
            }) {
                return Err(format!("worker task is blocked: {task_id}"));
            }
            let execution = state
                .executions
                .get(&execution_id)
                .ok_or_else(|| format!("unknown execution: {execution_id}"))?;
            if execution.parent_execution.as_deref() != Some(task.parent_execution.as_str()) {
                return Err(format!(
                    "worker execution {execution_id} does not belong to parent {}",
                    task.parent_execution
                ));
            }
            if execution.authority != task.delegated_authority {
                return Err(format!(
                    "worker execution authority does not match task: {task_id}"
                ));
            }
            if execution.graph_generation != task.graph_generation {
                return Err(format!(
                    "worker execution graph generation does not match task: {task_id}"
                ));
            }
            let task = state.tasks.get_mut(&task_id).expect("task exists above");
            task.state = WorkerTaskState::Running {
                execution_id: execution_id.clone(),
            };
            Ok(ExecutionResponse::Task { task: task.clone() })
        }
        ExecutionCommand::CompleteTask {
            task_id,
            execution_id,
            result_refs,
        } => {
            let task = state
                .tasks
                .get_mut(&task_id)
                .ok_or_else(|| format!("unknown worker task: {task_id}"))?;
            require_running_execution(task, &execution_id)?;
            task.state = WorkerTaskState::Completed {
                execution_id,
                result_refs,
            };
            Ok(ExecutionResponse::Task { task: task.clone() })
        }
        ExecutionCommand::FailTask {
            task_id,
            execution_id,
            cause,
        } => {
            validate_identity("worker task failure cause", &cause)?;
            let task = state
                .tasks
                .get_mut(&task_id)
                .ok_or_else(|| format!("unknown worker task: {task_id}"))?;
            require_running_execution(task, &execution_id)?;
            task.state = WorkerTaskState::Failed {
                execution_id,
                cause,
            };
            Ok(ExecutionResponse::Task { task: task.clone() })
        }
        ExecutionCommand::CreateDelegatedTask { .. }
        | ExecutionCommand::CompleteDelegatedTask { .. }
        | ExecutionCommand::GetDelegatedTask { .. } => {
            Err("delegated task runtime is not active at this stack layer".into())
        }
        ExecutionCommand::GetExecution { .. }
        | ExecutionCommand::GetTask { .. }
        | ExecutionCommand::InvokeCallable { .. } => {
            Err("read-only execution command reached mutation path".into())
        }
    }
}

fn invoke_callable(
    context: &ExecutionContext<'_, '_>,
    execution_id: &str,
    callable_id: &str,
    input: &[u8],
) -> Result<ExecutionResponse, String> {
    let (_, state) = read_state(context)?;
    let execution = active_execution(&state, execution_id)?;
    let active_generation = context
        .call
        .graph_generation
        .ok_or_else(|| "callable invocation requires an active graph generation".to_owned())?;
    if execution.graph_generation != active_generation.as_str() {
        return Err(format!(
            "execution {execution_id} is pinned to graph generation {}, but active generation is {}",
            execution.graph_generation,
            active_generation.as_str()
        ));
    }
    let callable = state
        .callables
        .get(callable_id)
        .ok_or_else(|| format!("unknown callable: {callable_id}"))?;
    let required = parse_execution_authority(&callable.required_authority)?;
    if !parse_execution_authority(&execution.authority)?.permits_all(&required)
        || !context.call.authority.permits_all(&required)
    {
        return Err(format!("callable authority denied: {callable_id}"));
    }
    let service = ServiceId::parse(&callable.service).map_err(|error| error.to_string())?;
    let output = context
        .kernel
        .invoke_service_abi(&service, input, &required, None)
        .map_err(|error| error.to_string())?;
    Ok(ExecutionResponse::Invocation { output })
}

fn mutate_state<F>(
    context: &ExecutionContext<'_, '_>,
    mutation: F,
) -> Result<ExecutionResponse, String>
where
    F: FnOnce(&mut ExecutionProjection) -> Result<ExecutionResponse, String>,
{
    let (old, mut state) = read_state(context)?;
    let response = mutation(&mut state)?;
    context
        .kernel
        .transact_durable(
            &execution_namespace(),
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
    context: &ExecutionContext<'_, '_>,
) -> Result<(Option<Vec<u8>>, ExecutionProjection), String> {
    let old = context
        .kernel
        .read_durable(&execution_namespace(), STATE_KEY)
        .map_err(|error| error.to_string())?;
    let state = old
        .as_deref()
        .map(|bytes| serde_json::from_slice(bytes).map_err(|error| error.to_string()))
        .transpose()?
        .unwrap_or_default();
    Ok((old, state))
}

fn ensure_new_execution(state: &ExecutionProjection, id: &str) -> Result<(), String> {
    if state.executions.contains_key(id) {
        Err(format!("execution already exists: {id}"))
    } else {
        Ok(())
    }
}

fn active_execution<'a>(
    state: &'a ExecutionProjection,
    id: &str,
) -> Result<&'a ExecutionRecord, String> {
    let execution = state
        .executions
        .get(id)
        .ok_or_else(|| format!("unknown execution: {id}"))?;
    if execution.state != ExecutionState::Active {
        return Err(format!("execution is not active: {id}"));
    }
    Ok(execution)
}

fn require_running_execution(task: &WorkerTaskRecord, execution_id: &str) -> Result<(), String> {
    match &task.state {
        WorkerTaskState::Running {
            execution_id: running,
        } if running == execution_id => Ok(()),
        _ => Err(format!(
            "worker task {} is not running on execution {execution_id}",
            task.id
        )),
    }
}

fn runnable_tasks(state: &ExecutionProjection) -> Vec<String> {
    state
        .tasks
        .values()
        .filter(|task| task.state == WorkerTaskState::Pending)
        .filter(|task| {
            task.depends_on.iter().all(|dependency| {
                matches!(
                    state.tasks.get(dependency).map(|task| &task.state),
                    Some(WorkerTaskState::Completed { .. })
                )
            })
        })
        .map(|task| task.id.clone())
        .collect()
}

fn validate_identity(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{label} must not be empty"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{
        Kernel, KernelConfig, LocalPersistence, PluginState, ResolvedHarness,
        ResolvedHarnessActivation,
    };
    use std::{
        collections::BTreeSet,
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn authority(values: &[&str]) -> ExecutionAuthority {
        ExecutionAuthority::new(values.iter().copied())
    }

    fn caller_authority() -> Authority {
        Authority::new([
            capability(PERSISTENCE_SCHEMA),
            capability(PERSISTENCE_READ),
            capability(PERSISTENCE_WRITE),
            capability("fs.read"),
            capability("fs.write"),
        ])
    }

    fn kernel_with(path: &PathBuf) -> Kernel {
        let manifest = execution_manifest(caller_authority());
        let plugin = manifest.id.clone();
        let persistence = LocalPersistence::open(path).unwrap();
        let resolved =
            ResolvedHarness::resolve([manifest.clone()], [], [], &caller_authority()).unwrap();
        let mut kernel =
            Kernel::with_persistence(KernelConfig::new([manifest]).unwrap(), persistence);
        kernel.activate_resolved_harness(&resolved).unwrap();
        kernel
            .register_embedded_factory(plugin.clone(), execution_factory)
            .unwrap();
        kernel.activate_all().unwrap();
        assert_eq!(kernel.state(&plugin), Some(PluginState::Active));
        kernel
    }

    fn invoke(
        kernel: &mut Kernel,
        command: &ExecutionCommand,
    ) -> Result<ExecutionResponse, String> {
        let input = phenix_core::PhenixValue::from(command);
        let output = kernel
            .invoke(
                &execution_service(),
                &serde_json::to_vec(&input).unwrap(),
                &caller_authority(),
                None,
            )
            .map_err(|error| error.to_string())?;
        let output: phenix_core::PhenixValue =
            serde_json::from_slice(&output).map_err(|error| error.to_string())?;
        ExecutionResponse::try_from(phenix_core::Project(&output))
            .map_err(|error| error.to_string())
    }

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

    fn create(kernel: &mut Kernel, id: &str, requested: ExecutionAuthority) -> ExecutionRecord {
        match invoke(
            kernel,
            &ExecutionCommand::CreateExecution {
                id: id.into(),
                requested_authority: requested,
            },
        )
        .unwrap()
        {
            ExecutionResponse::Execution { execution } => execution,
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[test]
    fn delegation_attenuates_authority_and_restores_durably() {
        let path = temp_db("execution-restore");
        {
            let mut kernel = kernel_with(&path);
            let root = create(&mut kernel, "root", authority(&["fs.read"]));
            assert_eq!(root.authority, authority(&["fs.read"]));
            assert_eq!(
                root.graph_generation,
                kernel.graph_generation().unwrap().as_str()
            );
            let child = match invoke(
                &mut kernel,
                &ExecutionCommand::DelegateExecution {
                    parent_execution: "root".into(),
                    id: "child".into(),
                    requested_authority: authority(&["fs.read", "fs.write"]),
                },
            )
            .unwrap()
            {
                ExecutionResponse::Execution { execution } => execution,
                other => panic!("unexpected response: {other:?}"),
            };
            assert_eq!(child.authority, authority(&["fs.read"]));
            assert_eq!(child.graph_generation, root.graph_generation);
        }
        let mut restored = kernel_with(&path);
        assert!(matches!(
            invoke(
                &mut restored,
                &ExecutionCommand::GetExecution { id: "child".into() },
            )
            .unwrap(),
            ExecutionResponse::ExecutionLookup { execution: Some(_) }
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn task_creation_rules_make_dependency_cycles_unrepresentable() {
        let path = temp_db("execution-task-dag");
        let mut kernel = kernel_with(&path);
        create(&mut kernel, "root", authority(&["fs.read"]));

        let missing = invoke(
            &mut kernel,
            &ExecutionCommand::CreateTask {
                id: "a".into(),
                parent_execution: "root".into(),
                description: "task a".into(),
                depends_on: BTreeSet::from(["b".to_owned()]),
                requested_authority: authority(&["fs.read"]),
            },
        )
        .unwrap_err();
        assert!(missing.contains("unknown worker task dependency: b"));

        invoke(
            &mut kernel,
            &ExecutionCommand::CreateTask {
                id: "a".into(),
                parent_execution: "root".into(),
                description: "task a".into(),
                depends_on: BTreeSet::new(),
                requested_authority: authority(&["fs.read"]),
            },
        )
        .unwrap();
        invoke(
            &mut kernel,
            &ExecutionCommand::CreateTask {
                id: "b".into(),
                parent_execution: "root".into(),
                description: "task b".into(),
                depends_on: BTreeSet::from(["a".to_owned()]),
                requested_authority: authority(&["fs.read"]),
            },
        )
        .unwrap();

        let immutable = invoke(
            &mut kernel,
            &ExecutionCommand::CreateTask {
                id: "a".into(),
                parent_execution: "root".into(),
                description: "task a".into(),
                depends_on: BTreeSet::from(["b".to_owned()]),
                requested_authority: authority(&["fs.read"]),
            },
        )
        .unwrap_err();
        assert!(immutable.contains("worker task already exists: a"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn task_dependencies_are_deterministic_and_share_execution_authority() {
        let path = temp_db("execution-tasks");
        let mut kernel = kernel_with(&path);
        create(&mut kernel, "root", authority(&["fs.read"]));
        for (id, depends_on) in [
            ("a", BTreeSet::new()),
            ("b", BTreeSet::from(["a".to_owned()])),
        ] {
            invoke(
                &mut kernel,
                &ExecutionCommand::CreateTask {
                    id: id.into(),
                    parent_execution: "root".into(),
                    description: format!("task {id}"),
                    depends_on,
                    requested_authority: authority(&["fs.read", "fs.write"]),
                },
            )
            .unwrap();
        }
        assert_eq!(
            invoke(&mut kernel, &ExecutionCommand::RunnableTasks).unwrap(),
            ExecutionResponse::RunnableTasks {
                task_ids: vec!["a".into()]
            }
        );
        let child = match invoke(
            &mut kernel,
            &ExecutionCommand::DelegateExecution {
                parent_execution: "root".into(),
                id: "worker-a".into(),
                requested_authority: authority(&["fs.read"]),
            },
        )
        .unwrap()
        {
            ExecutionResponse::Execution { execution } => execution,
            other => panic!("unexpected response: {other:?}"),
        };
        assert_eq!(child.authority, authority(&["fs.read"]));
        invoke(
            &mut kernel,
            &ExecutionCommand::StartTask {
                task_id: "a".into(),
                execution_id: "worker-a".into(),
            },
        )
        .unwrap();
        invoke(
            &mut kernel,
            &ExecutionCommand::CompleteTask {
                task_id: "a".into(),
                execution_id: "worker-a".into(),
                result_refs: vec!["artifact:result".into()],
            },
        )
        .unwrap();
        assert_eq!(
            invoke(&mut kernel, &ExecutionCommand::RunnableTasks).unwrap(),
            ExecutionResponse::RunnableTasks {
                task_ids: vec!["b".into()]
            }
        );
        let _ = fs::remove_file(path);
    }
}
