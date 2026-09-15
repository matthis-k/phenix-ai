use crate::{attempt_service, resource_service};
use phenix_core::{
    ComponentInterface, PluginContext, PluginHost, PluginInstance, ServiceId, TransactionOp,
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
            let resource_old = context
                .kernel
                .read_durable(
                    &resource_service::execution_resource_namespace(),
                    resource_service::RESOURCE_STATE_KEY,
                )
                .map_err(|error| error.to_string())?;
            let mut resources = resource_service::restore(resource_old.as_deref())?;
            let ledger = resources
                .settle_reservation(&root_execution_id, &reservation_id, actual)
                .map_err(|error| format!("root budget settlement failed: {error:?}"))?;
            let resource_encoded = resource_service::encode_state(&resources)?;
            let resource_mutation = context
                .kernel
                .prepare_durable_transaction(
                    &resource_service::execution_resource_namespace(),
                    &[
                        TransactionOp::AssertValue {
                            key: resource_service::RESOURCE_STATE_KEY.into(),
                            expected: resource_old,
                        },
                        TransactionOp::Put {
                            key: resource_service::RESOURCE_STATE_KEY.into(),
                            value: resource_encoded,
                        },
                    ],
                )
                .map_err(|error| error.to_string())?;

            let attempt_old = context
                .kernel
                .read_durable(
                    &attempt_service::attempt_namespace(),
                    attempt_service::ATTEMPT_STATE_KEY,
                )
                .map_err(|error| error.to_string())?;
            let mut attempts = attempt_service::restore(attempt_old.as_deref())?;
            let attempt = attempts.settle(&attempt_id, outcome)?;
            let attempt_encoded = attempt_service::encode_ledger(&attempts)?;
            let attempt_mutation = context
                .kernel
                .prepare_durable_transaction(
                    &attempt_service::attempt_namespace(),
                    &[
                        TransactionOp::AssertValue {
                            key: attempt_service::ATTEMPT_STATE_KEY.into(),
                            expected: attempt_old,
                        },
                        TransactionOp::Put {
                            key: attempt_service::ATTEMPT_STATE_KEY.into(),
                            value: attempt_encoded,
                        },
                    ],
                )
                .map_err(|error| error.to_string())?;

            context
                .kernel
                .transact_prepared(&[resource_mutation, attempt_mutation])
                .map_err(|error| error.to_string())?;

            Ok(StepTransactionResponse::Settled { ledger, attempt })
        }
    }
}
