use super::{
    context::{ContextAnchor, ContextNeed},
    memory::{MemoryScope, MemorySourceReference},
};
use phenix_core::{ComponentInterface, InterfaceId, ServiceId};
use serde::{Deserialize, Serialize};

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct MemoryContextAssociation {
    pub memory_id: String,
    pub anchor: ContextAnchor,
    pub source_refs: Vec<MemorySourceReference>,
    pub observed_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct MemoryContextRecallRequest {
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
    pub source_refs: Vec<MemorySourceReference>,
    pub signals: Vec<MemoryContextMatch>,
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
}

impl RecallEvidence {
    #[must_use]
    pub fn is_auto_selectable(&self) -> bool {
        self.candidate.evidence_class() >= 2
            && self.missing_needs.is_empty()
            && matches!(self.completeness, CandidateCompleteness::Complete)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum MemoryContextCommand {
    Associate {
        association: MemoryContextAssociation,
    },
    Recall {
        request: MemoryContextRecallRequest,
    },
    ConfirmUse {
        memory_id: String,
        anchor: ContextAnchor,
        at: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum MemoryContextResponse {
    Associated {
        association: MemoryContextAssociation,
    },
    Recall {
        candidates: Vec<MemoryContextCandidate>,
    },
    Confirmed,
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
            last_observed_at: 1,
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
    fn lexical_or_stronger_evidence_still_requires_complete_need_coverage() {
        let mut evidence = RecallEvidence {
            candidate: candidate(vec![MemoryContextMatch::Lexical]),
            resolved_needs: Vec::new(),
            missing_needs: Vec::new(),
            completeness: CandidateCompleteness::Complete,
        };
        assert!(evidence.is_auto_selectable());

        evidence.completeness = CandidateCompleteness::Incomplete {
            reason: "descriptor scan truncated".to_owned(),
        };
        assert!(!evidence.is_auto_selectable());
    }
}
