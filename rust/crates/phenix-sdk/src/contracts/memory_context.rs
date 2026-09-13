use super::{
    context::{ContextAnchor, ContextNeed},
    memory::{MemoryScope, MemorySourceReference},
};
use phenix_core::{ComponentInterface, InterfaceId, ServiceId};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

pub const MEMORY_CONTEXT_SERVICE: &str = "memory.context@1";

#[derive(
    Clone,
    Copy,
    Debug,
    Eq,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
    phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum MemoryContextMatch {
    ExplicitLink,
    ExactAnchor,
    ExactSource,
    ConfirmedUse,
    RepeatedObservation,
    Lexical,
    Recency,
    Semantic,
}

impl MemoryContextMatch {
    #[must_use]
    pub const fn evidence_class(self) -> u8 {
        match self {
            Self::ExplicitLink | Self::ExactAnchor | Self::ExactSource => 4,
            Self::ConfirmedUse => 3,
            Self::Lexical => 2,
            Self::RepeatedObservation => 1,
            Self::Recency | Self::Semantic => 0,
        }
    }
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum AssociationObservationSource {
    WorkspaceSelection,
    RootAdmission,
    TaskBinding,
    ExplicitLink,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct MemoryContextAssociation {
    pub memory_id: String,
    pub anchor: ContextAnchor,
    #[serde(default)]
    pub source_refs: Vec<MemorySourceReference>,
    pub observed_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct MemoryAssociationObservation {
    pub event_id: String,
    pub request_id: String,
    pub source: AssociationObservationSource,
    pub association: MemoryContextAssociation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct MemoryAssociationConfirmation {
    pub receipt_id: String,
    pub request_id: String,
    pub memory_id: String,
    pub anchor: ContextAnchor,
    pub confirmed_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct MemoryAssociationState {
    pub association: MemoryContextAssociation,
    pub observation_count: u32,
    pub confirmed_recoveries: u32,
    pub last_observed_at: u64,
    pub last_confirmed_at: Option<u64>,
}

impl MemoryAssociationState {
    #[must_use]
    pub fn apply_observation(&self, event: &MemoryAssociationObservation) -> Self {
        let mut next = self.clone();
        next.association = event.association.clone();
        next.observation_count = next.observation_count.saturating_add(1);
        next.last_observed_at = next.last_observed_at.max(event.association.observed_at);
        next
    }

    #[must_use]
    pub fn apply_confirmation(&self, confirmation: &MemoryAssociationConfirmation) -> Self {
        let mut next = self.clone();
        next.confirmed_recoveries = next.confirmed_recoveries.saturating_add(1);
        next.last_confirmed_at = Some(
            next.last_confirmed_at
                .map_or(confirmation.confirmed_at, |current| {
                    current.max(confirmation.confirmed_at)
                }),
        );
        next
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct MemoryContextRecallRequest {
    pub request_id: String,
    pub scopes: Vec<MemoryScope>,
    pub prompt: String,
    pub known: Vec<ContextAnchor>,
    pub needs: Vec<ContextNeed>,
    pub at: u64,
    pub limit: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct MemoryContextCandidate {
    pub memory_id: String,
    pub anchor: ContextAnchor,
    #[serde(default)]
    pub source_refs: Vec<MemorySourceReference>,
    #[serde(default)]
    pub signals: Vec<MemoryContextMatch>,
    pub observation_count: u32,
    pub confirmed_recoveries: u32,
    pub last_observed_at: u64,
}

impl MemoryContextCandidate {
    #[must_use]
    pub fn evidence_class(&self) -> u8 {
        self.signals
            .iter()
            .copied()
            .map(MemoryContextMatch::evidence_class)
            .max()
            .unwrap_or(0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "completeness", rename_all = "snake_case", deny_unknown_fields)]
pub enum CandidateCompleteness {
    Complete,
    Incomplete { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct RecallEvidence {
    pub candidate: MemoryContextCandidate,
    #[serde(default)]
    pub resolved_needs: Vec<ContextNeed>,
    #[serde(default)]
    pub missing_needs: Vec<ContextNeed>,
    pub completeness: CandidateCompleteness,
    pub query_relevant: bool,
    pub live_validated: bool,
}

impl RecallEvidence {
    #[must_use]
    pub fn is_auto_selectable(&self) -> bool {
        self.candidate.evidence_class() >= 2
            && self.missing_needs.is_empty()
            && matches!(self.completeness, CandidateCompleteness::Complete)
            && self.query_relevant
            && self.live_validated
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "resolution", rename_all = "snake_case", deny_unknown_fields)]
pub enum RecallResolution {
    Unique { winner: RecallEvidence },
    Ambiguous { candidates: Vec<RecallEvidence> },
    NotFound,
}

#[must_use]
pub fn resolve_recall(mut evidence: Vec<RecallEvidence>) -> RecallResolution {
    evidence.retain(RecallEvidence::is_auto_selectable);
    if evidence.is_empty() {
        return RecallResolution::NotFound;
    }
    evidence.sort_by(compare_evidence);
    if evidence.len() == 1 || compare_evidence(&evidence[0], &evidence[1]) != Ordering::Equal {
        RecallResolution::Unique {
            winner: evidence.remove(0),
        }
    } else {
        let best = evidence[0].clone();
        let tied = evidence
            .into_iter()
            .take_while(|candidate| compare_evidence(candidate, &best) == Ordering::Equal)
            .collect();
        RecallResolution::Ambiguous { candidates: tied }
    }
}

fn compare_evidence(left: &RecallEvidence, right: &RecallEvidence) -> Ordering {
    right
        .candidate
        .evidence_class()
        .cmp(&left.candidate.evidence_class())
        .then_with(|| {
            right
                .candidate
                .confirmed_recoveries
                .cmp(&left.candidate.confirmed_recoveries)
        })
        .then_with(|| {
            right
                .candidate
                .observation_count
                .cmp(&left.candidate.observation_count)
        })
        .then_with(|| {
            right
                .candidate
                .last_observed_at
                .cmp(&left.candidate.last_observed_at)
        })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum MemoryContextCommand {
    Observe {
        observation: MemoryAssociationObservation,
    },
    Recall {
        request: MemoryContextRecallRequest,
    },
    Resolve {
        request: MemoryContextRecallRequest,
    },
    ConfirmUse {
        confirmation: MemoryAssociationConfirmation,
    },
    GetAssociation {
        memory_id: String,
        anchor: ContextAnchor,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum MemoryContextResponse {
    Observed {
        state: MemoryAssociationState,
        duplicate: bool,
    },
    Recall {
        candidates: Vec<MemoryContextCandidate>,
        completeness: CandidateCompleteness,
    },
    Resolution {
        resolution: RecallResolution,
    },
    Confirmed {
        state: MemoryAssociationState,
        duplicate: bool,
    },
    Association {
        state: Option<MemoryAssociationState>,
    },
}

pub struct MemoryContextInterface;

impl ComponentInterface for MemoryContextInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(MEMORY_CONTEXT_SERVICE)
            .expect("static memory context interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<MemoryContextCommand, MemoryContextResponse>()
    }
}

#[must_use]
pub fn memory_context_service() -> ServiceId {
    ServiceId::parse(MEMORY_CONTEXT_SERVICE).expect("static memory context service id is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(signals: Vec<MemoryContextMatch>) -> MemoryContextCandidate {
        MemoryContextCandidate {
            memory_id: "memory-1".to_owned(),
            anchor: ContextAnchor::Project {
                key: "project-1".to_owned(),
            },
            source_refs: Vec::new(),
            signals,
            observation_count: 1,
            confirmed_recoveries: 0,
            last_observed_at: 1,
        }
    }

    fn evidence(signals: Vec<MemoryContextMatch>) -> RecallEvidence {
        RecallEvidence {
            candidate: candidate(signals),
            resolved_needs: Vec::new(),
            missing_needs: Vec::new(),
            completeness: CandidateCompleteness::Complete,
            query_relevant: true,
            live_validated: true,
        }
    }

    #[test]
    fn weak_signals_never_cross_the_auto_selection_floor() {
        assert_eq!(
            candidate(vec![
                MemoryContextMatch::Semantic,
                MemoryContextMatch::Recency
            ])
            .evidence_class(),
            0
        );
        assert_eq!(
            candidate(vec![MemoryContextMatch::RepeatedObservation]).evidence_class(),
            1
        );
    }

    #[test]
    fn lexical_or_stronger_evidence_requires_live_validation() {
        let mut evidence = evidence(vec![MemoryContextMatch::Lexical]);
        assert!(evidence.is_auto_selectable());
        evidence.live_validated = false;
        assert!(!evidence.is_auto_selectable());
    }

    #[test]
    fn tied_best_candidates_are_ambiguous() {
        let left = evidence(vec![MemoryContextMatch::Lexical]);
        let mut right = evidence(vec![MemoryContextMatch::Lexical]);
        right.candidate.memory_id = "memory-2".into();
        assert!(matches!(
            resolve_recall(vec![left, right]),
            RecallResolution::Ambiguous { .. }
        ));
    }
}
