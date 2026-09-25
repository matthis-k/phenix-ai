use super::{
    BudgetReservation, ExactContextReference, ExecutionAuthority, ModelTurnUsage, RouteDecision,
};
use phenix_core::ArtifactRevision;
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
    pub contract_revision: ArtifactRevision,
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
        let encoded = serde_json::to_vec(&phenix_core::PhenixValue::from(self))
            .expect("delegated worker result has a deterministic PhenixValue encoding");
        let actual_encoded_bytes = u64::try_from(encoded.len()).unwrap_or(u64::MAX);
        let requested = self.encoded_result_bytes.max(actual_encoded_bytes);
        if requested > binding.resources.max_result_bytes {
            Err(DelegationAdmissionError::ResultLimitExceeded {
                requested,
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
    fn result_validation_uses_actual_encoded_size_not_only_reported_size() {
        let mut resources = resources();
        resources.max_result_bytes = 128;
        let binding = DelegationTaskBinding {
            contract_revision: ArtifactRevision::from_content(b"contract-1"),
            parent_policy_revision: "policy-1".into(),
            resources,
        };
        let result = DelegatedWorkerResult {
            findings: vec![DelegatedFinding {
                kind: "summary".into(),
                summary: "x".repeat(512),
                evidence: Vec::new(),
            }],
            evidence: Vec::new(),
            escalation: None,
            usage: ModelTurnUsage::default(),
            encoded_result_bytes: 1,
        };

        assert!(matches!(
            result.validate_against(&binding),
            Err(DelegationAdmissionError::ResultLimitExceeded {
                requested,
                allowed: 128
            }) if requested > 128
        ));
    }

    #[test]
    fn bounded_typed_findings_preserve_exact_evidence_references() {
        use phenix_core::{ContextResourceId, ContextRevisionId};

        let binding = DelegationTaskBinding {
            contract_revision: ArtifactRevision::from_content(b"contract-1"),
            parent_policy_revision: "policy-1".into(),
            resources: resources(),
        };
        let evidence = ExactContextReference {
            resource_id: ContextResourceId::parse("doc:evidence").unwrap(),
            revision: ContextRevisionId::parse("revision-1").unwrap(),
        };
        let result = DelegatedWorkerResult {
            findings: vec![DelegatedFinding {
                kind: "summary".into(),
                summary: "bounded finding".into(),
                evidence: vec![evidence.clone()],
            }],
            evidence: vec![evidence.clone()],
            escalation: None,
            usage: ModelTurnUsage::default(),
            encoded_result_bytes: 0,
        };

        result.validate_against(&binding).unwrap();
        assert_eq!(result.findings[0].kind, "summary");
        assert_eq!(result.findings[0].evidence, vec![evidence.clone()]);
        assert_eq!(result.evidence, vec![evidence]);
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
