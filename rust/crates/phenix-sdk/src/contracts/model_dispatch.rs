use super::{ModelInferenceResponse, RouteDecision};
use phenix_core::{
    Bytes, ComponentInterface, InterfaceId, ModelInferenceFailure, ModelToolDescriptor,
    ModelToolTurn, ServiceId,
};
use serde::{Deserialize, Serialize};

pub const MODEL_DISPATCH_SERVICE: &str = "phenix.models.dispatch@1";

/// A model request whose local routing, authentication, and encoding checks
/// already succeeded. Consuming it is the provider-boundary operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct PreparedDispatch {
    decision: RouteDecision,
    request: Bytes,
}

impl PreparedDispatch {
    #[must_use]
    pub fn new(decision: RouteDecision, request: Bytes) -> Self {
        Self { decision, request }
    }

    #[must_use]
    pub fn decision(&self) -> &RouteDecision {
        &self.decision
    }

    #[must_use]
    pub fn into_parts(self) -> (RouteDecision, Bytes) {
        (self.decision, self.request)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelDispatchCommand {
    PrepareResolved {
        decision: RouteDecision,
        input: Bytes,
        #[serde(default)]
        tools: Vec<ModelToolDescriptor>,
        #[serde(default)]
        continuation: Vec<ModelToolTurn>,
    },
    InvokePrepared {
        prepared: PreparedDispatch,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelDispatchResponse {
    Ready {
        prepared: PreparedDispatch,
    },
    Inference {
        decision: RouteDecision,
        response: ModelInferenceResponse,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ModelDispatchFailure {
    pub failure: ModelInferenceFailure,
}

impl ModelDispatchFailure {
    #[must_use]
    pub fn retryable(&self) -> bool {
        self.failure.retryable()
    }

    #[must_use]
    pub fn requires_context_recovery(&self) -> bool {
        self.failure.requires_context_recovery()
    }
}

pub struct ModelDispatchInterface;

impl ComponentInterface for ModelDispatchInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(MODEL_DISPATCH_SERVICE)
            .expect("static model dispatch interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::fallible_of::<
            ModelDispatchCommand,
            ModelDispatchResponse,
            ModelDispatchFailure,
        >()
    }
}

#[must_use]
pub fn model_dispatch_service() -> ServiceId {
    ServiceId::parse(MODEL_DISPATCH_SERVICE).expect("static model dispatch service id is valid")
}
