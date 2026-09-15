use super::usage::{CapacityKnowledge, ContextDemand, EffectiveModelCapabilities, ModelTurnUsage};
pub use phenix_core::{
    model_inference_service, ModelInferenceInterface, ModelInferenceRequest,
    ModelInferenceResponse, MODEL_INFERENCE_SERVICE,
};
use phenix_core::{
    Bytes, CallableId, CapabilityGenerationId, ComponentInterface, InterfaceId, ModelId,
    ModelToolDescriptor, PhenixValue, PluginId, RoutingProfileId, ServiceId,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MODEL_ROUTING_SERVICE: &str = "phenix.models.routing@1";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct ModelTarget {
    pub provider_plugin: PluginId,
    pub model: ModelId,
    #[serde(default)]
    pub options: BTreeMap<String, PhenixValue>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct RoutingProfile {
    pub id: RoutingProfileId,
    pub default_target: ModelTarget,
    #[serde(default)]
    pub callable_targets: BTreeMap<CallableId, ModelTarget>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct RoutingProfileDescriptor {
    pub id: RoutingProfileId,
    pub providers: Vec<PluginId>,
}

#[derive(
    Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(deny_unknown_fields)]
pub struct RoutingRequirements {
    pub context: ContextDemand,
    #[serde(default)]
    pub required_capabilities: BTreeSet<String>,
    #[serde(default)]
    pub require_known_capacity: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "eligibility", rename_all = "snake_case", deny_unknown_fields)]
pub enum RouteEligibility {
    Eligible,
    MissingCapabilities { capabilities: Vec<String> },
    InsufficientContextCapacity,
    UnknownContextCapacity,
}

impl RoutingRequirements {
    #[must_use]
    pub fn evaluate(&self, capabilities: &EffectiveModelCapabilities) -> RouteEligibility {
        let missing: Vec<String> = self
            .required_capabilities
            .union(&self.context.required_capabilities)
            .filter(|required| !capabilities.optional.contains(*required))
            .cloned()
            .collect();
        if !missing.is_empty() {
            return RouteEligibility::MissingCapabilities {
                capabilities: missing,
            };
        }

        let required_context = self
            .context
            .total_input_tokens()
            .saturating_add(self.context.output_reserve_tokens);
        match &capabilities.capacity {
            CapacityKnowledge::Known { limits }
            | CapacityKnowledge::ConfiguredConservative { limits } => {
                if limits.context_window_tokens < required_context
                    || limits
                        .max_output_tokens
                        .is_some_and(|limit| limit < self.context.output_reserve_tokens)
                {
                    RouteEligibility::InsufficientContextCapacity
                } else {
                    RouteEligibility::Eligible
                }
            }
            CapacityKnowledge::Unknown if self.require_known_capacity => {
                RouteEligibility::UnknownContextCapacity
            }
            CapacityKnowledge::Unknown => RouteEligibility::Eligible,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(rename_all = "snake_case")]
pub enum RoutingEstimateSource {
    Static,
    Historical,
    Learned,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct RoutingEstimate {
    pub source: RoutingEstimateSource,
    pub expected_quality_millis: Option<u32>,
    pub expected_latency_ms: Option<u64>,
    pub expected_cost_microunits: Option<u64>,
    pub confidence_millis: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct RoutingCandidate {
    pub capabilities: EffectiveModelCapabilities,
    pub estimate: Option<RoutingEstimate>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct RouteDecision {
    pub target: ModelTarget,
    pub capability_generation: CapabilityGenerationId,
    pub policy_revision: String,
    pub estimate: Option<RoutingEstimate>,
}

#[derive(
    Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(deny_unknown_fields)]
pub struct RoutingEvidence {
    pub success: bool,
    pub latency_ms: Option<u64>,
    pub usage: ModelTurnUsage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum ModelCommand {
    RegisterProfile {
        profile: RoutingProfile,
    },
    GetProfile {
        id: RoutingProfileId,
    },
    ListProfiles,
    SetProviderAuthenticated {
        provider_plugin: PluginId,
        authenticated: bool,
    },
    Resolve {
        profile_id: RoutingProfileId,
        callable_id: Option<CallableId>,
    },
    Invoke {
        profile_id: RoutingProfileId,
        callable_id: Option<CallableId>,
        input: Bytes,
        #[serde(default)]
        tools: Vec<ModelToolDescriptor>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModelResponse {
    Profile {
        profile: Option<RoutingProfile>,
    },
    Profiles {
        profiles: Vec<RoutingProfileDescriptor>,
    },
    Authentication {
        provider_plugin: PluginId,
        authenticated: bool,
    },
    Target {
        target: ModelTarget,
    },
    Inference {
        target: ModelTarget,
        response: ModelInferenceResponse,
    },
}

pub struct ModelRoutingInterface;

impl ComponentInterface for ModelRoutingInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(MODEL_ROUTING_SERVICE)
            .expect("static model routing interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<ModelCommand, ModelResponse>()
    }
}

#[must_use]
pub fn model_routing_service() -> ServiceId {
    ServiceId::parse(MODEL_ROUTING_SERVICE).expect("static model routing service id is valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{ContextControl, ModelLimits};

    fn capabilities(capacity: CapacityKnowledge) -> EffectiveModelCapabilities {
        EffectiveModelCapabilities {
            target: ModelTarget {
                provider_plugin: PluginId::parse("provider.fixture").unwrap(),
                model: ModelId::parse("model.fixture").unwrap(),
                options: BTreeMap::new(),
            },
            generation: CapabilityGenerationId::parse("generation-1").unwrap(),
            context: ContextControl::ReplaceableTurns,
            capacity,
            optional: BTreeSet::new(),
        }
    }

    #[test]
    fn hard_capacity_is_checked_before_ranking() {
        let requirements = RoutingRequirements {
            context: ContextDemand {
                mandatory_input_tokens: 800,
                reducible_input_tokens: 100,
                output_reserve_tokens: 200,
                required_capabilities: BTreeSet::new(),
            },
            required_capabilities: BTreeSet::new(),
            require_known_capacity: true,
        };
        assert_eq!(
            requirements.evaluate(&capabilities(CapacityKnowledge::Known {
                limits: ModelLimits {
                    context_window_tokens: 1_000,
                    max_output_tokens: Some(500),
                },
            })),
            RouteEligibility::InsufficientContextCapacity
        );
    }

    #[test]
    fn unknown_capacity_is_only_rejected_when_policy_requires_knowledge() {
        let mut requirements = RoutingRequirements::default();
        assert_eq!(
            requirements.evaluate(&capabilities(CapacityKnowledge::Unknown)),
            RouteEligibility::Eligible
        );
        requirements.require_known_capacity = true;
        assert_eq!(
            requirements.evaluate(&capabilities(CapacityKnowledge::Unknown)),
            RouteEligibility::UnknownContextCapacity
        );
    }
}
