use super::{ExactContextReference, StepPlan};
use phenix_core::Bytes;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(
    Clone,
    Copy,
    Debug,
    Eq,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
    phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum ContextRetention {
    Pinned,
    Full,
    Compact,
    Reference,
    DropAllowed,
}

#[derive(
    Clone,
    Copy,
    Debug,
    Eq,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
    phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum CachePlacement {
    StablePrefix,
    Epoch,
    Volatile,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextSource {
    Exact { reference: ExactContextReference },
    Memory { memory_id: String },
    Tool { call_id: String },
    Skill { skill_id: String },
    Delegation { task_id: String },
    Inline { identity: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContextCandidate {
    pub id: String,
    pub source: ContextSource,
    pub content_identity: String,
    pub content: Bytes,
    pub estimated_tokens: u64,
    pub mandatory: bool,
    pub retention: ContextRetention,
    pub cache: CachePlacement,
    pub recovery: Option<ExactContextReference>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(rename_all = "snake_case")]
pub enum ContextProjectionForm {
    Full,
    Reference,
    Omitted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct AdmittedContextItem {
    pub id: String,
    pub source: ContextSource,
    pub content_identity: String,
    pub form: ContextProjectionForm,
    pub cache: CachePlacement,
    pub retention: ContextRetention,
    pub estimated_tokens: u64,
    pub recovery: Option<ExactContextReference>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContextAdmissionRequest {
    pub execution_id: String,
    pub step_plan: StepPlan,
    pub candidates: Vec<ContextCandidate>,
    pub cache_epoch: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContextAdmissionResult {
    pub execution_id: String,
    pub policy_revision: String,
    pub cache_epoch: u64,
    pub admitted: Vec<AdmittedContextItem>,
    pub used_input_tokens: u64,
    pub omitted_input_tokens: u64,
    pub deduplicated_items: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextAdmissionError {
    MandatoryContextExhausted { required: u64, budget: u64 },
    LossyProjectionWithoutRecovery { id: String },
}

impl ContextAdmissionRequest {
    pub fn admit(mut self) -> Result<ContextAdmissionResult, ContextAdmissionError> {
        self.candidates.sort_by(|left, right| {
            right
                .mandatory
                .cmp(&left.mandatory)
                .then_with(|| left.cache.cmp(&right.cache))
                .then_with(|| left.retention.cmp(&right.retention))
                .then_with(|| left.id.cmp(&right.id))
        });

        let budget = self.step_plan.context.total_input_tokens();
        let mandatory_tokens = self
            .candidates
            .iter()
            .filter(|candidate| candidate.mandatory)
            .fold(0_u64, |total, candidate| {
                total.saturating_add(candidate.estimated_tokens)
            });
        if mandatory_tokens > budget {
            return Err(ContextAdmissionError::MandatoryContextExhausted {
                required: mandatory_tokens,
                budget,
            });
        }

        let mut identities = BTreeSet::new();
        let mut used = 0_u64;
        let mut omitted = 0_u64;
        let mut deduplicated = 0_u32;
        let mut admitted = Vec::with_capacity(self.candidates.len());

        for candidate in self.candidates {
            if !identities.insert(candidate.content_identity.clone()) {
                deduplicated = deduplicated.saturating_add(1);
                continue;
            }

            let fits = used.saturating_add(candidate.estimated_tokens) <= budget;
            let form = if candidate.mandatory || fits {
                used = used.saturating_add(candidate.estimated_tokens);
                ContextProjectionForm::Full
            } else if candidate.recovery.is_some() {
                omitted = omitted.saturating_add(candidate.estimated_tokens);
                ContextProjectionForm::Reference
            } else if candidate.retention == ContextRetention::DropAllowed {
                omitted = omitted.saturating_add(candidate.estimated_tokens);
                ContextProjectionForm::Omitted
            } else {
                return Err(ContextAdmissionError::LossyProjectionWithoutRecovery {
                    id: candidate.id,
                });
            };

            admitted.push(AdmittedContextItem {
                id: candidate.id,
                source: candidate.source,
                content_identity: candidate.content_identity,
                form,
                cache: candidate.cache,
                retention: candidate.retention,
                estimated_tokens: candidate.estimated_tokens,
                recovery: candidate.recovery,
            });
        }

        Ok(ContextAdmissionResult {
            execution_id: self.execution_id,
            policy_revision: self.step_plan.policy_revision,
            cache_epoch: self.cache_epoch,
            admitted,
            used_input_tokens: used,
            omitted_input_tokens: omitted,
            deduplicated_items: deduplicated,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{
        ContextDemand, DelegationResourcePolicy, ReasoningBudget, RetryBudget, RoutingRequirements,
        SkillProvisionBudget, ToolProvisionBudget,
    };
    use std::collections::BTreeSet;

    fn plan(input_tokens: u64) -> StepPlan {
        let context = ContextDemand {
            mandatory_input_tokens: input_tokens,
            reducible_input_tokens: 0,
            output_reserve_tokens: 128,
            required_capabilities: BTreeSet::new(),
        };
        StepPlan {
            policy_revision: "p1".into(),
            routing: RoutingRequirements {
                context: context.clone(),
                required_capabilities: BTreeSet::new(),
                require_known_capacity: false,
            },
            context,
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
            delegation: DelegationResourcePolicy::default(),
            retry: RetryBudget {
                max_attempts: 1,
                reserved_attempts: 1,
            },
            reservation: crate::contracts::BudgetReservation {
                input_tokens,
                output_tokens: 128,
                cost_microunits: None,
            },
            deadline_at_ms: None,
            reducible_input_dropped_tokens: 0,
        }
    }

    fn candidate(id: &str, tokens: u64, mandatory: bool) -> ContextCandidate {
        ContextCandidate {
            id: id.into(),
            source: ContextSource::Inline {
                identity: id.into(),
            },
            content_identity: id.into(),
            content: Bytes::from(id.as_bytes().to_vec()),
            estimated_tokens: tokens,
            mandatory,
            retention: ContextRetention::DropAllowed,
            cache: CachePlacement::Epoch,
            recovery: None,
        }
    }

    #[test]
    fn mandatory_items_are_admitted_before_reducible_items() {
        let result = ContextAdmissionRequest {
            execution_id: "e1".into(),
            step_plan: plan(100),
            candidates: vec![
                candidate("optional", 80, false),
                candidate("required", 60, true),
            ],
            cache_epoch: 1,
        }
        .admit()
        .unwrap();

        assert_eq!(result.used_input_tokens, 60);
        assert_eq!(result.admitted[0].id, "required");
        assert_eq!(result.admitted[1].form, ContextProjectionForm::Omitted);
    }

    #[test]
    fn exact_duplicates_are_counted_once() {
        let first = candidate("same", 20, true);
        let mut second = candidate("alias", 20, true);
        second.content_identity = first.content_identity.clone();
        let result = ContextAdmissionRequest {
            execution_id: "e1".into(),
            step_plan: plan(40),
            candidates: vec![first, second],
            cache_epoch: 1,
        }
        .admit()
        .unwrap();
        assert_eq!(result.admitted.len(), 1);
        assert_eq!(result.deduplicated_items, 1);
    }
}
