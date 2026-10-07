use super::{
    CacheCompactionDecision, CacheCompactionDecisionRequest, CodeQuery, CodeQueryResult,
    CompactionCommit, CompactionProposal, ContinuationExportResult, ContinuationImportProjection,
    ContinuationImportRequest, ContinuationProjectionRequest, ProjectionRevision,
    context_admission::{ContextAdmissionRequest, ContextAdmissionResult, ContextCandidate},
};
use phenix_core::{
    Bytes, CallableId, ComponentInterface, ContextResourceId, ContextRevisionId, InterfaceId,
    RoutingProfileId, ServiceId, SessionId,
};
pub use phenix_core::{
    ContextDescriptor, ContextResourceKind, ContextResourceRevision, ContextScope,
};
use serde::{Deserialize, Serialize};

pub const CONTEXT_SERVICE: &str = "phenix.context@1";
pub const CONTEXT_RECOVERY_SERVICE: &str = "phenix.context-recovery@1";
pub const CONTEXT_IDENTIFY_NEEDS_CALLABLE: &str = "context.identify_needs";

#[derive(
    Clone,
    Debug,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Serialize,
    Deserialize,
    phenix_sdk_macros::PhenixValue,
)]
pub struct ExactContextReference {
    pub resource_id: ContextResourceId,
    pub revision: ContextRevisionId,
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum ContextInjectionRequester {
    User,
    Agent,
    Orchestration,
    ContextPolicy,
    Hook,
    Frontend,
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum ContextInjectionLifetime {
    Execution,
    Objective,
    Session,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct ContextInjection {
    pub sequence: u64,
    pub execution_id: String,
    pub source: ExactContextReference,
    pub requester: ContextInjectionRequester,
    pub lifetime: ContextInjectionLifetime,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct ProjectedContextEntry {
    pub injection: ContextInjection,
    pub resource: ContextResourceRevision,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct ExecutionContextProjection {
    pub execution_id: String,
    pub entries: Vec<ProjectedContextEntry>,
}

impl ExecutionContextProjection {
    /// Whether the projection contains context the user explicitly supplied.
    ///
    /// Recovery treats only user-requested exact resources as explicit context. Automatic
    /// baseline context such as mandatory skills and discovered project instructions does not
    /// suppress fallback memory recovery.
    #[must_use]
    pub fn has_user_explicit_resource(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.injection.requester == ContextInjectionRequester::User)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContextInvocationPreparation {
    pub request_input_tokens: u64,
    pub candidates: Vec<ContextCandidate>,
    pub projection: ProjectionRevision,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContextInvocationMaterialization {
    pub input: Bytes,
    pub projection: ProjectionRevision,
    /// Byte boundary immediately after the reusable context prefix and before this request.
    pub cache_prefix_bytes: u64,
    /// Deterministic identity of the model-facing context prefix before the current request.
    pub cache_prefix_identity: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct RepositoryContextSource {
    pub path: String,
    pub content: Bytes,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContextCodeQueryRequest {
    pub execution_id: String,
    pub scope: ContextScope,
    pub query: CodeQuery,
    pub requester: ContextInjectionRequester,
    pub lifetime: ContextInjectionLifetime,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum ContextCommand {
    Register {
        resource_id: ContextResourceId,
        kind: ContextResourceKind,
        source: String,
        scope: ContextScope,
        content: Bytes,
    },
    Get {
        resource_id: ContextResourceId,
        revision: ContextRevisionId,
    },
    List,
    DiscoverRepository {
        workspace_id: String,
        sources: Vec<RepositoryContextSource>,
    },
    LoadCodeQuery {
        request: ContextCodeQueryRequest,
    },
    Load {
        execution_id: String,
        resource_id: ContextResourceId,
        revision: ContextRevisionId,
        requester: ContextInjectionRequester,
        lifetime: ContextInjectionLifetime,
        reason: String,
    },
    LoadDelegatedResult {
        task_id: String,
    },
    AdmitDelegatedResult {
        task_id: String,
    },
    LoadOnce {
        admission_id: String,
        execution_id: String,
        resource_id: ContextResourceId,
        revision: ContextRevisionId,
        requester: ContextInjectionRequester,
        lifetime: ContextInjectionLifetime,
        reason: String,
    },
    Project {
        execution_id: String,
    },
    PrepareInvocation {
        execution_id: String,
        input: Bytes,
    },
    MaterializeInvocation {
        execution_id: String,
        input: Bytes,
        expected_projection: ProjectionRevision,
    },
    GetProjectionState {
        execution_id: String,
    },
    Admit {
        request: ContextAdmissionRequest,
    },
    EvaluateCompactionCost {
        request: CacheCompactionDecisionRequest,
    },
    PrepareCompaction {
        proposal: CompactionProposal,
    },
    CommitCompaction {
        execution_id: String,
        checkpoint_id: String,
    },
    InvalidateProjection {
        execution_id: String,
    },
    ExportContinuation {
        request: ContinuationProjectionRequest,
    },
    ProjectContinuationImport {
        request: ContinuationImportRequest,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case")]
pub enum ContextResponse {
    Registered {
        resource: ContextResourceRevision,
    },
    Resource {
        resource: Option<ContextResourceRevision>,
    },
    Resources {
        descriptors: Vec<ContextDescriptor>,
    },
    Discovered {
        descriptors: Vec<ContextDescriptor>,
    },
    CodeQueryLoaded {
        injection: ContextInjection,
        resource: ContextResourceRevision,
        result: CodeQueryResult,
    },
    Loaded {
        injection: ContextInjection,
        resource: ContextResourceRevision,
    },
    DelegatedResultAdmitted {
        injection: ContextInjection,
        resource: ContextResourceRevision,
        result: ContextAdmissionResult,
        projection: ProjectionRevision,
    },
    Projection {
        projection: ExecutionContextProjection,
    },
    InvocationPrepared {
        preparation: ContextInvocationPreparation,
    },
    InvocationMaterialized {
        materialization: ContextInvocationMaterialization,
    },
    ProjectionState {
        projection: ProjectionRevision,
    },
    Admission {
        result: ContextAdmissionResult,
        projection: ProjectionRevision,
    },
    CompactionCostDecision {
        decision: CacheCompactionDecision,
    },
    CompactionPrepared {
        checkpoint_id: String,
        projection: ProjectionRevision,
    },
    CompactionCommitted {
        commit: CompactionCommit,
    },
    ProjectionInvalidated {
        projection: ProjectionRevision,
    },
    ContinuationExported {
        result: ContinuationExportResult,
    },
    ContinuationImportProjected {
        projection: ContinuationImportProjection,
    },
}

pub struct ContextInterface;

impl ComponentInterface for ContextInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(CONTEXT_SERVICE).expect("static context interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<ContextCommand, ContextResponse>()
    }
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
#[serde(tag = "anchor", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextAnchor {
    Workspace {
        workspace_id: String,
    },
    Repository {
        canonical_remote: String,
        workspace_id: Option<String>,
    },
    Path {
        path: String,
    },
    Project {
        key: String,
    },
    Task {
        key: String,
    },
    Session {
        session_id: SessionId,
    },
    Resource {
        service: ServiceId,
        resource: String,
    },
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
#[serde(tag = "need", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextNeed {
    Workspace {
        query: String,
    },
    Repository {
        query: String,
    },
    Project {
        query: String,
    },
    Task {
        query: String,
    },
    Session {
        query: String,
    },
    Resource {
        service: Option<ServiceId>,
        query: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContextRecoveryState {
    pub anchors: Vec<ContextAnchor>,
    pub has_durable_session_history: bool,
    pub has_explicit_resource: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContextRecoveryRequest {
    pub profile_id: RoutingProfileId,
    pub prompt: String,
    pub state: ContextRecoveryState,
    pub at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "decision", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextRecoveryDecision {
    Sufficient,
    Missing { needs: Vec<ContextNeed> },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextRecoveryCommand {
    Assess { request: ContextRecoveryRequest },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextRecoveryResponse {
    Decision { decision: ContextRecoveryDecision },
}

pub struct ContextRecoveryInterface;

impl ComponentInterface for ContextRecoveryInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(CONTEXT_RECOVERY_SERVICE)
            .expect("static context recovery interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<ContextRecoveryCommand, ContextRecoveryResponse>()
    }
}

#[must_use]
pub fn context_service() -> ServiceId {
    ServiceId::parse(CONTEXT_SERVICE).expect("static context service id is valid")
}

#[must_use]
pub fn context_recovery_service() -> ServiceId {
    ServiceId::parse(CONTEXT_RECOVERY_SERVICE).expect("static context recovery service id is valid")
}

#[must_use]
pub fn context_identify_needs_callable() -> CallableId {
    CallableId::parse(CONTEXT_IDENTIFY_NEEDS_CALLABLE)
        .expect("static context recovery callable id is valid")
}

#[cfg(test)]
mod recovery_projection_tests {
    use super::*;
    use phenix_core::{ContextResourceId, ContextRevisionId};

    fn entry(requester: ContextInjectionRequester) -> ProjectedContextEntry {
        let resource_id = ContextResourceId::parse("skill:write").unwrap();
        let revision = ContextRevisionId::parse("revision").unwrap();
        ProjectedContextEntry {
            injection: ContextInjection {
                sequence: 1,
                execution_id: "execution-1".into(),
                source: ExactContextReference {
                    resource_id: resource_id.clone(),
                    revision: revision.clone(),
                },
                requester,
                lifetime: ContextInjectionLifetime::Execution,
                reason: "test".into(),
            },
            resource: ContextResourceRevision {
                descriptor: ContextDescriptor {
                    resource_id,
                    revision,
                    kind: ContextResourceKind::Skill,
                    source: "test".into(),
                    scope: ContextScope::Workspace,
                    content_identity: "test".into(),
                    estimated_bytes: 1,
                },
                content: b"x".to_vec().into(),
            },
        }
    }

    #[test]
    fn recovery_explicit_resource_ignores_context_policy_baseline() {
        let projection = ExecutionContextProjection {
            execution_id: "execution-1".into(),
            entries: vec![entry(ContextInjectionRequester::ContextPolicy)],
        };
        assert!(!projection.has_user_explicit_resource());
    }

    #[test]
    fn recovery_explicit_resource_accepts_user_exact_resource() {
        let projection = ExecutionContextProjection {
            execution_id: "execution-1".into(),
            entries: vec![entry(ContextInjectionRequester::User)],
        };
        assert!(projection.has_user_explicit_resource());
    }
}
