use super::{BudgetReservation, RemainingBudget};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum BudgetReservationPurpose {
    RootStep,
    Retry,
    Helper,
    Delegation,
    Verification,
    RecoveryClassifier,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct RootBudgetLimits {
    pub fresh_input_tokens: u64,
    pub output_tokens: u64,
    pub cost_microunits: Option<u64>,
    pub attempts: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct BudgetReservationRequest {
    pub reservation_id: String,
    pub parent_reservation_id: Option<String>,
    pub policy_revision: String,
    pub purpose: BudgetReservationPurpose,
    pub budget: BudgetReservation,
    pub attempts: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct BudgetActual {
    pub fresh_input_tokens: u64,
    pub output_tokens: u64,
    pub cost_microunits: Option<u64>,
    pub attempts: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum BudgetReservationState {
    Reserved,
    Settled { actual: BudgetActual },
    Released,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct BudgetReservationRecord {
    pub request: BudgetReservationRequest,
    pub state: BudgetReservationState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct RootBudgetLedger {
    pub root_execution_id: String,
    pub limits: RootBudgetLimits,
    #[serde(default)]
    pub reservations: BTreeMap<String, BudgetReservationRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum BudgetLedgerError {
    DuplicateReservation { reservation_id: String },
    UnknownParentReservation { reservation_id: String },
    ParentNotReserved { reservation_id: String },
    InputBudgetExceeded { requested: u64, remaining: u64 },
    OutputBudgetExceeded { requested: u64, remaining: u64 },
    CostBudgetExceeded { requested: u64, remaining: u64 },
    AttemptBudgetExceeded { requested: u32, remaining: u32 },
    UnknownReservation { reservation_id: String },
    ReservationNotActive { reservation_id: String },
    ActualExceedsReservation { reservation_id: String },
}

impl RootBudgetLedger {
    #[must_use]
    pub fn remaining(&self) -> RemainingBudget {
        let mut remaining = RemainingBudget {
            fresh_input_tokens: self.limits.fresh_input_tokens,
            output_tokens: self.limits.output_tokens,
            cost_microunits: self.limits.cost_microunits,
            attempts: self.limits.attempts,
        };
        for record in self.reservations.values() {
            if let BudgetReservationState::Reserved = record.state {
                remaining.fresh_input_tokens = remaining
                    .fresh_input_tokens
                    .saturating_sub(record.request.budget.input_tokens);
                remaining.output_tokens = remaining
                    .output_tokens
                    .saturating_sub(record.request.budget.output_tokens);
                remaining.attempts = remaining.attempts.saturating_sub(record.request.attempts);
                if let (Some(current), Some(reserved)) = (
                    remaining.cost_microunits,
                    record.request.budget.cost_microunits,
                ) {
                    remaining.cost_microunits = Some(current.saturating_sub(reserved));
                }
            } else if let BudgetReservationState::Settled { actual } = &record.state {
                remaining.fresh_input_tokens = remaining
                    .fresh_input_tokens
                    .saturating_sub(actual.fresh_input_tokens);
                remaining.output_tokens = remaining
                    .output_tokens
                    .saturating_sub(actual.output_tokens);
                remaining.attempts = remaining.attempts.saturating_sub(actual.attempts);
                if let (Some(current), Some(actual_cost)) =
                    (remaining.cost_microunits, actual.cost_microunits)
                {
                    remaining.cost_microunits = Some(current.saturating_sub(actual_cost));
                }
            }
        }
        remaining
    }

    pub fn reserve(
        &mut self,
        request: BudgetReservationRequest,
    ) -> Result<(), BudgetLedgerError> {
        if self.reservations.contains_key(&request.reservation_id) {
            return Err(BudgetLedgerError::DuplicateReservation {
                reservation_id: request.reservation_id,
            });
        }
        if let Some(parent_id) = &request.parent_reservation_id {
            let Some(parent) = self.reservations.get(parent_id) else {
                return Err(BudgetLedgerError::UnknownParentReservation {
                    reservation_id: parent_id.clone(),
                });
            };
            if parent.state != BudgetReservationState::Reserved {
                return Err(BudgetLedgerError::ParentNotReserved {
                    reservation_id: parent_id.clone(),
                });
            }
        }

        let remaining = self.remaining();
        if request.budget.input_tokens > remaining.fresh_input_tokens {
            return Err(BudgetLedgerError::InputBudgetExceeded {
                requested: request.budget.input_tokens,
                remaining: remaining.fresh_input_tokens,
            });
        }
        if request.budget.output_tokens > remaining.output_tokens {
            return Err(BudgetLedgerError::OutputBudgetExceeded {
                requested: request.budget.output_tokens,
                remaining: remaining.output_tokens,
            });
        }
        if request.attempts > remaining.attempts {
            return Err(BudgetLedgerError::AttemptBudgetExceeded {
                requested: request.attempts,
                remaining: remaining.attempts,
            });
        }
        if let (Some(requested), Some(remaining)) =
            (request.budget.cost_microunits, remaining.cost_microunits)
        {
            if requested > remaining {
                return Err(BudgetLedgerError::CostBudgetExceeded {
                    requested,
                    remaining,
                });
            }
        }

        self.reservations.insert(
            request.reservation_id.clone(),
            BudgetReservationRecord {
                request,
                state: BudgetReservationState::Reserved,
            },
        );
        Ok(())
    }

    pub fn settle(
        &mut self,
        reservation_id: &str,
        actual: BudgetActual,
    ) -> Result<(), BudgetLedgerError> {
        let Some(record) = self.reservations.get_mut(reservation_id) else {
            return Err(BudgetLedgerError::UnknownReservation {
                reservation_id: reservation_id.to_owned(),
            });
        };
        if record.state != BudgetReservationState::Reserved {
            return Err(BudgetLedgerError::ReservationNotActive {
                reservation_id: reservation_id.to_owned(),
            });
        }
        let cost_exceeded = match (actual.cost_microunits, record.request.budget.cost_microunits) {
            (Some(actual), Some(reserved)) => actual > reserved,
            (Some(_), None) => false,
            _ => false,
        };
        if actual.fresh_input_tokens > record.request.budget.input_tokens
            || actual.output_tokens > record.request.budget.output_tokens
            || actual.attempts > record.request.attempts
            || cost_exceeded
        {
            return Err(BudgetLedgerError::ActualExceedsReservation {
                reservation_id: reservation_id.to_owned(),
            });
        }
        record.state = BudgetReservationState::Settled { actual };
        Ok(())
    }

    pub fn release(&mut self, reservation_id: &str) -> Result<(), BudgetLedgerError> {
        let Some(record) = self.reservations.get_mut(reservation_id) else {
            return Err(BudgetLedgerError::UnknownReservation {
                reservation_id: reservation_id.to_owned(),
            });
        };
        if record.state != BudgetReservationState::Reserved {
            return Err(BudgetLedgerError::ReservationNotActive {
                reservation_id: reservation_id.to_owned(),
            });
        }
        record.state = BudgetReservationState::Released;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger() -> RootBudgetLedger {
        RootBudgetLedger {
            root_execution_id: "root".into(),
            limits: RootBudgetLimits {
                fresh_input_tokens: 10_000,
                output_tokens: 2_000,
                cost_microunits: Some(10_000),
                attempts: 4,
            },
            reservations: BTreeMap::new(),
        }
    }

    fn reservation(id: &str, input: u64, attempts: u32) -> BudgetReservationRequest {
        BudgetReservationRequest {
            reservation_id: id.into(),
            parent_reservation_id: None,
            policy_revision: "policy-1".into(),
            purpose: BudgetReservationPurpose::RootStep,
            budget: BudgetReservation {
                input_tokens: input,
                output_tokens: 500,
                cost_microunits: Some(1_000),
            },
            attempts,
        }
    }

    #[test]
    fn concurrent_reservations_cannot_spend_the_same_budget() {
        let mut ledger = ledger();
        ledger.reserve(reservation("one", 7_000, 2)).unwrap();
        assert!(matches!(
            ledger.reserve(reservation("two", 4_000, 2)),
            Err(BudgetLedgerError::InputBudgetExceeded { .. })
        ));
    }

    #[test]
    fn settling_releases_unused_reserved_capacity() {
        let mut ledger = ledger();
        ledger.reserve(reservation("one", 7_000, 2)).unwrap();
        ledger
            .settle(
                "one",
                BudgetActual {
                    fresh_input_tokens: 2_000,
                    output_tokens: 100,
                    cost_microunits: Some(200),
                    attempts: 1,
                },
            )
            .unwrap();
        assert_eq!(ledger.remaining().fresh_input_tokens, 8_000);
        assert_eq!(ledger.remaining().attempts, 3);
    }
}
