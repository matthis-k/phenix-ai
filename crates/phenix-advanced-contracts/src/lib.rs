//! P2 reference and provenance contracts, without a kernel or agent dependency.
//! Source references are claims about evidence, never serialized authority.
use phenix_sdk::*;
use serde::{Deserialize, Serialize};

pub mod compaction;
pub use compaction::*;
pub mod compaction_summary;
pub use compaction_summary::*;
pub mod model_tokens;
pub use model_tokens::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum EvidenceScopeKind {
    Session,
    Project,
    User,
    Shared,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct EvidenceScope {
    pub kind: EvidenceScopeKind,
    pub id: String,
}

impl EvidenceScope {
    /// Identity comparison only. Runtime authorization must be checked
    /// against the caller's real host grants by the selected provider.
    pub fn same_scope(&self, other: &Self) -> bool {
        self == other
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct SourceRef {
    pub owner: String,
    pub resource: String,
    pub revision: String,
    /// Lowercase, hexadecimal SHA-256 of the exact source revision.
    pub digest_sha256: String,
    pub range_start: u64,
    pub range_end: u64,
    pub scope: EvidenceScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct SourceObservation {
    pub owner: String,
    pub resource: String,
    pub revision: String,
    pub digest_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum SourceMatch {
    Current,
    Changed,
    Unrelated,
}

pub fn compare_source(reference: &SourceRef, actual: &SourceObservation) -> SourceMatch {
    if reference.owner != actual.owner || reference.resource != actual.resource {
        return SourceMatch::Unrelated;
    }
    if reference.revision != actual.revision || reference.digest_sha256 != actual.digest_sha256 {
        return SourceMatch::Changed;
    }
    SourceMatch::Current
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum EvidenceKind {
    Fact,
    Event,
    Preference,
    Task,
    CodeObservation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum EvidenceFreshness {
    Current,
    NeedsValidation,
    Historical,
    Withdrawn,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub id: String,
    pub scope: EvidenceScope,
    pub kind: EvidenceKind,
    pub statement: String,
    pub sources: Vec<SourceRef>,
    pub revision: u64,
    pub observed_at: u64,
    pub valid_from: u64,
    pub valid_until: Option<u64>,
    pub confidence_basis_points: u64,
    pub freshness: EvidenceFreshness,
    pub supersedes: Vec<String>,
}

impl EvidenceRecord {
    /// Historical as-of selection is independent of the current projection's
    /// freshness: an archived, supported fact can still be valid in the past.
    pub fn valid_at(&self, timestamp: u64) -> bool {
        timestamp >= self.valid_from
            && self.valid_until.is_none_or(|end| timestamp < end)
            && self.freshness != EvidenceFreshness::Withdrawn
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum EvidenceProblem {
    InvalidIdentity,
    InvalidDigest,
    InvalidRange,
    MissingSource,
    DuplicateSource,
    InvalidValidity,
    InvalidConfidence,
    SelfSupersession,
    NotFound,
    Conflict,
    ScopeDenied,
    SourceChanged,
    StorageFailed,
    CapacityExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct EvidenceError {
    pub problem: EvidenceProblem,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct EvidenceKey {
    pub id: String,
    pub scope: EvidenceScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct EvidenceSearch {
    pub scopes: Vec<EvidenceScope>,
    pub query: String,
    pub as_of: Option<u64>,
    pub limit: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct EvidencePage {
    pub records: Vec<EvidenceRecord>,
    pub next: Option<String>,
}

fn valid_identity(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 512 && !s.chars().any(char::is_control)
}

fn valid_digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn validate_reference(source: &SourceRef) -> Result<(), EvidenceProblem> {
    if !valid_identity(&source.owner)
        || !valid_identity(&source.resource)
        || !valid_identity(&source.revision)
        || !valid_identity(&source.scope.id)
    {
        return Err(EvidenceProblem::InvalidIdentity);
    }
    if !valid_digest(&source.digest_sha256) {
        return Err(EvidenceProblem::InvalidDigest);
    }
    if source.range_start > source.range_end {
        return Err(EvidenceProblem::InvalidRange);
    }
    Ok(())
}

pub fn validate_record(record: &EvidenceRecord) -> Result<(), EvidenceProblem> {
    if !valid_identity(&record.id)
        || !valid_identity(&record.scope.id)
        || !valid_identity(&record.statement)
        || record.revision == 0
    {
        return Err(EvidenceProblem::InvalidIdentity);
    }
    if record.sources.is_empty() {
        return Err(EvidenceProblem::MissingSource);
    }
    if record.sources.len() > 256 {
        return Err(EvidenceProblem::InvalidRange);
    }
    for (i, source) in record.sources.iter().enumerate() {
        validate_reference(source)?;
        if record.sources[..i].contains(source) {
            return Err(EvidenceProblem::DuplicateSource);
        }
    }
    if record.confidence_basis_points > 10_000 {
        return Err(EvidenceProblem::InvalidConfidence);
    }
    if record
        .valid_until
        .is_some_and(|end| end <= record.valid_from)
    {
        return Err(EvidenceProblem::InvalidValidity);
    }
    if record.supersedes.iter().any(|id| id == &record.id) {
        return Err(EvidenceProblem::SelfSupersession);
    }
    if record.supersedes.iter().any(|id| !valid_identity(id)) {
        return Err(EvidenceProblem::InvalidIdentity);
    }
    Ok(())
}

pub fn encode<T: Portable>(value: &T) -> Result<Blob, Failure> {
    Ok(Blob(value.to_value().encode()?))
}

pub fn decode<T: Portable>(value: &Blob) -> Result<T, Failure> {
    T::from_value(Value::decode(&value.0)?)
}

/// Project identity is a resource selected by a trusted host admission,
/// not a capability carried by a caller-provided project string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum ProjectStorageProblem {
    NotFound,
    Conflict,
    Denied,
    InvalidInput,
    StorageFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ProjectStorageError {
    pub problem: ProjectStorageProblem,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ProjectRecordKey {
    pub project: String,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ProjectRecord {
    pub revision: u64,
    pub data: Blob,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ProjectEdit {
    pub key: String,
    pub expected: u64,
    pub data: Blob,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ProjectTransaction {
    pub project: String,
    pub edits: Vec<ProjectEdit>,
}

/// Stable project membership is stored by a provider. A session ID or
/// project ID passed by a caller never grants membership on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum ProjectMembershipProblem {
    Conflict,
    Denied,
    InvalidInput,
    StorageFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ProjectMembershipError {
    pub problem: ProjectMembershipProblem,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ProjectMemberKey {
    pub project: String,
    pub session: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ProjectMemberChange {
    pub project: String,
    pub session: String,
    pub expected: u64,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ProjectMember {
    pub project: String,
    pub session: String,
    pub revision: u64,
    pub active: bool,
}

#[contract(id = "phenix.advanced.project-membership@1")]
pub trait ProjectMembership {
    /// Only a host-authorized project root may grant or revoke a session.
    fn set(&self, change: ProjectMemberChange) -> Result<ProjectMember, ProjectMembershipError>;
    /// Only the authenticated session root may inspect its own membership.
    /// A missing or revoked membership is denied.
    fn get(&self, key: ProjectMemberKey) -> Result<ProjectMember, ProjectMembershipError>;
}

#[contract(id = "phenix.advanced.project-storage@1")]
pub trait ProjectStorage {
    fn read(&self, key: ProjectRecordKey) -> Result<ProjectRecord, ProjectStorageError>;
    fn transact(&self, edit: ProjectTransaction)
        -> Result<Vec<ProjectRecord>, ProjectStorageError>;
}

#[contract(id = "phenix.advanced.source@1")]
pub trait SourceEvidence {
    fn observe(&self, reference: SourceRef) -> Result<SourceObservation, EvidenceError>;
}

#[contract(id = "phenix.advanced.evidence@1")]
pub trait EvidenceStore {
    fn put(&self, record: EvidenceRecord) -> Result<EvidenceRecord, EvidenceError>;
    fn get(&self, key: EvidenceKey) -> Result<EvidenceRecord, EvidenceError>;
    fn search(&self, request: EvidenceSearch) -> Result<EvidencePage, EvidenceError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct CheckpointKey {
    pub session: String,
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum CheckpointStoreProblem {
    InvalidCheckpoint,
    Conflict,
    NotFound,
    ScopeDenied,
    SourceChanged,
    StorageFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct CheckpointStoreError {
    pub problem: CheckpointStoreProblem,
    pub message: String,
}

/// A versioned metadata ledger. Checkpoint creation and lookup revalidate the
/// Basic journal; the journal stays authoritative and no Model view is changed.
#[contract(id = "phenix.advanced.compaction-checkpoints@1")]
pub trait CompactionStore {
    fn put(
        &self,
        checkpoint: CompactionCheckpoint,
    ) -> Result<CompactionCheckpoint, CheckpointStoreError>;
    fn get(&self, key: CheckpointKey) -> Result<CompactionCheckpoint, CheckpointStoreError>;
    /// Return the highest persisted revision for a session only after
    /// independently verifying its canonical Basic journal source bytes.
    /// An absent ledger is NotFound; erasure is SourceChanged, not fallback.
    fn latest(
        &self,
        scope: phenix_basic_contracts::Scope,
    ) -> Result<CompactionCheckpoint, CheckpointStoreError>;
}

const GRAPH: StaticGraphContribution = StaticGraphContribution {
    plugin: "phenix.advanced-contracts",
    events: &[],
    actions: &[],
    nodes: &[],
};

#[plugin(
    id = "phenix.advanced-contracts",
    version = "1.0.0",
    profile = "definitions",
    define(SourceEvidence),
    define(EvidenceStore),
    define(ProjectStorage),
    define(ProjectMembership),
    define(CompactionStore),
    define(CompactionSummarizer),
    define(ModelTokenMeter),
    graph(super::GRAPH)
)]
pub mod definitions {
    use super::*;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definition_package_remains_inert_and_separate_from_basic() {
        let metadata = ContractMetadata::decode(definitions::METADATA.bytes()).unwrap();
        assert_eq!(metadata.execution, MetadataExecution::None);
        assert_eq!(metadata.definitions.len(), 6);
        let graph = GraphContribution::decode(definitions::GRAPH_METADATA.bytes()).unwrap();
        assert!(graph.nodes.is_empty());
        assert!(graph.events.is_empty());
        assert!(graph.actions.is_empty());
    }
}
