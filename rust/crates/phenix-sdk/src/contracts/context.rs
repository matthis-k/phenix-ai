use super::{
    CompactionCommit, CompactionProposal, ContextAdmissionRequest, ContextAdmissionResult,
    ContextDescriptor, ContextInjection, ContextResource, ContextResourceRevision,
    ExecutionContextProjection, ProjectionRevision,
};
use phenix_core::{ComponentInterface, InterfaceId, ServiceId};
use serde::{Deserialize, Serialize};

pub const CONTEXT_SERVICE: &str = "phenix.context@1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum ContextCommand {
    Register {
        resource: ContextResource,
    },
    Get {
        id: String,
    },
    List,
    Discover {
        query: String,
        limit: u32,
    },
    Load {
        id: String,
    },
    GetProjection {
        execution_id: String,
    },
    Admit {
        request: ContextAdmissionRequest,
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
    Loaded {
        injection: ContextInjection,
        resource: ContextResourceRevision,
    },
    Projection {
        projection: ExecutionContextProjection,
    },
    Admission {
        result: ContextAdmissionResult,
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
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum ContextProjectionState {
    Active,
    Stale,
}
