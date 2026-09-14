use phenix_core::CallableId;
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
