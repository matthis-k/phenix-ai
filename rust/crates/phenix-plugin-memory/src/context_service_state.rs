use crate::association_store::AssociationStore;
use phenix_sdk::{resolve_recall, MemoryContextCommand, MemoryContextResponse};

pub(crate) const MEMORY_CONTEXT_STATE_KEY: &str = "context/service-state";
pub(crate) const MAX_MEMORY_CONTEXT_STATE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum MemoryContextServiceError {
    InvalidSnapshot(String),
    SnapshotTooLarge { bytes: usize, allowed: usize },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct MemoryContextServiceState {
    associations: AssociationStore,
}

impl MemoryContextServiceState {
    pub(crate) fn restore(snapshot: Option<&[u8]>) -> Result<Self, MemoryContextServiceError> {
        let associations = match snapshot {
            Some(bytes) => {
                if bytes.len() > MAX_MEMORY_CONTEXT_STATE_BYTES {
                    return Err(MemoryContextServiceError::SnapshotTooLarge {
                        bytes: bytes.len(),
                        allowed: MAX_MEMORY_CONTEXT_STATE_BYTES,
                    });
                }
                serde_json::from_slice(bytes).map_err(|error| {
                    MemoryContextServiceError::InvalidSnapshot(error.to_string())
                })?
            }
            None => AssociationStore::default(),
        };
        Ok(Self { associations })
    }

    pub(crate) fn snapshot(&self) -> Result<Vec<u8>, MemoryContextServiceError> {
        let bytes = serde_json::to_vec(&self.associations)
            .map_err(|error| MemoryContextServiceError::InvalidSnapshot(error.to_string()))?;
        if bytes.len() > MAX_MEMORY_CONTEXT_STATE_BYTES {
            return Err(MemoryContextServiceError::SnapshotTooLarge {
                bytes: bytes.len(),
                allowed: MAX_MEMORY_CONTEXT_STATE_BYTES,
            });
        }
        Ok(bytes)
    }

    /// Handles association-state operations. `Recall` stays with the semantic
    /// memory retrieval implementation because it needs records, freshness,
    /// provenance indexes, and caller scope rather than only association state.
    pub(crate) fn handle_local(
        &mut self,
        command: MemoryContextCommand,
    ) -> Option<Result<MemoryContextResponse, String>> {
        let response = match command {
            MemoryContextCommand::Observe { observation } => self
                .associations
                .observe(&observation)
                .map(|(state, duplicate)| MemoryContextResponse::Observed { state, duplicate })
                .map_err(|error| format!("memory association observation failed: {error:?}")),
            MemoryContextCommand::Resolve { evidence } => Ok(MemoryContextResponse::Resolution {
                resolution: resolve_recall(evidence),
            }),
            MemoryContextCommand::ConfirmUse { confirmation } => self
                .associations
                .confirm(&confirmation)
                .map(|(state, duplicate)| MemoryContextResponse::Confirmed { state, duplicate })
                .map_err(|error| format!("memory association confirmation failed: {error:?}")),
            MemoryContextCommand::GetAssociation { memory_id, anchor } => self
                .associations
                .get(&memory_id, &anchor)
                .map(|state| MemoryContextResponse::Association { state })
                .map_err(|error| format!("memory association lookup failed: {error:?}")),
            MemoryContextCommand::Recall { .. } => return None,
        };
        Some(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_sdk::{
        AssociationObservationSource, ContextAnchor, MemoryAssociationObservation,
        MemoryContextAssociation,
    };

    fn observation() -> MemoryAssociationObservation {
        MemoryAssociationObservation {
            event_id: "event-1".into(),
            request_id: "request-1".into(),
            source: AssociationObservationSource::RootAdmission,
            association: MemoryContextAssociation {
                memory_id: "memory-1".into(),
                anchor: ContextAnchor::Project {
                    key: "phenix".into(),
                },
                source_refs: Vec::new(),
                observed_at: 10,
            },
        }
    }

    #[test]
    fn observation_idempotency_survives_snapshot_restore() {
        let mut state = MemoryContextServiceState::default();
        let command = MemoryContextCommand::Observe {
            observation: observation(),
        };
        state.handle_local(command.clone()).unwrap().unwrap();

        let mut restored =
            MemoryContextServiceState::restore(Some(&state.snapshot().unwrap())).unwrap();
        let response = restored.handle_local(command).unwrap().unwrap();
        assert!(matches!(
            response,
            MemoryContextResponse::Observed {
                duplicate: true,
                ..
            }
        ));
    }
}
