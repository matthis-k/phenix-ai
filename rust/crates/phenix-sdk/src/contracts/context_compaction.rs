use super::{ContextRetention, ExactContextReference};
use phenix_core::Bytes;
use serde::{Deserialize, Serialize};

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
