use super::{
    AttemptOutcome, AttemptUsageRecord, StepAttemptPhase, StepAttemptRecord, UsageAggregate,
    UsageQuantity,
};
use phenix_core::{ComponentInterface, InterfaceId, ServiceId};
use serde::{Deserialize, Serialize};

pub const EFFICIENCY_EVALUATION_SERVICE: &str = "phenix.efficiency-evaluation@1";
pub const EFFICIENCY_OUTCOME_EVIDENCE_SERVICE: &str = "phenix.efficiency-outcome-evidence@1";

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationOutcome {
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
    Unresolved,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ReacquisitionCauseAggregate {
    pub cause_identity: String,
    pub source_attempt_id: Option<String>,
    pub fresh_input_tokens: super::UsageMetricAggregate,
    pub tool_result_bytes: u64,
    pub model_calls: u32,
    pub tool_calls: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct EfficiencyOutcomeEvidence {
    pub source_identity: String,
    pub evaluator_identity: String,
    pub evidence_revision: String,
    pub outcome: EvaluationOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct EfficiencyTaskRecord {
    pub task_fixture_revision: String,
    pub root_execution_id: String,
    pub policy_revision: String,
    pub outcome_evaluator_identity: String,
    pub outcome_evidence_identity: String,
    pub outcome_evidence_revision: String,
    pub price_revision: String,
    pub outcome: EvaluationOutcome,
    pub usage: UsageAggregate,
    #[serde(default)]
    pub reacquisition_causes: Vec<ReacquisitionCauseAggregate>,
    pub known_cost_microunits: u64,
    pub cost_complete: bool,
    pub root_elapsed_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct EfficiencyAttemptCharge {
    pub record: AttemptUsageRecord,
    pub known_cost_microunits: u64,
    pub cost_complete: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct EfficiencyTaskEvidence {
    pub task_fixture_revision: String,
    pub root_execution_id: String,
    pub policy_revision: String,
    pub outcome_evaluator_identity: String,
    pub price_revision: String,
    pub outcome: EvaluationOutcome,
    pub outcome_evidence: EfficiencyOutcomeEvidence,
    #[serde(default)]
    pub attempts: Vec<EfficiencyAttemptCharge>,
    pub root_elapsed_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct EfficiencyDurableTaskEvidence {
    pub task_fixture_revision: String,
    pub root_execution_id: String,
    pub policy_revision: String,
    pub outcome_evaluator_identity: String,
    pub price_revision: String,
    pub outcome: EvaluationOutcome,
    pub outcome_evidence: EfficiencyOutcomeEvidence,
    #[serde(default)]
    pub attempts: Vec<StepAttemptRecord>,
    pub root_elapsed_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct EfficiencyCollectionRequest {
    pub task_fixture_revision: String,
    pub root_execution_id: String,
    pub policy_revision: String,
    pub outcome_evaluator_identity: String,
    pub price_revision: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct EfficiencyOutcomeEvidenceRequest {
    pub task_fixture_revision: String,
    pub root_execution_id: String,
    pub evaluator_identity: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum EfficiencyOutcomeEvidenceCommand {
    Resolve {
        request: EfficiencyOutcomeEvidenceRequest,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case", deny_unknown_fields)]
pub enum EfficiencyOutcomeEvidenceResponse {
    Evidence {
        evidence: Option<EfficiencyOutcomeEvidence>,
    },
}

pub struct EfficiencyOutcomeEvidenceInterface;

impl ComponentInterface for EfficiencyOutcomeEvidenceInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(EFFICIENCY_OUTCOME_EVIDENCE_SERVICE)
            .expect("static efficiency outcome evidence interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<
            EfficiencyOutcomeEvidenceCommand,
            EfficiencyOutcomeEvidenceResponse,
        >()
    }
}

#[must_use]
pub fn efficiency_outcome_evidence_service() -> ServiceId {
    ServiceId::parse(EFFICIENCY_OUTCOME_EVIDENCE_SERVICE)
        .expect("static efficiency outcome evidence service id is valid")
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum EfficiencyEvaluationCommand {
    CollectTask {
        request: EfficiencyCollectionRequest,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case", deny_unknown_fields)]
pub enum EfficiencyEvaluationResponse {
    Task { record: EfficiencyTaskRecord },
}

pub struct EfficiencyEvaluationInterface;

impl ComponentInterface for EfficiencyEvaluationInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(EFFICIENCY_EVALUATION_SERVICE)
            .expect("static efficiency evaluation interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<EfficiencyEvaluationCommand, EfficiencyEvaluationResponse>(
        )
    }
}

#[must_use]
pub fn efficiency_evaluation_service() -> ServiceId {
    ServiceId::parse(EFFICIENCY_EVALUATION_SERVICE)
        .expect("static efficiency evaluation service id is valid")
}

pub fn derive_efficiency_task_record_from_attempts(
    durable: &EfficiencyDurableTaskEvidence,
) -> Result<EfficiencyTaskRecord, EfficiencyEvaluationError> {
    let mut charges = Vec::with_capacity(durable.attempts.len());
    for attempt in &durable.attempts {
        if attempt.phase != StepAttemptPhase::Settled {
            return Err(EfficiencyEvaluationError::UnsettledAttempt {
                attempt_id: attempt.attribution.attempt_id.clone(),
            });
        }
        let (record, known_cost_microunits, cost_complete) =
            if let Some(usage) = &attempt.usage {
                let mut record = usage.clone();
                for reacquisition in &attempt.reacquisition {
                    if let Some(existing) = record.reacquisition.iter().find(|existing| {
                        existing.reacquisition_id == reacquisition.reacquisition_id
                    }) {
                        if existing != reacquisition {
                            return Err(EfficiencyEvaluationError::ReacquisitionIdentityConflict {
                                attempt_id: attempt.attribution.attempt_id.clone(),
                                reacquisition_id: reacquisition.reacquisition_id.clone(),
                            });
                        }
                    } else {
                        record.reacquisition.push(reacquisition.clone());
                    }
                }
                let cost = attempt
                    .settled_actual
                    .as_ref()
                    .and_then(|actual| actual.cost_microunits);
                (record, cost.unwrap_or(0), cost.is_some())
            } else if attempt.dispatch_id.is_none() {
                (
                    AttemptUsageRecord {
                        attribution: attempt.attribution.clone(),
                        usage: phenix_core::ModelTurnUsage {
                            fresh_input_tokens: UsageQuantity::Reported { value: 0 },
                            cache_read_tokens: UsageQuantity::Reported { value: 0 },
                            cache_write_tokens: UsageQuantity::Reported { value: 0 },
                            output_tokens: UsageQuantity::Reported { value: 0 },
                            reasoning_tokens: UsageQuantity::Reported { value: 0 },
                        },
                        latency_ms: None,
                        tool_input_bytes: 0,
                        tool_result_bytes: 0,
                        outcome: attempt.outcome.unwrap_or(AttemptOutcome::Failed),
                        reacquisition: Vec::new(),
                    },
                    0,
                    true,
                )
            } else {
                (
                    AttemptUsageRecord {
                        attribution: attempt.attribution.clone(),
                        usage: phenix_core::ModelTurnUsage {
                            fresh_input_tokens: UsageQuantity::Unavailable,
                            cache_read_tokens: UsageQuantity::Unavailable,
                            cache_write_tokens: UsageQuantity::Unavailable,
                            output_tokens: UsageQuantity::Unavailable,
                            reasoning_tokens: UsageQuantity::Unavailable,
                        },
                        latency_ms: None,
                        tool_input_bytes: 0,
                        tool_result_bytes: 0,
                        outcome: attempt.outcome.unwrap_or(AttemptOutcome::Failed),
                        reacquisition: Vec::new(),
                    },
                    attempt
                        .settled_actual
                        .as_ref()
                        .and_then(|actual| actual.cost_microunits)
                        .unwrap_or(0),
                    false,
                )
            };
        charges.push(EfficiencyAttemptCharge {
            record,
            known_cost_microunits,
            cost_complete,
        });
    }

    derive_efficiency_task_record(&EfficiencyTaskEvidence {
        task_fixture_revision: durable.task_fixture_revision.clone(),
        root_execution_id: durable.root_execution_id.clone(),
        policy_revision: durable.policy_revision.clone(),
        outcome_evaluator_identity: durable.outcome_evaluator_identity.clone(),
        price_revision: durable.price_revision.clone(),
        outcome: durable.outcome,
        outcome_evidence: durable.outcome_evidence.clone(),
        attempts: charges,
        root_elapsed_ms: durable.root_elapsed_ms,
    })
}

pub fn derive_efficiency_task_record(
    evidence: &EfficiencyTaskEvidence,
) -> Result<EfficiencyTaskRecord, EfficiencyEvaluationError> {
    if evidence.outcome_evidence.source_identity.trim().is_empty()
        || evidence
            .outcome_evidence
            .evidence_revision
            .trim()
            .is_empty()
    {
        return Err(EfficiencyEvaluationError::InvalidOutcomeEvidence);
    }
    if evidence.outcome_evidence.evaluator_identity != evidence.outcome_evaluator_identity {
        return Err(
            EfficiencyEvaluationError::OutcomeEvidenceEvaluatorMismatch {
                expected: evidence.outcome_evaluator_identity.clone(),
                observed: evidence.outcome_evidence.evaluator_identity.clone(),
            },
        );
    }
    if evidence.outcome_evidence.outcome != evidence.outcome {
        return Err(EfficiencyEvaluationError::OutcomeEvidenceOutcomeMismatch {
            expected: evidence.outcome,
            observed: evidence.outcome_evidence.outcome,
        });
    }

    let mut seen_attempts = std::collections::BTreeSet::new();
    let mut usage = UsageAggregate::default();
    let mut reacquisition_causes =
        std::collections::BTreeMap::<(String, Option<String>), ReacquisitionCauseAggregate>::new();
    let mut known_cost_microunits = 0_u64;
    let mut cost_complete = true;

    for charge in &evidence.attempts {
        let attribution = &charge.record.attribution;
        if attribution.root_execution_id != evidence.root_execution_id {
            return Err(EfficiencyEvaluationError::AttemptRootMismatch {
                expected: evidence.root_execution_id.clone(),
                observed: attribution.root_execution_id.clone(),
                attempt_id: attribution.attempt_id.clone(),
            });
        }
        if attribution.policy_revision != evidence.policy_revision {
            return Err(EfficiencyEvaluationError::AttemptPolicyMismatch {
                expected: evidence.policy_revision.clone(),
                observed: attribution.policy_revision.clone(),
                attempt_id: attribution.attempt_id.clone(),
            });
        }
        if !seen_attempts.insert(attribution.attempt_id.clone()) {
            return Err(EfficiencyEvaluationError::DuplicateAttempt {
                attempt_id: attribution.attempt_id.clone(),
            });
        }

        usage.observe(&charge.record);
        for reacquisition in &charge.record.reacquisition {
            let key = (
                reacquisition.cause_identity.clone(),
                reacquisition.source_attempt_id.clone(),
            );
            let aggregate = reacquisition_causes.entry(key.clone()).or_insert_with(|| {
                ReacquisitionCauseAggregate {
                    cause_identity: key.0,
                    source_attempt_id: key.1,
                    fresh_input_tokens: super::UsageMetricAggregate::default(),
                    tool_result_bytes: 0,
                    model_calls: 0,
                    tool_calls: 0,
                }
            });
            aggregate
                .fresh_input_tokens
                .observe(&reacquisition.fresh_input_tokens);
            aggregate.tool_result_bytes = aggregate
                .tool_result_bytes
                .saturating_add(reacquisition.tool_result_bytes);
            aggregate.model_calls = aggregate
                .model_calls
                .saturating_add(reacquisition.model_calls);
            aggregate.tool_calls = aggregate
                .tool_calls
                .saturating_add(reacquisition.tool_calls);
        }
        known_cost_microunits = known_cost_microunits.saturating_add(charge.known_cost_microunits);
        cost_complete &= charge.cost_complete;
    }

    Ok(EfficiencyTaskRecord {
        task_fixture_revision: evidence.task_fixture_revision.clone(),
        root_execution_id: evidence.root_execution_id.clone(),
        policy_revision: evidence.policy_revision.clone(),
        outcome_evaluator_identity: evidence.outcome_evaluator_identity.clone(),
        outcome_evidence_identity: evidence.outcome_evidence.source_identity.clone(),
        outcome_evidence_revision: evidence.outcome_evidence.evidence_revision.clone(),
        price_revision: evidence.price_revision.clone(),
        outcome: evidence.outcome,
        usage,
        reacquisition_causes: reacquisition_causes.into_values().collect(),
        known_cost_microunits,
        cost_complete,
        root_elapsed_ms: evidence.root_elapsed_ms,
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct CostPerSuccess {
    pub total_cost_microunits: u64,
    pub successful_tasks: u32,
}

impl CostPerSuccess {
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        (self.successful_tasks != 0)
            .then(|| self.total_cost_microunits as f64 / f64::from(self.successful_tasks))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct EfficiencyCohortReport {
    pub policy_revision: String,
    pub outcome_evaluator_identity: String,
    pub price_revision: String,
    pub total_tasks: u32,
    pub resolved_tasks: u32,
    pub successful_tasks: u32,
    pub failed_tasks: u32,
    pub cancelled_tasks: u32,
    pub timed_out_tasks: u32,
    pub unresolved_tasks: u32,
    pub known_cost_microunits: u64,
    pub incomplete_cost_records: u32,
    pub usage: UsageAggregate,
    #[serde(default)]
    pub reacquisition_causes: Vec<ReacquisitionCauseAggregate>,
    pub cost_per_success: Option<CostPerSuccess>,
    pub rollout_comparable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum EfficiencyEvaluationError {
    EmptyCohort,
    InvalidOutcomeEvidence,
    OutcomeEvidenceEvaluatorMismatch {
        expected: String,
        observed: String,
    },
    OutcomeEvidenceOutcomeMismatch {
        expected: EvaluationOutcome,
        observed: EvaluationOutcome,
    },
    MixedPolicyRevision {
        expected: String,
        observed: String,
    },
    MixedOutcomeEvaluator {
        expected: String,
        observed: String,
    },
    MixedPriceRevision {
        expected: String,
        observed: String,
    },
    MismatchedTaskSet,
    ComparisonOutcomeEvaluatorMismatch {
        baseline: String,
        candidate: String,
    },
    ComparisonPriceRevisionMismatch {
        baseline: String,
        candidate: String,
    },
    CohortTooLarge,
    AttemptRootMismatch {
        expected: String,
        observed: String,
        attempt_id: String,
    },
    AttemptPolicyMismatch {
        expected: String,
        observed: String,
        attempt_id: String,
    },
    DuplicateAttempt {
        attempt_id: String,
    },
    ReacquisitionIdentityConflict {
        attempt_id: String,
        reacquisition_id: String,
    },
    UnsettledAttempt {
        attempt_id: String,
    },
}

pub fn evaluate_efficiency_cohort(
    records: &[EfficiencyTaskRecord],
) -> Result<EfficiencyCohortReport, EfficiencyEvaluationError> {
    let Some(first) = records.first() else {
        return Err(EfficiencyEvaluationError::EmptyCohort);
    };
    let total_tasks =
        u32::try_from(records.len()).map_err(|_| EfficiencyEvaluationError::CohortTooLarge)?;
    let mut successful_tasks = 0_u32;
    let mut failed_tasks = 0_u32;
    let mut cancelled_tasks = 0_u32;
    let mut timed_out_tasks = 0_u32;
    let mut unresolved_tasks = 0_u32;
    let mut incomplete_cost_records = 0_u32;
    let mut known_cost_microunits = 0_u64;
    let mut usage = UsageAggregate::default();
    let mut reacquisition_causes =
        std::collections::BTreeMap::<(String, Option<String>), ReacquisitionCauseAggregate>::new();

    for record in records {
        if record.policy_revision != first.policy_revision {
            return Err(EfficiencyEvaluationError::MixedPolicyRevision {
                expected: first.policy_revision.clone(),
                observed: record.policy_revision.clone(),
            });
        }
        if record.outcome_evaluator_identity != first.outcome_evaluator_identity {
            return Err(EfficiencyEvaluationError::MixedOutcomeEvaluator {
                expected: first.outcome_evaluator_identity.clone(),
                observed: record.outcome_evaluator_identity.clone(),
            });
        }
        if record.price_revision != first.price_revision {
            return Err(EfficiencyEvaluationError::MixedPriceRevision {
                expected: first.price_revision.clone(),
                observed: record.price_revision.clone(),
            });
        }
        known_cost_microunits = known_cost_microunits.saturating_add(record.known_cost_microunits);
        merge_usage(&mut usage, &record.usage);
        for source in &record.reacquisition_causes {
            let key = (
                source.cause_identity.clone(),
                source.source_attempt_id.clone(),
            );
            let target = reacquisition_causes.entry(key.clone()).or_insert_with(|| {
                ReacquisitionCauseAggregate {
                    cause_identity: key.0,
                    source_attempt_id: key.1,
                    fresh_input_tokens: super::UsageMetricAggregate::default(),
                    tool_result_bytes: 0,
                    model_calls: 0,
                    tool_calls: 0,
                }
            });
            merge_metric(&mut target.fresh_input_tokens, &source.fresh_input_tokens);
            target.tool_result_bytes = target
                .tool_result_bytes
                .saturating_add(source.tool_result_bytes);
            target.model_calls = target.model_calls.saturating_add(source.model_calls);
            target.tool_calls = target.tool_calls.saturating_add(source.tool_calls);
        }
        if !record.cost_complete {
            incomplete_cost_records = incomplete_cost_records.saturating_add(1);
        }
        match record.outcome {
            EvaluationOutcome::Succeeded => {
                successful_tasks = successful_tasks.saturating_add(1);
            }
            EvaluationOutcome::Failed => {
                failed_tasks = failed_tasks.saturating_add(1);
            }
            EvaluationOutcome::Cancelled => {
                cancelled_tasks = cancelled_tasks.saturating_add(1);
            }
            EvaluationOutcome::TimedOut => {
                timed_out_tasks = timed_out_tasks.saturating_add(1);
            }
            EvaluationOutcome::Unresolved => {
                unresolved_tasks = unresolved_tasks.saturating_add(1);
            }
        }
    }

    let resolved_tasks = total_tasks.saturating_sub(unresolved_tasks);
    let rollout_comparable =
        unresolved_tasks == 0 && incomplete_cost_records == 0 && successful_tasks > 0;
    let cost_per_success = rollout_comparable.then_some(CostPerSuccess {
        total_cost_microunits: known_cost_microunits,
        successful_tasks,
    });

    Ok(EfficiencyCohortReport {
        policy_revision: first.policy_revision.clone(),
        outcome_evaluator_identity: first.outcome_evaluator_identity.clone(),
        price_revision: first.price_revision.clone(),
        total_tasks,
        resolved_tasks,
        successful_tasks,
        failed_tasks,
        cancelled_tasks,
        timed_out_tasks,
        unresolved_tasks,
        known_cost_microunits,
        incomplete_cost_records,
        usage,
        reacquisition_causes: reacquisition_causes.into_values().collect(),
        cost_per_success,
        rollout_comparable,
    })
}

fn merge_metric(target: &mut super::UsageMetricAggregate, source: &super::UsageMetricAggregate) {
    target.reported = target.reported.saturating_add(source.reported);
    target.estimated = target.estimated.saturating_add(source.estimated);
    target.unavailable_records = target
        .unavailable_records
        .saturating_add(source.unavailable_records);
}

fn merge_usage(target: &mut UsageAggregate, source: &UsageAggregate) {
    merge_metric(&mut target.fresh_input_tokens, &source.fresh_input_tokens);
    merge_metric(&mut target.cache_read_tokens, &source.cache_read_tokens);
    merge_metric(&mut target.cache_write_tokens, &source.cache_write_tokens);
    merge_metric(&mut target.output_tokens, &source.output_tokens);
    merge_metric(&mut target.reasoning_tokens, &source.reasoning_tokens);
    target.tool_input_bytes = target
        .tool_input_bytes
        .saturating_add(source.tool_input_bytes);
    target.tool_result_bytes = target
        .tool_result_bytes
        .saturating_add(source.tool_result_bytes);
    merge_metric(
        &mut target.reacquisition_fresh_input_tokens,
        &source.reacquisition_fresh_input_tokens,
    );
    target.reacquisition_tool_result_bytes = target
        .reacquisition_tool_result_bytes
        .saturating_add(source.reacquisition_tool_result_bytes);
    target.attempts = target.attempts.saturating_add(source.attempts);
    target.failed_attempts = target
        .failed_attempts
        .saturating_add(source.failed_attempts);
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct EfficiencyPolicyComparison {
    pub baseline: EfficiencyCohortReport,
    pub candidate: EfficiencyCohortReport,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct EfficiencyPolicyVariant {
    pub label: String,
    #[serde(default)]
    pub active_reduction_stages: std::collections::BTreeSet<String>,
    pub records: Vec<EfficiencyTaskRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct EfficiencyVariantComparison {
    pub label: String,
    pub active_reduction_stages: std::collections::BTreeSet<String>,
    pub added_stages: std::collections::BTreeSet<String>,
    pub removed_stages: std::collections::BTreeSet<String>,
    pub comparison: EfficiencyPolicyComparison,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct EfficiencyVariantSetReport {
    pub baseline_label: String,
    pub baseline_stages: std::collections::BTreeSet<String>,
    pub baseline: EfficiencyCohortReport,
    pub variants: Vec<EfficiencyVariantComparison>,
}

pub fn compare_efficiency_policies(
    baseline: &[EfficiencyTaskRecord],
    candidate: &[EfficiencyTaskRecord],
) -> Result<EfficiencyPolicyComparison, EfficiencyEvaluationError> {
    let cohort_shape = |records: &[EfficiencyTaskRecord]| {
        let mut roots = std::collections::BTreeSet::new();
        let mut fixture_counts = std::collections::BTreeMap::<String, u32>::new();
        for record in records {
            if !roots.insert(record.root_execution_id.as_str()) {
                return None;
            }
            *fixture_counts
                .entry(record.task_fixture_revision.clone())
                .or_default() += 1;
        }
        Some(fixture_counts)
    };
    let Some(baseline_shape) = cohort_shape(baseline) else {
        return Err(EfficiencyEvaluationError::MismatchedTaskSet);
    };
    let Some(candidate_shape) = cohort_shape(candidate) else {
        return Err(EfficiencyEvaluationError::MismatchedTaskSet);
    };
    if baseline_shape != candidate_shape {
        return Err(EfficiencyEvaluationError::MismatchedTaskSet);
    }

    let baseline = evaluate_efficiency_cohort(baseline)?;
    let candidate = evaluate_efficiency_cohort(candidate)?;
    if baseline.outcome_evaluator_identity != candidate.outcome_evaluator_identity {
        return Err(
            EfficiencyEvaluationError::ComparisonOutcomeEvaluatorMismatch {
                baseline: baseline.outcome_evaluator_identity,
                candidate: candidate.outcome_evaluator_identity,
            },
        );
    }
    if baseline.price_revision != candidate.price_revision {
        return Err(EfficiencyEvaluationError::ComparisonPriceRevisionMismatch {
            baseline: baseline.price_revision,
            candidate: candidate.price_revision,
        });
    }

    Ok(EfficiencyPolicyComparison {
        baseline,
        candidate,
    })
}

pub fn compare_efficiency_variant_set(
    baseline: &EfficiencyPolicyVariant,
    variants: &[EfficiencyPolicyVariant],
) -> Result<EfficiencyVariantSetReport, EfficiencyEvaluationError> {
    let baseline_report = evaluate_efficiency_cohort(&baseline.records)?;
    let mut comparisons = Vec::with_capacity(variants.len());
    for variant in variants {
        let comparison = compare_efficiency_policies(&baseline.records, &variant.records)?;
        let added_stages = variant
            .active_reduction_stages
            .difference(&baseline.active_reduction_stages)
            .cloned()
            .collect();
        let removed_stages = baseline
            .active_reduction_stages
            .difference(&variant.active_reduction_stages)
            .cloned()
            .collect();
        comparisons.push(EfficiencyVariantComparison {
            label: variant.label.clone(),
            active_reduction_stages: variant.active_reduction_stages.clone(),
            added_stages,
            removed_stages,
            comparison,
        });
    }
    Ok(EfficiencyVariantSetReport {
        baseline_label: baseline.label.clone(),
        baseline_stages: baseline.active_reduction_stages.clone(),
        baseline: baseline_report,
        variants: comparisons,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempt(
        root_execution_id: &str,
        attempt_id: &str,
        kind: super::super::UsageAttemptKind,
        outcome: super::super::AttemptOutcome,
        cost: u64,
    ) -> EfficiencyAttemptCharge {
        EfficiencyAttemptCharge {
            record: AttemptUsageRecord {
                attribution: super::super::UsageAttribution {
                    root_execution_id: root_execution_id.into(),
                    execution_id: format!("execution-{attempt_id}"),
                    attempt_id: attempt_id.into(),
                    parent_attempt_id: None,
                    policy_revision: "policy-1".into(),
                    kind,
                    task_id: Some("task-0".into()),
                },
                usage: phenix_core::ModelTurnUsage {
                    fresh_input_tokens: phenix_core::UsageQuantity::Reported { value: 10 },
                    cache_read_tokens: phenix_core::UsageQuantity::Reported { value: 0 },
                    cache_write_tokens: phenix_core::UsageQuantity::Reported { value: 0 },
                    output_tokens: phenix_core::UsageQuantity::Reported { value: 5 },
                    reasoning_tokens: phenix_core::UsageQuantity::Reported { value: 0 },
                },
                latency_ms: Some(100),
                tool_input_bytes: 0,
                tool_result_bytes: 0,
                outcome,
                reacquisition: Vec::new(),
            },
            known_cost_microunits: cost,
            cost_complete: true,
        }
    }

    fn durable_plan() -> super::super::StepPlan {
        let context = super::super::ContextDemand {
            mandatory_input_tokens: 10,
            reducible_input_tokens: 0,
            output_reserve_tokens: 5,
            required_capabilities: std::collections::BTreeSet::new(),
        };
        super::super::StepPlan {
            policy_revision: "policy-1".into(),
            historical_estimator_snapshot: None,
            routing: super::super::RoutingRequirements {
                context: context.clone(),
                required_capabilities: std::collections::BTreeSet::new(),
                require_known_capacity: false,
            },
            context,
            reasoning: super::super::ReasoningBudget::BackendDefault,
            tools: super::super::ToolProvisionBudget {
                initial: std::collections::BTreeSet::new(),
                expandable: std::collections::BTreeSet::new(),
                max_schemas: 0,
                max_result_bytes: 0,
            },
            skills: super::super::SkillProvisionBudget {
                initial: std::collections::BTreeSet::new(),
                expandable: std::collections::BTreeSet::new(),
                max_loaded: 0,
            },
            delegation: super::super::DelegationResourcePolicy::default(),
            retry: super::super::RetryBudget {
                max_attempts: 1,
                reserved_attempts: 1,
            },
            reservation: super::super::BudgetReservation {
                input_tokens: 10,
                output_tokens: 5,
                cost_microunits: Some(123),
            },
            deadline_at_ms: None,
            reducible_input_dropped_tokens: 0,
        }
    }

    fn durable_route() -> super::super::RouteDecision {
        super::super::RouteDecision {
            target: super::super::ModelTarget {
                provider_plugin: phenix_core::PluginId::parse("provider.fixture").unwrap(),
                model: phenix_core::ModelId::parse("model.fixture").unwrap(),
                options: std::collections::BTreeMap::new(),
            },
            capability_generation: phenix_core::CapabilityGenerationId::parse("generation-1")
                .unwrap(),
            policy_revision: "route-policy-1".into(),
            candidate_ordinal: 0,
            estimate: None,
        }
    }

    fn task(id: usize, outcome: EvaluationOutcome, cost: u64) -> EfficiencyTaskRecord {
        EfficiencyTaskRecord {
            task_fixture_revision: format!("task-{id}@1"),
            root_execution_id: format!("execution-{id}"),
            policy_revision: "policy-1".into(),
            outcome_evaluator_identity: "tests-v1".into(),
            outcome_evidence_identity: format!("tests-v1/task-{id}"),
            outcome_evidence_revision: "result-v1".into(),
            price_revision: "prices-v1".into(),
            outcome,
            usage: UsageAggregate::default(),
            reacquisition_causes: Vec::new(),
            known_cost_microunits: cost,
            cost_complete: true,
            root_elapsed_ms: Some(1_000),
        }
    }

    #[test]
    fn variant_set_reports_one_stage_and_combined_profiles_explicitly() {
        let baseline = EfficiencyPolicyVariant {
            label: "baseline".into(),
            active_reduction_stages: std::collections::BTreeSet::new(),
            records: vec![task(0, EvaluationOutcome::Succeeded, 10)],
        };
        let lazy_tools = EfficiencyPolicyVariant {
            label: "lazy-tools".into(),
            active_reduction_stages: std::collections::BTreeSet::from(["lazy_tools".into()]),
            records: vec![{
                let mut record = task(0, EvaluationOutcome::Succeeded, 8);
                record.policy_revision = "policy-lazy-tools".into();
                record
            }],
        };
        let combined = EfficiencyPolicyVariant {
            label: "combined".into(),
            active_reduction_stages: std::collections::BTreeSet::from([
                "lazy_tools".into(),
                "cache_retention".into(),
            ]),
            records: vec![{
                let mut record = task(0, EvaluationOutcome::Succeeded, 7);
                record.policy_revision = "policy-combined".into();
                record
            }],
        };

        let report = compare_efficiency_variant_set(&baseline, &[lazy_tools, combined]).unwrap();
        assert_eq!(report.variants.len(), 2);
        assert_eq!(
            report.variants[0].added_stages,
            std::collections::BTreeSet::from(["lazy_tools".into()])
        );
        assert_eq!(
            report.variants[1].added_stages,
            std::collections::BTreeSet::from(["cache_retention".into(), "lazy_tools".into()])
        );
    }

    #[test]
    fn task_derivation_counts_root_helper_and_delegated_attempts_once() {
        let evidence = EfficiencyTaskEvidence {
            task_fixture_revision: "task-0@1".into(),
            root_execution_id: "root-1".into(),
            policy_revision: "policy-1".into(),
            outcome_evaluator_identity: "tests-v1".into(),
            price_revision: "prices-v1".into(),
            outcome: EvaluationOutcome::Succeeded,
            outcome_evidence: EfficiencyOutcomeEvidence {
                source_identity: "tests-v1/task-0".into(),
                evaluator_identity: "tests-v1".into(),
                evidence_revision: "result-v1".into(),
                outcome: EvaluationOutcome::Succeeded,
            },
            attempts: vec![
                attempt(
                    "root-1",
                    "root-attempt",
                    super::super::UsageAttemptKind::Root,
                    super::super::AttemptOutcome::Succeeded,
                    10,
                ),
                attempt(
                    "root-1",
                    "helper-attempt",
                    super::super::UsageAttemptKind::Helper,
                    super::super::AttemptOutcome::Succeeded,
                    4,
                ),
                attempt(
                    "root-1",
                    "delegated-attempt",
                    super::super::UsageAttemptKind::Delegated,
                    super::super::AttemptOutcome::Failed,
                    7,
                ),
            ],
            root_elapsed_ms: Some(250),
        };

        let record = derive_efficiency_task_record(&evidence).unwrap();
        assert_eq!(record.known_cost_microunits, 21);
        assert_eq!(record.usage.attempts, 3);
        assert_eq!(record.usage.failed_attempts, 1);
        assert_eq!(record.usage.fresh_input_tokens.reported, 30);
        assert_eq!(record.usage.output_tokens.reported, 15);
        assert!(record.cost_complete);
    }

    #[test]
    fn durable_attempt_derivation_uses_atomic_route_projection_usage_and_cost_facts() {
        let attribution = super::super::UsageAttribution {
            root_execution_id: "root-1".into(),
            execution_id: "root-1".into(),
            attempt_id: "attempt-1".into(),
            parent_attempt_id: None,
            policy_revision: "policy-1".into(),
            kind: super::super::UsageAttemptKind::Root,
            task_id: Some("task-1".into()),
        };
        let mut attempt =
            super::super::StepAttemptRecord::new(attribution.clone(), durable_plan()).unwrap();
        attempt.bind_reservation("reservation-1".into()).unwrap();
        attempt.bind_route(durable_route()).unwrap();
        attempt
            .bind_projection(super::super::ProjectionRevision {
                revision: 3,
                cache_epoch: 2,
            })
            .unwrap();
        attempt.mark_dispatched("dispatch-1".into()).unwrap();
        let usage = AttemptUsageRecord {
            attribution,
            usage: phenix_core::ModelTurnUsage {
                fresh_input_tokens: UsageQuantity::Reported { value: 10 },
                cache_read_tokens: UsageQuantity::Reported { value: 20 },
                cache_write_tokens: UsageQuantity::Reported { value: 0 },
                output_tokens: UsageQuantity::Reported { value: 5 },
                reasoning_tokens: UsageQuantity::Reported { value: 2 },
            },
            latency_ms: Some(30),
            tool_input_bytes: 0,
            tool_result_bytes: 0,
            outcome: AttemptOutcome::Succeeded,
            reacquisition: Vec::new(),
        };
        attempt
            .settle_with_usage(
                AttemptOutcome::Succeeded,
                super::super::BudgetActual {
                    fresh_input_tokens: 10,
                    output_tokens: 5,
                    cost_microunits: Some(123),
                    attempts: 1,
                },
                usage,
            )
            .unwrap();

        let record = derive_efficiency_task_record_from_attempts(&EfficiencyDurableTaskEvidence {
            task_fixture_revision: "task-1@1".into(),
            root_execution_id: "root-1".into(),
            policy_revision: "policy-1".into(),
            outcome_evaluator_identity: "tests-v1".into(),
            price_revision: "prices-v1".into(),
            outcome: EvaluationOutcome::Succeeded,
            outcome_evidence: EfficiencyOutcomeEvidence {
                source_identity: "tests/task-1".into(),
                evaluator_identity: "tests-v1".into(),
                evidence_revision: "result-v1".into(),
                outcome: EvaluationOutcome::Succeeded,
            },
            attempts: vec![attempt],
            root_elapsed_ms: Some(30),
        })
        .unwrap();

        assert_eq!(record.known_cost_microunits, 123);
        assert!(record.cost_complete);
        assert_eq!(record.usage.fresh_input_tokens.reported, 10);
        assert_eq!(record.usage.cache_read_tokens.reported, 20);
    }

    #[test]
    fn durable_derivation_counts_delegated_attempt_work_once() {
        fn settled_attempt(
            attribution: super::super::UsageAttribution,
            cost_microunits: u64,
            input_tokens: u64,
            output_tokens: u64,
        ) -> super::super::StepAttemptRecord {
            let mut attempt =
                super::super::StepAttemptRecord::new(attribution.clone(), durable_plan()).unwrap();
            attempt
                .bind_reservation(format!("reservation-{}", attribution.attempt_id))
                .unwrap();
            attempt.bind_route(durable_route()).unwrap();
            attempt
                .bind_projection(super::super::ProjectionRevision {
                    revision: 1,
                    cache_epoch: 1,
                })
                .unwrap();
            attempt
                .mark_dispatched(format!("dispatch-{}", attribution.attempt_id))
                .unwrap();
            attempt
                .settle_with_usage(
                    AttemptOutcome::Succeeded,
                    super::super::BudgetActual {
                        fresh_input_tokens: input_tokens,
                        output_tokens,
                        cost_microunits: Some(cost_microunits),
                        attempts: 1,
                    },
                    AttemptUsageRecord {
                        attribution,
                        usage: phenix_core::ModelTurnUsage {
                            fresh_input_tokens: UsageQuantity::Reported {
                                value: input_tokens,
                            },
                            cache_read_tokens: UsageQuantity::Reported { value: 0 },
                            cache_write_tokens: UsageQuantity::Reported { value: 0 },
                            output_tokens: UsageQuantity::Reported {
                                value: output_tokens,
                            },
                            reasoning_tokens: UsageQuantity::Reported { value: 0 },
                        },
                        latency_ms: None,
                        tool_input_bytes: 0,
                        tool_result_bytes: 0,
                        outcome: AttemptOutcome::Succeeded,
                        reacquisition: Vec::new(),
                    },
                )
                .unwrap();
            attempt
        }

        let mut root = settled_attempt(
            super::super::UsageAttribution {
                root_execution_id: "root-1".into(),
                execution_id: "root-1".into(),
                attempt_id: "root-attempt".into(),
                parent_attempt_id: None,
                policy_revision: "policy-1".into(),
                kind: super::super::UsageAttemptKind::Root,
                task_id: None,
            },
            100,
            20,
            5,
        );
        let delegated = settled_attempt(
            super::super::UsageAttribution {
                root_execution_id: "root-1".into(),
                execution_id: "delegated:task-1".into(),
                attempt_id: "delegated-attempt".into(),
                parent_attempt_id: Some("root-attempt".into()),
                policy_revision: "policy-1".into(),
                kind: super::super::UsageAttemptKind::Delegated,
                task_id: Some("task-1".into()),
            },
            40,
            7,
            3,
        );
        root.record_reacquisition(super::super::ReacquisitionUsage {
            reacquisition_id: "delegation:task-1:parent-context".into(),
            cause_identity: "delegation:task-1".into(),
            source_attempt_id: Some("delegated-attempt".into()),
            fresh_input_tokens: UsageQuantity::Estimated {
                value: 3,
                basis: "delegated result context admission".into(),
            },
            tool_result_bytes: 0,
            model_calls: 0,
            tool_calls: 0,
        })
        .unwrap();

        let record = derive_efficiency_task_record_from_attempts(&EfficiencyDurableTaskEvidence {
            task_fixture_revision: "task-1@1".into(),
            root_execution_id: "root-1".into(),
            policy_revision: "policy-1".into(),
            outcome_evaluator_identity: "tests-v1".into(),
            price_revision: "prices-v1".into(),
            outcome: EvaluationOutcome::Succeeded,
            outcome_evidence: EfficiencyOutcomeEvidence {
                source_identity: "tests/task-1".into(),
                evaluator_identity: "tests-v1".into(),
                evidence_revision: "result-v1".into(),
                outcome: EvaluationOutcome::Succeeded,
            },
            attempts: vec![root, delegated],
            root_elapsed_ms: None,
        })
        .unwrap();

        assert_eq!(record.known_cost_microunits, 140);
        assert_eq!(record.usage.attempts, 2);
        assert_eq!(record.usage.fresh_input_tokens.reported, 27);
        assert_eq!(record.usage.output_tokens.reported, 8);
        assert_eq!(record.reacquisition_causes.len(), 1);
        assert_eq!(
            record.reacquisition_causes[0].cause_identity,
            "delegation:task-1"
        );
        assert_eq!(
            record.reacquisition_causes[0].source_attempt_id.as_deref(),
            Some("delegated-attempt")
        );
        assert_eq!(
            record.reacquisition_causes[0].fresh_input_tokens.estimated,
            3
        );
        assert!(record.cost_complete);
    }

    #[test]
    fn task_derivation_requires_matching_terminal_outcome_evidence() {
        let mut evidence = EfficiencyTaskEvidence {
            task_fixture_revision: "task-0@1".into(),
            root_execution_id: "root-1".into(),
            policy_revision: "policy-1".into(),
            outcome_evaluator_identity: "tests-v1".into(),
            price_revision: "prices-v1".into(),
            outcome: EvaluationOutcome::Succeeded,
            outcome_evidence: EfficiencyOutcomeEvidence {
                source_identity: "tests-v1/task-0".into(),
                evaluator_identity: "tests-v1".into(),
                evidence_revision: "result-v1".into(),
                outcome: EvaluationOutcome::Succeeded,
            },
            attempts: Vec::new(),
            root_elapsed_ms: Some(250),
        };
        assert!(derive_efficiency_task_record(&evidence).is_ok());

        evidence.outcome_evidence.evaluator_identity = "other-evaluator".into();
        assert!(matches!(
            derive_efficiency_task_record(&evidence),
            Err(EfficiencyEvaluationError::OutcomeEvidenceEvaluatorMismatch { .. })
        ));
        evidence.outcome_evidence.evaluator_identity = "tests-v1".into();
        evidence.outcome_evidence.outcome = EvaluationOutcome::Failed;
        assert!(matches!(
            derive_efficiency_task_record(&evidence),
            Err(EfficiencyEvaluationError::OutcomeEvidenceOutcomeMismatch { .. })
        ));
    }

    #[test]
    fn task_derivation_preserves_reacquisition_cause_attribution() {
        let mut charged = attempt(
            "root-1",
            "retry-attempt",
            super::super::UsageAttemptKind::Retry,
            super::super::AttemptOutcome::Succeeded,
            3,
        );
        charged
            .record
            .reacquisition
            .push(super::super::ReacquisitionUsage {
                cause_identity: "context-reduction:checkpoint-7".into(),
                source_attempt_id: Some("root-attempt".into()),
                fresh_input_tokens: super::super::UsageQuantity::Reported { value: 11 },
                tool_result_bytes: 120,
                model_calls: 1,
                tool_calls: 2,
            });
        let evidence = EfficiencyTaskEvidence {
            task_fixture_revision: "task-0@1".into(),
            root_execution_id: "root-1".into(),
            policy_revision: "policy-1".into(),
            outcome_evaluator_identity: "tests-v1".into(),
            price_revision: "prices-v1".into(),
            outcome: EvaluationOutcome::Succeeded,
            outcome_evidence: EfficiencyOutcomeEvidence {
                source_identity: "tests-v1/task-0".into(),
                evaluator_identity: "tests-v1".into(),
                evidence_revision: "result-v1".into(),
                outcome: EvaluationOutcome::Succeeded,
            },
            attempts: vec![charged],
            root_elapsed_ms: Some(250),
        };

        let record = derive_efficiency_task_record(&evidence).unwrap();
        assert_eq!(record.reacquisition_causes.len(), 1);
        let cause = &record.reacquisition_causes[0];
        assert_eq!(cause.cause_identity, "context-reduction:checkpoint-7");
        assert_eq!(cause.source_attempt_id.as_deref(), Some("root-attempt"));
        assert_eq!(cause.fresh_input_tokens.reported, 11);
        assert_eq!(cause.tool_result_bytes, 120);
        assert_eq!(cause.model_calls, 1);
        assert_eq!(cause.tool_calls, 2);
    }

    #[test]
    fn task_derivation_rejects_duplicate_attempt_charges() {
        let duplicated = attempt(
            "root-1",
            "attempt-1",
            super::super::UsageAttemptKind::Root,
            super::super::AttemptOutcome::Succeeded,
            10,
        );
        let evidence = EfficiencyTaskEvidence {
            task_fixture_revision: "task-0@1".into(),
            root_execution_id: "root-1".into(),
            policy_revision: "policy-1".into(),
            outcome_evaluator_identity: "tests-v1".into(),
            price_revision: "prices-v1".into(),
            outcome: EvaluationOutcome::Succeeded,
            outcome_evidence: EfficiencyOutcomeEvidence {
                source_identity: "tests-v1/task-0".into(),
                evaluator_identity: "tests-v1".into(),
                evidence_revision: "result-v1".into(),
                outcome: EvaluationOutcome::Succeeded,
            },
            attempts: vec![duplicated.clone(), duplicated],
            root_elapsed_ms: Some(100),
        };

        assert_eq!(
            derive_efficiency_task_record(&evidence),
            Err(EfficiencyEvaluationError::DuplicateAttempt {
                attempt_id: "attempt-1".into(),
            })
        );
    }

    #[test]
    fn task_derivation_rejects_cross_root_or_cross_policy_charges() {
        let mut wrong_root = attempt(
            "other-root",
            "attempt-1",
            super::super::UsageAttemptKind::Root,
            super::super::AttemptOutcome::Succeeded,
            10,
        );
        let evidence = EfficiencyTaskEvidence {
            task_fixture_revision: "task-0@1".into(),
            root_execution_id: "root-1".into(),
            policy_revision: "policy-1".into(),
            outcome_evaluator_identity: "tests-v1".into(),
            price_revision: "prices-v1".into(),
            outcome: EvaluationOutcome::Succeeded,
            outcome_evidence: EfficiencyOutcomeEvidence {
                source_identity: "tests-v1/task-0".into(),
                evaluator_identity: "tests-v1".into(),
                evidence_revision: "result-v1".into(),
                outcome: EvaluationOutcome::Succeeded,
            },
            attempts: vec![wrong_root.clone()],
            root_elapsed_ms: Some(100),
        };
        assert!(matches!(
            derive_efficiency_task_record(&evidence),
            Err(EfficiencyEvaluationError::AttemptRootMismatch { .. })
        ));

        wrong_root.record.attribution.root_execution_id = "root-1".into();
        wrong_root.record.attribution.policy_revision = "policy-2".into();
        let evidence = EfficiencyTaskEvidence {
            attempts: vec![wrong_root],
            ..evidence
        };
        assert!(matches!(
            derive_efficiency_task_record(&evidence),
            Err(EfficiencyEvaluationError::AttemptPolicyMismatch { .. })
        ));
    }

    #[test]
    fn cohort_preserves_fresh_cache_and_reacquisition_categories() {
        let mut record = task(0, EvaluationOutcome::Succeeded, 10);
        record.usage.fresh_input_tokens.reported = 100;
        record.usage.cache_read_tokens.reported = 200;
        record.usage.cache_write_tokens.reported = 25;
        record.usage.output_tokens.reported = 50;
        record.usage.reacquisition_fresh_input_tokens.reported = 30;

        let report = evaluate_efficiency_cohort(&[record]).unwrap();
        assert_eq!(report.usage.fresh_input_tokens.reported, 100);
        assert_eq!(report.usage.cache_read_tokens.reported, 200);
        assert_eq!(report.usage.cache_write_tokens.reported, 25);
        assert_eq!(report.usage.output_tokens.reported, 50);
        assert_eq!(report.usage.reacquisition_fresh_input_tokens.reported, 30);
    }

    #[test]
    fn cohort_rejects_mixed_policy_revisions() {
        let first = task(0, EvaluationOutcome::Succeeded, 10);
        let mut second = task(1, EvaluationOutcome::Succeeded, 10);
        second.policy_revision = "policy-2".into();

        assert!(matches!(
            evaluate_efficiency_cohort(&[first, second]),
            Err(EfficiencyEvaluationError::MixedPolicyRevision { .. })
        ));
    }

    #[test]
    fn paired_comparison_requires_the_same_task_fixture_set() {
        let baseline = vec![
            task(0, EvaluationOutcome::Succeeded, 10),
            task(1, EvaluationOutcome::Succeeded, 10),
        ];
        let candidate = vec![
            task(0, EvaluationOutcome::Succeeded, 8),
            task(2, EvaluationOutcome::Succeeded, 8),
        ];

        assert_eq!(
            compare_efficiency_policies(&baseline, &candidate),
            Err(EfficiencyEvaluationError::MismatchedTaskSet)
        );
    }

    #[test]
    fn paired_comparison_requires_matching_repetition_counts_per_fixture() {
        let mut baseline = vec![
            task(0, EvaluationOutcome::Succeeded, 10),
            task(1, EvaluationOutcome::Succeeded, 10),
            task(2, EvaluationOutcome::Succeeded, 10),
        ];
        baseline[1].task_fixture_revision = baseline[0].task_fixture_revision.clone();

        let mut candidate = vec![
            task(0, EvaluationOutcome::Succeeded, 8),
            task(1, EvaluationOutcome::Succeeded, 8),
            task(2, EvaluationOutcome::Succeeded, 8),
        ];
        candidate[2].task_fixture_revision = candidate[1].task_fixture_revision.clone();

        assert_eq!(
            compare_efficiency_policies(&baseline, &candidate),
            Err(EfficiencyEvaluationError::MismatchedTaskSet)
        );
    }

    #[test]
    fn paired_comparison_requires_the_same_outcome_evaluator_and_price_revision() {
        let baseline = vec![task(0, EvaluationOutcome::Succeeded, 10)];
        let mut candidate = vec![task(0, EvaluationOutcome::Succeeded, 8)];
        candidate[0].outcome_evaluator_identity = "tests-v2".into();

        assert!(matches!(
            compare_efficiency_policies(&baseline, &candidate),
            Err(EfficiencyEvaluationError::ComparisonOutcomeEvaluatorMismatch { .. })
        ));

        candidate[0].outcome_evaluator_identity = "tests-v1".into();
        candidate[0].price_revision = "prices-v2".into();
        assert!(matches!(
            compare_efficiency_policies(&baseline, &candidate),
            Err(EfficiencyEvaluationError::ComparisonPriceRevisionMismatch { .. })
        ));
    }

    #[test]
    fn failure_work_counts_against_cost_per_success() {
        let mut policy_a = (0..8)
            .map(|id| task(id, EvaluationOutcome::Succeeded, 10))
            .collect::<Vec<_>>();
        policy_a.extend((8..10).map(|id| task(id, EvaluationOutcome::Failed, 10)));

        let mut policy_b = (0..8)
            .map(|id| task(id, EvaluationOutcome::Succeeded, 8))
            .collect::<Vec<_>>();
        policy_b.extend((8..10).map(|id| task(id, EvaluationOutcome::Failed, 28)));

        let a = evaluate_efficiency_cohort(&policy_a).unwrap();
        let b = evaluate_efficiency_cohort(&policy_b).unwrap();

        assert_eq!(a.cost_per_success.unwrap().as_f64(), Some(12.5));
        assert_eq!(b.cost_per_success.unwrap().as_f64(), Some(15.0));
    }

    #[test]
    fn unresolved_or_incomplete_cost_never_becomes_zero_cost_win() {
        let mut records = vec![
            task(0, EvaluationOutcome::Succeeded, 10),
            task(1, EvaluationOutcome::Unresolved, 7),
        ];
        records[1].cost_complete = false;

        let report = evaluate_efficiency_cohort(&records).unwrap();
        assert_eq!(report.known_cost_microunits, 17);
        assert_eq!(report.unresolved_tasks, 1);
        assert_eq!(report.incomplete_cost_records, 1);
        assert_eq!(report.cost_per_success, None);
        assert!(!report.rollout_comparable);
    }

    #[test]
    fn zero_successes_are_undefined_not_free() {
        let report = evaluate_efficiency_cohort(&[
            task(0, EvaluationOutcome::Failed, 10),
            task(1, EvaluationOutcome::Failed, 10),
        ])
        .unwrap();

        assert_eq!(report.successful_tasks, 0);
        assert_eq!(report.known_cost_microunits, 20);
        assert_eq!(report.cost_per_success, None);
        assert!(!report.rollout_comparable);
    }
}
