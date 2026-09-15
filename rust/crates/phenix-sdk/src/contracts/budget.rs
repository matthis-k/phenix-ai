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
    ActiveChildren { reservation_id: String },
    InputBudgetExceeded { requested: u64, remaining: u64 },
    OutputBudgetExceeded { requested: u64, remaining: u64 },
    CostBudgetExceeded { requested: u64, remaining: u64 },
    AttemptBudgetExceeded { requested: u32, remaining: u32 },
    UnknownReservation { reservation_id: String },
    ReservationNotActive { reservation_id: String },
    ActualExceedsReservation { reservation_id: String },
    ActualBelowSettledChildren { reservation_id: String },
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
        for record in self
            .reservations
            .values()
            .filter(|record| record.request.parent_reservation_id.is_none())
        {
            subtract_record(&mut remaining, record);
        }
        remaining
    }

    pub fn reserve(&mut self, request: BudgetReservationRequest) -> Result<(), BudgetLedgerError> {
        if self.reservations.contains_key(&request.reservation_id) {
            return Err(BudgetLedgerError::DuplicateReservation {
                reservation_id: request.reservation_id,
            });
        }

        let remaining = if let Some(parent_id) = &request.parent_reservation_id {
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
            self.remaining_within(parent_id)?
        } else {
            self.remaining()
        };
        validate_fits(&request, &remaining)?;

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
        let record = self.reservations.get(reservation_id).ok_or_else(|| {
            BudgetLedgerError::UnknownReservation {
                reservation_id: reservation_id.to_owned(),
            }
        })?;
        if record.state != BudgetReservationState::Reserved {
            return Err(BudgetLedgerError::ReservationNotActive {
                reservation_id: reservation_id.to_owned(),
            });
        }
        if self.has_active_children(reservation_id) {
            return Err(BudgetLedgerError::ActiveChildren {
                reservation_id: reservation_id.to_owned(),
            });
        }
        if actual_exceeds_request(&actual, &record.request) {
            return Err(BudgetLedgerError::ActualExceedsReservation {
                reservation_id: reservation_id.to_owned(),
            });
        }
        let children = self.settled_direct_child_usage(reservation_id);
        if actual.fresh_input_tokens < children.fresh_input_tokens
            || actual.output_tokens < children.output_tokens
            || actual.attempts < children.attempts
            || actual_cost_for_request(&actual, &record.request)
                < children.cost_microunits.unwrap_or(0)
        {
            return Err(BudgetLedgerError::ActualBelowSettledChildren {
                reservation_id: reservation_id.to_owned(),
            });
        }
        self.reservations
            .get_mut(reservation_id)
            .expect("reservation checked above")
            .state = BudgetReservationState::Settled { actual };
        Ok(())
    }

    pub fn release(&mut self, reservation_id: &str) -> Result<(), BudgetLedgerError> {
        let record = self.reservations.get(reservation_id).ok_or_else(|| {
            BudgetLedgerError::UnknownReservation {
                reservation_id: reservation_id.to_owned(),
            }
        })?;
        if record.state != BudgetReservationState::Reserved {
            return Err(BudgetLedgerError::ReservationNotActive {
                reservation_id: reservation_id.to_owned(),
            });
        }
        if self.has_active_children(reservation_id) {
            return Err(BudgetLedgerError::ActiveChildren {
                reservation_id: reservation_id.to_owned(),
            });
        }
        self.reservations
            .get_mut(reservation_id)
            .expect("reservation checked above")
            .state = BudgetReservationState::Released;
        Ok(())
    }

    pub fn remaining_within(
        &self,
        reservation_id: &str,
    ) -> Result<RemainingBudget, BudgetLedgerError> {
        let parent = self.reservations.get(reservation_id).ok_or_else(|| {
            BudgetLedgerError::UnknownParentReservation {
                reservation_id: reservation_id.to_owned(),
            }
        })?;
        let mut remaining = RemainingBudget {
            fresh_input_tokens: parent.request.budget.input_tokens,
            output_tokens: parent.request.budget.output_tokens,
            cost_microunits: parent.request.budget.cost_microunits,
            attempts: parent.request.attempts,
        };
        for record in self.reservations.values().filter(|record| {
            record.request.parent_reservation_id.as_deref() == Some(reservation_id)
        }) {
            subtract_record(&mut remaining, record);
        }
        Ok(remaining)
    }

    fn has_active_children(&self, reservation_id: &str) -> bool {
        self.reservations.values().any(|record| {
            record.request.parent_reservation_id.as_deref() == Some(reservation_id)
                && record.state == BudgetReservationState::Reserved
        })
    }

    fn settled_direct_child_usage(&self, reservation_id: &str) -> BudgetActual {
        self.reservations
            .values()
            .filter(|record| {
                record.request.parent_reservation_id.as_deref() == Some(reservation_id)
            })
            .fold(
                BudgetActual {
                    fresh_input_tokens: 0,
                    output_tokens: 0,
                    cost_microunits: Some(0),
                    attempts: 0,
                },
                |mut total, record| {
                    if let BudgetReservationState::Settled { actual } = &record.state {
                        total.fresh_input_tokens = total
                            .fresh_input_tokens
                            .saturating_add(actual.fresh_input_tokens);
                        total.output_tokens =
                            total.output_tokens.saturating_add(actual.output_tokens);
                        total.attempts = total.attempts.saturating_add(actual.attempts);
                        let child_cost = actual_cost_for_request(actual, &record.request);
                        total.cost_microunits = total
                            .cost_microunits
                            .map(|cost| cost.saturating_add(child_cost));
                    }
                    total
                },
            )
    }
}

fn validate_fits(
    request: &BudgetReservationRequest,
    remaining: &RemainingBudget,
) -> Result<(), BudgetLedgerError> {
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
    Ok(())
}

fn subtract_record(remaining: &mut RemainingBudget, record: &BudgetReservationRecord) {
    match &record.state {
        BudgetReservationState::Reserved => {
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
        }
        BudgetReservationState::Settled { actual } => {
            remaining.fresh_input_tokens = remaining
                .fresh_input_tokens
                .saturating_sub(actual.fresh_input_tokens);
            remaining.output_tokens = remaining.output_tokens.saturating_sub(actual.output_tokens);
            remaining.attempts = remaining.attempts.saturating_sub(actual.attempts);
            if let Some(current) = remaining.cost_microunits {
                remaining.cost_microunits =
                    Some(current.saturating_sub(actual_cost_for_request(actual, &record.request)));
            }
        }
        BudgetReservationState::Released => {}
    }
}

fn actual_cost_for_request(actual: &BudgetActual, request: &BudgetReservationRequest) -> u64 {
    actual
        .cost_microunits
        .or(request.budget.cost_microunits)
        .unwrap_or(0)
}

fn actual_exceeds_request(actual: &BudgetActual, request: &BudgetReservationRequest) -> bool {
    let cost_exceeded = match (actual.cost_microunits, request.budget.cost_microunits) {
        (Some(actual), Some(reserved)) => actual > reserved,
        (Some(_), None) => false,
        _ => false,
    };
    actual.fresh_input_tokens > request.budget.input_tokens
        || actual.output_tokens > request.budget.output_tokens
        || actual.attempts > request.attempts
        || cost_exceeded
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

    mod budget_hierarchy {
        use super::*;

        #[test]
        fn child_reservation_spends_parent_capacity_without_double_charging_root() {
            let mut ledger = ledger();
            let mut parent = reservation("parent", 8_000, 3);
            parent.budget.output_tokens = 1_500;
            parent.budget.cost_microunits = Some(8_000);
            ledger.reserve(parent.clone()).unwrap();

            let mut child = reservation("child", 3_000, 1);
            child.parent_reservation_id = Some("parent".into());
            child.budget.output_tokens = 500;
            child.budget.cost_microunits = Some(2_000);
            ledger.reserve(child).unwrap();

            assert_eq!(ledger.remaining().fresh_input_tokens, 2_000);
            assert_eq!(ledger.remaining().attempts, 1);
            assert_eq!(ledger.remaining().cost_microunits, Some(2_000));
        }

        #[test]
        fn siblings_cannot_oversubscribe_parent_reservation() {
            let mut ledger = ledger();
            let mut parent = reservation("parent", 5_000, 2);
            parent.budget.output_tokens = 1_000;
            parent.budget.cost_microunits = Some(5_000);
            ledger.reserve(parent).unwrap();

            let mut first = reservation("first", 4_000, 1);
            first.parent_reservation_id = Some("parent".into());
            first.budget.cost_microunits = Some(3_000);
            ledger.reserve(first).unwrap();

            let mut second = reservation("second", 2_000, 1);
            second.parent_reservation_id = Some("parent".into());
            assert!(matches!(
                ledger.reserve(second),
                Err(BudgetLedgerError::InputBudgetExceeded {
                    remaining: 1_000,
                    ..
                })
            ));
        }

        #[test]
        fn parent_cannot_settle_while_child_is_active() {
            let mut ledger = ledger();
            let parent = reservation("parent", 5_000, 2);
            ledger.reserve(parent).unwrap();
            let mut child = reservation("child", 1_000, 1);
            child.parent_reservation_id = Some("parent".into());
            ledger.reserve(child).unwrap();
            assert!(matches!(
                ledger.settle(
                    "parent",
                    BudgetActual {
                        fresh_input_tokens: 1_000,
                        output_tokens: 100,
                        cost_microunits: Some(100),
                        attempts: 1,
                    },
                ),
                Err(BudgetLedgerError::ActiveChildren { .. })
            ));
        }
    }
}
