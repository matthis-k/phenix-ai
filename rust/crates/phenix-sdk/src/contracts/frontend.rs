use phenix_core::{ComponentInterface, InterfaceId, InterfaceSchema, PhenixValue};
use phenix_sdk_macros::PhenixValue;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const FRONTEND_SERVICE: &str = "phenix.frontend-services@1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
pub struct FrontendProviderDescriptor {
    pub id: String,
    #[serde(default)]
    pub capabilities: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
pub struct LiveFrontendProvider {
    pub connection_id: String,
    pub descriptor: FrontendProviderDescriptor,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, PhenixValue)]
pub struct FrontendServiceRequest {
    pub correlation_id: u64,
    pub connection_id: String,
    pub provider: String,
    pub method: String,
    pub params: PhenixValue,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, PhenixValue)]
pub struct FrontendServiceResult {
    pub correlation_id: u64,
    pub result: PhenixValue,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum FrontendCommand {
    SetProviders {
        connection_id: String,
        providers: Vec<FrontendProviderDescriptor>,
    },
    Disconnect {
        connection_id: String,
    },
    Catalog,
    BindRoot {
        execution_id: String,
        connection_id: String,
    },
    ReleaseRoot {
        execution_id: String,
    },
    BeginExecutionCall {
        execution_id: String,
        provider: String,
        method: String,
        params: PhenixValue,
    },
    /// Capability discovery is scoped to the active execution's root owner.
    CheckExecutionCapabilities {
        execution_id: String,
        provider: String,
        required_capabilities: BTreeSet<String>,
    },
    /// Atomically checks requirements against the current owner advertisement.
    BeginExecutionCallWithRequirements {
        execution_id: String,
        provider: String,
        method: String,
        params: PhenixValue,
        required_capabilities: BTreeSet<String>,
    },
    BeginDirectCall {
        connection_id: String,
        provider: String,
        method: String,
        params: PhenixValue,
    },
    CompleteCall {
        connection_id: String,
        correlation_id: u64,
        result: PhenixValue,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case")]
pub enum FrontendResponse {
    Providers {
        providers: Vec<LiveFrontendProvider>,
    },
    Request {
        request: FrontendServiceRequest,
    },
    CapabilityCheck {
        supported: bool,
    },
    Result {
        result: FrontendServiceResult,
    },
    Updated,
}

pub struct FrontendInterface;

impl ComponentInterface for FrontendInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(FRONTEND_SERVICE).expect("static frontend interface id is valid")
    }

    fn schema() -> InterfaceSchema {
        InterfaceSchema::of::<FrontendCommand, FrontendResponse>()
    }
}
