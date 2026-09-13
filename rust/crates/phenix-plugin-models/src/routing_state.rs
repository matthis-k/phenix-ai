use phenix_core::CallableId;
use phenix_sdk::{
    select_route, EffectiveModelCapabilities, ModelTarget, RejectedRoutingCandidate, RouteDecision,
    RouteSelection, RouteSelectionError, RouteSelectionPolicy, RoutingCandidate, RoutingEstimate,
    RoutingEvidence, RoutingProfile, RoutingRequirements,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct RoutingRuntimeState {
    capabilities: BTreeMap<String, EffectiveModelCapabilities>,
    estimates: BTreeMap<String, RoutingEstimate>,
    evidence: BTreeMap<String, Vec<RoutingEvidence>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RoutingRuntimeError {
    InvalidTargetIdentity,
    MissingEffectiveCapabilities { target: ModelTarget },
    Selection(RouteSelectionError),
}

impl RoutingRuntimeState {
    pub(crate) fn publish_capabilities(
        &mut self,
        capabilities: EffectiveModelCapabilities,
    ) -> Result<(), RoutingRuntimeError> {
        let key = target_key(&capabilities.target)?;
        self.capabilities.insert(key, capabilities);
        Ok(())
    }

    pub(crate) fn publish_estimate(
        &mut self,
        target: &ModelTarget,
        estimate: RoutingEstimate,
    ) -> Result<(), RoutingRuntimeError> {
        self.estimates.insert(target_key(target)?, estimate);
        Ok(())
    }

    pub(crate) fn record_evidence(
        &mut self,
        decision: &RouteDecision,
        evidence: RoutingEvidence,
    ) -> Result<(), RoutingRuntimeError> {
        self.evidence
            .entry(target_key(&decision.target)?)
            .or_default()
            .push(evidence);
        Ok(())
    }

    pub(crate) fn evidence_for(
        &self,
        target: &ModelTarget,
    ) -> Result<&[RoutingEvidence], RoutingRuntimeError> {
        Ok(self
            .evidence
            .get(&target_key(target)?)
            .map(Vec::as_slice)
            .unwrap_or_default())
    }

    pub(crate) fn candidates(
        &self,
        profile: &RoutingProfile,
        callable_id: Option<&CallableId>,
    ) -> Result<Vec<RoutingCandidate>, RoutingRuntimeError> {
        let mut ordered = Vec::new();
        if let Some(target) = callable_id.and_then(|id| profile.callable_targets.get(id)) {
            ordered.push(target.clone());
        } else {
            ordered.push(profile.default_target.clone());
        }
        ordered.extend(profile.fallback_targets.iter().cloned());

        let mut seen = BTreeSet::new();
        let mut candidates = Vec::with_capacity(ordered.len());
        for target in ordered {
            let key = target_key(&target)?;
            if !seen.insert(key.clone()) {
                continue;
            }
            let capabilities = self.capabilities.get(&key).cloned().ok_or_else(|| {
                RoutingRuntimeError::MissingEffectiveCapabilities {
                    target: target.clone(),
                }
            })?;
            let ordinal = u32::try_from(candidates.len()).unwrap_or(u32::MAX);
            candidates.push(RoutingCandidate {
                capabilities,
                estimate: self.estimates.get(&key).cloned(),
                ordinal,
            });
        }
        Ok(candidates)
    }

    pub(crate) fn resolve(
        &self,
        profile: &RoutingProfile,
        callable_id: Option<&CallableId>,
        requirements: &RoutingRequirements,
        policy: &RouteSelectionPolicy,
    ) -> Result<RouteSelection, RoutingRuntimeError> {
        let candidates = self.candidates(profile, callable_id)?;
        select_route(&candidates, requirements, policy).map_err(RoutingRuntimeError::Selection)
    }
}

fn target_key(target: &ModelTarget) -> Result<String, RoutingRuntimeError> {
    serde_json::to_string(target).map_err(|_| RoutingRuntimeError::InvalidTargetIdentity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{CapabilityGenerationId, ModelId, PluginId, RoutingProfileId};
    use phenix_sdk::{
        CapacityKnowledge, ContextControl, ContextDemand, ModelLimits, RoutingEstimateMode,
    };

    fn target(model: &str) -> ModelTarget {
        ModelTarget {
            provider_plugin: PluginId::parse("provider.fixture").unwrap(),
            model: ModelId::parse(model).unwrap(),
            options: BTreeMap::new(),
        }
    }

    fn capabilities(target: ModelTarget) -> EffectiveModelCapabilities {
        EffectiveModelCapabilities {
            target,
            generation: CapabilityGenerationId::parse("generation-1").unwrap(),
            context: ContextControl::ReplaceableTurns,
            capacity: CapacityKnowledge::Known {
                limits: ModelLimits {
                    context_window_tokens: 10_000,
                    max_output_tokens: Some(2_000),
                },
            },
            optional: BTreeSet::new(),
        }
    }

    fn profile() -> RoutingProfile {
        RoutingProfile {
            id: RoutingProfileId::parse("profile.default").unwrap(),
            default_target: target("primary"),
            fallback_targets: vec![target("fallback")],
            callable_targets: BTreeMap::new(),
        }
    }

    #[test]
    fn candidate_order_is_profile_order_and_requires_effective_capabilities() {
        let mut state = RoutingRuntimeState::default();
        let profile = profile();
        state
            .publish_capabilities(capabilities(profile.default_target.clone()))
            .unwrap();
        assert!(matches!(
            state.candidates(&profile, None),
            Err(RoutingRuntimeError::MissingEffectiveCapabilities { .. })
        ));
        state
            .publish_capabilities(capabilities(profile.fallback_targets[0].clone()))
            .unwrap();
        let candidates = state.candidates(&profile, None).unwrap();
        assert_eq!(candidates[0].ordinal, 0);
        assert_eq!(candidates[0].capabilities.target.model.as_str(), "primary");
        assert_eq!(candidates[1].ordinal, 1);
    }

    #[test]
    fn runtime_resolve_reuses_sdk_hard_admission_and_selection() {
        let mut state = RoutingRuntimeState::default();
        let profile = profile();
        for target in std::iter::once(&profile.default_target).chain(profile.fallback_targets.iter()) {
            state.publish_capabilities(capabilities(target.clone())).unwrap();
        }
        let selection = state
            .resolve(
                &profile,
                None,
                &RoutingRequirements {
                    context: ContextDemand {
                        mandatory_input_tokens: 100,
                        reducible_input_tokens: 100,
                        output_reserve_tokens: 100,
                        required_capabilities: BTreeSet::new(),
                    },
                    required_capabilities: BTreeSet::new(),
                    require_known_capacity: true,
                },
                &RouteSelectionPolicy {
                    revision: "policy-1".into(),
                    estimates: RoutingEstimateMode::Ignore,
                    max_candidate_attempts: 2,
                },
            )
            .unwrap();
        assert_eq!(selection.decision.target.model.as_str(), "primary");
    }
}
