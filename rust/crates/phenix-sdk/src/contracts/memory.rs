use super::memory_freshness::{MemoryCanonicalReference, MemoryFreshnessRecord};
use phenix_core::{
    CallableId, ComponentInterface, InterfaceId, RoutingProfileId, ServiceId, SessionId,
};
use serde::{Deserialize, Serialize};

pub const MEMORY_SERVICE: &str = "phenix.memory@1";
pub const MEMORY_EMBED_SERVICE: &str = "memory.embed@1";
pub const MEMORY_RANK_SERVICE: &str = "memory.rank@1";
pub const CONTEXT_COMPACTION_SERVICE: &str = "context.compact@1";
pub const CONTEXT_EXPANSION_SERVICE: &str = "context.expand@1";

pub const MEMORY_SUMMARIZE_CALLABLE: &str = "memory.summarize";
pub const MEMORY_EXTRACT_CALLABLE: &str = "memory.extract";
pub const MEMORY_CONSOLIDATE_CALLABLE: &str = "memory.consolidate";
pub const MEMORY_RESOLVE_CALLABLE: &str = "memory.resolve";

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
pub enum MemoryKind {
    Episode,
    Fact,
    Procedure,
    Decision,
}

#[derive(
    Clone,
    Debug,
    Eq,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
    phenix_sdk_macros::PhenixValue,
)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub enum MemoryScope {
    Global,
    Workspace { workspace_id: String },
    Session { session_id: SessionId },
    Agent { agent_id: String },
}

#[derive(
    Clone,
    Debug,
    Eq,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
    phenix_sdk_macros::PhenixValue,
)]
#[serde(deny_unknown_fields)]
pub struct MemorySourceReference {
    pub service: ServiceId,
    pub resource: String,
    pub start: Option<u64>,
    pub end: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct MemoryRecord {
    pub id: String,
    pub kind: MemoryKind,
    pub scope: MemoryScope,
    pub content: String,
    pub source_refs: Vec<MemorySourceReference>,
    pub supersedes: Vec<String>,
    pub valid_from: Option<u64>,
    pub valid_until: Option<u64>,
    pub created_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct MemoryNode {
    pub id: String,
    pub scope: MemoryScope,
    pub summary: String,
    pub children: Vec<String>,
    pub source_refs: Vec<MemorySourceReference>,
    pub created_at: u64,
    pub generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct MemoryRecallQuery {
    pub scopes: Vec<MemoryScope>,
    pub kinds: Vec<MemoryKind>,
    pub query: String,
    pub at: u64,
    pub limit: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct MemoryExtractionObservation {
    pub content: String,
    pub source_refs: Vec<MemorySourceReference>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct MemoryExtractionRequest {
    pub execution_id: String,
    pub parent_attempt_id: String,
    pub profile_id: RoutingProfileId,
    pub id: String,
    pub kind: MemoryKind,
    pub scope: MemoryScope,
    pub observations: Vec<MemoryExtractionObservation>,
    pub created_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct MemoryConsolidationRequest {
    pub execution_id: String,
    pub parent_attempt_id: String,
    pub profile_id: RoutingProfileId,
    pub ids: Vec<String>,
    pub consolidated_id: String,
    pub created_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct MemoryExpansion {
    pub node: MemoryNode,
    pub children: Vec<MemoryNode>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum MemoryCommand {
    Record {
        record: MemoryRecord,
    },
    RecordNode {
        node: MemoryNode,
    },
    Get {
        id: String,
    },
    GetFreshness {
        id: String,
    },
    GetNode {
        id: String,
    },
    Recall {
        query: MemoryRecallQuery,
    },
    Extract {
        request: MemoryExtractionRequest,
    },
    Consolidate {
        request: MemoryConsolidationRequest,
    },
    ObserveRevision {
        service: ServiceId,
        resource: String,
        revision: String,
        observed_at: u64,
        limit: u32,
    },
    ObserveConflict {
        source: MemorySourceReference,
        affected_ids: Vec<String>,
        observed_at: u64,
    },
    BindCanonicalReference {
        id: String,
        reference: MemoryCanonicalReference,
        observed_at: u64,
    },
    Revalidate {
        id: String,
        execution_id: String,
        parent_attempt_id: String,
        profile_id: RoutingProfileId,
        at: u64,
    },
    ExpandNode {
        id: String,
    },
    Promote {
        id: String,
        promoted_id: String,
        scope: MemoryScope,
        created_at: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum MemoryResponse {
    Record {
        record: MemoryRecord,
    },
    Freshness {
        state: Option<MemoryFreshnessRecord>,
    },
    Affected {
        memory_ids: Vec<String>,
    },
    Node {
        node: Option<MemoryNode>,
    },
    Memory {
        record: Option<MemoryRecord>,
    },
    Recall {
        records: Vec<MemoryRecord>,
    },
    Expansion {
        expansion: Option<MemoryExpansion>,
    },
}

pub struct MemoryInterface;

impl ComponentInterface for MemoryInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(MEMORY_SERVICE).expect("static memory interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<MemoryCommand, MemoryResponse>()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct MemoryEmbeddingRequest {
    pub inputs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct MemoryEmbeddingResponse {
    pub embeddings: Vec<Vec<f64>>,
}

pub struct MemoryEmbeddingInterface;

impl ComponentInterface for MemoryEmbeddingInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(MEMORY_EMBED_SERVICE)
            .expect("static memory embedding interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<MemoryEmbeddingRequest, MemoryEmbeddingResponse>()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct MemoryRankCandidate {
    pub id: String,
    pub content: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct MemoryRankRequest {
    pub query: String,
    pub candidates: Vec<MemoryRankCandidate>,
    pub limit: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct MemoryRankResponse {
    pub ids: Vec<String>,
}

pub struct MemoryRankInterface;

impl ComponentInterface for MemoryRankInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(MEMORY_RANK_SERVICE)
            .expect("static memory ranking interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<MemoryRankRequest, MemoryRankResponse>()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct CompactContextItem {
    pub id: String,
    pub content: String,
    pub source_refs: Vec<MemorySourceReference>,
    pub exact: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContextCompactionRequest {
    pub execution_id: String,
    pub parent_attempt_id: String,
    pub scope: MemoryScope,
    pub profile_id: RoutingProfileId,
    pub configuration_revision: String,
    pub target_tokens: u32,
    pub items: Vec<CompactContextItem>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContextCheckpoint {
    pub scope: MemoryScope,
    pub id: String,
    pub summary: String,
    pub summary_node_id: String,
    pub covered_refs: Vec<MemorySourceReference>,
    pub retained_exact_refs: Vec<MemorySourceReference>,
    pub configuration_revision: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextCompactionCommand {
    Compact { request: ContextCompactionRequest },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextCompactionResponse {
    Compacted { checkpoint: ContextCheckpoint },
}

pub struct ContextCompactionInterface;

impl ComponentInterface for ContextCompactionInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(CONTEXT_COMPACTION_SERVICE)
            .expect("static context compaction interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<ContextCompactionCommand, ContextCompactionResponse>()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextExpansionCommand {
    Expand {
        scope: MemoryScope,
        checkpoint_id: String,
        depth: u32,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextExpansionResponse {
    Expanded { items: Vec<CompactContextItem> },
}

pub struct ContextExpansionInterface;

impl ComponentInterface for ContextExpansionInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(CONTEXT_EXPANSION_SERVICE)
            .expect("static expansion interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<ContextExpansionCommand, ContextExpansionResponse>()
    }
}

#[must_use]
pub fn memory_service() -> ServiceId {
    ServiceId::parse(MEMORY_SERVICE).expect("static memory service id is valid")
}

#[must_use]
pub fn memory_embedding_service() -> ServiceId {
    ServiceId::parse(MEMORY_EMBED_SERVICE).expect("static memory embedding service id is valid")
}

#[must_use]
pub fn memory_rank_service() -> ServiceId {
    ServiceId::parse(MEMORY_RANK_SERVICE).expect("static memory ranking service id is valid")
}

#[must_use]
pub fn context_compaction_service() -> ServiceId {
    ServiceId::parse(CONTEXT_COMPACTION_SERVICE).expect("static compaction service id is valid")
}

#[must_use]
pub fn context_expansion_service() -> ServiceId {
    ServiceId::parse(CONTEXT_EXPANSION_SERVICE).expect("static expansion service id is valid")
}

#[must_use]
pub fn memory_summarize_callable() -> CallableId {
    CallableId::parse(MEMORY_SUMMARIZE_CALLABLE).expect("static memory callable id is valid")
}

#[must_use]
pub fn memory_extract_callable() -> CallableId {
    CallableId::parse(MEMORY_EXTRACT_CALLABLE).expect("static memory callable id is valid")
}

#[must_use]
pub fn memory_consolidate_callable() -> CallableId {
    CallableId::parse(MEMORY_CONSOLIDATE_CALLABLE).expect("static memory callable id is valid")
}

#[must_use]
pub fn memory_resolve_callable() -> CallableId {
    CallableId::parse(MEMORY_RESOLVE_CALLABLE).expect("static memory callable id is valid")
}
