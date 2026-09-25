use phenix_core::{CallableId, CapabilityGenerationId};
use phenix_sdk::{
    select_route, EffectiveModelCapabilities, ModelTarget, RouteDecision, RouteSelection,
    RouteSelectionError, RouteSelectionPolicy, RoutingCandidate, RoutingEstimate, RoutingEvidence,
    RoutingProfile, RoutingRequirements,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct RoutingRuntimeState {
    capabilities: BTreeMap<String, EffectiveModelCapabilities>,
    #[serde(skip)]
    estimates: BTreeMap<String, RoutingEstimate>,
    evidence: BTreeMap<String, Vec<RoutingEvidence>>,
    #[serde(default)]
    evidence_sequence: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RoutingRuntimeError {
    InvalidTargetIdentity,
    MissingEffectiveCapabilities {
        target: ModelTarget,
    },
    StaleCapabilityGeneration {
        target: ModelTarget,
        decision: CapabilityGenerationId,
        current: CapabilityGenerationId,
    },
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

    pub(crate) fn record_evidence(
        &mut self,
        decision: &RouteDecision,
        evidence: RoutingEvidence,
    ) -> Result<(), RoutingRuntimeError> {
        self.validate_decision(decision)?;
        self.evidence
            .entry(target_key(&decision.target)?)
            .or_default()
            .push(evidence);
        self.evidence_sequence = self.evidence_sequence.saturating_add(1);
        self.rebuild_estimates();
        Ok(())
    }

    pub(crate) fn rebuild_estimates(&mut self) {
        self.estimates.clear();
        let snapshot_revision = format!("routing-evidence/{}", self.evidence_sequence);
        for (target, records) in &self.evidence {
            if records.is_empty() {
                continue;
            }
            let total = u64::try_from(records.len()).unwrap_or(u64::MAX);
            let successes = u64::try_from(records.iter().filter(|record| record.success).count())
                .unwrap_or(u64::MAX);
            let expected_quality_millis = Some(
                u32::try_from(successes.saturating_mul(1_000) / total)
                    .unwrap_or(u32::MAX),
            );
            let latency = records
                .iter()
                .filter_map(|record| record.latency_ms)
                .fold((0_u64, 0_u64), |(sum, count), value| {
                    (sum.saturating_add(value), count.saturating_add(1))
                });
            let expected_latency_ms =
                (latency.1 != 0).then(|| latency.0 / latency.1);
            let confidence_millis = Some(
                u16::try_from(total.saturating_mul(100).min(1_000))
                    .unwrap_or(1_000),
            );
            self.estimates.insert(
                target.clone(),
                RoutingEstimate {
                    source: phenix_sdk::RoutingEstimateSource::Historical,
                    expected_quality_millis,
                    expected_latency_ms,
                    expected_cost_microunits: None,
                    confidence_millis,
                    estimator_snapshot_revision: Some(snapshot_revision.clone()),
                    evidence_cutoff_sequence: Some(self.evidence_sequence),
                },
            );
        }
    }

    pub(crate) fn validate_decision(
        &self,
        decision: &RouteDecision,
    ) -> Result<&EffectiveModelCapabilities, RoutingRuntimeError> {
        let current = self
            .capabilities
            .get(&target_key(&decision.target)?)
            .ok_or_else(|| RoutingRuntimeError::MissingEffectiveCapabilities {
                target: decision.target.clone(),
            })?;
        if current.generation != decision.capability_generation {
            return Err(RoutingRuntimeError::StaleCapabilityGeneration {
                target: decision.target.clone(),
                decision: decision.capability_generation.clone(),
                current: current.generation.clone(),
            });
        }
        Ok(current)
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
    use phenix_core::{ModelId, PluginId, RoutingProfileId};
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

    fn capabilities(target: ModelTarget, generation: &str) -> EffectiveModelCapabilities {
        EffectiveModelCapabilities {
            target,
            generation: CapabilityGenerationId::parse(generation).unwrap(),
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
            .publish_capabilities(capabilities(profile.default_target.clone(), "generation-1"))
            .unwrap();
        assert!(matches!(
            state.candidates(&profile, None),
            Err(RoutingRuntimeError::MissingEffectiveCapabilities { .. })
        ));
        state
            .publish_capabilities(capabilities(
                profile.fallback_targets[0].clone(),
                "generation-1",
            ))
            .unwrap();
        let candidates = state.candidates(&profile, None).unwrap();
        assert_eq!(candidates[0].ordinal, 0);
        assert_eq!(candidates[0].capabilities.target.model.as_str(), "primary");
        assert_eq!(candidates[1].ordinal, 1);
    }

    #[test]
    fn stale_route_decision_is_rejected_after_capability_refresh() {
        let mut state = RoutingRuntimeState::default();
        let target = target("primary");
        state
            .publish_capabilities(capabilities(target.clone(), "generation-1"))
            .unwrap();
        let decision = RouteDecision {
            target: target.clone(),
            capability_generation: CapabilityGenerationId::parse("generation-1").unwrap(),
            policy_revision: "policy-1".into(),
            candidate_ordinal: 0,
            estimate: None,
        };
        state.validate_decision(&decision).unwrap();
        state
            .publish_capabilities(capabilities(target, "generation-2"))
            .unwrap();
        assert!(matches!(
            state.validate_decision(&decision),
            Err(RoutingRuntimeError::StaleCapabilityGeneration { .. })
        ));
    }

    #[test]
    fn historical_estimates_are_rebuilt_from_completed_evidence() {
        let mut state = RoutingRuntimeState::default();
        let profile = profile();
        for target in
            std::iter::once(&profile.default_target).chain(profile.fallback_targets.iter())
        {
            state
                .publish_capabilities(capabilities(target.clone(), "generation-1"))
                .unwrap();
        }

        let decision = |target: ModelTarget, ordinal: u32| RouteDecision {
            target,
            capability_generation: CapabilityGenerationId::parse("generation-1").unwrap(),
            policy_revision: "policy-1".into(),
            candidate_ordinal: ordinal,
            estimate: None,
        };
        let unavailable_usage = || phenix_core::ModelTurnUsage {
            fresh_input_tokens: phenix_core::UsageQuantity::Unavailable,
            cache_read_tokens: phenix_core::UsageQuantity::Unavailable,
            cache_write_tokens: phenix_core::UsageQuantity::Unavailable,
            output_tokens: phenix_core::UsageQuantity::Unavailable,
            reasoning_tokens: phenix_core::UsageQuantity::Unavailable,
        };

        state
            .record_evidence(
                &decision(profile.default_target.clone(), 0),
                RoutingEvidence {
                    success: false,
                    latency_ms: Some(200),
                    usage: unavailable_usage(),
                },
            )
            .unwrap();
        state
            .record_evidence(
                &decision(profile.fallback_targets[0].clone(), 1),
                RoutingEvidence {
                    success: true,
                    latency_ms: Some(50),
                    usage: unavailable_usage(),
                },
            )
            .unwrap();

        let candidates = state.candidates(&profile, None).unwrap();
        assert_eq!(
            candidates[0]
                .estimate
                .as_ref()
                .and_then(|estimate| estimate.evidence_cutoff_sequence),
            Some(2)
        );
        assert_eq!(
            candidates[1]
                .estimate
                .as_ref()
                .and_then(|estimate| estimate.estimator_snapshot_revision.as_deref()),
            Some("routing-evidence/2")
        );

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
                    estimates: RoutingEstimateMode::PreferTrusted {
                        min_confidence_millis: 100,
                    },
                    max_candidate_attempts: 2,
                },
            )
            .unwrap();
        assert_eq!(selection.decision.target.model.as_str(), "fallback");
        assert_eq!(
            selection
                .decision
                .estimate
                .as_ref()
                .and_then(|estimate| estimate.evidence_cutoff_sequence),
            Some(2)
        );
    }

    #[test]
    fn runtime_resolve_reuses_sdk_hard_admission_and_selection() {
        let mut state = RoutingRuntimeState::default();
        let profile = profile();
        for target in
            std::iter::once(&profile.default_target).chain(profile.fallback_targets.iter())
        {
            state
                .publish_capabilities(capabilities(target.clone(), "generation-1"))
                .unwrap();
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
