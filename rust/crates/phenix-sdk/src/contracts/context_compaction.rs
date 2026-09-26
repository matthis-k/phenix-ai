use super::{ContextRetention, ExactContextReference};
use phenix_core::{Bytes, ComponentInterface, InterfaceId, ServiceId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const CONTEXT_REDUCER_SERVICE: &str = "phenix.context-reducer@1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ProjectionRevision {
    pub revision: u64,
    pub cache_epoch: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ToolCallGroupReference {
    pub call_id: String,
    pub call: ExactContextReference,
    pub result: ExactContextReference,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContextCheckpoint {
    pub checkpoint_id: String,
    pub execution_id: String,
    pub source_revision: ProjectionRevision,
    pub content_identity: String,
    pub compact_view: Bytes,
    #[serde(default)]
    pub exact_sources: Vec<ExactContextReference>,
    #[serde(default)]
    pub tool_groups: Vec<ToolCallGroupReference>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct RetentionTransition {
    pub item_id: String,
    pub from: ContextRetention,
    pub to: ContextRetention,
    pub recovery: Option<ExactContextReference>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct CacheCostScenario {
    pub future_turns: u32,
    pub expected_cache_hits: u32,
    pub expected_cache_misses: u32,
    /// Cost of one future turn when the prefix must be freshly processed.
    pub fresh_input_cost_microunits: Option<u64>,
    /// Cost of one future turn when the prefix is served from cache.
    pub cache_read_cost_microunits: Option<u64>,
    /// One-time write/prefill cost for this projection. Use Some(0) when none is expected.
    pub cache_write_cost_microunits: Option<u64>,
    /// One-time helper/reducer work needed to establish this projection.
    pub setup_cost_microunits: u64,
    /// Expected later reacquisition work attributable to this projection.
    pub reacquisition_cost_microunits: Option<u64>,
}

impl CacheCostScenario {
    pub fn total_cost_microunits(&self) -> Result<Option<u64>, CacheCompactionCostError> {
        if self
            .expected_cache_hits
            .saturating_add(self.expected_cache_misses)
            != self.future_turns
        {
            return Err(CacheCompactionCostError::TurnAccountingMismatch {
                future_turns: self.future_turns,
                expected_cache_hits: self.expected_cache_hits,
                expected_cache_misses: self.expected_cache_misses,
            });
        }

        let cache_reads = if self.expected_cache_hits == 0 {
            0
        } else {
            let Some(cost) = self.cache_read_cost_microunits else {
                return Ok(None);
            };
            cost.checked_mul(u64::from(self.expected_cache_hits))
                .ok_or(CacheCompactionCostError::CostOverflow)?
        };
        let fresh = if self.expected_cache_misses == 0 {
            0
        } else {
            let Some(cost) = self.fresh_input_cost_microunits else {
                return Ok(None);
            };
            cost.checked_mul(u64::from(self.expected_cache_misses))
                .ok_or(CacheCompactionCostError::CostOverflow)?
        };
        let Some(write) = self.cache_write_cost_microunits else {
            return Ok(None);
        };
        let Some(reacquisition) = self.reacquisition_cost_microunits else {
            return Ok(None);
        };

        self.setup_cost_microunits
            .checked_add(write)
            .and_then(|cost| cost.checked_add(cache_reads))
            .and_then(|cost| cost.checked_add(fresh))
            .and_then(|cost| cost.checked_add(reacquisition))
            .map(Some)
            .ok_or(CacheCompactionCostError::CostOverflow)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct CacheCompactionDecisionRequest {
    pub retain: CacheCostScenario,
    pub compact: CacheCostScenario,
    /// Capacity/context pressure is a correctness fallback when monetary estimates are incomplete.
    pub context_pressure_requires_compaction: bool,
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum CacheCompactionChoice {
    Retain,
    Compact,
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum CacheCompactionDecisionBasis {
    KnownCost,
    ContextPressureFallback,
    RetainFallback,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct CacheCompactionDecision {
    pub choice: CacheCompactionChoice,
    pub basis: CacheCompactionDecisionBasis,
    pub retain_cost_microunits: Option<u64>,
    pub compact_cost_microunits: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum CacheCompactionCostError {
    TurnAccountingMismatch {
        future_turns: u32,
        expected_cache_hits: u32,
        expected_cache_misses: u32,
    },
    CostOverflow,
}

pub fn choose_cache_aware_compaction(
    request: &CacheCompactionDecisionRequest,
) -> Result<CacheCompactionDecision, CacheCompactionCostError> {
    let retain = request.retain.total_cost_microunits()?;
    let compact = request.compact.total_cost_microunits()?;

    let (choice, basis) = match (retain, compact) {
        (Some(retain), Some(compact)) => (
            if compact < retain {
                CacheCompactionChoice::Compact
            } else {
                CacheCompactionChoice::Retain
            },
            CacheCompactionDecisionBasis::KnownCost,
        ),
        _ if request.context_pressure_requires_compaction => (
            CacheCompactionChoice::Compact,
            CacheCompactionDecisionBasis::ContextPressureFallback,
        ),
        _ => (
            CacheCompactionChoice::Retain,
            CacheCompactionDecisionBasis::RetainFallback,
        ),
    };

    Ok(CacheCompactionDecision {
        choice,
        basis,
        retain_cost_microunits: retain,
        compact_cost_microunits: compact,
    })
}

#[derive(
    Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
    phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum ContextReducerStage {
    CodeEvidence,
    ObservationSummary,
    HistorySummary,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ReducerEligibleItem {
    pub item_id: String,
    pub recovery: Option<ExactContextReference>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContextReducerRequest {
    pub execution_id: String,
    pub expected_projection: ProjectionRevision,
    pub configuration_revision: String,
    pub authority_revision: String,
    pub capability_generation: String,
    pub stage: ContextReducerStage,
    pub query: String,
    pub helper_reservation_id: String,
    pub max_output_bytes: u64,
    pub eligible: Vec<ReducerEligibleItem>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct DerivedReductionSummary {
    pub item_id: String,
    pub content: Bytes,
    #[serde(default)]
    pub exact_sources: Vec<ExactContextReference>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ContextReducerProposal {
    pub execution_id: String,
    pub expected_projection: ProjectionRevision,
    pub configuration_revision: String,
    pub authority_revision: String,
    pub capability_generation: String,
    pub helper_reservation_id: String,
    pub stage: ContextReducerStage,
    pub helper_attempt_id: String,
    #[serde(default)]
    pub retained_item_ids: Vec<String>,
    #[serde(default)]
    pub omitted_item_ids: Vec<String>,
    #[serde(default)]
    pub summaries: Vec<DerivedReductionSummary>,
    pub encoded_output_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextReducerCommand {
    Reduce { request: ContextReducerRequest },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextReducerResponse {
    Proposal { proposal: ContextReducerProposal },
}

pub struct ContextReducerInterface;

impl ComponentInterface for ContextReducerInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(CONTEXT_REDUCER_SERVICE)
            .expect("static context reducer interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<ContextReducerCommand, ContextReducerResponse>()
    }
}

#[must_use]
pub fn context_reducer_service() -> ServiceId {
    ServiceId::parse(CONTEXT_REDUCER_SERVICE).expect("static context reducer service id is valid")
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReducerValidationError {
    StaleProjection,
    ExecutionMismatch,
    ConfigurationMismatch,
    AuthorityMismatch,
    CapabilityGenerationMismatch,
    HelperReservationMismatch,
    StageMismatch,
    UnknownItem { item_id: String },
    DuplicateItem { item_id: String },
    MissingItemDecision { item_id: String },
    MissingRecoveryReference { item_id: String },
    DuplicateSummary { item_id: String },
    SummaryWithoutExactSource { item_id: String },
    SummaryMissingEligibleRecovery { item_id: String },
    OutputBudgetExceeded { requested: u64, allowed: u64 },
}

impl ContextReducerProposal {
    pub fn validate_against(
        &self,
        request: &ContextReducerRequest,
        actual_projection: &ProjectionRevision,
    ) -> Result<(), ReducerValidationError> {
        if &request.expected_projection != actual_projection
            || &self.expected_projection != actual_projection
        {
            return Err(ReducerValidationError::StaleProjection);
        }
        if self.execution_id != request.execution_id {
            return Err(ReducerValidationError::ExecutionMismatch);
        }
        if self.configuration_revision != request.configuration_revision {
            return Err(ReducerValidationError::ConfigurationMismatch);
        }
        if self.authority_revision != request.authority_revision {
            return Err(ReducerValidationError::AuthorityMismatch);
        }
        if self.capability_generation != request.capability_generation {
            return Err(ReducerValidationError::CapabilityGenerationMismatch);
        }
        if self.helper_reservation_id != request.helper_reservation_id {
            return Err(ReducerValidationError::HelperReservationMismatch);
        }
        if self.stage != request.stage {
            return Err(ReducerValidationError::StageMismatch);
        }
        let encoded = serde_json::to_vec(&phenix_core::PhenixValue::from(self))
            .expect("context reducer proposal has a deterministic PhenixValue encoding");
        let actual_encoded_bytes = u64::try_from(encoded.len()).unwrap_or(u64::MAX);
        let requested_output_bytes = self.encoded_output_bytes.max(actual_encoded_bytes);
        if requested_output_bytes > request.max_output_bytes {
            return Err(ReducerValidationError::OutputBudgetExceeded {
                requested: requested_output_bytes,
                allowed: request.max_output_bytes,
            });
        }

        let eligible = request
            .eligible
            .iter()
            .map(|item| (item.item_id.as_str(), item))
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut seen = BTreeSet::new();
        for item_id in self
            .retained_item_ids
            .iter()
            .chain(self.omitted_item_ids.iter())
        {
            if !seen.insert(item_id.as_str()) {
                return Err(ReducerValidationError::DuplicateItem {
                    item_id: item_id.clone(),
                });
            }
            let Some(item) = eligible.get(item_id.as_str()) else {
                return Err(ReducerValidationError::UnknownItem {
                    item_id: item_id.clone(),
                });
            };
            if self.omitted_item_ids.contains(item_id) && item.recovery.is_none() {
                return Err(ReducerValidationError::MissingRecoveryReference {
                    item_id: item_id.clone(),
                });
            }
        }
        for item in &request.eligible {
            if !seen.contains(item.item_id.as_str()) {
                return Err(ReducerValidationError::MissingItemDecision {
                    item_id: item.item_id.clone(),
                });
            }
        }

        let mut summary_seen = BTreeSet::new();
        for summary in &self.summaries {
            if !eligible.contains_key(summary.item_id.as_str()) {
                return Err(ReducerValidationError::UnknownItem {
                    item_id: summary.item_id.clone(),
                });
            }
            if !summary_seen.insert(summary.item_id.as_str()) {
                return Err(ReducerValidationError::DuplicateSummary {
                    item_id: summary.item_id.clone(),
                });
            }
            if summary.exact_sources.is_empty() {
                return Err(ReducerValidationError::SummaryWithoutExactSource {
                    item_id: summary.item_id.clone(),
                });
            }
            if let Some(expected) = eligible
                .get(summary.item_id.as_str())
                .and_then(|item| item.recovery.as_ref())
            {
                if !summary.exact_sources.contains(expected) {
                    return Err(ReducerValidationError::SummaryMissingEligibleRecovery {
                        item_id: summary.item_id.clone(),
                    });
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct CompactionProposal {
    pub execution_id: String,
    pub expected_projection: ProjectionRevision,
    pub next_cache_epoch: u64,
    #[serde(default)]
    pub transitions: Vec<RetentionTransition>,
    pub checkpoint: ContextCheckpoint,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct CompactionCommit {
    pub proposal: CompactionProposal,
    pub committed_projection: ProjectionRevision,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompactionValidationError {
    StaleProjection {
        expected: ProjectionRevision,
        actual: ProjectionRevision,
    },
    CacheEpochDidNotAdvance {
        current: u64,
        proposed: u64,
    },
    PinnedItemDemoted {
        item_id: String,
    },
    RetentionExpanded {
        item_id: String,
        from: ContextRetention,
        to: ContextRetention,
    },
    MissingRecoveryReference {
        item_id: String,
        to: ContextRetention,
    },
    CheckpointSourceMismatch,
}

impl CompactionProposal {
    pub fn validate_against(
        &self,
        actual: &ProjectionRevision,
    ) -> Result<(), CompactionValidationError> {
        if &self.expected_projection != actual {
            return Err(CompactionValidationError::StaleProjection {
                expected: self.expected_projection.clone(),
                actual: actual.clone(),
            });
        }
        if self.next_cache_epoch <= actual.cache_epoch {
            return Err(CompactionValidationError::CacheEpochDidNotAdvance {
                current: actual.cache_epoch,
                proposed: self.next_cache_epoch,
            });
        }
        if self.checkpoint.source_revision != self.expected_projection
            || self.checkpoint.execution_id != self.execution_id
        {
            return Err(CompactionValidationError::CheckpointSourceMismatch);
        }

        for transition in &self.transitions {
            if transition.from == ContextRetention::Pinned
                && transition.to != ContextRetention::Pinned
            {
                return Err(CompactionValidationError::PinnedItemDemoted {
                    item_id: transition.item_id.clone(),
                });
            }
            if retention_rank(transition.to) < retention_rank(transition.from) {
                return Err(CompactionValidationError::RetentionExpanded {
                    item_id: transition.item_id.clone(),
                    from: transition.from,
                    to: transition.to,
                });
            }
            if matches!(
                transition.to,
                ContextRetention::Compact | ContextRetention::Reference
            ) && transition.recovery.is_none()
            {
                return Err(CompactionValidationError::MissingRecoveryReference {
                    item_id: transition.item_id.clone(),
                    to: transition.to,
                });
            }
        }
        Ok(())
    }
}

const fn retention_rank(retention: ContextRetention) -> u8 {
    match retention {
        ContextRetention::Pinned => 0,
        ContextRetention::Full => 1,
        ContextRetention::Compact => 2,
        ContextRetention::Reference => 3,
        ContextRetention::DropAllowed => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{ContextResourceId, ContextRevisionId};

    fn exact(id: &str) -> ExactContextReference {
        ExactContextReference {
            resource_id: ContextResourceId::parse(id).unwrap(),
            revision: ContextRevisionId::parse("rev-1").unwrap(),
        }
    }

    fn proposal(transition: RetentionTransition) -> CompactionProposal {
        let revision = ProjectionRevision {
            revision: 7,
            cache_epoch: 2,
        };
        CompactionProposal {
            execution_id: "e1".into(),
            expected_projection: revision.clone(),
            next_cache_epoch: 3,
            transitions: vec![transition],
            checkpoint: ContextCheckpoint {
                checkpoint_id: "checkpoint-1".into(),
                execution_id: "e1".into(),
                source_revision: revision,
                content_identity: "sha256:fixture".into(),
                compact_view: Bytes::from(b"summary".to_vec()),
                exact_sources: vec![exact("context:source")],
                tool_groups: Vec::new(),
            },
        }
    }

    fn cache_scenario(
        hits: u32,
        misses: u32,
        fresh: Option<u64>,
        read: Option<u64>,
        write: Option<u64>,
        setup: u64,
        reacquisition: Option<u64>,
    ) -> CacheCostScenario {
        CacheCostScenario {
            future_turns: hits + misses,
            expected_cache_hits: hits,
            expected_cache_misses: misses,
            fresh_input_cost_microunits: fresh,
            cache_read_cost_microunits: read,
            cache_write_cost_microunits: write,
            setup_cost_microunits: setup,
            reacquisition_cost_microunits: reacquisition,
        }
    }

    #[test]
    fn known_cache_cost_can_prefer_larger_retained_prefix() {
        let decision = choose_cache_aware_compaction(&CacheCompactionDecisionRequest {
            // Retaining costs 3 cached reads.
            retain: cache_scenario(3, 0, Some(50), Some(2), Some(0), 0, Some(0)),
            // Compacting has lower read cost but pays rewrite/helper cost.
            compact: cache_scenario(3, 0, Some(20), Some(1), Some(5), 8, Some(2)),
            context_pressure_requires_compaction: false,
        })
        .unwrap();

        assert_eq!(decision.retain_cost_microunits, Some(6));
        assert_eq!(decision.compact_cost_microunits, Some(18));
        assert_eq!(decision.choice, CacheCompactionChoice::Retain);
        assert_eq!(decision.basis, CacheCompactionDecisionBasis::KnownCost);
    }

    #[test]
    fn known_total_cost_can_choose_compaction() {
        let decision = choose_cache_aware_compaction(&CacheCompactionDecisionRequest {
            retain: cache_scenario(0, 4, Some(20), Some(2), Some(0), 0, Some(0)),
            compact: cache_scenario(4, 0, Some(10), Some(2), Some(4), 4, Some(0)),
            context_pressure_requires_compaction: false,
        })
        .unwrap();

        assert_eq!(decision.retain_cost_microunits, Some(80));
        assert_eq!(decision.compact_cost_microunits, Some(16));
        assert_eq!(decision.choice, CacheCompactionChoice::Compact);
    }

    #[test]
    fn unknown_cost_uses_capacity_rule_not_invented_zero() {
        let retain = cache_scenario(2, 0, Some(10), None, Some(0), 0, Some(0));
        let compact = cache_scenario(2, 0, Some(10), Some(1), Some(3), 2, Some(0));

        let retain_decision = choose_cache_aware_compaction(&CacheCompactionDecisionRequest {
            retain: retain.clone(),
            compact: compact.clone(),
            context_pressure_requires_compaction: false,
        })
        .unwrap();
        assert_eq!(retain_decision.choice, CacheCompactionChoice::Retain);
        assert_eq!(
            retain_decision.basis,
            CacheCompactionDecisionBasis::RetainFallback
        );
        assert_eq!(retain_decision.retain_cost_microunits, None);

        let compact_decision = choose_cache_aware_compaction(&CacheCompactionDecisionRequest {
            retain,
            compact,
            context_pressure_requires_compaction: true,
        })
        .unwrap();
        assert_eq!(compact_decision.choice, CacheCompactionChoice::Compact);
        assert_eq!(
            compact_decision.basis,
            CacheCompactionDecisionBasis::ContextPressureFallback
        );
    }

    fn reducer_request() -> ContextReducerRequest {
        ContextReducerRequest {
            execution_id: "e1".into(),
            expected_projection: ProjectionRevision {
                revision: 7,
                cache_epoch: 2,
            },
            configuration_revision: "config-1".into(),
            authority_revision: "authority-1".into(),
            capability_generation: "capability-1".into(),
            stage: ContextReducerStage::HistorySummary,
            query: "current task".into(),
            helper_reservation_id: "reservation-1".into(),
            max_output_bytes: 1024,
            eligible: vec![ReducerEligibleItem {
                item_id: "history-1".into(),
                recovery: Some(exact("context:history-1")),
            }],
        }
    }

    #[test]
    fn reducer_backend_has_a_replaceable_provider_neutral_interface() {
        assert_eq!(
            ContextReducerInterface::interface_id().as_str(),
            CONTEXT_REDUCER_SERVICE
        );
        assert_eq!(context_reducer_service().as_str(), CONTEXT_REDUCER_SERVICE);
    }

    #[test]
    fn reducer_cannot_omit_content_without_exact_recovery() {
        let mut request = reducer_request();
        request.eligible[0].recovery = None;
        let proposal = ContextReducerProposal {
            execution_id: "e1".into(),
            expected_projection: request.expected_projection.clone(),
            configuration_revision: request.configuration_revision.clone(),
            authority_revision: request.authority_revision.clone(),
            capability_generation: request.capability_generation.clone(),
            helper_reservation_id: request.helper_reservation_id.clone(),
            stage: request.stage,
            helper_attempt_id: "attempt-1".into(),
            retained_item_ids: Vec::new(),
            omitted_item_ids: vec!["history-1".into()],
            summaries: Vec::new(),
            encoded_output_bytes: 10,
        };

        assert!(matches!(
            proposal.validate_against(&request, &request.expected_projection),
            Err(ReducerValidationError::MissingRecoveryReference { .. })
        ));
    }

    #[test]
    fn reducer_proposal_is_bound_to_runtime_and_helper_revisions() {
        let request = reducer_request();
        let mut proposal = ContextReducerProposal {
            execution_id: "e1".into(),
            expected_projection: request.expected_projection.clone(),
            configuration_revision: request.configuration_revision.clone(),
            authority_revision: request.authority_revision.clone(),
            capability_generation: request.capability_generation.clone(),
            helper_reservation_id: request.helper_reservation_id.clone(),
            stage: request.stage,
            helper_attempt_id: "attempt-1".into(),
            retained_item_ids: vec!["history-1".into()],
            omitted_item_ids: Vec::new(),
            summaries: Vec::new(),
            encoded_output_bytes: 10,
        };

        proposal.authority_revision = "authority-2".into();
        assert_eq!(
            proposal.validate_against(&request, &request.expected_projection),
            Err(ReducerValidationError::AuthorityMismatch)
        );
        proposal.authority_revision = request.authority_revision.clone();
        proposal.helper_reservation_id = "reservation-other".into();
        assert_eq!(
            proposal.validate_against(&request, &request.expected_projection),
            Err(ReducerValidationError::HelperReservationMismatch)
        );
    }

    #[test]
    fn reducer_rejects_fabricated_ids_and_oversized_output() {
        let request = reducer_request();
        let mut proposal = ContextReducerProposal {
            execution_id: "e1".into(),
            expected_projection: request.expected_projection.clone(),
            configuration_revision: request.configuration_revision.clone(),
            authority_revision: request.authority_revision.clone(),
            capability_generation: request.capability_generation.clone(),
            helper_reservation_id: request.helper_reservation_id.clone(),
            stage: request.stage,
            helper_attempt_id: "attempt-1".into(),
            retained_item_ids: vec!["fabricated".into()],
            omitted_item_ids: Vec::new(),
            summaries: Vec::new(),
            encoded_output_bytes: 10,
        };
        assert!(matches!(
            proposal.validate_against(&request, &request.expected_projection),
            Err(ReducerValidationError::UnknownItem { .. })
        ));

        proposal.retained_item_ids = vec!["history-1".into()];
        proposal.encoded_output_bytes = 1025;
        assert!(matches!(
            proposal.validate_against(&request, &request.expected_projection),
            Err(ReducerValidationError::OutputBudgetExceeded { .. })
        ));
    }

    #[test]
    fn reducer_summary_must_reference_the_eligible_items_exact_recovery() {
        let request = reducer_request();
        let proposal = ContextReducerProposal {
            execution_id: "e1".into(),
            expected_projection: request.expected_projection.clone(),
            configuration_revision: request.configuration_revision.clone(),
            authority_revision: request.authority_revision.clone(),
            capability_generation: request.capability_generation.clone(),
            helper_reservation_id: request.helper_reservation_id.clone(),
            stage: request.stage,
            helper_attempt_id: "attempt-1".into(),
            retained_item_ids: vec!["history-1".into()],
            omitted_item_ids: Vec::new(),
            summaries: vec![DerivedReductionSummary {
                item_id: "history-1".into(),
                content: Bytes::from(b"summary".to_vec()),
                exact_sources: vec![exact("context:other")],
            }],
            encoded_output_bytes: 10,
        };

        assert_eq!(
            proposal.validate_against(&request, &request.expected_projection),
            Err(ReducerValidationError::SummaryMissingEligibleRecovery {
                item_id: "history-1".into(),
            })
        );
    }

    #[test]
    fn reducer_rejects_underreported_encoded_output_size() {
        let mut request = reducer_request();
        request.max_output_bytes = 64;
        let proposal = ContextReducerProposal {
            execution_id: "e1".into(),
            expected_projection: request.expected_projection.clone(),
            configuration_revision: request.configuration_revision.clone(),
            authority_revision: request.authority_revision.clone(),
            capability_generation: request.capability_generation.clone(),
            helper_reservation_id: request.helper_reservation_id.clone(),
            stage: request.stage,
            helper_attempt_id: "attempt-1".into(),
            retained_item_ids: vec!["history-1".into()],
            omitted_item_ids: Vec::new(),
            summaries: Vec::new(),
            encoded_output_bytes: 1,
        };

        assert!(matches!(
            proposal.validate_against(&request, &request.expected_projection),
            Err(ReducerValidationError::OutputBudgetExceeded {
                requested,
                allowed: 64
            }) if requested > 64
        ));
    }

    #[test]
    fn reducer_requires_an_explicit_decision_for_every_eligible_item() {
        let request = reducer_request();
        let proposal = ContextReducerProposal {
            execution_id: "e1".into(),
            expected_projection: request.expected_projection.clone(),
            configuration_revision: request.configuration_revision.clone(),
            authority_revision: request.authority_revision.clone(),
            capability_generation: request.capability_generation.clone(),
            helper_reservation_id: request.helper_reservation_id.clone(),
            stage: request.stage,
            helper_attempt_id: "attempt-1".into(),
            retained_item_ids: Vec::new(),
            omitted_item_ids: Vec::new(),
            summaries: Vec::new(),
            encoded_output_bytes: 1,
        };

        assert!(matches!(
            proposal.validate_against(&request, &request.expected_projection),
            Err(ReducerValidationError::MissingItemDecision { item_id })
                if item_id == "history-1"
        ));
    }

    #[test]
    fn compact_requires_exact_recovery() {
        let proposal = proposal(RetentionTransition {
            item_id: "item-1".into(),
            from: ContextRetention::Full,
            to: ContextRetention::Compact,
            recovery: None,
        });
        assert!(matches!(
            proposal.validate_against(&proposal.expected_projection),
            Err(CompactionValidationError::MissingRecoveryReference { .. })
        ));
    }

    #[test]
    fn pinned_items_cannot_be_compacted() {
        let proposal = proposal(RetentionTransition {
            item_id: "goal".into(),
            from: ContextRetention::Pinned,
            to: ContextRetention::Reference,
            recovery: Some(exact("context:goal")),
        });
        assert!(matches!(
            proposal.validate_against(&proposal.expected_projection),
            Err(CompactionValidationError::PinnedItemDemoted { .. })
        ));
    }

    #[test]
    fn stale_proposal_is_rejected() {
        let proposal = proposal(RetentionTransition {
            item_id: "item-1".into(),
            from: ContextRetention::Full,
            to: ContextRetention::Reference,
            recovery: Some(exact("context:item")),
        });
        let actual = ProjectionRevision {
            revision: 8,
            cache_epoch: 2,
        };
        assert!(matches!(
            proposal.validate_against(&actual),
            Err(CompactionValidationError::StaleProjection { .. })
        ));
    }
}
