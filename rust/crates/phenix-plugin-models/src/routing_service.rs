use crate::routing_state::RoutingRuntimeState;
use phenix_core::RoutingProfileId;
use phenix_sdk::{ModelCommand, ModelResponse, RouteDecision, RoutingProfile};

pub(crate) const ROUTING_RUNTIME_KEY: &str = "runtime/routing-state";
pub(crate) const MAX_ROUTING_SNAPSHOT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RoutingServiceStateError {
    InvalidSnapshot(String),
    SnapshotTooLarge { bytes: usize, allowed: usize },
}

#[derive(Default)]
pub(crate) struct RoutingServiceState {
    runtime: RoutingRuntimeState,
}

impl RoutingServiceState {
    pub(crate) fn restore(snapshot: Option<&[u8]>) -> Result<Self, RoutingServiceStateError> {
        let runtime = match snapshot {
            Some(bytes) => {
                if bytes.len() > MAX_ROUTING_SNAPSHOT_BYTES {
                    return Err(RoutingServiceStateError::SnapshotTooLarge {
                        bytes: bytes.len(),
                        allowed: MAX_ROUTING_SNAPSHOT_BYTES,
                    });
                }
                serde_json::from_slice(bytes)
                    .map_err(|error| RoutingServiceStateError::InvalidSnapshot(error.to_string()))?
            }
            None => RoutingRuntimeState::default(),
        };
        Ok(Self { runtime })
    }

    pub(crate) fn snapshot(&self) -> Result<Vec<u8>, RoutingServiceStateError> {
        let bytes = serde_json::to_vec(&self.runtime)
            .map_err(|error| RoutingServiceStateError::InvalidSnapshot(error.to_string()))?;
        if bytes.len() > MAX_ROUTING_SNAPSHOT_BYTES {
            return Err(RoutingServiceStateError::SnapshotTooLarge {
                bytes: bytes.len(),
                allowed: MAX_ROUTING_SNAPSHOT_BYTES,
            });
        }
        Ok(bytes)
    }

    pub(crate) fn validate_decision(&self, decision: &RouteDecision) -> Result<(), String> {
        self.runtime
            .validate_decision(decision)
            .map(|_| ())
            .map_err(|error| format!("resolved routing decision is invalid: {error:?}"))
    }

    pub(crate) fn handle_state_command<F>(
        &mut self,
        command: ModelCommand,
        mut load_profile: F,
    ) -> Option<Result<ModelResponse, String>>
    where
        F: FnMut(&RoutingProfileId) -> Result<Option<RoutingProfile>, String>,
    {
        let response = match command {
            ModelCommand::PublishCapabilities { capabilities } => {
                let response = capabilities.clone();
                self.runtime
                    .publish_capabilities(capabilities)
                    .map(|()| ModelResponse::Capabilities {
                        capabilities: response,
                    })
                    .map_err(|error| format!("routing capability publication failed: {error:?}"))
            }
            ModelCommand::ListCandidates {
                profile_id,
                callable_id,
            } => require_profile(&mut load_profile, &profile_id).and_then(|profile| {
                self.runtime
                    .candidates(&profile, callable_id.as_ref())
                    .map(|candidates| ModelResponse::Candidates { candidates })
                    .map_err(|error| format!("routing candidate construction failed: {error:?}"))
            }),
            ModelCommand::ResolveWithRequirements {
                profile_id,
                callable_id,
                requirements,
                policy,
            } => require_profile(&mut load_profile, &profile_id).and_then(|profile| {
                self.runtime
                    .resolve(&profile, callable_id.as_ref(), &requirements, &policy)
                    .map(|selection| ModelResponse::Decision { selection })
                    .map_err(|error| format!("routing selection failed: {error:?}"))
            }),
            ModelCommand::RecordEvidence { decision, evidence } => self
                .runtime
                .record_evidence(&decision, evidence)
                .map(|()| ModelResponse::EvidenceRecorded)
                .map_err(|error| format!("routing evidence recording failed: {error:?}")),
            ModelCommand::RegisterProfile { .. }
            | ModelCommand::GetProfile { .. }
            | ModelCommand::ListProfiles
            | ModelCommand::SetProviderAuthenticated { .. }
            | ModelCommand::Resolve { .. }
            | ModelCommand::Invoke { .. } => return None,
        };
        Some(response)
    }
}

fn require_profile<F>(
    load_profile: &mut F,
    id: &RoutingProfileId,
) -> Result<RoutingProfile, String>
where
    F: FnMut(&RoutingProfileId) -> Result<Option<RoutingProfile>, String>,
{
    load_profile(id)?.ok_or_else(|| format!("unknown routing profile: {id}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{CapabilityGenerationId, ModelId, PluginId};
    use phenix_sdk::{
        CapacityKnowledge, ContextControl, EffectiveModelCapabilities, ModelLimits, ModelTarget,
    };
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn capability_publication_round_trips_through_snapshot() {
        let target = ModelTarget {
            provider_plugin: PluginId::parse("provider.fixture").unwrap(),
            model: ModelId::parse("model.fixture").unwrap(),
            options: BTreeMap::new(),
        };
        let capabilities = EffectiveModelCapabilities {
            target,
            generation: CapabilityGenerationId::parse("generation-1").unwrap(),
            context: ContextControl::ReplaceableTurns,
            capacity: CapacityKnowledge::Known {
                limits: ModelLimits {
                    context_window_tokens: 16_000,
                    max_output_tokens: Some(2_000),
                },
            },
            optional: BTreeSet::new(),
        };
        let mut state = RoutingServiceState::default();
        let response = state
            .handle_state_command(
                ModelCommand::PublishCapabilities {
                    capabilities: capabilities.clone(),
                },
                |_| Ok(None),
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            response,
            ModelResponse::Capabilities {
                capabilities: capabilities.clone(),
            }
        );

        let restored = RoutingServiceState::restore(Some(&state.snapshot().unwrap())).unwrap();
        assert_eq!(restored.runtime, state.runtime);
    }
}
