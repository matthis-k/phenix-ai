use crate::{attempt_service, resource_service};
use phenix_core::{
    ComponentInterface, KernelError, PluginContext, PluginHost, PluginInstance, ServiceId,
    TransactionOp,
};
use phenix_sdk::{
    step_transaction_service, StepTransactionCommand, StepTransactionInterface,
    StepTransactionResponse,
};

const MAX_STEP_TRANSACTION_CONFLICT_RETRIES: usize = 8;

type StepTransactionContext<'host, 'runtime> = PluginContext<'host, 'runtime, ()>;

fn context<'host, 'runtime>(
    host: &'host PluginHost<'runtime>,
) -> StepTransactionContext<'host, 'runtime> {
    PluginContext::new(host, (), (), ())
}

#[must_use]
pub(crate) fn step_transaction_factory() -> Box<dyn PluginInstance> {
    Box::new(StepTransactionPlugin)
}

struct StepTransactionPlugin;

impl PluginInstance for StepTransactionPlugin {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &step_transaction_service() {
            return Err(format!("unsupported step transaction service: {service}"));
        }
        let context = context(host);
        let command = context
            .kernel
            .decode_projected::<StepTransactionCommand>(
                &StepTransactionInterface::interface_id(),
                input,
            )
            .map_err(|error| error.to_string())?;
        let response = handle(&context, host, command)?;
        context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string())
    }
}

fn handle(
    context: &StepTransactionContext<'_, '_>,
    host: &PluginHost<'_>,
    command: StepTransactionCommand,
) -> Result<StepTransactionResponse, String> {
    for conflict_retry in 0..=MAX_STEP_TRANSACTION_CONFLICT_RETRIES {
        match handle_once(context, host, command.clone()) {
            Ok(response) => return Ok(response),
            Err(StepTransactionError::Kernel(error))
                if conflict_retry < MAX_STEP_TRANSACTION_CONFLICT_RETRIES
                    && is_step_state_conflict(&error) =>
            {
                continue;
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    unreachable!("step-transaction conflict retry loop always returns")
}

fn handle_once(
    context: &StepTransactionContext<'_, '_>,
    host: &PluginHost<'_>,
    command: StepTransactionCommand,
) -> Result<StepTransactionResponse, StepTransactionError> {
    match command {
        StepTransactionCommand::Settle {
            root_execution_id,
            reservation_id,
            actual,
            attempt_id,
            outcome,
            usage,
        } => {
            let resource_old = read_resource_state(context)?;
            let mut resources = resource_service::restore(resource_old.as_deref())?;
            let ledger = resources
                .settle_reservation(&root_execution_id, &reservation_id, actual.clone())
                .map_err(|error| format!("root budget settlement failed: {error:?}"))?;
            let resource_operations = resource_operations(resource_old, &resources)?;

            let attempt_old = read_attempt_state(context)?;
            let mut attempts = attempt_service::restore(attempt_old.as_deref())?;
            let attempt = attempts.settle_with_usage(&attempt_id, outcome, actual, *usage)?;
            let attempt_operations = attempt_operations(attempt_old, &attempts)?;

            let resource_namespace = resource_service::execution_resource_namespace();
            let attempt_namespace = attempt_service::attempt_namespace();
            host.transact_owned_durable_many(&[
                (&resource_namespace, resource_operations.as_slice()),
                (&attempt_namespace, attempt_operations.as_slice()),
            ])?;

            Ok(StepTransactionResponse::Settled { ledger, attempt })
        }
        StepTransactionCommand::AbortBeforeDispatch {
            root_execution_id,
            reservation_id,
            attempt_id,
            outcome,
        } => {
            let attempt_old = read_attempt_state(context)?;
            let mut attempts = attempt_service::restore(attempt_old.as_deref())?;
            let attempt = attempts.abort(&attempt_id, outcome)?;
            let attempt_operations = attempt_operations(attempt_old, &attempts)?;
            let attempt_namespace = attempt_service::attempt_namespace();

            let ledger = if let Some(reservation_id) = reservation_id {
                let resource_old = read_resource_state(context)?;
                let mut resources = resource_service::restore(resource_old.as_deref())?;
                let ledger = resources
                    .release_reservation(&root_execution_id, &reservation_id)
                    .map_err(|error| format!("root budget release failed: {error:?}"))?;
                let resource_operations = resource_operations(resource_old, &resources)?;
                let resource_namespace = resource_service::execution_resource_namespace();
                host.transact_owned_durable_many(&[
                    (&resource_namespace, resource_operations.as_slice()),
                    (&attempt_namespace, attempt_operations.as_slice()),
                ])?;
                Some(ledger)
            } else {
                context
                    .kernel
                    .transact_durable(&attempt_namespace, &attempt_operations)?;
                None
            };

            Ok(StepTransactionResponse::Aborted { ledger, attempt })
        }
    }
}

#[derive(Debug)]
enum StepTransactionError {
    Kernel(KernelError),
    Other(String),
}

impl std::fmt::Display for StepTransactionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Kernel(error) => error.fmt(formatter),
            Self::Other(error) => formatter.write_str(error),
        }
    }
}

impl From<KernelError> for StepTransactionError {
    fn from(error: KernelError) -> Self {
        Self::Kernel(error)
    }
}

impl From<String> for StepTransactionError {
    fn from(error: String) -> Self {
        Self::Other(error)
    }
}

fn is_step_state_conflict(error: &KernelError) -> bool {
    let KernelError::Persistence { message, .. } = error else {
        return false;
    };
    [
        (
            attempt_service::attempt_namespace(),
            attempt_service::ATTEMPT_STATE_KEY,
        ),
        (
            resource_service::execution_resource_namespace(),
            resource_service::RESOURCE_STATE_KEY,
        ),
    ]
    .into_iter()
    .any(|(namespace, key)| {
        message == &format!("transaction assertion failed for {namespace}/{key}")
    })
}

fn read_resource_state(
    context: &StepTransactionContext<'_, '_>,
) -> Result<Option<Vec<u8>>, String> {
    context
        .kernel
        .read_durable(
            &resource_service::execution_resource_namespace(),
            resource_service::RESOURCE_STATE_KEY,
        )
        .map_err(|error| error.to_string())
}

fn read_attempt_state(context: &StepTransactionContext<'_, '_>) -> Result<Option<Vec<u8>>, String> {
    context
        .kernel
        .read_durable(
            &attempt_service::attempt_namespace(),
            attempt_service::ATTEMPT_STATE_KEY,
        )
        .map_err(|error| error.to_string())
}

fn resource_operations(
    old: Option<Vec<u8>>,
    state: &crate::resource_transaction::ExecutionResourceState,
) -> Result<Vec<TransactionOp>, String> {
    Ok(vec![
        TransactionOp::AssertValue {
            key: resource_service::RESOURCE_STATE_KEY.into(),
            expected: old,
        },
        TransactionOp::Put {
            key: resource_service::RESOURCE_STATE_KEY.into(),
            value: resource_service::encode_state(state)?,
        },
    ])
}

fn attempt_operations(
    old: Option<Vec<u8>>,
    ledger: &attempt_service::AttemptLedger,
) -> Result<Vec<TransactionOp>, String> {
    Ok(vec![
        TransactionOp::AssertValue {
            key: attempt_service::ATTEMPT_STATE_KEY.into(),
            expected: old,
        },
        TransactionOp::Put {
            key: attempt_service::ATTEMPT_STATE_KEY.into(),
            value: attempt_service::encode_ledger(ledger)?,
        },
    ])
}
