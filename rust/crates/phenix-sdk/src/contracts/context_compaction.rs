use super::{ContextRetention, ExactContextReference};
use phenix_core::Bytes;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ProjectionRevision {
    pub revision: u64,
    pub cache_epoch: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ToolCallGroupReference {
    pub call_id: String,
    pub call: ExactContextReference,
    pub result: ExactContextReference,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContextCheckpoint {
    pub checkpoint_id: String,
    pub execution_id: String,
    pub source_revision: ProjectionRevision,
    pub content_identity: String,
    pub compact_view: Bytes,
    #[serde(default)]
    pub exact_sources: Vec<ExactContextReference>,
    #[serde(default)]
    pub tool_groups: Vec<ToolCallGroupReference>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct RetentionTransition {
    pub item_id: String,
    pub from: ContextRetention,
    pub to: ContextRetention,
    pub recovery: Option<ExactContextReference>,
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum ContextReducerStage {
    CodeEvidence,
    ObservationSummary,
    HistorySummary,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ReducerEligibleItem {
    pub item_id: String,
    pub recovery: Option<ExactContextReference>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContextReducerRequest {
    pub execution_id: String,
    pub expected_projection: ProjectionRevision,
    pub configuration_revision: String,
    pub authority_revision: String,
    pub capability_generation: String,
    pub stage: ContextReducerStage,
    pub query: String,
    pub helper_reservation_id: String,
    pub max_output_bytes: u64,
    pub eligible: Vec<ReducerEligibleItem>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct DerivedReductionSummary {
    pub item_id: String,
    pub content: Bytes,
    #[serde(default)]
    pub exact_sources: Vec<ExactContextReference>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContextReducerProposal {
    pub execution_id: String,
    pub expected_projection: ProjectionRevision,
    pub stage: ContextReducerStage,
    pub helper_attempt_id: String,
    #[serde(default)]
    pub retained_item_ids: Vec<String>,
    #[serde(default)]
    pub omitted_item_ids: Vec<String>,
    #[serde(default)]
    pub summaries: Vec<DerivedReductionSummary>,
    pub encoded_output_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReducerValidationError {
    StaleProjection,
    ExecutionMismatch,
    StageMismatch,
    UnknownItem { item_id: String },
    DuplicateItem { item_id: String },
    MissingItemDecision { item_id: String },
    MissingRecoveryReference { item_id: String },
    DuplicateSummary { item_id: String },
    SummaryWithoutExactSource { item_id: String },
    SummaryMissingEligibleRecovery { item_id: String },
    OutputBudgetExceeded { requested: u64, allowed: u64 },
}

impl ContextReducerProposal {
    pub fn validate_against(
        &self,
        request: &ContextReducerRequest,
        actual_projection: &ProjectionRevision,
    ) -> Result<(), ReducerValidationError> {
        if &request.expected_projection != actual_projection
            || &self.expected_projection != actual_projection
        {
            return Err(ReducerValidationError::StaleProjection);
        }
        if self.execution_id != request.execution_id {
            return Err(ReducerValidationError::ExecutionMismatch);
        }
        if self.stage != request.stage {
            return Err(ReducerValidationError::StageMismatch);
        }
        let encoded = serde_json::to_vec(&phenix_core::PhenixValue::from(self))
            .expect("context reducer proposal has a deterministic PhenixValue encoding");
        let actual_encoded_bytes = u64::try_from(encoded.len()).unwrap_or(u64::MAX);
        let requested_output_bytes = self.encoded_output_bytes.max(actual_encoded_bytes);
        if requested_output_bytes > request.max_output_bytes {
            return Err(ReducerValidationError::OutputBudgetExceeded {
                requested: requested_output_bytes,
                allowed: request.max_output_bytes,
            });
        }

        let eligible = request
            .eligible
            .iter()
            .map(|item| (item.item_id.as_str(), item))
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut seen = BTreeSet::new();
        for item_id in self
            .retained_item_ids
            .iter()
            .chain(self.omitted_item_ids.iter())
        {
            if !seen.insert(item_id.as_str()) {
                return Err(ReducerValidationError::DuplicateItem {
                    item_id: item_id.clone(),
                });
            }
            let Some(item) = eligible.get(item_id.as_str()) else {
                return Err(ReducerValidationError::UnknownItem {
                    item_id: item_id.clone(),
                });
            };
            if self.omitted_item_ids.contains(item_id) && item.recovery.is_none() {
                return Err(ReducerValidationError::MissingRecoveryReference {
                    item_id: item_id.clone(),
                });
            }
        }
        for item in &request.eligible {
            if !seen.contains(item.item_id.as_str()) {
                return Err(ReducerValidationError::MissingItemDecision {
                    item_id: item.item_id.clone(),
                });
            }
        }

        let mut summary_seen = BTreeSet::new();
        for summary in &self.summaries {
            if !eligible.contains_key(summary.item_id.as_str()) {
                return Err(ReducerValidationError::UnknownItem {
                    item_id: summary.item_id.clone(),
                });
            }
            if !summary_seen.insert(summary.item_id.as_str()) {
                return Err(ReducerValidationError::DuplicateSummary {
                    item_id: summary.item_id.clone(),
                });
            }
            if summary.exact_sources.is_empty() {
                return Err(ReducerValidationError::SummaryWithoutExactSource {
                    item_id: summary.item_id.clone(),
                });
            }
            if let Some(expected) = eligible
                .get(summary.item_id.as_str())
                .and_then(|item| item.recovery.as_ref())
            {
                if !summary.exact_sources.contains(expected) {
                    return Err(ReducerValidationError::SummaryMissingEligibleRecovery {
                        item_id: summary.item_id.clone(),
                    });
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct CompactionProposal {
    pub execution_id: String,
    pub expected_projection: ProjectionRevision,
    pub next_cache_epoch: u64,
    #[serde(default)]
    pub transitions: Vec<RetentionTransition>,
    pub checkpoint: ContextCheckpoint,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct CompactionCommit {
    pub proposal: CompactionProposal,
    pub committed_projection: ProjectionRevision,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompactionValidationError {
    StaleProjection {
        expected: ProjectionRevision,
        actual: ProjectionRevision,
    },
    CacheEpochDidNotAdvance {
        current: u64,
        proposed: u64,
    },
    PinnedItemDemoted {
        item_id: String,
    },
    RetentionExpanded {
        item_id: String,
        from: ContextRetention,
        to: ContextRetention,
    },
    MissingRecoveryReference {
        item_id: String,
        to: ContextRetention,
    },
    CheckpointSourceMismatch,
}

impl CompactionProposal {
    pub fn validate_against(
        &self,
        actual: &ProjectionRevision,
    ) -> Result<(), CompactionValidationError> {
        if &self.expected_projection != actual {
            return Err(CompactionValidationError::StaleProjection {
                expected: self.expected_projection.clone(),
                actual: actual.clone(),
            });
        }
        if self.next_cache_epoch <= actual.cache_epoch {
            return Err(CompactionValidationError::CacheEpochDidNotAdvance {
                current: actual.cache_epoch,
                proposed: self.next_cache_epoch,
            });
        }
        if self.checkpoint.source_revision != self.expected_projection
            || self.checkpoint.execution_id != self.execution_id
        {
            return Err(CompactionValidationError::CheckpointSourceMismatch);
        }

        for transition in &self.transitions {
            if transition.from == ContextRetention::Pinned
                && transition.to != ContextRetention::Pinned
            {
                return Err(CompactionValidationError::PinnedItemDemoted {
                    item_id: transition.item_id.clone(),
                });
            }
            if retention_rank(transition.to) < retention_rank(transition.from) {
                return Err(CompactionValidationError::RetentionExpanded {
                    item_id: transition.item_id.clone(),
                    from: transition.from,
                    to: transition.to,
                });
            }
            if matches!(
                transition.to,
                ContextRetention::Compact | ContextRetention::Reference
            ) && transition.recovery.is_none()
            {
                return Err(CompactionValidationError::MissingRecoveryReference {
                    item_id: transition.item_id.clone(),
                    to: transition.to,
                });
            }
        }
        Ok(())
    }
}

const fn retention_rank(retention: ContextRetention) -> u8 {
    match retention {
        ContextRetention::Pinned => 0,
        ContextRetention::Full => 1,
        ContextRetention::Compact => 2,
        ContextRetention::Reference => 3,
        ContextRetention::DropAllowed => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{ContextResourceId, ContextRevisionId};

    fn exact(id: &str) -> ExactContextReference {
        ExactContextReference {
            resource_id: ContextResourceId::parse(id).unwrap(),
            revision: ContextRevisionId::parse("rev-1").unwrap(),
        }
    }

    fn proposal(transition: RetentionTransition) -> CompactionProposal {
        let revision = ProjectionRevision {
            revision: 7,
            cache_epoch: 2,
        };
        CompactionProposal {
            execution_id: "e1".into(),
            expected_projection: revision.clone(),
            next_cache_epoch: 3,
            transitions: vec![transition],
            checkpoint: ContextCheckpoint {
                checkpoint_id: "checkpoint-1".into(),
                execution_id: "e1".into(),
                source_revision: revision,
                content_identity: "sha256:fixture".into(),
                compact_view: Bytes::from(b"summary".to_vec()),
                exact_sources: vec![exact("context:source")],
                tool_groups: Vec::new(),
            },
        }
    }

    fn reducer_request() -> ContextReducerRequest {
        ContextReducerRequest {
            execution_id: "e1".into(),
            expected_projection: ProjectionRevision {
                revision: 7,
                cache_epoch: 2,
            },
            configuration_revision: "config-1".into(),
            authority_revision: "authority-1".into(),
            capability_generation: "capability-1".into(),
            stage: ContextReducerStage::HistorySummary,
            query: "current task".into(),
            helper_reservation_id: "reservation-1".into(),
            max_output_bytes: 1024,
            eligible: vec![ReducerEligibleItem {
                item_id: "history-1".into(),
                recovery: Some(exact("context:history-1")),
            }],
        }
    }

    #[test]
    fn reducer_cannot_omit_content_without_exact_recovery() {
        let mut request = reducer_request();
        request.eligible[0].recovery = None;
        let proposal = ContextReducerProposal {
            execution_id: "e1".into(),
            expected_projection: request.expected_projection.clone(),
            stage: request.stage,
            helper_attempt_id: "attempt-1".into(),
            retained_item_ids: Vec::new(),
            omitted_item_ids: vec!["history-1".into()],
            summaries: Vec::new(),
            encoded_output_bytes: 10,
        };

        assert!(matches!(
            proposal.validate_against(&request, &request.expected_projection),
            Err(ReducerValidationError::MissingRecoveryReference { .. })
        ));
    }

    #[test]
    fn reducer_rejects_fabricated_ids_and_oversized_output() {
        let request = reducer_request();
        let mut proposal = ContextReducerProposal {
            execution_id: "e1".into(),
            expected_projection: request.expected_projection.clone(),
            stage: request.stage,
            helper_attempt_id: "attempt-1".into(),
            retained_item_ids: vec!["fabricated".into()],
            omitted_item_ids: Vec::new(),
            summaries: Vec::new(),
            encoded_output_bytes: 10,
        };
        assert!(matches!(
            proposal.validate_against(&request, &request.expected_projection),
            Err(ReducerValidationError::UnknownItem { .. })
        ));

        proposal.retained_item_ids = vec!["history-1".into()];
        proposal.encoded_output_bytes = 1025;
        assert!(matches!(
            proposal.validate_against(&request, &request.expected_projection),
            Err(ReducerValidationError::OutputBudgetExceeded { .. })
        ));
    }

    #[test]
    fn reducer_summary_must_reference_the_eligible_items_exact_recovery() {
        let request = reducer_request();
        let proposal = ContextReducerProposal {
            execution_id: "e1".into(),
            expected_projection: request.expected_projection.clone(),
            stage: request.stage,
            helper_attempt_id: "attempt-1".into(),
            retained_item_ids: vec!["history-1".into()],
            omitted_item_ids: Vec::new(),
            summaries: vec![DerivedReductionSummary {
                item_id: "history-1".into(),
                content: Bytes::from(b"summary".to_vec()),
                exact_sources: vec![exact("context:other")],
            }],
            encoded_output_bytes: 10,
        };

        assert_eq!(
            proposal.validate_against(&request, &request.expected_projection),
            Err(ReducerValidationError::SummaryMissingEligibleRecovery {
                item_id: "history-1".into(),
            })
        );
    }

    #[test]
    fn reducer_rejects_underreported_encoded_output_size() {
        let mut request = reducer_request();
        request.max_output_bytes = 64;
        let proposal = ContextReducerProposal {
            execution_id: "e1".into(),
            expected_projection: request.expected_projection.clone(),
            stage: request.stage,
            helper_attempt_id: "attempt-1".into(),
            retained_item_ids: vec!["history-1".into()],
            omitted_item_ids: Vec::new(),
            summaries: Vec::new(),
            encoded_output_bytes: 1,
        };

        assert!(matches!(
            proposal.validate_against(&request, &request.expected_projection),
            Err(ReducerValidationError::OutputBudgetExceeded {
                requested,
                allowed: 64
            }) if requested > 64
        ));
    }

    #[test]
    fn reducer_requires_an_explicit_decision_for_every_eligible_item() {
        let request = reducer_request();
        let proposal = ContextReducerProposal {
            execution_id: "e1".into(),
            expected_projection: request.expected_projection.clone(),
            stage: request.stage,
            helper_attempt_id: "attempt-1".into(),
            retained_item_ids: Vec::new(),
            omitted_item_ids: Vec::new(),
            summaries: Vec::new(),
            encoded_output_bytes: 1,
        };

        assert!(matches!(
            proposal.validate_against(&request, &request.expected_projection),
            Err(ReducerValidationError::MissingItemDecision { item_id })
                if item_id == "history-1"
        ));
    }

    #[test]
    fn compact_requires_exact_recovery() {
        let proposal = proposal(RetentionTransition {
            item_id: "item-1".into(),
            from: ContextRetention::Full,
            to: ContextRetention::Compact,
            recovery: None,
        });
        assert!(matches!(
            proposal.validate_against(&proposal.expected_projection),
            Err(CompactionValidationError::MissingRecoveryReference { .. })
        ));
    }

    #[test]
    fn pinned_items_cannot_be_compacted() {
        let proposal = proposal(RetentionTransition {
            item_id: "goal".into(),
            from: ContextRetention::Pinned,
            to: ContextRetention::Reference,
            recovery: Some(exact("context:goal")),
        });
        assert!(matches!(
            proposal.validate_against(&proposal.expected_projection),
            Err(CompactionValidationError::PinnedItemDemoted { .. })
        ));
    }

    #[test]
    fn stale_proposal_is_rejected() {
        let proposal = proposal(RetentionTransition {
            item_id: "item-1".into(),
            from: ContextRetention::Full,
            to: ContextRetention::Reference,
            recovery: Some(exact("context:item")),
        });
        let actual = ProjectionRevision {
            revision: 8,
            cache_epoch: 2,
        };
        assert!(matches!(
            proposal.validate_against(&actual),
            Err(CompactionValidationError::StaleProjection { .. })
        ));
    }
}
