use super::{
    ContextAnchor, ContextNeed, ContextRecoveryDecision, ContextRecoveryState, ModelTarget,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryColdGate {
    CurrentContextSufficient,
    NeedsClassification,
}

#[must_use]
pub fn recovery_cold_gate(state: &ContextRecoveryState) -> RecoveryColdGate {
    if state.has_durable_session_history || state.has_explicit_resource {
        return RecoveryColdGate::CurrentContextSufficient;
    }
    if state.anchors.iter().any(|anchor| {
        matches!(
            anchor,
            ContextAnchor::Repository { .. }
                | ContextAnchor::Project { .. }
                | ContextAnchor::Task { .. }
        )
    }) {
        RecoveryColdGate::CurrentContextSufficient
    } else {
        RecoveryColdGate::NeedsClassification
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct RecoveryClassifierPolicy {
    pub max_prompt_bytes: u32,
    pub max_anchors: u32,
    pub max_needs: u32,
    pub max_need_query_bytes: u32,
    pub max_attempts: u32,
    pub max_output_tokens: u64,
    pub classifier_timeout_ms: u64,
    pub total_timeout_ms: u64,
}

impl Default for RecoveryClassifierPolicy {
    fn default() -> Self {
        Self {
            max_prompt_bytes: 4_096,
            max_anchors: 32,
            max_needs: 4,
            max_need_query_bytes: 512,
            max_attempts: 1,
            max_output_tokens: 512,
            classifier_timeout_ms: 10_000,
            total_timeout_ms: 30_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct RecoveryBootstrapRequest {
    pub request_id: String,
    pub prompt: String,
    pub state: ContextRecoveryState,
    pub classifier_target: ModelTarget,
    pub caller_deadline_at_ms: Option<u64>,
    pub now_ms: u64,
    pub policy: RecoveryClassifierPolicy,
}

impl RecoveryBootstrapRequest {
    #[must_use]
    pub fn effective_deadline_at_ms(&self) -> u64 {
        let policy_deadline = self.now_ms.saturating_add(self.policy.total_timeout_ms);
        self.caller_deadline_at_ms
            .map_or(policy_deadline, |deadline| deadline.min(policy_deadline))
    }

    pub fn validate_input(&self) -> Result<(), RecoveryClassificationError> {
        if self.prompt.len() > self.policy.max_prompt_bytes as usize {
            return Err(RecoveryClassificationError::PromptEvidenceTooLarge {
                requested: self.prompt.len() as u64,
                allowed: self.policy.max_prompt_bytes as u64,
            });
        }
        if self.state.anchors.len() > self.policy.max_anchors as usize {
            return Err(RecoveryClassificationError::TooManyAnchors {
                requested: self.state.anchors.len() as u32,
                allowed: self.policy.max_anchors,
            });
        }
        if self.now_ms >= self.effective_deadline_at_ms() {
            return Err(RecoveryClassificationError::DeadlineExceeded);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum RecoveryClassificationError {
    PromptEvidenceTooLarge { requested: u64, allowed: u64 },
    TooManyAnchors { requested: u32, allowed: u32 },
    DeadlineExceeded,
    MissingNeeds,
    TooManyNeeds { requested: u32, allowed: u32 },
    EmptyNeedQuery,
    NeedQueryTooLarge { requested: u64, allowed: u64 },
    DuplicateNeed,
}

pub fn validate_recovery_decision(
    decision: ContextRecoveryDecision,
    policy: &RecoveryClassifierPolicy,
) -> Result<ContextRecoveryDecision, RecoveryClassificationError> {
    let ContextRecoveryDecision::Missing { needs } = &decision else {
        return Ok(decision);
    };
    if needs.is_empty() {
        return Err(RecoveryClassificationError::MissingNeeds);
    }
    if needs.len() > policy.max_needs as usize {
        return Err(RecoveryClassificationError::TooManyNeeds {
            requested: needs.len() as u32,
            allowed: policy.max_needs,
        });
    }

    let mut identities = BTreeSet::new();
    for need in needs {
        let query = need_query(need);
        if query.trim().is_empty() {
            return Err(RecoveryClassificationError::EmptyNeedQuery);
        }
        if query.len() > policy.max_need_query_bytes as usize {
            return Err(RecoveryClassificationError::NeedQueryTooLarge {
                requested: query.len() as u64,
                allowed: policy.max_need_query_bytes as u64,
            });
        }
        let identity = serde_json::to_string(need).expect("ContextNeed is serializable");
        if !identities.insert(identity) {
            return Err(RecoveryClassificationError::DuplicateNeed);
        }
    }
    Ok(decision)
}

fn need_query(need: &ContextNeed) -> &str {
    match need {
        ContextNeed::Workspace { query }
        | ContextNeed::Repository { query }
        | ContextNeed::Project { query }
        | ContextNeed::Task { query }
        | ContextNeed::Session { query }
        | ContextNeed::Resource { query, .. } => query,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_path_does_not_suppress_fallback_recovery() {
        let state = ContextRecoveryState {
            anchors: vec![ContextAnchor::Path {
                path: "/tmp".into(),
            }],
            has_durable_session_history: false,
            has_explicit_resource: false,
        };
        assert_eq!(
            recovery_cold_gate(&state),
            RecoveryColdGate::NeedsClassification
        );
    }

    #[test]
    fn repository_anchor_keeps_memory_cold() {
        let state = ContextRecoveryState {
            anchors: vec![ContextAnchor::Repository {
                canonical_remote: "github.com/owner/repo".into(),
                workspace_id: None,
            }],
            has_durable_session_history: false,
            has_explicit_resource: false,
        };
        assert_eq!(
            recovery_cold_gate(&state),
            RecoveryColdGate::CurrentContextSufficient
        );
    }

    #[test]
    fn duplicate_classifier_needs_are_rejected() {
        let decision = ContextRecoveryDecision::Missing {
            needs: vec![
                ContextNeed::Repository {
                    query: "prs".into(),
                },
                ContextNeed::Repository {
                    query: "prs".into(),
                },
            ],
        };
        assert_eq!(
            validate_recovery_decision(decision, &RecoveryClassifierPolicy::default()),
            Err(RecoveryClassificationError::DuplicateNeed)
        );
    }
}
