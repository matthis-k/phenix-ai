use super::{
    BudgetReservation, CachePlacement, ContextAdmissionRequest, ContextCandidate, ContextRetention,
    ContextSource, ExactContextReference, ExecutionAuthority, ModelTurnUsage, RouteDecision,
    StepPlan,
};
use phenix_core::{ArtifactRevision, Bytes, ContextResourceId};
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
    pub contract: Bytes,
    pub parent_policy_revision: String,
    #[serde(default)]
    pub parent_plan: Option<StepPlan>,
    #[serde(default)]
    pub originating_attempt_id: Option<String>,
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
#[serde(deny_unknown_fields)]
pub struct DelegatedResultContextEnvelope {
    pub task_id: String,
    #[serde(default)]
    pub findings: Vec<DelegatedFinding>,
    #[serde(default)]
    pub evidence: Vec<ExactContextReference>,
    pub escalation: Option<DelegationEscalation>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct DelegatedResultContextDraft {
    pub resource_id: ContextResourceId,
    pub source: String,
    pub content: Bytes,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum DelegationAdmissionError {
    Disabled,
    DepthExceeded { requested: u32, allowed: u32 },
    AttemptLimitExceeded { requested: u32, allowed: u32 },
    ResultLimitExceeded { requested: u64, allowed: u64 },
    DeadlineExceeded { deadline_at_ms: u64, now_ms: u64 },
    MissingParentPlan,
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

    pub fn context_draft(
        &self,
        task_id: &str,
        binding: &DelegationTaskBinding,
    ) -> Result<DelegatedResultContextDraft, DelegationAdmissionError> {
        self.validate_against(binding)?;
        let envelope = DelegatedResultContextEnvelope {
            task_id: task_id.to_owned(),
            findings: self.findings.clone(),
            evidence: self.evidence.clone(),
            escalation: self.escalation.clone(),
        };
        let content = serde_json::to_vec(&envelope)
            .expect("delegated result context envelope has deterministic JSON encoding");
        let task_identity = ArtifactRevision::from_content(task_id.as_bytes());
        let resource_id = ContextResourceId::parse(format!("delegated-result:{task_identity}"))
            .expect("artifact revisions form valid context resource identities");
        Ok(DelegatedResultContextDraft {
            resource_id,
            source: format!("delegation-result:{task_id}"),
            content: content.into(),
        })
    }

    pub fn context_admission(
        &self,
        task_id: &str,
        binding: &DelegationTaskBinding,
        execution_id: impl Into<String>,
        cache_epoch: u64,
    ) -> Result<ContextAdmissionRequest, DelegationAdmissionError> {
        let draft = self.context_draft(task_id, binding)?;
        let step_plan = binding
            .parent_plan
            .clone()
            .ok_or(DelegationAdmissionError::MissingParentPlan)?;
        let content_identity = ArtifactRevision::from_content(draft.content.as_slice()).to_string();
        let estimated_tokens = u64::try_from(draft.content.as_slice().len()).unwrap_or(u64::MAX);
        Ok(ContextAdmissionRequest {
            execution_id: execution_id.into(),
            step_plan,
            candidates: vec![ContextCandidate {
                id: draft.resource_id.as_str().to_owned(),
                source: ContextSource::Delegation {
                    task_id: task_id.to_owned(),
                },
                content_identity,
                content: draft.content,
                estimated_tokens,
                mandatory: true,
                retention: ContextRetention::Full,
                cache: CachePlacement::Epoch,
                recovery: None,
            }],
            cache_epoch,
        })
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

    fn step_plan() -> StepPlan {
        let context = crate::contracts::ContextDemand {
            mandatory_input_tokens: 64 * 1024,
            reducible_input_tokens: 0,
            output_reserve_tokens: 128,
            required_capabilities: Default::default(),
        };
        StepPlan {
            policy_revision: "policy-1".into(),
            historical_estimator_snapshot: None,
            routing: crate::contracts::RoutingRequirements {
                context: context.clone(),
                required_capabilities: Default::default(),
                require_known_capacity: false,
            },
            context,
            reasoning: crate::contracts::ReasoningBudget::BackendDefault,
            tools: crate::contracts::ToolProvisionBudget {
                initial: Default::default(),
                expandable: Default::default(),
                max_schemas: 0,
                max_result_bytes: 0,
            },
            skills: crate::contracts::SkillProvisionBudget {
                initial: Default::default(),
                expandable: Default::default(),
                max_loaded: 0,
            },
            delegation: DelegationResourcePolicy::default(),
            retry: crate::contracts::RetryBudget {
                max_attempts: 1,
                reserved_attempts: 1,
            },
            reservation: BudgetReservation {
                input_tokens: 64 * 1024,
                output_tokens: 128,
                cost_microunits: None,
            },
            deadline_at_ms: None,
            reducible_input_dropped_tokens: 0,
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
            contract: b"contract-1".to_vec().into(),
            parent_policy_revision: "policy-1".into(),
            parent_plan: Some(step_plan()),
            originating_attempt_id: None,
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
            contract: b"contract-1".to_vec().into(),
            parent_policy_revision: "policy-1".into(),
            parent_plan: Some(step_plan()),
            originating_attempt_id: None,
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
    fn bounded_result_projects_to_deterministic_external_context_draft() {
        use phenix_core::{ContextResourceId, ContextRevisionId};

        let binding = DelegationTaskBinding {
            contract_revision: ArtifactRevision::from_content(b"contract-1"),
            contract: b"contract-1".to_vec().into(),
            parent_policy_revision: "policy-1".into(),
            parent_plan: Some(step_plan()),
            originating_attempt_id: Some("attempt-1".into()),
            resources: resources(),
        };
        let evidence = ExactContextReference {
            resource_id: ContextResourceId::parse("doc:evidence").unwrap(),
            revision: ContextRevisionId::parse("revision-1").unwrap(),
        };
        let result = DelegatedWorkerResult {
            findings: vec![DelegatedFinding {
                kind: "fact".into(),
                summary: "bounded finding".into(),
                evidence: vec![evidence.clone()],
            }],
            evidence: vec![evidence],
            escalation: None,
            usage: ModelTurnUsage::default(),
            encoded_result_bytes: 0,
        };

        let first = result.context_draft("task with spaces", &binding).unwrap();
        let replay = result.context_draft("task with spaces", &binding).unwrap();
        assert_eq!(first, replay);
        assert!(first
            .resource_id
            .as_str()
            .starts_with("delegated-result:sha256:"));
        assert_eq!(first.source, "delegation-result:task with spaces");
        let envelope: DelegatedResultContextEnvelope =
            serde_json::from_slice(first.content.as_ref()).unwrap();
        assert_eq!(envelope.task_id, "task with spaces");
        assert_eq!(envelope.findings[0].kind, "fact");
        assert_eq!(envelope.findings[0].evidence.len(), 1);
    }

    #[test]
    fn bounded_result_projects_to_ordinary_context_admission() {
        let binding = DelegationTaskBinding {
            contract_revision: ArtifactRevision::from_content(b"contract-1"),
            contract: b"contract-1".to_vec().into(),
            parent_policy_revision: "policy-1".into(),
            parent_plan: Some(step_plan()),
            originating_attempt_id: Some("attempt-1".into()),
            resources: resources(),
        };
        let result = DelegatedWorkerResult {
            findings: vec![DelegatedFinding {
                kind: "fact".into(),
                summary: "bounded finding".into(),
                evidence: Vec::new(),
            }],
            evidence: Vec::new(),
            escalation: None,
            usage: ModelTurnUsage::default(),
            encoded_result_bytes: 0,
        };
        let request = result
            .context_admission("task-1", &binding, "parent-execution", 7)
            .unwrap();

        assert_eq!(request.execution_id, "parent-execution");
        assert_eq!(request.cache_epoch, 7);
        assert_eq!(request.candidates.len(), 1);
        assert!(request.candidates[0].mandatory);
        assert_eq!(
            request.candidates[0].source,
            ContextSource::Delegation {
                task_id: "task-1".into()
            }
        );
        let admitted = request.admit().unwrap();
        assert_eq!(admitted.admitted.len(), 1);
        assert_eq!(
            admitted.admitted[0].form,
            crate::contracts::ContextProjectionForm::Full
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
