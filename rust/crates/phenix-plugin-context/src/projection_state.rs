use phenix_sdk::{
    AdmittedContextItem, CompactionCommit, CompactionProposal, CompactionValidationError,
    ContextAdmissionResult, ContextProjectionForm, ContextRetention, ProjectionRevision,
    RetentionTransition,
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ContextProjectionState {
    pub execution_id: String,
    pub revision: ProjectionRevision,
    pub admitted: BTreeMap<String, AdmittedContextItem>,
    prepared: BTreeMap<String, CompactionProposal>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ProjectionStateError {
    ExecutionMismatch,
    StaleAdmissionEpoch { current: u64, incoming: u64 },
    DuplicatePreparedCheckpoint { checkpoint_id: String },
    UnknownPreparedCheckpoint { checkpoint_id: String },
    UnknownContextItem { item_id: String },
    Compaction(CompactionValidationError),
}

impl ContextProjectionState {
    pub(crate) fn new(execution_id: impl Into<String>) -> Self {
        Self {
            execution_id: execution_id.into(),
            revision: ProjectionRevision {
                revision: 0,
                cache_epoch: 0,
            },
            admitted: BTreeMap::new(),
            prepared: BTreeMap::new(),
        }
    }

    pub(crate) fn apply_admission(
        &mut self,
        result: ContextAdmissionResult,
    ) -> Result<(), ProjectionStateError> {
        if result.execution_id != self.execution_id {
            return Err(ProjectionStateError::ExecutionMismatch);
        }
        if result.cache_epoch < self.revision.cache_epoch {
            return Err(ProjectionStateError::StaleAdmissionEpoch {
                current: self.revision.cache_epoch,
                incoming: result.cache_epoch,
            });
        }

        self.prepared.clear();
        self.revision.revision = self.revision.revision.saturating_add(1);
        self.revision.cache_epoch = result.cache_epoch;
        self.admitted = result
            .admitted
            .into_iter()
            .map(|item| (item.id.clone(), item))
            .collect();
        Ok(())
    }

    pub(crate) fn prepare_compaction(
        &mut self,
        proposal: CompactionProposal,
    ) -> Result<(), ProjectionStateError> {
        if proposal.execution_id != self.execution_id {
            return Err(ProjectionStateError::ExecutionMismatch);
        }
        proposal
            .validate_against(&self.revision)
            .map_err(ProjectionStateError::Compaction)?;
        let checkpoint_id = proposal.checkpoint.checkpoint_id.clone();
        if self.prepared.contains_key(&checkpoint_id) {
            return Err(ProjectionStateError::DuplicatePreparedCheckpoint { checkpoint_id });
        }
        self.prepared.insert(checkpoint_id, proposal);
        Ok(())
    }

    pub(crate) fn commit_compaction(
        &mut self,
        checkpoint_id: &str,
    ) -> Result<CompactionCommit, ProjectionStateError> {
        let Some(proposal) = self.prepared.remove(checkpoint_id) else {
            return Err(ProjectionStateError::UnknownPreparedCheckpoint {
                checkpoint_id: checkpoint_id.to_owned(),
            });
        };
        proposal
            .validate_against(&self.revision)
            .map_err(ProjectionStateError::Compaction)?;

        for transition in &proposal.transitions {
            self.apply_transition(transition)?;
        }
        self.revision = ProjectionRevision {
            revision: self.revision.revision.saturating_add(1),
            cache_epoch: proposal.next_cache_epoch,
        };
        self.prepared.clear();

        Ok(CompactionCommit {
            proposal,
            committed_projection: self.revision.clone(),
        })
    }

    pub(crate) fn invalidate_for_context_mutation(&mut self) {
        self.prepared.clear();
        self.revision.revision = self.revision.revision.saturating_add(1);
    }

    fn apply_transition(
        &mut self,
        transition: &RetentionTransition,
    ) -> Result<(), ProjectionStateError> {
        let Some(item) = self.admitted.get_mut(&transition.item_id) else {
            return Err(ProjectionStateError::UnknownContextItem {
                item_id: transition.item_id.clone(),
            });
        };
        item.retention = transition.to;
        item.recovery = transition.recovery.clone().or_else(|| item.recovery.clone());
        item.form = match transition.to {
            ContextRetention::Pinned | ContextRetention::Full | ContextRetention::Compact => {
                ContextProjectionForm::Full
            }
            ContextRetention::Reference => ContextProjectionForm::Reference,
            ContextRetention::DropAllowed => ContextProjectionForm::Omitted,
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::Bytes;
    use phenix_sdk::{
        CachePlacement, ContextCheckpoint, ContextSource, ToolCallGroupReference,
    };

    fn state() -> ContextProjectionState {
        let mut state = ContextProjectionState::new("execution-1");
        state
            .apply_admission(ContextAdmissionResult {
                execution_id: "execution-1".into(),
                policy_revision: "policy-1".into(),
                cache_epoch: 1,
                admitted: vec![AdmittedContextItem {
                    id: "item-1".into(),
                    source: ContextSource::Inline {
                        identity: "item-1".into(),
                    },
                    content_identity: "sha256:item-1".into(),
                    form: ContextProjectionForm::Full,
                    cache: CachePlacement::Epoch,
                    retention: ContextRetention::Full,
                    estimated_tokens: 100,
                    recovery: None,
                }],
                used_input_tokens: 100,
                omitted_input_tokens: 0,
                deduplicated_items: 0,
            })
            .unwrap();
        state
    }

    fn proposal(state: &ContextProjectionState) -> CompactionProposal {
        CompactionProposal {
            execution_id: state.execution_id.clone(),
            expected_projection: state.revision.clone(),
            next_cache_epoch: state.revision.cache_epoch + 1,
            transitions: vec![RetentionTransition {
                item_id: "item-1".into(),
                from: ContextRetention::Full,
                to: ContextRetention::DropAllowed,
                recovery: None,
            }],
            checkpoint: ContextCheckpoint {
                checkpoint_id: "checkpoint-1".into(),
                execution_id: state.execution_id.clone(),
                source_revision: state.revision.clone(),
                content_identity: "sha256:checkpoint".into(),
                compact_view: Bytes::from(b"summary".to_vec()),
                exact_sources: Vec::new(),
                tool_groups: Vec::<ToolCallGroupReference>::new(),
            },
        }
    }

    #[test]
    fn context_mutation_invalidates_prepared_compaction() {
        let mut state = state();
        state.prepare_compaction(proposal(&state)).unwrap();
        state.invalidate_for_context_mutation();
        assert!(matches!(
            state.commit_compaction("checkpoint-1"),
            Err(ProjectionStateError::UnknownPreparedCheckpoint { .. })
        ));
    }

    #[test]
    fn compaction_commit_advances_revision_and_cache_epoch() {
        let mut state = state();
        let original = state.revision.clone();
        state.prepare_compaction(proposal(&state)).unwrap();
        let commit = state.commit_compaction("checkpoint-1").unwrap();
        assert_eq!(commit.committed_projection.revision, original.revision + 1);
        assert_eq!(commit.committed_projection.cache_epoch, original.cache_epoch + 1);
        assert_eq!(
            state.admitted["item-1"].form,
            ContextProjectionForm::Omitted
        );
    }
}
