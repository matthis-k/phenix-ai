use crate::resource_transaction::ExecutionResourceState;
use phenix_core::{
    ComponentInterface, DurableSchema, PluginContext, PluginHost, PluginInstance,
    ResourceNamespace, ServiceId, TransactionOp,
};
use phenix_sdk::{
    execution_resource_service, ExecutionResourceCommand, ExecutionResourceInterface,
    ExecutionResourceResponse,
};

const EXECUTION_RESOURCE_NAMESPACE: &str = "phenix.execution.resources.state";
pub(crate) const RESOURCE_STATE_KEY: &str = "state";
const MAX_RESOURCE_STATE_BYTES: usize = 16 * 1024 * 1024;

type ResourceContext<'host, 'runtime> = PluginContext<'host, 'runtime, ()>;

fn context<'host, 'runtime>(host: &'host PluginHost<'runtime>) -> ResourceContext<'host, 'runtime> {
    PluginContext::new(host, (), (), ())
}

pub(crate) fn execution_resource_namespace() -> ResourceNamespace {
    ResourceNamespace::parse(EXECUTION_RESOURCE_NAMESPACE).expect("static namespace is valid")
}

#[must_use]
pub(crate) fn resource_factory() -> Box<dyn PluginInstance> {
    Box::new(ExecutionResourcePlugin)
}

struct ExecutionResourcePlugin;

impl PluginInstance for ExecutionResourcePlugin {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        let context = context(host);
        context
            .kernel
            .register_durable_schema(&DurableSchema::new(execution_resource_namespace(), 1))
            .map_err(|error| error.to_string())?;
        let snapshot = context
            .kernel
            .read_durable(&execution_resource_namespace(), RESOURCE_STATE_KEY)
            .map_err(|error| error.to_string())?;
        restore(snapshot.as_deref()).map(|_| ())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &execution_resource_service() {
            return Err(format!("unsupported execution resource service: {service}"));
        }
        let context = context(host);
        let command = context
            .kernel
            .decode_projected::<ExecutionResourceCommand>(
                &ExecutionResourceInterface::interface_id(),
                input,
            )
            .map_err(|error| error.to_string())?;
        let old = context
            .kernel
            .read_durable(&execution_resource_namespace(), RESOURCE_STATE_KEY)
            .map_err(|error| error.to_string())?;
        let state = restore(old.as_deref())?;
        let response = if is_mutation(&command) {
            mutate(&context, old, state, command)?
        } else {
            read(&state, command)?
        };
        context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string())
    }
}

fn is_mutation(command: &ExecutionResourceCommand) -> bool {
    !matches!(
        command,
        ExecutionResourceCommand::Remaining { .. }
            | ExecutionResourceCommand::RemainingWithin { .. }
            | ExecutionResourceCommand::GetDelegated { .. }
    )
}

fn read(
    state: &ExecutionResourceState,
    command: ExecutionResourceCommand,
) -> Result<ExecutionResourceResponse, String> {
    match command {
        ExecutionResourceCommand::Remaining { root_execution_id } => state
            .remaining(&root_execution_id)
            .map(|budget| ExecutionResourceResponse::Remaining { budget })
            .map_err(|error| format!("execution resource remaining failed: {error:?}")),
        ExecutionResourceCommand::RemainingWithin {
            root_execution_id,
            reservation_id,
        } => state
            .remaining_within(&root_execution_id, &reservation_id)
            .map(|budget| ExecutionResourceResponse::Remaining { budget })
            .map_err(|error| format!("execution resource nested remaining failed: {error:?}")),
        ExecutionResourceCommand::GetDelegated { task_id } => {
            Ok(ExecutionResourceResponse::DelegatedTaskLookup {
                task: state.delegated_task(&task_id).cloned(),
            })
        }
        _ => Err("mutating execution resource command reached read path".into()),
    }
}

fn mutate(
    context: &ResourceContext<'_, '_>,
    old: Option<Vec<u8>>,
    mut next: ExecutionResourceState,
    command: ExecutionResourceCommand,
) -> Result<ExecutionResourceResponse, String> {
    let response = match command {
        ExecutionResourceCommand::RegisterRootBudget { ledger } => next
            .register_root_budget(ledger)
            .map(|ledger| ExecutionResourceResponse::RootBudget { ledger })
            .map_err(|error| format!("root budget registration failed: {error:?}"))?,
        ExecutionResourceCommand::Reserve {
            root_execution_id,
            reservation,
        } => next
            .reserve(&root_execution_id, reservation)
            .map(|ledger| ExecutionResourceResponse::RootBudget { ledger })
            .map_err(|error| format!("root budget reservation failed: {error:?}"))?,
        ExecutionResourceCommand::SettleReservation {
            root_execution_id,
            reservation_id,
            actual,
        } => next
            .settle_reservation(&root_execution_id, &reservation_id, actual)
            .map(|ledger| ExecutionResourceResponse::RootBudget { ledger })
            .map_err(|error| format!("root budget settlement failed: {error:?}"))?,
        ExecutionResourceCommand::ReleaseReservation {
            root_execution_id,
            reservation_id,
        } => next
            .release_reservation(&root_execution_id, &reservation_id)
            .map(|ledger| ExecutionResourceResponse::RootBudget { ledger })
            .map_err(|error| format!("root budget release failed: {error:?}"))?,
        ExecutionResourceCommand::AdmitDelegated {
            root_execution_id,
            reservation,
            task,
            binding,
            parent_authority,
            policy,
            now_ms,
        } => next
            .admit_delegated(
                &root_execution_id,
                reservation,
                task,
                binding,
                &parent_authority,
                &policy,
                now_ms,
            )
            .map(|task| ExecutionResourceResponse::DelegatedTask { task })
            .map_err(|error| format!("delegated resource admission failed: {error:?}"))?,
        ExecutionResourceCommand::StartDelegated {
            task_id,
            execution_id,
            now_ms,
        } => next
            .start_delegated(&task_id, execution_id, now_ms)
            .map(|task| ExecutionResourceResponse::DelegatedTask { task })
            .map_err(|error| format!("delegated resource start failed: {error:?}"))?,
        ExecutionResourceCommand::CompleteDelegated {
            task_id,
            execution_id,
            result,
            actual,
        } => next
            .complete_delegated(&task_id, &execution_id, result, actual)
            .map(|task| ExecutionResourceResponse::DelegatedTask { task })
            .map_err(|error| format!("delegated resource completion failed: {error:?}"))?,
        ExecutionResourceCommand::FailDelegated {
            task_id,
            execution_id,
            cause,
            actual,
        } => {
            if cause.trim().is_empty() {
                return Err("delegated resource failure cause must not be empty".into());
            }
            next.fail_delegated(&task_id, &execution_id, cause, actual)
                .map(|task| ExecutionResourceResponse::DelegatedTask { task })
                .map_err(|error| format!("delegated resource failure failed: {error:?}"))?
        }
        ExecutionResourceCommand::Remaining { .. }
        | ExecutionResourceCommand::RemainingWithin { .. }
        | ExecutionResourceCommand::GetDelegated { .. } => {
            return Err("read-only execution resource command reached mutation path".into())
        }
    };
    let encoded = encode_state(&next)?;
    context
        .kernel
        .transact_durable(
            &execution_resource_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: RESOURCE_STATE_KEY.into(),
                    expected: old,
                },
                TransactionOp::Put {
                    key: RESOURCE_STATE_KEY.into(),
                    value: encoded,
                },
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(response)
}

pub(crate) fn encode_state(state: &ExecutionResourceState) -> Result<Vec<u8>, String> {
    let encoded = serde_json::to_vec(state).map_err(|error| error.to_string())?;
    if encoded.len() > MAX_RESOURCE_STATE_BYTES {
        return Err(format!(
            "execution resource state exceeds {MAX_RESOURCE_STATE_BYTES} bytes"
        ));
    }
    Ok(encoded)
}

pub(crate) fn restore(snapshot: Option<&[u8]>) -> Result<ExecutionResourceState, String> {
    let Some(bytes) = snapshot else {
        return Ok(ExecutionResourceState::default());
    };
    if bytes.len() > MAX_RESOURCE_STATE_BYTES {
        return Err(format!(
            "execution resource state exceeds {MAX_RESOURCE_STATE_BYTES} bytes"
        ));
    }
    serde_json::from_slice(bytes).map_err(|error| error.to_string())
}
