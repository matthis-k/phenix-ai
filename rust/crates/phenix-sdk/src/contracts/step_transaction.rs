use super::{
    AttemptOutcome, AttemptUsageRecord, BudgetActual, RootBudgetLedger, StepAttemptRecord,
};
use phenix_core::{ComponentInterface, InterfaceId, ServiceId};
use serde::{Deserialize, Serialize};

pub const STEP_TRANSACTION_SERVICE: &str = "phenix.execution.step-transaction@1";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum StepTransactionCommand {
    Settle {
        root_execution_id: String,
        reservation_id: String,
        actual: BudgetActual,
        attempt_id: String,
        outcome: AttemptOutcome,
        usage: Box<AttemptUsageRecord>,
    },
    AbortBeforeDispatch {
        root_execution_id: String,
        reservation_id: Option<String>,
        attempt_id: String,
        outcome: AttemptOutcome,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case", deny_unknown_fields)]
pub enum StepTransactionResponse {
    Settled {
        ledger: RootBudgetLedger,
        attempt: StepAttemptRecord,
    },
    Aborted {
        ledger: Option<RootBudgetLedger>,
        attempt: StepAttemptRecord,
    },
}

pub struct StepTransactionInterface;

impl ComponentInterface for StepTransactionInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(STEP_TRANSACTION_SERVICE)
            .expect("static step transaction interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<StepTransactionCommand, StepTransactionResponse>()
    }
}

#[must_use]
pub fn step_transaction_service() -> ServiceId {
    ServiceId::parse(STEP_TRANSACTION_SERVICE).expect("static step transaction service id is valid")
}
