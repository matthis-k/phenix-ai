use super::{
    BudgetReservation, ExactContextReference, ExecutionAuthority, ModelTurnUsage, RouteDecision,
};
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(deny_unknown_fields)]
pub struct DelegationResourcePolicy {
    pub enabled: bool,
    pub max_depth: u32,
    pub max_children: u32,
    pub max_attempts: u32,
    pub max_result_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct DelegatedWorkResources {
    pub target: RouteDecision,
    pub authority: ExecutionAuthority,
    #[serde(default)]
    pub context: Vec<ExactContextReference>,
    pub budget: BudgetReservation,
    pub deadline_at_ms: u64,
    pub depth: u32,
    pub attempts: u32,
    pub max_result_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct DelegationTaskBinding {
    pub contract_fingerprint: String,
    pub parent_policy_revision: String,
    pub resources: DelegatedWorkResources,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct DelegatedFinding {
    pub kind: String,
    pub summary: String,
    #[serde(default)]
    pub evidence: Vec<ExactContextReference>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct DelegationEscalation {
    pub scope: String,
    pub reason: String,
    #[serde(default)]
    pub evidence: Vec<ExactContextReference>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct DelegatedWorkerResult {
    #[serde(default)]
    pub findings: Vec<DelegatedFinding>,
    #[serde(default)]
    pub evidence: Vec<ExactContextReference>,
    pub escalation: Option<DelegationEscalation>,
    pub usage: ModelTurnUsage,
    pub encoded_result_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum DelegationAdmissionError {
    Disabled,
    DepthExceeded { requested: u32, allowed: u32 },
    AttemptLimitExceeded { requested: u32, allowed: u32 },
    ResultLimitExceeded { requested: u64, allowed: u64 },
    DeadlineExceeded { deadline_at_ms: u64, now_ms: u64 },
}

impl DelegatedWorkResources {
    pub fn validate_policy(
        &self,
        policy: &DelegationResourcePolicy,
    ) -> Result<(), DelegationAdmissionError> {
        if !policy.enabled {
            return Err(DelegationAdmissionError::Disabled);
        }
        if self.depth > policy.max_depth {
            return Err(DelegationAdmissionError::DepthExceeded {
                requested: self.depth,
                allowed: policy.max_depth,
            });
        }
        if self.attempts > policy.max_attempts {
            return Err(DelegationAdmissionError::AttemptLimitExceeded {
                requested: self.attempts,
                allowed: policy.max_attempts,
            });
        }
        if self.max_result_bytes > policy.max_result_bytes {
            return Err(DelegationAdmissionError::ResultLimitExceeded {
                requested: self.max_result_bytes,
                allowed: policy.max_result_bytes,
            });
        }
        Ok(())
    }

    pub fn validate_deadline(&self, now_ms: u64) -> Result<(), DelegationAdmissionError> {
        if now_ms >= self.deadline_at_ms {
            Err(DelegationAdmissionError::DeadlineExceeded {
                deadline_at_ms: self.deadline_at_ms,
                now_ms,
            })
        } else {
            Ok(())
        }
    }
}

impl DelegatedWorkerResult {
    pub fn validate_against(
        &self,
        binding: &DelegationTaskBinding,
    ) -> Result<(), DelegationAdmissionError> {
        if self.encoded_result_bytes > binding.resources.max_result_bytes {
            Err(DelegationAdmissionError::ResultLimitExceeded {
                requested: self.encoded_result_bytes,
                allowed: binding.resources.max_result_bytes,
            })
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{ModelTarget, RoutingEstimate};
    use phenix_core::{CapabilityGenerationId, ModelId, PluginId};
    use std::collections::BTreeMap;

    fn resources() -> DelegatedWorkResources {
        DelegatedWorkResources {
            target: RouteDecision {
                target: ModelTarget {
                    provider_plugin: PluginId::parse("provider.fixture").unwrap(),
                    model: ModelId::parse("model.fixture").unwrap(),
                    options: BTreeMap::new(),
                },
                capability_generation: CapabilityGenerationId::parse("generation-1").unwrap(),
                policy_revision: "policy-1".to_owned(),
                candidate_ordinal: 0,
                estimate: None::<RoutingEstimate>,
            },
            authority: ExecutionAuthority::new(Vec::<String>::new()),
            context: Vec::new(),
            budget: BudgetReservation {
                input_tokens: 1_000,
                output_tokens: 250,
                cost_microunits: None,
            },
            deadline_at_ms: 1_000,
            depth: 1,
            attempts: 1,
            max_result_bytes: 64 * 1024,
        }
    }

    #[test]
    fn delegation_is_disabled_by_default() {
        assert_eq!(
            resources().validate_policy(&DelegationResourcePolicy::default()),
            Err(DelegationAdmissionError::Disabled)
        );
    }

    #[test]
    fn admission_rejects_resource_expansion_before_worker_start() {
        let policy = DelegationResourcePolicy {
            enabled: true,
            max_depth: 1,
            max_children: 2,
            max_attempts: 1,
            max_result_bytes: 32 * 1024,
        };
        assert_eq!(
            resources().validate_policy(&policy),
            Err(DelegationAdmissionError::ResultLimitExceeded {
                requested: 64 * 1024,
                allowed: 32 * 1024,
            })
        );
    }
}
