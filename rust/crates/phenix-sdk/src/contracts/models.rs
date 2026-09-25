use super::usage::{CapacityKnowledge, ContextDemand, EffectiveModelCapabilities, ModelTurnUsage};
pub use phenix_core::{
    model_inference_service, ModelCacheControl, ModelCacheRetention, ModelCacheWritePolicy,
    ModelInferenceInterface, ModelInferenceRequest, ModelInferenceResponse,
    MODEL_INFERENCE_SERVICE,
};
use phenix_core::{
    CallableId, CapabilityGenerationId, ComponentInterface, EventTypeId, InterfaceId, ModelId,
    PhenixValue, PluginId, PreparedMutationHandle, RoutingProfileId, ServiceId,
};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

pub const MODEL_ROUTING_SERVICE: &str = "phenix.models.routing@1";

pub const MODEL_DIAGNOSTIC_EVENT: &str = "phenix.models.diagnostic";
pub const MODEL_DIAGNOSTIC_EVENT_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ModelDiagnosticEvent {
    AuthenticationChanged {
        provider_plugin: String,
        authenticated: bool,
        authenticated_providers: Vec<String>,
    },
    RoutingDecision {
        profile_id: String,
        callable_id: Option<String>,
        provider_plugin: String,
        model: String,
        candidate_ordinal: u32,
        rejected_candidates: usize,
    },
    DispatchPreflight {
        provider_plugin: String,
        model: String,
        authenticated: bool,
        authenticated_providers: Vec<String>,
        candidate_ordinal: u32,
        policy_revision: String,
        capability_generation: String,
        input_bytes: usize,
        tool_count: usize,
        continuation_turns: usize,
    },
    DispatchPreflightRejected {
        provider_plugin: String,
        model: String,
        authenticated: bool,
        authenticated_providers: Vec<String>,
        reason: String,
    },
    DispatchPrepared {
        provider_plugin: String,
        model: String,
        request_bytes: usize,
        requested_cache: ModelCacheControl,
        effective_cache: ModelCacheControl,
    },
    DispatchInvocationStarted {
        provider_plugin: String,
        model: String,
        request_bytes: usize,
    },
    DispatchInvocationSucceeded {
        provider_plugin: String,
        model: String,
    },
    DispatchInvocationFailed {
        provider_plugin: String,
        model: String,
        reason: String,
    },
}

#[must_use]
pub fn model_diagnostic_event_type() -> EventTypeId {
    EventTypeId::parse(MODEL_DIAGNOSTIC_EVENT).expect("static model diagnostic event id is valid")
}

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
    pub fallback_targets: Vec<ModelTarget>,
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
    #[serde(default)]
    pub estimator_snapshot_revision: Option<String>,
    #[serde(default)]
    pub evidence_cutoff_sequence: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct RoutingCandidate {
    pub capabilities: EffectiveModelCapabilities,
    pub estimate: Option<RoutingEstimate>,
    pub ordinal: u32,
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoutingEstimateMode {
    Ignore,
    PreferTrusted { min_confidence_millis: u16 },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct RouteSelectionPolicy {
    pub revision: String,
    pub estimates: RoutingEstimateMode,
    pub max_candidate_attempts: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct RouteDecision {
    pub target: ModelTarget,
    pub capability_generation: CapabilityGenerationId,
    pub policy_revision: String,
    pub candidate_ordinal: u32,
    pub estimate: Option<RoutingEstimate>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct RejectedRoutingCandidate {
    pub target: ModelTarget,
    pub ordinal: u32,
    pub reason: RouteEligibility,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct RouteSelection {
    pub decision: RouteDecision,
    #[serde(default)]
    pub rejected: Vec<RejectedRoutingCandidate>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum RouteSelectionError {
    NoEligibleCandidates {
        rejected: Vec<RejectedRoutingCandidate>,
    },
    CandidateAttemptLimitExceeded {
        eligible: u32,
        allowed: u32,
    },
}

#[derive(
    Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(deny_unknown_fields)]
pub struct RoutingEvidence {
    pub success: bool,
    pub latency_ms: Option<u64>,
    pub cost_microunits: Option<u64>,
    pub usage: ModelTurnUsage,
}

#[must_use]
pub fn select_route(
    candidates: &[RoutingCandidate],
    requirements: &RoutingRequirements,
    policy: &RouteSelectionPolicy,
) -> Result<RouteSelection, RouteSelectionError> {
    let mut rejected = Vec::new();
    let mut eligible: Vec<&RoutingCandidate> = Vec::new();

    for candidate in candidates {
        match requirements.evaluate(&candidate.capabilities) {
            RouteEligibility::Eligible => eligible.push(candidate),
            reason => rejected.push(RejectedRoutingCandidate {
                target: candidate.capabilities.target.clone(),
                ordinal: candidate.ordinal,
                reason,
            }),
        }
    }

    if eligible.is_empty() {
        return Err(RouteSelectionError::NoEligibleCandidates { rejected });
    }

    let eligible_count = u32::try_from(eligible.len()).unwrap_or(u32::MAX);
    if eligible_count > policy.max_candidate_attempts {
        eligible.sort_by_key(|candidate| candidate.ordinal);
        eligible.truncate(policy.max_candidate_attempts as usize);
        if eligible.is_empty() {
            return Err(RouteSelectionError::CandidateAttemptLimitExceeded {
                eligible: eligible_count,
                allowed: policy.max_candidate_attempts,
            });
        }
    }

    eligible.sort_by(|left, right| compare_candidates(left, right, policy.estimates));
    let winner = eligible[0];
    Ok(RouteSelection {
        decision: RouteDecision {
            target: winner.capabilities.target.clone(),
            capability_generation: winner.capabilities.generation.clone(),
            policy_revision: policy.revision.clone(),
            candidate_ordinal: winner.ordinal,
            estimate: winner.estimate.clone(),
        },
        rejected,
    })
}

fn compare_candidates(
    left: &RoutingCandidate,
    right: &RoutingCandidate,
    mode: RoutingEstimateMode,
) -> Ordering {
    match mode {
        RoutingEstimateMode::Ignore => left.ordinal.cmp(&right.ordinal),
        RoutingEstimateMode::PreferTrusted {
            min_confidence_millis,
        } => {
            let left_estimate = trusted_estimate(left, min_confidence_millis);
            let right_estimate = trusted_estimate(right, min_confidence_millis);
            match (left_estimate, right_estimate) {
                (Some(left_estimate), Some(right_estimate)) => right_estimate
                    .expected_quality_millis
                    .unwrap_or(0)
                    .cmp(&left_estimate.expected_quality_millis.unwrap_or(0))
                    .then_with(|| {
                        left_estimate
                            .expected_cost_microunits
                            .unwrap_or(u64::MAX)
                            .cmp(&right_estimate.expected_cost_microunits.unwrap_or(u64::MAX))
                    })
                    .then_with(|| {
                        left_estimate
                            .expected_latency_ms
                            .unwrap_or(u64::MAX)
                            .cmp(&right_estimate.expected_latency_ms.unwrap_or(u64::MAX))
                    })
                    .then_with(|| left.ordinal.cmp(&right.ordinal)),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => left.ordinal.cmp(&right.ordinal),
            }
        }
    }
}

fn trusted_estimate(candidate: &RoutingCandidate, minimum: u16) -> Option<&RoutingEstimate> {
    candidate
        .estimate
        .as_ref()
        .filter(|estimate| estimate.confidence_millis.unwrap_or(0) >= minimum)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum ModelCommand {
    PreparePackagedProfiles {
        profiles: Vec<RoutingProfile>,
    },
    RegisterProfile {
        profile: RoutingProfile,
    },
    ReplaceProfile {
        expected: RoutingProfile,
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
    PublishCapabilities {
        capabilities: EffectiveModelCapabilities,
    },
    ListCandidates {
        profile_id: RoutingProfileId,
        callable_id: Option<CallableId>,
    },
    ResolveWithRequirements {
        profile_id: RoutingProfileId,
        callable_id: Option<CallableId>,
        requirements: RoutingRequirements,
        policy: RouteSelectionPolicy,
    },
    RecordEvidence {
        decision: RouteDecision,
        evidence: RoutingEvidence,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModelResponse {
    PreparedProfiles {
        mutation: PreparedMutationHandle,
        profiles: Vec<RoutingProfile>,
    },
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
    Capabilities {
        capabilities: EffectiveModelCapabilities,
    },
    Candidates {
        candidates: Vec<RoutingCandidate>,
    },
    Decision {
        selection: RouteSelection,
    },
    EvidenceRecorded,
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

    fn capabilities(model: &str, capacity: CapacityKnowledge) -> EffectiveModelCapabilities {
        EffectiveModelCapabilities {
            target: ModelTarget {
                provider_plugin: PluginId::parse("provider.fixture").unwrap(),
                model: ModelId::parse(model).unwrap(),
                options: BTreeMap::new(),
            },
            generation: CapabilityGenerationId::parse("generation-1").unwrap(),
            context: ContextControl::ReplaceableTurns,
            capacity,
            cache: Default::default(),
            optional: BTreeSet::new(),
        }
    }

    fn requirements() -> RoutingRequirements {
        RoutingRequirements {
            context: ContextDemand {
                mandatory_input_tokens: 800,
                reducible_input_tokens: 100,
                output_reserve_tokens: 200,
                required_capabilities: BTreeSet::new(),
            },
            required_capabilities: BTreeSet::new(),
            require_known_capacity: true,
        }
    }

    #[test]
    fn hard_capacity_is_checked_before_ranking() {
        assert_eq!(
            requirements().evaluate(&capabilities(
                "model.fixture",
                CapacityKnowledge::Known {
                    limits: ModelLimits {
                        context_window_tokens: 1_000,
                        max_output_tokens: Some(500),
                    },
                },
            )),
            RouteEligibility::InsufficientContextCapacity
        );
    }

    #[test]
    fn deterministic_default_uses_profile_order_not_estimates() {
        let candidates = vec![
            RoutingCandidate {
                capabilities: capabilities(
                    "first",
                    CapacityKnowledge::Known {
                        limits: ModelLimits {
                            context_window_tokens: 2_000,
                            max_output_tokens: Some(500),
                        },
                    },
                ),
                estimate: Some(RoutingEstimate {
                    source: RoutingEstimateSource::Historical,
                    expected_quality_millis: Some(100),
                    expected_latency_ms: Some(100),
                    expected_cost_microunits: Some(100),
                    confidence_millis: Some(1_000),
                    estimator_snapshot_revision: None,
                    evidence_cutoff_sequence: None,
                }),
                ordinal: 0,
            },
            RoutingCandidate {
                capabilities: capabilities(
                    "second",
                    CapacityKnowledge::Known {
                        limits: ModelLimits {
                            context_window_tokens: 2_000,
                            max_output_tokens: Some(500),
                        },
                    },
                ),
                estimate: Some(RoutingEstimate {
                    source: RoutingEstimateSource::Historical,
                    expected_quality_millis: Some(900),
                    expected_latency_ms: Some(10),
                    expected_cost_microunits: Some(10),
                    confidence_millis: Some(1_000),
                    estimator_snapshot_revision: None,
                    evidence_cutoff_sequence: None,
                }),
                ordinal: 1,
            },
        ];
        let selection = select_route(
            &candidates,
            &requirements(),
            &RouteSelectionPolicy {
                revision: "deterministic".into(),
                estimates: RoutingEstimateMode::Ignore,
                max_candidate_attempts: 8,
            },
        )
        .unwrap();
        assert_eq!(selection.decision.target.model.as_str(), "first");
    }

    #[test]
    fn estimates_only_rank_candidates_that_pass_hard_admission() {
        let candidates = vec![
            RoutingCandidate {
                capabilities: capabilities(
                    "high-quality-too-small",
                    CapacityKnowledge::Known {
                        limits: ModelLimits {
                            context_window_tokens: 500,
                            max_output_tokens: Some(500),
                        },
                    },
                ),
                estimate: Some(RoutingEstimate {
                    source: RoutingEstimateSource::Learned,
                    expected_quality_millis: Some(1_000),
                    expected_latency_ms: Some(1),
                    expected_cost_microunits: Some(1),
                    confidence_millis: Some(1_000),
                    estimator_snapshot_revision: None,
                    evidence_cutoff_sequence: None,
                }),
                ordinal: 0,
            },
            RoutingCandidate {
                capabilities: capabilities(
                    "eligible",
                    CapacityKnowledge::Known {
                        limits: ModelLimits {
                            context_window_tokens: 2_000,
                            max_output_tokens: Some(500),
                        },
                    },
                ),
                estimate: None,
                ordinal: 1,
            },
        ];
        let selection = select_route(
            &candidates,
            &requirements(),
            &RouteSelectionPolicy {
                revision: "adaptive".into(),
                estimates: RoutingEstimateMode::PreferTrusted {
                    min_confidence_millis: 800,
                },
                max_candidate_attempts: 8,
            },
        )
        .unwrap();
        assert_eq!(selection.decision.target.model.as_str(), "eligible");
        assert_eq!(selection.rejected.len(), 1);
    }
}
