use super::UsageAggregate;
use serde::{Deserialize, Serialize};

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
pub struct EfficiencyTaskRecord {
    pub task_fixture_revision: String,
    pub root_execution_id: String,
    pub policy_revision: String,
    pub outcome_evaluator_identity: String,
    pub price_revision: String,
    pub outcome: EvaluationOutcome,
    pub usage: UsageAggregate,
    pub known_cost_microunits: u64,
    pub cost_complete: bool,
    pub root_elapsed_ms: Option<u64>,
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
    pub cost_per_success: Option<CostPerSuccess>,
    pub rollout_comparable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum EfficiencyEvaluationError {
    EmptyCohort,
    MixedPolicyRevision { expected: String, observed: String },
    MixedOutcomeEvaluator { expected: String, observed: String },
    MixedPriceRevision { expected: String, observed: String },
    MismatchedTaskSet,
    CohortTooLarge,
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

pub fn compare_efficiency_policies(
    baseline: &[EfficiencyTaskRecord],
    candidate: &[EfficiencyTaskRecord],
) -> Result<EfficiencyPolicyComparison, EfficiencyEvaluationError> {
    let baseline_keys = baseline
        .iter()
        .map(|record| (&record.task_fixture_revision, &record.root_execution_id))
        .collect::<std::collections::BTreeSet<_>>();
    let candidate_keys = candidate
        .iter()
        .map(|record| (&record.task_fixture_revision, &record.root_execution_id))
        .collect::<std::collections::BTreeSet<_>>();
    if baseline.len() != baseline_keys.len()
        || candidate.len() != candidate_keys.len()
        || baseline_keys.len() != candidate_keys.len()
        || baseline_keys
            .iter()
            .map(|(task, _)| *task)
            .collect::<std::collections::BTreeSet<_>>()
            != candidate_keys
                .iter()
                .map(|(task, _)| *task)
                .collect::<std::collections::BTreeSet<_>>()
    {
        return Err(EfficiencyEvaluationError::MismatchedTaskSet);
    }

    Ok(EfficiencyPolicyComparison {
        baseline: evaluate_efficiency_cohort(baseline)?,
        candidate: evaluate_efficiency_cohort(candidate)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: usize, outcome: EvaluationOutcome, cost: u64) -> EfficiencyTaskRecord {
        EfficiencyTaskRecord {
            task_fixture_revision: format!("task-{id}@1"),
            root_execution_id: format!("execution-{id}"),
            policy_revision: "policy-1".into(),
            outcome_evaluator_identity: "tests-v1".into(),
            price_revision: "prices-v1".into(),
            outcome,
            usage: UsageAggregate::default(),
            known_cost_microunits: cost,
            cost_complete: true,
            root_elapsed_ms: Some(1_000),
        }
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
