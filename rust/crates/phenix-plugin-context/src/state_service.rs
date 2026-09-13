use crate::projection_state::{ContextProjectionState, ProjectionStateError};
use phenix_sdk::{ContextCommand, ContextResponse, ProjectionRevision};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const CONTEXT_PROJECTION_STATE_KEY: &str = "projection/service-state";
pub(crate) const MAX_CONTEXT_PROJECTION_STATE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ContextStateServiceError {
    InvalidSnapshot(String),
    SnapshotTooLarge { bytes: usize, allowed: usize },
    Admission(String),
    UnknownExecution { execution_id: String },
    Projection(ProjectionStateError),
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct ContextStateService {
    projections: BTreeMap<String, ContextProjectionState>,
}

impl ContextStateService {
    pub(crate) fn restore(snapshot: Option<&[u8]>) -> Result<Self, ContextStateServiceError> {
        match snapshot {
            Some(bytes) => {
                if bytes.len() > MAX_CONTEXT_PROJECTION_STATE_BYTES {
                    return Err(ContextStateServiceError::SnapshotTooLarge {
                        bytes: bytes.len(),
                        allowed: MAX_CONTEXT_PROJECTION_STATE_BYTES,
                    });
                }
                serde_json::from_slice(bytes)
                    .map_err(|error| ContextStateServiceError::InvalidSnapshot(error.to_string()))
            }
            None => Ok(Self::default()),
        }
    }

    pub(crate) fn snapshot(&self) -> Result<Vec<u8>, ContextStateServiceError> {
        let bytes = serde_json::to_vec(self)
            .map_err(|error| ContextStateServiceError::InvalidSnapshot(error.to_string()))?;
        if bytes.len() > MAX_CONTEXT_PROJECTION_STATE_BYTES {
            return Err(ContextStateServiceError::SnapshotTooLarge {
                bytes: bytes.len(),
                allowed: MAX_CONTEXT_PROJECTION_STATE_BYTES,
            });
        }
        Ok(bytes)
    }

    /// Handles projection-owner operations. Resource registration/load/project
    /// remain on the existing context persistence path.
    pub(crate) fn handle_state_command(
        &mut self,
        command: ContextCommand,
    ) -> Option<Result<ContextResponse, String>> {
        let response = match command {
            ContextCommand::Admit { request } => {
                let execution_id = request.execution_id.clone();
                request
                    .admit()
                    .map_err(|error| format!("context admission failed: {error:?}"))
                    .and_then(|result| {
                        let state = self
                            .projections
                            .entry(execution_id.clone())
                            .or_insert_with(|| ContextProjectionState::new(&execution_id));
                        state
                            .apply_admission(result.clone())
                            .map_err(|error| format!("context projection update failed: {error:?}"))?;
                        Ok(ContextResponse::Admission { result })
                    })
            }
            ContextCommand::PrepareCompaction { proposal } => {
                let execution_id = proposal.execution_id.clone();
                let checkpoint_id = proposal.checkpoint.checkpoint_id.clone();
                self.projection_mut(&execution_id)
                    .and_then(|state| {
                        state
                            .prepare_compaction(proposal)
                            .map_err(ContextStateServiceError::Projection)?;
                        Ok(state.revision.clone())
                    })
                    .map(|projection| ContextResponse::CompactionPrepared {
                        checkpoint_id,
                        projection,
                    })
                    .map_err(|error| format!("context compaction prepare failed: {error:?}"))
            }
            ContextCommand::CommitCompaction {
                execution_id,
                checkpoint_id,
            } => self
                .projection_mut(&execution_id)
                .and_then(|state| {
                    state
                        .commit_compaction(&checkpoint_id)
                        .map_err(ContextStateServiceError::Projection)
                })
                .map(|commit| ContextResponse::CompactionCommitted { commit })
                .map_err(|error| format!("context compaction commit failed: {error:?}")),
            ContextCommand::InvalidateProjection { execution_id } => self
                .projection_mut(&execution_id)
                .map(|state| {
                    state.invalidate_for_context_mutation();
                    ContextResponse::ProjectionInvalidated {
                        projection: state.revision.clone(),
                    }
                })
                .map_err(|error| format!("context projection invalidation failed: {error:?}")),
            ContextCommand::Register { .. }
            | ContextCommand::Get { .. }
            | ContextCommand::List
            | ContextCommand::DiscoverRepository { .. }
            | ContextCommand::Load { .. }
            | ContextCommand::Project { .. } => return None,
        };
        Some(response)
    }

    pub(crate) fn projection_revision(&self, execution_id: &str) -> Option<&ProjectionRevision> {
        self.projections.get(execution_id).map(|state| &state.revision)
    }

    fn projection_mut(
        &mut self,
        execution_id: &str,
    ) -> Result<&mut ContextProjectionState, ContextStateServiceError> {
        self.projections
            .get_mut(execution_id)
            .ok_or_else(|| ContextStateServiceError::UnknownExecution {
                execution_id: execution_id.to_owned(),
            })
    }
}
