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

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct RecoveryClassifierPolicy {
    #[serde(default)]
    pub max_prompt_bytes: Option<u32>,
    #[serde(default)]
    pub max_anchors: Option<u32>,
    #[serde(default)]
    pub max_needs: Option<u32>,
    #[serde(default)]
    pub max_need_query_bytes: Option<u32>,
    #[serde(default)]
    pub max_attempts: Option<u32>,
    #[serde(default)]
    pub max_output_tokens: Option<u64>,
    #[serde(default)]
    pub classifier_timeout_ms: Option<u64>,
    #[serde(default)]
    pub total_timeout_ms: Option<u64>,
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
    pub fn effective_deadline_at_ms(&self) -> Option<u64> {
        let policy_deadline = self
            .policy
            .total_timeout_ms
            .map(|timeout| self.now_ms.saturating_add(timeout));
        match (self.caller_deadline_at_ms, policy_deadline) {
            (Some(caller), Some(policy)) => Some(caller.min(policy)),
            (Some(caller), None) => Some(caller),
            (None, Some(policy)) => Some(policy),
            (None, None) => None,
        }
    }

    pub fn validate_input(&self) -> Result<(), RecoveryClassificationError> {
        if let Some(limit) = self.policy.max_prompt_bytes {
            if self.prompt.len() > limit as usize {
                return Err(RecoveryClassificationError::PromptEvidenceTooLarge {
                    requested: self.prompt.len() as u64,
                    allowed: limit as u64,
                });
            }
        }
        if let Some(limit) = self.policy.max_anchors {
            if self.state.anchors.len() > limit as usize {
                return Err(RecoveryClassificationError::TooManyAnchors {
                    requested: self.state.anchors.len() as u32,
                    allowed: limit,
                });
            }
        }
        if self
            .effective_deadline_at_ms()
            .is_some_and(|deadline| self.now_ms >= deadline)
        {
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
    if let Some(limit) = policy.max_needs {
        if needs.len() > limit as usize {
            return Err(RecoveryClassificationError::TooManyNeeds {
                requested: needs.len() as u32,
                allowed: limit,
            });
        }
    }

    let mut identities = BTreeSet::new();
    for need in needs {
        let query = need_query(need);
        if query.trim().is_empty() {
            return Err(RecoveryClassificationError::EmptyNeedQuery);
        }
        if let Some(limit) = policy.max_need_query_bytes {
            if query.len() > limit as usize {
                return Err(RecoveryClassificationError::NeedQueryTooLarge {
                    requested: query.len() as u64,
                    allowed: limit as u64,
                });
            }
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
    fn default_recovery_policy_has_no_implicit_limits_or_timeout() {
        let policy = RecoveryClassifierPolicy::default();
        assert_eq!(policy.max_prompt_bytes, None);
        assert_eq!(policy.max_anchors, None);
        assert_eq!(policy.max_needs, None);
        assert_eq!(policy.max_need_query_bytes, None);
        assert_eq!(policy.max_attempts, None);
        assert_eq!(policy.max_output_tokens, None);
        assert_eq!(policy.classifier_timeout_ms, None);
        assert_eq!(policy.total_timeout_ms, None);
    }

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
