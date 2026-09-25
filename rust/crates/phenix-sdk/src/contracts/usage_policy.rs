use super::{
    BudgetReservation, ContextDemand, DelegationResourcePolicy, ExecutionState, RoutingEstimate,
    RoutingRequirements,
};
use phenix_core::{CallableId, SkillId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct UsagePolicy {
    pub revision: String,
    pub max_fresh_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_cost_microunits: Option<u64>,
    pub max_retries: u32,
    pub max_tool_result_bytes: u64,
    pub max_tool_schemas: u32,
    pub max_skills: u32,
    pub require_known_capacity: bool,
    pub delegation: DelegationResourcePolicy,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct TaskRequirements {
    /// Fresh input that is part of the active request rather than a context candidate.
    pub request_input_tokens: u64,
    pub context: ContextDemand,
    #[serde(default)]
    pub required_capabilities: BTreeSet<String>,
    #[serde(default)]
    pub required_tools: BTreeSet<CallableId>,
    #[serde(default)]
    pub optional_tools: BTreeSet<CallableId>,
    #[serde(default)]
    pub required_skills: BTreeSet<SkillId>,
    #[serde(default)]
    pub optional_skills: BTreeSet<SkillId>,
    pub requested_reasoning: Option<String>,
    pub deadline_at_ms: Option<u64>,
}

#[derive(
    Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(deny_unknown_fields)]
pub struct RemainingBudget {
    pub fresh_input_tokens: u64,
    pub output_tokens: u64,
    pub cost_microunits: Option<u64>,
    pub attempts: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct UsagePlanningInput {
    pub task: TaskRequirements,
    pub execution_state: ExecutionState,
    pub remaining: RemainingBudget,
    pub now_ms: u64,
    #[serde(default)]
    pub historical_estimates: Vec<RoutingEstimate>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReasoningBudget {
    BackendDefault,
    Requested { level: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ToolProvisionBudget {
    #[serde(default)]
    pub initial: BTreeSet<CallableId>,
    #[serde(default)]
    pub expandable: BTreeSet<CallableId>,
    pub max_schemas: u32,
    pub max_result_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct SkillProvisionBudget {
    #[serde(default)]
    pub initial: BTreeSet<SkillId>,
    #[serde(default)]
    pub expandable: BTreeSet<SkillId>,
    pub max_loaded: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct RetryBudget {
    pub max_attempts: u32,
    pub reserved_attempts: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct StepPlan {
    pub policy_revision: String,
    pub routing: RoutingRequirements,
    pub context: ContextDemand,
    pub reasoning: ReasoningBudget,
    pub tools: ToolProvisionBudget,
    pub skills: SkillProvisionBudget,
    pub delegation: DelegationResourcePolicy,
    pub retry: RetryBudget,
    pub reservation: BudgetReservation,
    pub deadline_at_ms: Option<u64>,
    pub reducible_input_dropped_tokens: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum UsagePlanError {
    ExecutionNotActive,
    DeadlineExceeded { deadline_at_ms: u64, now_ms: u64 },
    NoAttemptsRemaining,
    MandatoryInputExceedsBudget { requested: u64, allowed: u64 },
    OutputReserveExceedsBudget { requested: u64, allowed: u64 },
    RequiredToolSetExceedsBudget { requested: u32, allowed: u32 },
    RequiredSkillSetExceedsBudget { requested: u32, allowed: u32 },
}

impl UsagePolicy {
    pub fn plan(&self, input: &UsagePlanningInput) -> Result<StepPlan, UsagePlanError> {
        if input.execution_state != ExecutionState::Active {
            return Err(UsagePlanError::ExecutionNotActive);
        }
        if let Some(deadline_at_ms) = input.task.deadline_at_ms {
            if input.now_ms >= deadline_at_ms {
                return Err(UsagePlanError::DeadlineExceeded {
                    deadline_at_ms,
                    now_ms: input.now_ms,
                });
            }
        }
        if input.remaining.attempts == 0 {
            return Err(UsagePlanError::NoAttemptsRemaining);
        }

        let fresh_input_budget = self
            .max_fresh_input_tokens
            .min(input.remaining.fresh_input_tokens);
        let output_budget = self.max_output_tokens.min(input.remaining.output_tokens);
        let cost_budget = match (self.max_cost_microunits, input.remaining.cost_microunits) {
            (Some(policy), Some(remaining)) => Some(policy.min(remaining)),
            (Some(policy), None) => Some(policy),
            (None, remaining) => remaining,
        };

        let mandatory_input_tokens = input
            .task
            .request_input_tokens
            .saturating_add(input.task.context.mandatory_input_tokens);
        if mandatory_input_tokens > fresh_input_budget {
            return Err(UsagePlanError::MandatoryInputExceedsBudget {
                requested: mandatory_input_tokens,
                allowed: fresh_input_budget,
            });
        }
        if input.task.context.output_reserve_tokens > output_budget {
            return Err(UsagePlanError::OutputReserveExceedsBudget {
                requested: input.task.context.output_reserve_tokens,
                allowed: output_budget,
            });
        }

        let required_tools = u32::try_from(input.task.required_tools.len()).unwrap_or(u32::MAX);
        if required_tools > self.max_tool_schemas {
            return Err(UsagePlanError::RequiredToolSetExceedsBudget {
                requested: required_tools,
                allowed: self.max_tool_schemas,
            });
        }
        let required_skills = u32::try_from(input.task.required_skills.len()).unwrap_or(u32::MAX);
        if required_skills > self.max_skills {
            return Err(UsagePlanError::RequiredSkillSetExceedsBudget {
                requested: required_skills,
                allowed: self.max_skills,
            });
        }

        let reducible_budget = fresh_input_budget.saturating_sub(mandatory_input_tokens);
        let planned_reducible = input
            .task
            .context
            .reducible_input_tokens
            .min(reducible_budget);
        let planned_context = ContextDemand {
            mandatory_input_tokens: input.task.context.mandatory_input_tokens,
            reducible_input_tokens: planned_reducible,
            output_reserve_tokens: input.task.context.output_reserve_tokens,
            required_capabilities: input.task.context.required_capabilities.clone(),
        };
        let routing_context = ContextDemand {
            mandatory_input_tokens,
            reducible_input_tokens: planned_reducible,
            output_reserve_tokens: planned_context.output_reserve_tokens,
            required_capabilities: planned_context.required_capabilities.clone(),
        };

        let max_attempts = self.max_retries.saturating_add(1);
        let reserved_attempts = max_attempts.min(input.remaining.attempts);

        Ok(StepPlan {
            policy_revision: self.revision.clone(),
            routing: RoutingRequirements {
                context: routing_context,
                required_capabilities: input.task.required_capabilities.clone(),
                require_known_capacity: self.require_known_capacity,
            },
            context: planned_context.clone(),
            reasoning: input
                .task
                .requested_reasoning
                .clone()
                .map(|level| ReasoningBudget::Requested { level })
                .unwrap_or(ReasoningBudget::BackendDefault),
            tools: ToolProvisionBudget {
                initial: input.task.required_tools.clone(),
                expandable: input.task.optional_tools.clone(),
                max_schemas: self.max_tool_schemas,
                max_result_bytes: self.max_tool_result_bytes,
            },
            skills: SkillProvisionBudget {
                initial: input.task.required_skills.clone(),
                expandable: input.task.optional_skills.clone(),
                max_loaded: self.max_skills,
            },
            delegation: self.delegation.clone(),
            retry: RetryBudget {
                max_attempts,
                reserved_attempts,
            },
            reservation: BudgetReservation {
                input_tokens: input
                    .task
                    .request_input_tokens
                    .saturating_add(planned_context.total_input_tokens()),
                output_tokens: planned_context.output_reserve_tokens,
                cost_microunits: cost_budget,
            },
            deadline_at_ms: input.task.deadline_at_ms,
            reducible_input_dropped_tokens: input
                .task
                .context
                .reducible_input_tokens
                .saturating_sub(planned_reducible),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> UsagePolicy {
        UsagePolicy {
            revision: "policy-1".to_owned(),
            max_fresh_input_tokens: 1_000,
            max_output_tokens: 250,
            max_cost_microunits: Some(5_000),
            max_retries: 1,
            max_tool_result_bytes: 32 * 1024,
            max_tool_schemas: 8,
            max_skills: 4,
            require_known_capacity: true,
            delegation: DelegationResourcePolicy::default(),
        }
    }

    fn input(context: ContextDemand) -> UsagePlanningInput {
        UsagePlanningInput {
            task: TaskRequirements {
                request_input_tokens: 0,
                context,
                required_capabilities: BTreeSet::new(),
                required_tools: BTreeSet::new(),
                optional_tools: BTreeSet::new(),
                required_skills: BTreeSet::new(),
                optional_skills: BTreeSet::new(),
                requested_reasoning: None,
                deadline_at_ms: Some(10_000),
            },
            execution_state: ExecutionState::Active,
            remaining: RemainingBudget {
                fresh_input_tokens: 2_000,
                output_tokens: 500,
                cost_microunits: Some(10_000),
                attempts: 3,
            },
            now_ms: 1_000,
            historical_estimates: Vec::new(),
        }
    }

    #[test]
    fn planning_preserves_mandatory_context_and_trims_only_reducible_context() {
        let request = input(ContextDemand {
            mandatory_input_tokens: 800,
            reducible_input_tokens: 500,
            output_reserve_tokens: 200,
            required_capabilities: BTreeSet::new(),
        });
        let plan = policy().plan(&request).unwrap();

        assert_eq!(plan.context.mandatory_input_tokens, 800);
        assert_eq!(plan.context.reducible_input_tokens, 200);
        assert_eq!(plan.reducible_input_dropped_tokens, 300);
        assert_eq!(plan.reservation.input_tokens, 1_000);
        assert_eq!(plan.reservation.output_tokens, 200);
        assert_eq!(plan.retry.reserved_attempts, 2);
    }

    #[test]
    fn planning_fails_instead_of_dropping_mandatory_context() {
        let request = input(ContextDemand {
            mandatory_input_tokens: 1_001,
            reducible_input_tokens: 0,
            output_reserve_tokens: 100,
            required_capabilities: BTreeSet::new(),
        });

        assert_eq!(
            policy().plan(&request),
            Err(UsagePlanError::MandatoryInputExceedsBudget {
                requested: 1_001,
                allowed: 1_000,
            })
        );
    }

    #[test]
    fn request_input_consumes_budget_without_expanding_context_admission_budget() {
        let mut request = input(ContextDemand {
            mandatory_input_tokens: 200,
            reducible_input_tokens: 600,
            output_reserve_tokens: 100,
            required_capabilities: BTreeSet::new(),
        });
        request.task.request_input_tokens = 300;

        let plan = policy().plan(&request).unwrap();

        assert_eq!(plan.context.mandatory_input_tokens, 200);
        assert_eq!(plan.context.reducible_input_tokens, 500);
        assert_eq!(plan.context.total_input_tokens(), 700);
        assert_eq!(plan.routing.context.mandatory_input_tokens, 500);
        assert_eq!(plan.routing.context.total_input_tokens(), 1_000);
        assert_eq!(plan.reservation.input_tokens, 1_000);
        assert_eq!(plan.reducible_input_dropped_tokens, 100);
    }

    #[test]
    fn request_input_and_mandatory_context_share_the_same_hard_floor() {
        let mut request = input(ContextDemand {
            mandatory_input_tokens: 701,
            reducible_input_tokens: 0,
            output_reserve_tokens: 100,
            required_capabilities: BTreeSet::new(),
        });
        request.task.request_input_tokens = 300;

        assert_eq!(
            policy().plan(&request),
            Err(UsagePlanError::MandatoryInputExceedsBudget {
                requested: 1_001,
                allowed: 1_000,
            })
        );
    }

    #[test]
    fn historical_estimates_are_derived_inputs_not_hard_constraint_overrides() {
        let request = input(ContextDemand {
            mandatory_input_tokens: 800,
            reducible_input_tokens: 500,
            output_reserve_tokens: 200,
            required_capabilities: BTreeSet::new(),
        });
        let baseline = policy().plan(&request).unwrap();

        let mut with_history = request;
        with_history.historical_estimates = vec![RoutingEstimate {
            source: super::super::RoutingEstimateSource::Historical,
            expected_quality_millis: Some(1_000),
            expected_latency_ms: Some(1),
            expected_cost_microunits: Some(1),
            confidence_millis: Some(1_000),
            estimator_snapshot_revision: Some("routing-evidence/7".into()),
            evidence_cutoff_sequence: Some(7),
        }];
        let planned = policy().plan(&with_history).unwrap();

        assert_eq!(planned, baseline);
        assert_eq!(planned.context.mandatory_input_tokens, 800);
        assert_eq!(planned.reservation.input_tokens, 1_000);
    }

    #[test]
    fn root_budget_clamps_policy_budget() {
        let mut request = input(ContextDemand {
            mandatory_input_tokens: 400,
            reducible_input_tokens: 400,
            output_reserve_tokens: 100,
            required_capabilities: BTreeSet::new(),
        });
        request.remaining.fresh_input_tokens = 600;
        request.remaining.attempts = 1;
        let plan = policy().plan(&request).unwrap();
        assert_eq!(plan.context.reducible_input_tokens, 200);
        assert_eq!(plan.retry.reserved_attempts, 1);
    }
}
