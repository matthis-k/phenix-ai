use super::{ContextRetention, ExactContextReference};
use phenix_core::Bytes;
use serde::{Deserialize, Serialize};

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
