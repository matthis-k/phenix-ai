use super::{ModelInferenceResponse, RouteDecision};
use phenix_core::{Bytes, ComponentInterface, InterfaceId, ModelToolDescriptor, ServiceId};
use serde::{Deserialize, Serialize};

pub const MODEL_DISPATCH_SERVICE: &str = "phenix.models.dispatch@1";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelDispatchCommand {
    InvokeResolved {
        decision: RouteDecision,
        input: Bytes,
        #[serde(default)]
        tools: Vec<ModelToolDescriptor>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelDispatchResponse {
    Inference {
        decision: RouteDecision,
        response: ModelInferenceResponse,
    },
}

pub struct ModelDispatchInterface;

impl ComponentInterface for ModelDispatchInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(MODEL_DISPATCH_SERVICE)
            .expect("static model dispatch interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<ModelDispatchCommand, ModelDispatchResponse>()
    }
}

#[must_use]
pub fn model_dispatch_service() -> ServiceId {
    ServiceId::parse(MODEL_DISPATCH_SERVICE).expect("static model dispatch service id is valid")
}
