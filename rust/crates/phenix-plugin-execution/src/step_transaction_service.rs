use crate::{attempt_service, resource_service};
use phenix_core::{
    ComponentInterface, PluginContext, PluginHost, PluginInstance, PreparedMutationHandle,
    ServiceId, TransactionOp,
};
use phenix_sdk::{
    step_transaction_service, StepTransactionCommand, StepTransactionInterface,
    StepTransactionResponse,
};

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
        let response = handle(&context, command)?;
        context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string())
    }
}

fn handle(
    context: &StepTransactionContext<'_, '_>,
    command: StepTransactionCommand,
) -> Result<StepTransactionResponse, String> {
    match command {
        StepTransactionCommand::Settle {
            root_execution_id,
            reservation_id,
            actual,
            attempt_id,
            outcome,
        } => {
            let resource_old = read_resource_state(context)?;
            let mut resources = resource_service::restore(resource_old.as_deref())?;
            let ledger = resources
                .settle_reservation(&root_execution_id, &reservation_id, actual)
                .map_err(|error| format!("root budget settlement failed: {error:?}"))?;
            let resource_mutation = prepare_resource_state(context, resource_old, &resources)?;

            let attempt_old = read_attempt_state(context)?;
            let mut attempts = attempt_service::restore(attempt_old.as_deref())?;
            let attempt = attempts.settle(&attempt_id, outcome)?;
            let attempt_mutation = prepare_attempt_state(context, attempt_old, &attempts)?;

            context
                .kernel
                .transact_prepared(&[resource_mutation, attempt_mutation])
                .map_err(|error| error.to_string())?;

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
            let attempt_mutation = prepare_attempt_state(context, attempt_old, &attempts)?;

            let ledger = if let Some(reservation_id) = reservation_id {
                let resource_old = read_resource_state(context)?;
                let mut resources = resource_service::restore(resource_old.as_deref())?;
                let ledger = resources
                    .release_reservation(&root_execution_id, &reservation_id)
                    .map_err(|error| format!("root budget release failed: {error:?}"))?;
                let resource_mutation = prepare_resource_state(context, resource_old, &resources)?;
                context
                    .kernel
                    .transact_prepared(&[resource_mutation, attempt_mutation])
                    .map_err(|error| error.to_string())?;
                Some(ledger)
            } else {
                context
                    .kernel
                    .transact_prepared(&[attempt_mutation])
                    .map_err(|error| error.to_string())?;
                None
            };

            Ok(StepTransactionResponse::Aborted { ledger, attempt })
        }
    }
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

fn prepare_resource_state(
    context: &StepTransactionContext<'_, '_>,
    old: Option<Vec<u8>>,
    state: &crate::resource_transaction::ExecutionResourceState,
) -> Result<PreparedMutationHandle, String> {
    let encoded = resource_service::encode_state(state)?;
    context
        .kernel
        .prepare_durable_transaction(
            &resource_service::execution_resource_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: resource_service::RESOURCE_STATE_KEY.into(),
                    expected: old,
                },
                TransactionOp::Put {
                    key: resource_service::RESOURCE_STATE_KEY.into(),
                    value: encoded,
                },
            ],
        )
        .map_err(|error| error.to_string())
}

fn prepare_attempt_state(
    context: &StepTransactionContext<'_, '_>,
    old: Option<Vec<u8>>,
    ledger: &attempt_service::AttemptLedger,
) -> Result<PreparedMutationHandle, String> {
    let encoded = attempt_service::encode_ledger(ledger)?;
    context
        .kernel
        .prepare_durable_transaction(
            &attempt_service::attempt_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: attempt_service::ATTEMPT_STATE_KEY.into(),
                    expected: old,
                },
                TransactionOp::Put {
                    key: attempt_service::ATTEMPT_STATE_KEY.into(),
                    value: encoded,
                },
            ],
        )
        .map_err(|error| error.to_string())
}
