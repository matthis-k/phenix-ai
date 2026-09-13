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
