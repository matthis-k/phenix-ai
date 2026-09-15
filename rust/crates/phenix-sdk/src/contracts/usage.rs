use super::models::ModelTarget;
use phenix_core::CapabilityGenerationId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(
    Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum UsageQuantity {
    Reported {
        value: u64,
    },
    Estimated {
        value: u64,
        basis: String,
    },
    #[default]
    Unavailable,
}

impl UsageQuantity {
    #[must_use]
    pub const fn value(&self) -> Option<u64> {
        match self {
            Self::Reported { value } | Self::Estimated { value, .. } => Some(*value),
            Self::Unavailable => None,
        }
    }
}

#[derive(
    Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(deny_unknown_fields)]
pub struct ModelTurnUsage {
    pub fresh_input_tokens: UsageQuantity,
    pub cache_read_tokens: UsageQuantity,
    pub cache_write_tokens: UsageQuantity,
    pub output_tokens: UsageQuantity,
    pub reasoning_tokens: UsageQuantity,
}

#[derive(
    Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(deny_unknown_fields)]
pub struct BudgetReservation {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_microunits: Option<u64>,
}

impl BudgetReservation {
    #[must_use]
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum ContextControl {
    ReplaceableTurns,
    AppendOnlyWithResetReplay,
    OpaqueManaged,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ModelLimits {
    pub context_window_tokens: u64,
    pub max_output_tokens: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "knowledge", rename_all = "snake_case", deny_unknown_fields)]
pub enum CapacityKnowledge {
    Known { limits: ModelLimits },
    ConfiguredConservative { limits: ModelLimits },
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct EffectiveModelCapabilities {
    pub target: ModelTarget,
    pub generation: CapabilityGenerationId,
    pub context: ContextControl,
    pub capacity: CapacityKnowledge,
    #[serde(default)]
    pub optional: BTreeSet<String>,
}

#[derive(
    Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(deny_unknown_fields)]
pub struct ContextDemand {
    pub mandatory_input_tokens: u64,
    pub reducible_input_tokens: u64,
    pub output_reserve_tokens: u64,
    #[serde(default)]
    pub required_capabilities: BTreeSet<String>,
}

impl ContextDemand {
    #[must_use]
    pub fn total_input_tokens(&self) -> u64 {
        self.mandatory_input_tokens
            .saturating_add(self.reducible_input_tokens)
    }
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum UsageAttemptKind {
    Root,
    Retry,
    Helper,
    Delegated,
    Verification,
    RecoveryClassifier,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct UsageAttribution {
    pub root_execution_id: String,
    pub execution_id: String,
    pub attempt_id: String,
    pub parent_attempt_id: Option<String>,
    pub policy_revision: String,
    pub kind: UsageAttemptKind,
    pub task_id: Option<String>,
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum AttemptOutcome {
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ReacquisitionUsage {
    pub cause_identity: String,
    pub source_attempt_id: Option<String>,
    pub fresh_input_tokens: UsageQuantity,
    pub tool_result_bytes: u64,
    pub model_calls: u32,
    pub tool_calls: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct AttemptUsageRecord {
    pub attribution: UsageAttribution,
    pub usage: ModelTurnUsage,
    pub latency_ms: Option<u64>,
    pub tool_input_bytes: u64,
    pub tool_result_bytes: u64,
    pub outcome: AttemptOutcome,
    #[serde(default)]
    pub reacquisition: Vec<ReacquisitionUsage>,
}

#[derive(
    Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(deny_unknown_fields)]
pub struct UsageMetricAggregate {
    pub reported: u64,
    pub estimated: u64,
    pub unavailable_records: u32,
}

impl UsageMetricAggregate {
    pub fn observe(&mut self, quantity: &UsageQuantity) {
        match quantity {
            UsageQuantity::Reported { value } => {
                self.reported = self.reported.saturating_add(*value);
            }
            UsageQuantity::Estimated { value, .. } => {
                self.estimated = self.estimated.saturating_add(*value);
            }
            UsageQuantity::Unavailable => {
                self.unavailable_records = self.unavailable_records.saturating_add(1);
            }
        }
    }
}

#[derive(
    Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(deny_unknown_fields)]
pub struct UsageAggregate {
    pub fresh_input_tokens: UsageMetricAggregate,
    pub cache_read_tokens: UsageMetricAggregate,
    pub cache_write_tokens: UsageMetricAggregate,
    pub output_tokens: UsageMetricAggregate,
    pub reasoning_tokens: UsageMetricAggregate,
    pub tool_input_bytes: u64,
    pub tool_result_bytes: u64,
    pub reacquisition_fresh_input_tokens: UsageMetricAggregate,
    pub reacquisition_tool_result_bytes: u64,
    pub attempts: u32,
    pub failed_attempts: u32,
}

impl UsageAggregate {
    pub fn observe(&mut self, record: &AttemptUsageRecord) {
        self.fresh_input_tokens
            .observe(&record.usage.fresh_input_tokens);
        self.cache_read_tokens
            .observe(&record.usage.cache_read_tokens);
        self.cache_write_tokens
            .observe(&record.usage.cache_write_tokens);
        self.output_tokens.observe(&record.usage.output_tokens);
        self.reasoning_tokens
            .observe(&record.usage.reasoning_tokens);
        self.tool_input_bytes = self
            .tool_input_bytes
            .saturating_add(record.tool_input_bytes);
        self.tool_result_bytes = self
            .tool_result_bytes
            .saturating_add(record.tool_result_bytes);
        self.attempts = self.attempts.saturating_add(1);
        if record.outcome != AttemptOutcome::Succeeded {
            self.failed_attempts = self.failed_attempts.saturating_add(1);
        }
        for reacquisition in &record.reacquisition {
            self.reacquisition_fresh_input_tokens
                .observe(&reacquisition.fresh_input_tokens);
            self.reacquisition_tool_result_bytes = self
                .reacquisition_tool_result_bytes
                .saturating_add(reacquisition.tool_result_bytes);
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct UsageLedger {
    pub root_execution_id: String,
    #[serde(default)]
    pub records: Vec<AttemptUsageRecord>,
}

impl UsageLedger {
    #[must_use]
    pub fn aggregate(&self) -> UsageAggregate {
        let mut aggregate = UsageAggregate::default();
        for record in &self.records {
            aggregate.observe(record);
        }
        aggregate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_usage_is_not_zero() {
        assert_eq!(UsageQuantity::default(), UsageQuantity::Unavailable);
        assert_eq!(UsageQuantity::Unavailable.value(), None);
    }

    #[test]
    fn context_demand_keeps_mandatory_and_reducible_work_separate() {
        let demand = ContextDemand {
            mandatory_input_tokens: 100,
            reducible_input_tokens: 250,
            output_reserve_tokens: 50,
            required_capabilities: BTreeSet::new(),
        };
        assert_eq!(demand.total_input_tokens(), 350);
        assert_eq!(demand.mandatory_input_tokens, 100);
    }

    #[test]
    fn budget_reservation_keeps_input_and_output_accounting_distinct() {
        let reservation = BudgetReservation {
            input_tokens: 300,
            output_tokens: 100,
            cost_microunits: None,
        };
        assert_eq!(reservation.total_tokens(), 400);
        assert_eq!(reservation.input_tokens, 300);
        assert_eq!(reservation.output_tokens, 100);
    }

    #[test]
    fn aggregates_keep_reported_estimated_and_missing_usage_separate() {
        let mut metric = UsageMetricAggregate::default();
        metric.observe(&UsageQuantity::Reported { value: 10 });
        metric.observe(&UsageQuantity::Estimated {
            value: 20,
            basis: "tokenizer".into(),
        });
        metric.observe(&UsageQuantity::Unavailable);
        assert_eq!(metric.reported, 10);
        assert_eq!(metric.estimated, 20);
        assert_eq!(metric.unavailable_records, 1);
    }
}
