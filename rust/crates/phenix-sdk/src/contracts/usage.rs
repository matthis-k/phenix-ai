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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_usage_is_not_zero() {
        assert_eq!(UsageQuantity::default(), UsageQuantity::Unavailable);
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
}
