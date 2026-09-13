use super::BudgetReservation;
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(deny_unknown_fields)]
pub struct ExplorationPolicy {
    pub enabled: bool,
    pub min_parent_input_tokens_saved: u64,
    pub max_child_input_tokens: u64,
    pub max_child_output_tokens: u64,
    pub max_child_cost_microunits: Option<u64>,
    pub max_result_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ExplorationOpportunity {
    pub task_id: String,
    pub description: String,
    pub separable: bool,
    pub requires_parent_transcript: bool,
    pub parent_input_tokens_if_inline: u64,
    pub expected_parent_reacquisition_tokens: u64,
    pub child_input_tokens: u64,
    pub child_output_tokens: u64,
    pub child_cost_microunits: Option<u64>,
    pub expected_result_input_tokens: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "decision", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExplorationDecision {
    KeepInParent {
        reason: ExplorationRejection,
    },
    Delegate {
        task_id: String,
        reservation: BudgetReservation,
        expected_parent_input_tokens_saved: u64,
        max_result_bytes: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExplorationRejection {
    Disabled,
    NotSeparable,
    ParentTranscriptRequired,
    ChildInputBudgetExceeded { requested: u64, allowed: u64 },
    ChildOutputBudgetExceeded { requested: u64, allowed: u64 },
    ChildCostBudgetExceeded { requested: u64, allowed: u64 },
    NoExpectedSavings,
    SavingsBelowThreshold { expected: u64, required: u64 },
}

impl ExplorationPolicy {
    #[must_use]
    pub fn assess(&self, opportunity: &ExplorationOpportunity) -> ExplorationDecision {
        if !self.enabled {
            return ExplorationDecision::KeepInParent {
                reason: ExplorationRejection::Disabled,
            };
        }
        if !opportunity.separable {
            return ExplorationDecision::KeepInParent {
                reason: ExplorationRejection::NotSeparable,
            };
        }
        if opportunity.requires_parent_transcript {
            return ExplorationDecision::KeepInParent {
                reason: ExplorationRejection::ParentTranscriptRequired,
            };
        }
        if opportunity.child_input_tokens > self.max_child_input_tokens {
            return ExplorationDecision::KeepInParent {
                reason: ExplorationRejection::ChildInputBudgetExceeded {
                    requested: opportunity.child_input_tokens,
                    allowed: self.max_child_input_tokens,
                },
            };
        }
        if opportunity.child_output_tokens > self.max_child_output_tokens {
            return ExplorationDecision::KeepInParent {
                reason: ExplorationRejection::ChildOutputBudgetExceeded {
                    requested: opportunity.child_output_tokens,
                    allowed: self.max_child_output_tokens,
                },
            };
        }
        if let (Some(requested), Some(allowed)) = (
            opportunity.child_cost_microunits,
            self.max_child_cost_microunits,
        ) {
            if requested > allowed {
                return ExplorationDecision::KeepInParent {
                    reason: ExplorationRejection::ChildCostBudgetExceeded { requested, allowed },
                };
            }
        }

        let inline_total = opportunity
            .parent_input_tokens_if_inline
            .saturating_add(opportunity.expected_parent_reacquisition_tokens);
        let delegated_parent_cost = opportunity.expected_result_input_tokens;
        let expected_saved = inline_total.saturating_sub(delegated_parent_cost);
        if expected_saved == 0 {
            return ExplorationDecision::KeepInParent {
                reason: ExplorationRejection::NoExpectedSavings,
            };
        }
        if expected_saved < self.min_parent_input_tokens_saved {
            return ExplorationDecision::KeepInParent {
                reason: ExplorationRejection::SavingsBelowThreshold {
                    expected: expected_saved,
                    required: self.min_parent_input_tokens_saved,
                },
            };
        }

        ExplorationDecision::Delegate {
            task_id: opportunity.task_id.clone(),
            reservation: BudgetReservation {
                input_tokens: opportunity.child_input_tokens,
                output_tokens: opportunity.child_output_tokens,
                cost_microunits: opportunity.child_cost_microunits,
            },
            expected_parent_input_tokens_saved: expected_saved,
            max_result_bytes: self.max_result_bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opportunity() -> ExplorationOpportunity {
        ExplorationOpportunity {
            task_id: "explore-1".into(),
            description: "inspect independent subsystem".into(),
            separable: true,
            requires_parent_transcript: false,
            parent_input_tokens_if_inline: 5_000,
            expected_parent_reacquisition_tokens: 500,
            child_input_tokens: 1_000,
            child_output_tokens: 500,
            child_cost_microunits: Some(100),
            expected_result_input_tokens: 800,
        }
    }

    fn policy() -> ExplorationPolicy {
        ExplorationPolicy {
            enabled: true,
            min_parent_input_tokens_saved: 1_000,
            max_child_input_tokens: 2_000,
            max_child_output_tokens: 1_000,
            max_child_cost_microunits: Some(500),
            max_result_bytes: 64 * 1024,
        }
    }

    #[test]
    fn disabled_policy_never_auto_spawns() {
        assert!(matches!(
            ExplorationPolicy::default().assess(&opportunity()),
            ExplorationDecision::KeepInParent {
                reason: ExplorationRejection::Disabled
            }
        ));
    }

    #[test]
    fn pressure_without_separability_does_not_delegate() {
        let mut opportunity = opportunity();
        opportunity.parent_input_tokens_if_inline = 100_000;
        opportunity.separable = false;
        assert!(matches!(
            policy().assess(&opportunity),
            ExplorationDecision::KeepInParent {
                reason: ExplorationRejection::NotSeparable
            }
        ));
    }

    #[test]
    fn bounded_separable_work_can_delegate() {
        assert!(matches!(
            policy().assess(&opportunity()),
            ExplorationDecision::Delegate { .. }
        ));
    }
}
