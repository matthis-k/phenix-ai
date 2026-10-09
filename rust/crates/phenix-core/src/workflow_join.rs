//! Versioned, deterministic structured-join decisions for Core plan scopes.
//!
//! Pure settlement algebra: the root scheduler owns child tasks, cancellation,
//! generation leases and actual service calls. This module never dispatches.

use serde::{Deserialize, Serialize};
use std::{collections::{BTreeMap, BTreeSet}, fmt, num::NonZeroUsize};

/// The only All policies recognized by the Core plan semantics.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowJoinAllPolicy {
    CollectAll,
    FailFast,
}

/// Closed join vocabulary. New policies require an IR semantic revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum WorkflowJoinPolicy {
    All(WorkflowJoinAllPolicy),
    FirstCompleted,
    FirstSuccess,
    Quorum(NonZeroUsize),
}

/// Internal child settlement for join selection, not a provider result value.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowChildSettlement {
    Completed,
    Failed,
    Cancelled,
}

/// Monotonic scheduler observation index, with branch ID as deterministic
/// tie-breaker for simultaneous observations. Never derive it from a clock.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowJoinObservation {
    pub branch: String,
    pub order: u64,
    pub settlement: WorkflowChildSettlement,
}

/// A decision reports scope admission/settlement, not provider side effects.
/// The root must retain leases for siblings it cancels until they actually
/// settle. A completed decision alone cannot release those leases.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkflowJoinDecision {
    Pending,
    Succeeded {
        selected: Vec<String>,
        cancel_remaining: bool,
    },
    Failed {
        selected: Vec<String>,
        cancel_remaining: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkflowJoinError {
    EmptyBranches,
    InvalidQuorum { required: usize, available: usize },
    UnknownBranch(String),
    DuplicateSettlement(String),
}

impl fmt::Display for WorkflowJoinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyBranches => f.write_str("join has no admitted branches"),
            Self::InvalidQuorum { required, available } => {
                write!(f, "quorum {required} exceeds {available} admitted branches")
            }
            Self::UnknownBranch(branch) => write!(f, "unknown join branch {branch}"),
            Self::DuplicateSettlement(branch) => {
                write!(f, "duplicate settlement for join branch {branch}")
            }
        }
    }
}

impl std::error::Error for WorkflowJoinError {}

impl WorkflowJoinPolicy {
    /// Evaluate an immutable snapshot. Different input enumeration orders
    /// produce the same answer when observation indices are unchanged.
    pub fn decide(
        self,
        admitted: &BTreeSet<String>,
        observations: &[WorkflowJoinObservation],
    ) -> Result<WorkflowJoinDecision, WorkflowJoinError> {
        if admitted.is_empty() {
            return Err(WorkflowJoinError::EmptyBranches);
        }
        if let Self::Quorum(quorum) = self {
            if quorum.get() > admitted.len() {
                return Err(WorkflowJoinError::InvalidQuorum {
                    required: quorum.get(),
                    available: admitted.len(),
                });
            }
        }
        let mut unique = BTreeMap::new();
        for observation in observations {
            if !admitted.contains(&observation.branch) {
                return Err(WorkflowJoinError::UnknownBranch(observation.branch.clone()));
            }
            if unique
                .insert(observation.branch.clone(), observation)
                .is_some()
            {
                return Err(WorkflowJoinError::DuplicateSettlement(
                    observation.branch.clone(),
                ));
            }
        }
        let mut ordered: Vec<_> = unique.values().copied().collect();
        ordered.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.branch.cmp(&right.branch))
        });
        let complete = ordered.len() == admitted.len();
        let successes: Vec<_> = ordered
            .iter()
            .filter(|item| item.settlement == WorkflowChildSettlement::Completed)
            .map(|item| item.branch.clone())
            .collect();
        let failures: Vec<_> = ordered
            .iter()
            .filter(|item| item.settlement != WorkflowChildSettlement::Completed)
            .map(|item| item.branch.clone())
            .collect();
        let all_ids = ordered
            .iter()
            .map(|item| item.branch.clone())
            .collect::<Vec<_>>();
        let remaining = admitted.len() - ordered.len();
        match self {
            Self::All(WorkflowJoinAllPolicy::CollectAll) => {
                if !complete {
                    Ok(WorkflowJoinDecision::Pending)
                } else if failures.is_empty() {
                    Ok(WorkflowJoinDecision::Succeeded {
                        selected: all_ids,
                        cancel_remaining: false,
                    })
                } else {
                    Ok(WorkflowJoinDecision::Failed {
                        selected: failures,
                        cancel_remaining: false,
                    })
                }
            }
            Self::All(WorkflowJoinAllPolicy::FailFast) => {
                if let Some(failed) = failures.first() {
                    Ok(WorkflowJoinDecision::Failed {
                        selected: vec![failed.clone()],
                        cancel_remaining: remaining != 0,
                    })
                } else if complete {
                    Ok(WorkflowJoinDecision::Succeeded {
                        selected: all_ids,
                        cancel_remaining: false,
                    })
                } else {
                    Ok(WorkflowJoinDecision::Pending)
                }
            }
            Self::FirstCompleted => match ordered.first() {
                None => Ok(WorkflowJoinDecision::Pending),
                Some(first) if first.settlement == WorkflowChildSettlement::Completed => {
                    Ok(WorkflowJoinDecision::Succeeded {
                        selected: vec![first.branch.clone()],
                        cancel_remaining: remaining != 0,
                    })
                }
                Some(first) => Ok(WorkflowJoinDecision::Failed {
                    selected: vec![first.branch.clone()],
                    cancel_remaining: remaining != 0,
                }),
            },
            Self::FirstSuccess => {
                if let Some(winner) = successes.first() {
                    Ok(WorkflowJoinDecision::Succeeded {
                        selected: vec![winner.clone()],
                        cancel_remaining: remaining != 0,
                    })
                } else if complete {
                    Ok(WorkflowJoinDecision::Failed {
                        selected: failures,
                        cancel_remaining: false,
                    })
                } else {
                    Ok(WorkflowJoinDecision::Pending)
                }
            }
            Self::Quorum(quorum) if successes.len() >= quorum.get() => {
                Ok(WorkflowJoinDecision::Succeeded {
                    selected: successes.into_iter().take(quorum.get()).collect(),
                    cancel_remaining: remaining != 0,
                })
            }
            Self::Quorum(quorum) if successes.len() + remaining < quorum.get() => {
                Ok(WorkflowJoinDecision::Failed {
                    selected: failures,
                    cancel_remaining: remaining != 0,
                })
            }
            Self::Quorum(_) => Ok(WorkflowJoinDecision::Pending),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn admitted() -> BTreeSet<String> {
        ["alpha", "beta", "gamma"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    fn observation(
        branch: &str,
        order: u64,
        settlement: WorkflowChildSettlement,
    ) -> WorkflowJoinObservation {
        WorkflowJoinObservation {
            branch: branch.into(),
            order,
            settlement,
        }
    }

    #[test]
    fn all_waits_or_short_circuits_by_explicit_policy() {
        let members = admitted();
        let failure = observation("beta", 2, WorkflowChildSettlement::Failed);
        assert_eq!(
            WorkflowJoinPolicy::All(WorkflowJoinAllPolicy::CollectAll)
                .decide(&members, std::slice::from_ref(&failure)),
            Ok(WorkflowJoinDecision::Pending)
        );
        assert_eq!(
            WorkflowJoinPolicy::All(WorkflowJoinAllPolicy::FailFast)
                .decide(&members, &[failure]),
            Ok(WorkflowJoinDecision::Failed {
                selected: vec!["beta".into()],
                cancel_remaining: true,
            })
        );
        let finished = vec![
            observation("gamma", 3, WorkflowChildSettlement::Completed),
            observation("beta", 2, WorkflowChildSettlement::Completed),
            observation("alpha", 1, WorkflowChildSettlement::Completed),
        ];
        assert_eq!(
            WorkflowJoinPolicy::All(WorkflowJoinAllPolicy::CollectAll)
                .decide(&members, &finished),
            Ok(WorkflowJoinDecision::Succeeded {
                selected: vec!["alpha".into(), "beta".into(), "gamma".into()],
                cancel_remaining: false,
            })
        );
    }

    #[test]
    fn first_completed_and_first_success_have_deterministic_ties() {
        let members = admitted();
        let observations = [
            observation("beta", 1, WorkflowChildSettlement::Failed),
            observation("alpha", 1, WorkflowChildSettlement::Completed),
        ];
        let reverse = [observations[1].clone(), observations[0].clone()];
        for input in [&observations[..], &reverse[..]] {
            assert_eq!(
                WorkflowJoinPolicy::FirstCompleted.decide(&members, input),
                Ok(WorkflowJoinDecision::Succeeded {
                    selected: vec!["alpha".into()],
                    cancel_remaining: true,
                })
            );
            assert_eq!(
                WorkflowJoinPolicy::FirstSuccess.decide(&members, input),
                Ok(WorkflowJoinDecision::Succeeded {
                    selected: vec!["alpha".into()],
                    cancel_remaining: true,
                })
            );
        }
        assert_eq!(
            WorkflowJoinPolicy::FirstSuccess.decide(
                &members,
                &[observation("beta", 1, WorkflowChildSettlement::Failed)]
            ),
            Ok(WorkflowJoinDecision::Pending)
        );
    }

    #[test]
    fn quorum_settles_at_threshold_or_when_success_is_impossible() {
        let members = admitted();
        let quorum = WorkflowJoinPolicy::Quorum(NonZeroUsize::new(2).unwrap());
        let two = [
            observation("gamma", 1, WorkflowChildSettlement::Completed),
            observation("alpha", 2, WorkflowChildSettlement::Completed),
        ];
        assert_eq!(
            quorum.decide(&members, &two),
            Ok(WorkflowJoinDecision::Succeeded {
                selected: vec!["gamma".into(), "alpha".into()],
                cancel_remaining: true,
            })
        );
        let impossible = [
            observation("alpha", 1, WorkflowChildSettlement::Cancelled),
            observation("beta", 2, WorkflowChildSettlement::Failed),
        ];
        assert_eq!(
            quorum.decide(&members, &impossible),
            Ok(WorkflowJoinDecision::Failed {
                selected: vec!["alpha".into(), "beta".into()],
                cancel_remaining: true,
            })
        );
        assert_eq!(
            WorkflowJoinPolicy::Quorum(NonZeroUsize::new(4).unwrap())
                .decide(&members, &[]),
            Err(WorkflowJoinError::InvalidQuorum {
                required: 4,
                available: 3,
            })
        );
    }

    #[test]
    fn invalid_observations_fail_before_any_join_decision() {
        let members = admitted();
        let one = observation("alpha", 1, WorkflowChildSettlement::Completed);
        assert_eq!(
            WorkflowJoinPolicy::FirstCompleted.decide(&members, &[one.clone(), one]),
            Err(WorkflowJoinError::DuplicateSettlement("alpha".into()))
        );
        assert_eq!(
            WorkflowJoinPolicy::FirstCompleted.decide(
                &members,
                &[observation("not-admitted", 1, WorkflowChildSettlement::Completed)]
            ),
            Err(WorkflowJoinError::UnknownBranch("not-admitted".into()))
        );
        assert_eq!(
            WorkflowJoinPolicy::FirstCompleted.decide(&BTreeSet::new(), &[]),
            Err(WorkflowJoinError::EmptyBranches)
        );
    }
}
