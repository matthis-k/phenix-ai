use phenix_sdk::{
    ContextAnchor, MemoryAssociationConfirmation, MemoryAssociationObservation,
    MemoryAssociationState,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct AssociationStore {
    associations: BTreeMap<String, MemoryAssociationState>,
    processed_observation_events: BTreeSet<String>,
    processed_confirmation_receipts: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AssociationStoreError {
    InvalidAnchorIdentity,
    UnknownAssociation,
    ConfirmationAssociationMismatch,
}

impl AssociationStore {
    pub(crate) fn observe(
        &mut self,
        observation: &MemoryAssociationObservation,
    ) -> Result<(MemoryAssociationState, bool), AssociationStoreError> {
        let key = association_key(
            &observation.association.memory_id,
            &observation.association.anchor,
        )?;
        if self
            .processed_observation_events
            .contains(&observation.event_id)
        {
            let state = self
                .associations
                .get(&key)
                .cloned()
                .ok_or(AssociationStoreError::UnknownAssociation)?;
            return Ok((state, true));
        }

        let current = self.associations.get(&key).cloned().unwrap_or_else(|| {
            MemoryAssociationState {
                association: observation.association.clone(),
                observation_count: 0,
                confirmed_recoveries: 0,
                last_observed_at: 0,
                last_confirmed_at: None,
            }
        });
        let next = current.apply_observation(observation);
        self.associations.insert(key, next.clone());
        self.processed_observation_events
            .insert(observation.event_id.clone());
        Ok((next, false))
    }

    pub(crate) fn confirm(
        &mut self,
        confirmation: &MemoryAssociationConfirmation,
    ) -> Result<(MemoryAssociationState, bool), AssociationStoreError> {
        let key = association_key(&confirmation.memory_id, &confirmation.anchor)?;
        if self
            .processed_confirmation_receipts
            .contains(&confirmation.receipt_id)
        {
            let state = self
                .associations
                .get(&key)
                .cloned()
                .ok_or(AssociationStoreError::UnknownAssociation)?;
            return Ok((state, true));
        }

        let current = self
            .associations
            .get(&key)
            .cloned()
            .ok_or(AssociationStoreError::UnknownAssociation)?;
        if current.association.memory_id != confirmation.memory_id
            || current.association.anchor != confirmation.anchor
        {
            return Err(AssociationStoreError::ConfirmationAssociationMismatch);
        }
        let next = current.apply_confirmation(confirmation);
        self.associations.insert(key, next.clone());
        self.processed_confirmation_receipts
            .insert(confirmation.receipt_id.clone());
        Ok((next, false))
    }

    pub(crate) fn get(
        &self,
        memory_id: &str,
        anchor: &ContextAnchor,
    ) -> Result<Option<MemoryAssociationState>, AssociationStoreError> {
        Ok(self
            .associations
            .get(&association_key(memory_id, anchor)?)
            .cloned())
    }

    pub(crate) fn states(&self) -> impl Iterator<Item = &MemoryAssociationState> {
        self.associations.values()
    }
}

fn association_key(
    memory_id: &str,
    anchor: &ContextAnchor,
) -> Result<String, AssociationStoreError> {
    let anchor = serde_json::to_string(anchor)
        .map_err(|_| AssociationStoreError::InvalidAnchorIdentity)?;
    Ok(format!("{memory_id}\0{anchor}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_sdk::{
        AssociationObservationSource, MemoryContextAssociation, MemorySourceReference,
    };

    fn observation(event_id: &str) -> MemoryAssociationObservation {
        MemoryAssociationObservation {
            event_id: event_id.into(),
            request_id: "request-1".into(),
            source: AssociationObservationSource::RootAdmission,
            association: MemoryContextAssociation {
                memory_id: "memory-1".into(),
                anchor: ContextAnchor::Project {
                    key: "phenix".into(),
                },
                source_refs: Vec::<MemorySourceReference>::new(),
                observed_at: 10,
            },
        }
    }

    #[test]
    fn repeated_event_id_is_idempotent() {
        let mut store = AssociationStore::default();
        let event = observation("event-1");
        let (first, duplicate) = store.observe(&event).unwrap();
        assert!(!duplicate);
        let (second, duplicate) = store.observe(&event).unwrap();
        assert!(duplicate);
        assert_eq!(first.observation_count, 1);
        assert_eq!(second.observation_count, 1);
    }

    #[test]
    fn repeated_confirmation_receipt_is_idempotent() {
        let mut store = AssociationStore::default();
        let event = observation("event-1");
        store.observe(&event).unwrap();
        let confirmation = MemoryAssociationConfirmation {
            receipt_id: "receipt-1".into(),
            request_id: "request-1".into(),
            memory_id: event.association.memory_id.clone(),
            anchor: event.association.anchor.clone(),
            confirmed_at: 20,
        };
        let (first, duplicate) = store.confirm(&confirmation).unwrap();
        assert!(!duplicate);
        let (second, duplicate) = store.confirm(&confirmation).unwrap();
        assert!(duplicate);
        assert_eq!(first.confirmed_recoveries, 1);
        assert_eq!(second.confirmed_recoveries, 1);
    }
}
