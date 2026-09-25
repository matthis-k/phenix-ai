use super::{
    BudgetReservation, BudgetReservationPurpose, BudgetReservationRequest, DelegatedWorkResources,
    DelegationResourcePolicy, DelegationTaskBinding, ExactContextReference, ExecutionAuthority,
    ExecutionResourceCommand, RouteDecision, StepPlan, WorkerTaskRecord, WorkerTaskState,
};
use phenix_core::ArtifactRevision;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

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
    pub inline_parent_reacquisition_tokens: u64,
    pub delegated_parent_reacquisition_tokens: u64,
    pub child_input_tokens: u64,
    pub child_output_tokens: u64,
    pub child_cost_microunits: Option<u64>,
    pub expected_result_input_tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ExplorationDelegationInput {
    pub root_execution_id: String,
    pub parent_execution: String,
    pub graph_generation: String,
    pub parent_reservation_id: Option<String>,
    pub parent_policy_revision: String,
    pub contract_revision: ArtifactRevision,
    pub target: RouteDecision,
    pub parent_authority: ExecutionAuthority,
    pub delegated_authority: ExecutionAuthority,
    #[serde(default)]
    pub context: Vec<ExactContextReference>,
    pub deadline_at_ms: u64,
    pub depth: u32,
    pub attempts: u32,
    pub existing_children: u32,
    #[serde(default)]
    pub depends_on: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ExplorationDelegationAdmission {
    pub root_execution_id: String,
    pub reservation: BudgetReservationRequest,
    pub task: WorkerTaskRecord,
    pub binding: DelegationTaskBinding,
    pub parent_authority: ExecutionAuthority,
    pub policy: DelegationResourcePolicy,
}

impl ExplorationDelegationAdmission {
    #[must_use]
    pub fn into_resource_command(self, now_ms: u64) -> ExecutionResourceCommand {
        ExecutionResourceCommand::AdmitDelegated {
            root_execution_id: self.root_execution_id,
            reservation: self.reservation,
            task: self.task,
            binding: self.binding,
            parent_authority: self.parent_authority,
            policy: self.policy,
            now_ms,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExplorationPreparationError {
    NotDelegated,
    TaskMismatch { expected: String, observed: String },
    AuthorityExpanded,
    PolicyRevisionMismatch { expected: String, observed: String },
    DelegationDisabled,
    ChildLimitReached { current: u32, allowed: u32 },
    ZeroAttempts,
    DepthUnavailable,
    DeadlineNotFuture { deadline_at_ms: u64, now_ms: u64 },
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
    UnknownChildCostUnderFiniteBudget { allowed: u64 },
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
        match (
            opportunity.child_cost_microunits,
            self.max_child_cost_microunits,
        ) {
            (Some(requested), Some(allowed)) if requested > allowed => {
                return ExplorationDecision::KeepInParent {
                    reason: ExplorationRejection::ChildCostBudgetExceeded { requested, allowed },
                };
            }
            (None, Some(allowed)) => {
                return ExplorationDecision::KeepInParent {
                    reason: ExplorationRejection::UnknownChildCostUnderFiniteBudget { allowed },
                };
            }
            _ => {}
        }

        let inline_total = opportunity
            .parent_input_tokens_if_inline
            .saturating_add(opportunity.inline_parent_reacquisition_tokens);
        let delegated_parent_cost = opportunity
            .expected_result_input_tokens
            .saturating_add(opportunity.delegated_parent_reacquisition_tokens);
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

pub fn prepare_exploration_delegation(
    opportunity: &ExplorationOpportunity,
    decision: &ExplorationDecision,
    input: ExplorationDelegationInput,
    plan: &StepPlan,
    now_ms: u64,
) -> Result<ExplorationDelegationAdmission, ExplorationPreparationError> {
    let ExplorationDecision::Delegate {
        task_id,
        reservation,
        max_result_bytes,
        ..
    } = decision
    else {
        return Err(ExplorationPreparationError::NotDelegated);
    };
    if task_id != &opportunity.task_id {
        return Err(ExplorationPreparationError::TaskMismatch {
            expected: opportunity.task_id.clone(),
            observed: task_id.clone(),
        });
    }
    if plan.policy_revision != input.parent_policy_revision {
        return Err(ExplorationPreparationError::PolicyRevisionMismatch {
            expected: plan.policy_revision.clone(),
            observed: input.parent_policy_revision.clone(),
        });
    }
    if !plan.delegation.enabled {
        return Err(ExplorationPreparationError::DelegationDisabled);
    }
    if input.existing_children >= plan.delegation.max_children {
        return Err(ExplorationPreparationError::ChildLimitReached {
            current: input.existing_children,
            allowed: plan.delegation.max_children,
        });
    }
    let attempts = input.attempts.min(plan.delegation.max_attempts);
    if attempts == 0 {
        return Err(ExplorationPreparationError::ZeroAttempts);
    }
    let depth = input.depth.min(plan.delegation.max_depth);
    if depth == 0 && input.depth > 0 {
        return Err(ExplorationPreparationError::DepthUnavailable);
    }
    let deadline_at_ms = plan
        .deadline_at_ms
        .map_or(input.deadline_at_ms, |deadline| {
            deadline.min(input.deadline_at_ms)
        });
    if deadline_at_ms <= now_ms {
        return Err(ExplorationPreparationError::DeadlineNotFuture {
            deadline_at_ms,
            now_ms,
        });
    }
    let max_result_bytes = (*max_result_bytes).min(plan.delegation.max_result_bytes);
    if !input
        .delegated_authority
        .capabilities
        .is_subset(&input.parent_authority.capabilities)
    {
        return Err(ExplorationPreparationError::AuthorityExpanded);
    }

    let binding = DelegationTaskBinding {
        contract_revision: input.contract_revision,
        parent_policy_revision: input.parent_policy_revision.clone(),
        resources: DelegatedWorkResources {
            target: input.target,
            authority: input.delegated_authority.clone(),
            context: input.context,
            budget: reservation.clone(),
            deadline_at_ms,
            depth,
            attempts,
            max_result_bytes,
        },
    };
    Ok(ExplorationDelegationAdmission {
        root_execution_id: input.root_execution_id,
        reservation: BudgetReservationRequest {
            reservation_id: format!("exploration/{task_id}"),
            parent_reservation_id: input.parent_reservation_id,
            policy_revision: plan.policy_revision.clone(),
            purpose: BudgetReservationPurpose::Delegation,
            budget: reservation.clone(),
            attempts,
        },
        task: WorkerTaskRecord {
            id: task_id.clone(),
            parent_execution: input.parent_execution,
            graph_generation: input.graph_generation,
            description: opportunity.description.clone(),
            depends_on: input.depends_on,
            delegated_authority: input.delegated_authority,
            state: WorkerTaskState::Pending,
        },
        binding,
        parent_authority: input.parent_authority,
        policy: plan.delegation.clone(),
    })
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
            inline_parent_reacquisition_tokens: 500,
            delegated_parent_reacquisition_tokens: 100,
            child_input_tokens: 1_000,
            child_output_tokens: 500,
            child_cost_microunits: Some(100),
            expected_result_input_tokens: 800,
        }
    }

    fn delegation_input() -> ExplorationDelegationInput {
        use crate::contracts::{ModelTarget, RoutingEstimate};
        use phenix_core::{CapabilityGenerationId, ModelId, PluginId};
        use std::collections::BTreeMap;

        ExplorationDelegationInput {
            root_execution_id: "root".into(),
            parent_execution: "parent-execution".into(),
            graph_generation: "generation-1".into(),
            parent_reservation_id: Some("attempt/root".into()),
            parent_policy_revision: "policy-1".into(),
            contract_revision: ArtifactRevision::from_content(b"exploration contract"),
            target: RouteDecision {
                target: ModelTarget {
                    provider_plugin: PluginId::parse("provider.fixture").unwrap(),
                    model: ModelId::parse("model.fixture").unwrap(),
                    options: BTreeMap::new(),
                },
                capability_generation: CapabilityGenerationId::parse("generation-1").unwrap(),
                policy_revision: "routing-1".into(),
                candidate_ordinal: 0,
                estimate: None::<RoutingEstimate>,
            },
            parent_authority: ExecutionAuthority::new(["workspace.read", "workspace.write"]),
            delegated_authority: ExecutionAuthority::new(["workspace.read"]),
            context: Vec::new(),
            deadline_at_ms: 10_000,
            depth: 1,
            attempts: 1,
            existing_children: 0,
            depends_on: BTreeSet::new(),
        }
    }

    fn step_plan() -> StepPlan {
        use crate::contracts::{
            ContextDemand, ReasoningBudget, RetryBudget, RoutingRequirements, SkillProvisionBudget,
            ToolProvisionBudget,
        };

        StepPlan {
            policy_revision: "policy-1".into(),
            routing: RoutingRequirements {
                context: ContextDemand::default(),
                required_capabilities: BTreeSet::new(),
                require_known_capacity: false,
            },
            context: ContextDemand::default(),
            reasoning: ReasoningBudget::BackendDefault,
            tools: ToolProvisionBudget {
                initial: BTreeSet::new(),
                expandable: BTreeSet::new(),
                max_schemas: 0,
                max_result_bytes: 0,
            },
            skills: SkillProvisionBudget {
                initial: BTreeSet::new(),
                expandable: BTreeSet::new(),
                max_loaded: 0,
            },
            delegation: DelegationResourcePolicy {
                enabled: true,
                max_depth: 2,
                max_children: 2,
                max_attempts: 2,
                max_result_bytes: 32 * 1024,
            },
            retry: RetryBudget {
                max_attempts: 1,
                reserved_attempts: 1,
            },
            reservation: BudgetReservation {
                input_tokens: 10_000,
                output_tokens: 2_000,
                cost_microunits: Some(10_000),
            },
            deadline_at_ms: Some(8_000),
            reducible_input_dropped_tokens: 0,
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
    fn finite_cost_policy_rejects_unknown_child_cost_before_admission() {
        let mut opportunity = opportunity();
        opportunity.child_cost_microunits = None;

        assert_eq!(
            policy().assess(&opportunity),
            ExplorationDecision::KeepInParent {
                reason: ExplorationRejection::UnknownChildCostUnderFiniteBudget { allowed: 500 },
            }
        );
    }

    #[test]
    fn delegated_parent_reacquisition_counts_against_expected_savings() {
        let mut opportunity = opportunity();
        opportunity.delegated_parent_reacquisition_tokens = 10_000;

        assert_eq!(
            policy().assess(&opportunity),
            ExplorationDecision::KeepInParent {
                reason: ExplorationRejection::NoExpectedSavings,
            }
        );
    }

    #[test]
    fn accepted_exploration_prepares_one_ordinary_delegation_admission() {
        let opportunity = opportunity();
        let decision = policy().assess(&opportunity);
        let admission = prepare_exploration_delegation(
            &opportunity,
            &decision,
            delegation_input(),
            &step_plan(),
            0,
        )
        .unwrap();

        assert_eq!(admission.task.id, opportunity.task_id);
        assert_eq!(
            admission.reservation.reservation_id,
            "exploration/explore-1"
        );
        assert_eq!(
            admission.reservation.purpose,
            BudgetReservationPurpose::Delegation
        );
        assert_eq!(
            admission.reservation.budget,
            admission.binding.resources.budget
        );
        assert_eq!(
            admission.task.delegated_authority,
            admission.binding.resources.authority
        );
        assert_eq!(
            admission.binding.resources.max_result_bytes,
            step_plan().delegation.max_result_bytes
        );
    }

    #[test]
    fn exploration_handoff_rejects_authority_expansion_before_resource_admission() {
        let opportunity = opportunity();
        let decision = policy().assess(&opportunity);
        let mut input = delegation_input();
        input.delegated_authority = ExecutionAuthority::new(["workspace.read", "network.admin"]);

        assert_eq!(
            prepare_exploration_delegation(&opportunity, &decision, input, &step_plan(), 0),
            Err(ExplorationPreparationError::AuthorityExpanded)
        );
    }

    #[test]
    fn parent_step_plan_clamps_child_deadline_attempts_depth_and_result_size() {
        let opportunity = opportunity();
        let decision = policy().assess(&opportunity);
        let mut input = delegation_input();
        input.deadline_at_ms = 10_000;
        input.attempts = 7;
        input.depth = 7;
        let admission =
            prepare_exploration_delegation(&opportunity, &decision, input, &step_plan(), 0)
                .unwrap();

        assert_eq!(admission.binding.resources.deadline_at_ms, 8_000);
        assert_eq!(admission.binding.resources.attempts, 2);
        assert_eq!(admission.binding.resources.depth, 2);
        assert_eq!(admission.binding.resources.max_result_bytes, 32 * 1024);
        assert_eq!(admission.reservation.attempts, 2);
        assert_eq!(admission.policy, step_plan().delegation);
    }

    #[test]
    fn exhausted_parent_child_limit_prevents_exploration_admission() {
        let opportunity = opportunity();
        let decision = policy().assess(&opportunity);
        let mut input = delegation_input();
        input.existing_children = 2;

        assert_eq!(
            prepare_exploration_delegation(&opportunity, &decision, input, &step_plan(), 0),
            Err(ExplorationPreparationError::ChildLimitReached {
                current: 2,
                allowed: 2,
            })
        );
    }

    #[test]
    fn disabled_parent_delegation_prevents_exploration_admission() {
        let opportunity = opportunity();
        let decision = policy().assess(&opportunity);
        let mut plan = step_plan();
        plan.delegation.enabled = false;

        assert_eq!(
            prepare_exploration_delegation(&opportunity, &decision, delegation_input(), &plan, 0),
            Err(ExplorationPreparationError::DelegationDisabled)
        );
    }

    #[test]
    fn bounded_separable_work_can_delegate() {
        assert!(matches!(
            policy().assess(&opportunity()),
            ExplorationDecision::Delegate { .. }
        ));
    }
}
