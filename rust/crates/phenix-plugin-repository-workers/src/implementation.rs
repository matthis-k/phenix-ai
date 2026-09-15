use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryPullRequestState {
    Open,
    Merged,
    Closed,
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryCheckState {
    Absent,
    Pending,
    Success,
    Failure,
    ActionRequired,
    Cancelled,
    Skipped,
}

impl RepositoryCheckState {
    fn is_failure(self) -> bool {
        matches!(
            self,
            Self::Failure | Self::ActionRequired | Self::Cancelled | Self::Skipped
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct RepositoryValidation {
    pub source: RepositoryCheckState,
    pub rust: RepositoryCheckState,
    pub product: RepositoryCheckState,
    pub integration_system: RepositoryCheckState,
    pub maintenance: RepositoryCheckState,
    pub maintenance_autofix: RepositoryCheckState,
}

impl RepositoryValidation {
    #[must_use]
    pub fn all_green(&self) -> bool {
        [
            self.source,
            self.rust,
            self.product,
            self.integration_system,
            self.maintenance,
            self.maintenance_autofix,
        ]
        .into_iter()
        .all(|state| state == RepositoryCheckState::Success)
    }

    #[must_use]
    pub fn has_failure(&self) -> bool {
        [
            self.source,
            self.rust,
            self.product,
            self.integration_system,
            self.maintenance,
            self.maintenance_autofix,
        ]
        .into_iter()
        .any(RepositoryCheckState::is_failure)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct RepositoryChecklistEvidence {
    pub item: String,
    pub proven: bool,
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryDiscussionKind {
    ConversationComment,
    Review,
    ReviewComment,
    ReviewThread,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct RepositoryDiscussionEvidence {
    pub id: u64,
    pub kind: RepositoryDiscussionKind,
    pub body: String,
    pub substantive: bool,
    pub blocking: bool,
    pub resolved: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct RepositoryIssueEvidence {
    pub number: u64,
    pub semantic_key: String,
    pub title: String,
    pub open: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct RepositoryPullRequestEvidence {
    pub number: u64,
    pub semantic_key: String,
    pub queue_order: u64,
    pub state: RepositoryPullRequestState,
    pub draft: bool,
    pub head_sha: String,
    pub base_sha: String,
    pub base_is_current: bool,
    #[serde(default)]
    pub dependencies: BTreeSet<u64>,
    pub contract_markdown: String,
    #[serde(default)]
    pub checklist_evidence: Vec<RepositoryChecklistEvidence>,
    #[serde(default)]
    pub discussions: Vec<RepositoryDiscussionEvidence>,
    pub missing_regression: bool,
    pub missing_spec_or_invariant: bool,
    pub validation: RepositoryValidation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct RepositoryWorkSnapshot {
    #[serde(default)]
    pub pull_requests: Vec<RepositoryPullRequestEvidence>,
    #[serde(default)]
    pub issues: Vec<RepositoryIssueEvidence>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryFinding {
    pub source_id: u64,
    pub kind: RepositoryDiscussionKind,
    pub body: String,
    pub blocking: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryIssueCluster {
    pub semantic_key: String,
    pub issues: Vec<u64>,
    pub addressed_by: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RepositoryWorkPriority {
    BrokenValidationOrReview,
    StaleOrIncomplete,
    MissingContractEvidence,
    DependencyBlocking,
    NextReady,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepositorySelectionReason {
    BrokenValidation,
    BlockingFinding,
    StaleOrIncomplete,
    MissingContractEvidence,
    DependencyBlocking,
    NextReady,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconstructedPullRequest {
    pub number: u64,
    pub semantic_key: String,
    pub queue_order: u64,
    pub state: RepositoryPullRequestState,
    pub draft: bool,
    pub head_sha: String,
    pub base_sha: String,
    pub reconciled_contract: String,
    pub findings: Vec<RepositoryFinding>,
    pub issue_traceability: Vec<u64>,
    pub dependencies: BTreeSet<u64>,
    pub dependencies_satisfied: bool,
    pub duplicate_of: Option<u64>,
    pub contract_complete: bool,
    pub green_boundary: bool,
    pub stale_claims: bool,
    pub missing_contract_evidence: bool,
    pub broken_validation: bool,
    pub priority: RepositoryWorkPriority,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryWorkSelection {
    pub pr_number: u64,
    pub reason: RepositorySelectionReason,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryWorkerQueue {
    pull_requests: BTreeMap<u64, ReconstructedPullRequest>,
    issue_clusters: Vec<RepositoryIssueCluster>,
}

impl RepositoryWorkerQueue {
    #[must_use]
    pub fn reconstruct(snapshot: &RepositoryWorkSnapshot) -> Self {
        let owners = active_semantic_owners(&snapshot.pull_requests);
        let green_boundaries = snapshot
            .pull_requests
            .iter()
            .map(|pr| (pr.number, raw_green_boundary(pr)))
            .collect::<BTreeMap<_, _>>();

        let dependency_blockers = snapshot
            .pull_requests
            .iter()
            .filter(|pr| pr.state == RepositoryPullRequestState::Open)
            .flat_map(|pr| pr.dependencies.iter().copied())
            .collect::<BTreeSet<_>>();

        let pull_requests = snapshot
            .pull_requests
            .iter()
            .map(|pr| {
                let findings = normalize_findings(&pr.discussions);
                let blocking_finding = findings.iter().any(|finding| finding.blocking);
                let evidence = pr
                    .checklist_evidence
                    .iter()
                    .map(|item| (item.item.as_str(), item.proven))
                    .collect::<BTreeMap<_, _>>();
                let stale_claims = has_stale_checked_claim(&pr.contract_markdown, &evidence);
                let mut issue_traceability = snapshot
                    .issues
                    .iter()
                    .filter(|issue| issue.open && issue.semantic_key == pr.semantic_key)
                    .map(|issue| issue.number)
                    .collect::<Vec<_>>();
                issue_traceability.sort_unstable();
                let reconciled_contract = reconcile_contract(
                    &pr.contract_markdown,
                    &evidence,
                    &findings,
                    &issue_traceability,
                );
                let checklist_complete = !pr.checklist_evidence.is_empty()
                    && pr.checklist_evidence.iter().all(|item| item.proven);
                let contract_complete = checklist_complete
                    && !pr.missing_regression
                    && !pr.missing_spec_or_invariant
                    && findings.is_empty()
                    && !pr.draft
                    && pr.base_is_current;
                let dependencies_satisfied = pr.dependencies.iter().all(|dependency| {
                    snapshot.pull_requests.iter().any(|candidate| {
                        candidate.number == *dependency
                            && candidate.state == RepositoryPullRequestState::Merged
                            && green_boundaries.get(dependency).copied().unwrap_or(false)
                    })
                });
                let duplicate_of = owners
                    .get(&pr.semantic_key)
                    .copied()
                    .filter(|owner| *owner != pr.number);
                let broken_validation = pr.validation.has_failure();
                let missing_contract_evidence =
                    pr.missing_regression || pr.missing_spec_or_invariant;
                let priority = if broken_validation || blocking_finding {
                    RepositoryWorkPriority::BrokenValidationOrReview
                } else if stale_claims
                    || !checklist_complete
                    || !findings.is_empty()
                    || pr.draft
                    || !pr.base_is_current
                {
                    RepositoryWorkPriority::StaleOrIncomplete
                } else if missing_contract_evidence {
                    RepositoryWorkPriority::MissingContractEvidence
                } else if dependency_blockers.contains(&pr.number) {
                    RepositoryWorkPriority::DependencyBlocking
                } else {
                    RepositoryWorkPriority::NextReady
                };
                let green_boundary = pr.state != RepositoryPullRequestState::Closed
                    && contract_complete
                    && pr.validation.all_green();

                (
                    pr.number,
                    ReconstructedPullRequest {
                        number: pr.number,
                        semantic_key: pr.semantic_key.clone(),
                        queue_order: pr.queue_order,
                        state: pr.state,
                        draft: pr.draft,
                        head_sha: pr.head_sha.clone(),
                        base_sha: pr.base_sha.clone(),
                        reconciled_contract,
                        findings,
                        issue_traceability,
                        dependencies: pr.dependencies.clone(),
                        dependencies_satisfied,
                        duplicate_of,
                        contract_complete,
                        green_boundary,
                        stale_claims,
                        missing_contract_evidence,
                        broken_validation,
                        priority,
                    },
                )
            })
            .collect();

        let issue_clusters = aggregate_issues(&snapshot.issues, &owners);
        Self {
            pull_requests,
            issue_clusters,
        }
    }

    #[must_use]
    pub fn pull_request(&self, number: u64) -> Option<&ReconstructedPullRequest> {
        self.pull_requests.get(&number)
    }

    #[must_use]
    pub fn issue_clusters(&self) -> &[RepositoryIssueCluster] {
        &self.issue_clusters
    }

    #[must_use]
    pub fn select_work(&self) -> Option<RepositoryWorkSelection> {
        self.pull_requests
            .values()
            .filter(|pr| {
                pr.state == RepositoryPullRequestState::Open
                    && pr.duplicate_of.is_none()
                    && pr.dependencies_satisfied
            })
            .min_by_key(|pr| (pr.priority, pr.queue_order, pr.number))
            .map(|pr| RepositoryWorkSelection {
                pr_number: pr.number,
                reason: selection_reason(pr),
            })
    }
}

fn raw_green_boundary(pr: &RepositoryPullRequestEvidence) -> bool {
    let base_correct = pr.state == RepositoryPullRequestState::Merged || pr.base_is_current;
    pr.state != RepositoryPullRequestState::Closed
        && !pr.draft
        && base_correct
        && !pr.missing_regression
        && !pr.missing_spec_or_invariant
        && !pr.checklist_evidence.is_empty()
        && pr.checklist_evidence.iter().all(|item| item.proven)
        && normalize_findings(&pr.discussions).is_empty()
        && pr.validation.all_green()
}

fn active_semantic_owners(prs: &[RepositoryPullRequestEvidence]) -> BTreeMap<String, u64> {
    let mut owners = BTreeMap::<String, (u64, u64)>::new();
    for pr in prs
        .iter()
        .filter(|pr| pr.state == RepositoryPullRequestState::Open)
    {
        let candidate = (pr.queue_order, pr.number);
        match owners.get(&pr.semantic_key) {
            Some(current) if *current <= candidate => {}
            _ => {
                owners.insert(pr.semantic_key.clone(), candidate);
            }
        }
    }
    owners
        .into_iter()
        .map(|(key, (_, number))| (key, number))
        .collect()
}

fn aggregate_issues(
    issues: &[RepositoryIssueEvidence],
    owners: &BTreeMap<String, u64>,
) -> Vec<RepositoryIssueCluster> {
    let mut clusters = BTreeMap::<String, Vec<u64>>::new();
    for issue in issues.iter().filter(|issue| issue.open) {
        clusters
            .entry(issue.semantic_key.clone())
            .or_default()
            .push(issue.number);
    }
    clusters
        .into_iter()
        .map(|(semantic_key, mut issue_numbers)| {
            issue_numbers.sort_unstable();
            RepositoryIssueCluster {
                addressed_by: owners.get(&semantic_key).copied(),
                semantic_key,
                issues: issue_numbers,
            }
        })
        .collect()
}

fn normalize_findings(discussions: &[RepositoryDiscussionEvidence]) -> Vec<RepositoryFinding> {
    discussions
        .iter()
        .filter(|discussion| discussion.substantive && !discussion.resolved)
        .map(|discussion| RepositoryFinding {
            source_id: discussion.id,
            kind: discussion.kind,
            body: discussion.body.clone(),
            blocking: discussion.blocking,
        })
        .collect()
}

fn selection_reason(pr: &ReconstructedPullRequest) -> RepositorySelectionReason {
    match pr.priority {
        RepositoryWorkPriority::BrokenValidationOrReview if pr.broken_validation => {
            RepositorySelectionReason::BrokenValidation
        }
        RepositoryWorkPriority::BrokenValidationOrReview => {
            RepositorySelectionReason::BlockingFinding
        }
        RepositoryWorkPriority::StaleOrIncomplete => RepositorySelectionReason::StaleOrIncomplete,
        RepositoryWorkPriority::MissingContractEvidence => {
            RepositorySelectionReason::MissingContractEvidence
        }
        RepositoryWorkPriority::DependencyBlocking => RepositorySelectionReason::DependencyBlocking,
        RepositoryWorkPriority::NextReady => RepositorySelectionReason::NextReady,
    }
}

fn has_stale_checked_claim(contract: &str, evidence: &BTreeMap<&str, bool>) -> bool {
    contract.lines().any(|line| {
        checklist_item(line).is_some_and(|(checked, item)| {
            checked && evidence.get(item).is_some_and(|proven| !proven)
        })
    })
}

const FINDINGS_START: &str = "<!-- repository-worker:findings:start -->";
const FINDINGS_END: &str = "<!-- repository-worker:findings:end -->";
const TRACE_START: &str = "<!-- repository-worker:traceability:start -->";
const TRACE_END: &str = "<!-- repository-worker:traceability:end -->";

fn reconcile_contract(
    contract: &str,
    evidence: &BTreeMap<&str, bool>,
    findings: &[RepositoryFinding],
    issues: &[u64],
) -> String {
    let contract = strip_managed_block(contract, FINDINGS_START, FINDINGS_END);
    let contract = strip_managed_block(&contract, TRACE_START, TRACE_END);
    let mut reconciled = reconcile_checklist(contract.trim_end(), evidence);
    if !findings.is_empty() {
        reconciled.push_str("\n\n");
        reconciled.push_str(FINDINGS_START);
        reconciled.push_str("\n## Repository findings\n\n");
        for finding in findings {
            let body = finding
                .body
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            reconciled.push_str(&format!(
                "- [ ] {:?} #{}: {}\n",
                finding.kind, finding.source_id, body
            ));
        }
        reconciled.push_str(FINDINGS_END);
    }
    if !issues.is_empty() {
        reconciled.push_str("\n\n");
        reconciled.push_str(TRACE_START);
        reconciled.push_str("\n## Issue traceability\n\nAddresses: ");
        reconciled.push_str(
            &issues
                .iter()
                .map(|number| format!("#{number}"))
                .collect::<Vec<_>>()
                .join(", "),
        );
        reconciled.push('\n');
        reconciled.push_str(TRACE_END);
    }
    reconciled
}

fn strip_managed_block(contract: &str, start: &str, end: &str) -> String {
    let Some(start_index) = contract.find(start) else {
        return contract.to_string();
    };
    let Some(relative_end) = contract[start_index..].find(end) else {
        return contract.to_string();
    };
    let end_index = start_index + relative_end + end.len();
    let mut stripped = String::with_capacity(contract.len());
    stripped.push_str(contract[..start_index].trim_end());
    let suffix = contract[end_index..].trim_start_matches(['\r', '\n']);
    if !suffix.is_empty() {
        stripped.push('\n');
        stripped.push_str(suffix);
    }
    stripped
}

fn reconcile_checklist(contract: &str, evidence: &BTreeMap<&str, bool>) -> String {
    contract
        .lines()
        .map(|line| {
            let Some((_, item)) = checklist_item(line) else {
                return line.to_string();
            };
            let Some(proven) = evidence.get(item) else {
                return line.to_string();
            };
            let Some(marker) = line.find("[x]").or_else(|| line.find("[ ]")) else {
                return line.to_string();
            };
            let replacement = if *proven { "[x]" } else { "[ ]" };
            format!("{}{}{}", &line[..marker], replacement, &line[marker + 3..])
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn checklist_item(line: &str) -> Option<(bool, &str)> {
    let trimmed = line.trim_start();
    let (checked, rest) = if let Some(rest) = trimmed.strip_prefix("- [x] ") {
        (true, rest)
    } else {
        let rest = trimmed.strip_prefix("- [ ] ")?;
        (false, rest)
    };
    Some((checked, rest.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn green_validation() -> RepositoryValidation {
        RepositoryValidation {
            source: RepositoryCheckState::Success,
            rust: RepositoryCheckState::Success,
            product: RepositoryCheckState::Success,
            integration_system: RepositoryCheckState::Success,
            maintenance: RepositoryCheckState::Success,
            maintenance_autofix: RepositoryCheckState::Success,
        }
    }

    fn evidence(item: &str, proven: bool) -> RepositoryChecklistEvidence {
        RepositoryChecklistEvidence {
            item: item.to_string(),
            proven,
        }
    }

    fn pr(number: u64, semantic_key: &str, queue_order: u64) -> RepositoryPullRequestEvidence {
        RepositoryPullRequestEvidence {
            number,
            semantic_key: semantic_key.to_string(),
            queue_order,
            state: RepositoryPullRequestState::Open,
            draft: false,
            head_sha: format!("head-{number}"),
            base_sha: "main".into(),
            base_is_current: true,
            dependencies: BTreeSet::new(),
            contract_markdown: "- [ ] implementation\n- [ ] regression".into(),
            checklist_evidence: vec![
                evidence("implementation", true),
                evidence("regression", true),
            ],
            discussions: vec![],
            missing_regression: false,
            missing_spec_or_invariant: false,
            validation: green_validation(),
        }
    }

    #[test]
    fn checklist_reconciliation_reverts_stale_claims_and_marks_proven_work() {
        let mut candidate = pr(10, "worker-handoff", 10);
        candidate.contract_markdown = "- [x] implementation\n- [ ] regression".into();
        candidate.checklist_evidence = vec![
            evidence("implementation", false),
            evidence("regression", true),
        ];

        let queue = RepositoryWorkerQueue::reconstruct(&RepositoryWorkSnapshot {
            pull_requests: vec![candidate],
            issues: vec![],
        });
        let reconstructed = queue.pull_request(10).unwrap();
        assert!(reconstructed.stale_claims);
        assert_eq!(
            reconstructed.reconciled_contract,
            "- [ ] implementation\n- [x] regression"
        );
    }

    #[test]
    fn broken_ci_and_blocking_findings_outrank_later_ready_work() {
        let mut broken = pr(10, "broken", 10);
        broken.validation.rust = RepositoryCheckState::Failure;
        let ready = pr(11, "ready", 11);
        let queue = RepositoryWorkerQueue::reconstruct(&RepositoryWorkSnapshot {
            pull_requests: vec![ready, broken],
            issues: vec![],
        });
        assert_eq!(
            queue.select_work(),
            Some(RepositoryWorkSelection {
                pr_number: 10,
                reason: RepositorySelectionReason::BrokenValidation,
            })
        );

        let mut blocked_review = pr(9, "review", 9);
        blocked_review
            .discussions
            .push(RepositoryDiscussionEvidence {
                id: 42,
                kind: RepositoryDiscussionKind::ReviewThread,
                body: "preserve exact authority".into(),
                substantive: true,
                blocking: true,
                resolved: false,
            });
        let queue = RepositoryWorkerQueue::reconstruct(&RepositoryWorkSnapshot {
            pull_requests: vec![blocked_review, pr(10, "later", 10)],
            issues: vec![],
        });
        let reconstructed = queue.pull_request(9).unwrap();
        assert_eq!(reconstructed.findings.len(), 1);
        assert!(reconstructed
            .reconciled_contract
            .contains("ReviewThread #42: preserve exact authority"));
        assert!(!reconstructed.contract_complete);
        assert_eq!(queue.select_work().unwrap().pr_number, 9);
    }

    #[test]
    fn dependency_requires_merged_green_boundary() {
        let mut predecessor = pr(10, "predecessor", 10);
        predecessor.state = RepositoryPullRequestState::Open;
        let mut dependent = pr(11, "dependent", 11);
        dependent.dependencies.insert(10);
        let queue = RepositoryWorkerQueue::reconstruct(&RepositoryWorkSnapshot {
            pull_requests: vec![predecessor.clone(), dependent.clone()],
            issues: vec![],
        });
        assert!(!queue.pull_request(11).unwrap().dependencies_satisfied);
        assert_eq!(queue.select_work().unwrap().pr_number, 10);

        predecessor.state = RepositoryPullRequestState::Merged;
        predecessor.base_is_current = false;
        let queue = RepositoryWorkerQueue::reconstruct(&RepositoryWorkSnapshot {
            pull_requests: vec![predecessor, dependent],
            issues: vec![],
        });
        assert!(queue.pull_request(11).unwrap().dependencies_satisfied);
        assert_eq!(queue.select_work().unwrap().pr_number, 11);
    }

    #[test]
    fn active_owner_suppresses_duplicate_parallel_pr_and_clusters_issues() {
        let owner = pr(20, "repo-worker", 20);
        let duplicate = pr(21, "repo-worker", 21);
        let queue = RepositoryWorkerQueue::reconstruct(&RepositoryWorkSnapshot {
            pull_requests: vec![owner, duplicate],
            issues: vec![
                RepositoryIssueEvidence {
                    number: 100,
                    semantic_key: "repo-worker".into(),
                    title: "resume".into(),
                    open: true,
                },
                RepositoryIssueEvidence {
                    number: 101,
                    semantic_key: "repo-worker".into(),
                    title: "comments".into(),
                    open: true,
                },
            ],
        });
        assert_eq!(queue.pull_request(21).unwrap().duplicate_of, Some(20));
        let owner = queue.pull_request(20).unwrap();
        assert_eq!(owner.issue_traceability, vec![100, 101]);
        assert!(owner.reconciled_contract.contains("Addresses: #100, #101"));
        assert_eq!(queue.select_work().unwrap().pr_number, 20);
        assert_eq!(
            queue.issue_clusters(),
            &[RepositoryIssueCluster {
                semantic_key: "repo-worker".into(),
                issues: vec![100, 101],
                addressed_by: Some(20),
            }]
        );
    }

    #[test]
    fn missing_regression_or_spec_has_distinct_priority_after_contract_is_proven() {
        let mut candidate = pr(25, "missing-evidence", 25);
        candidate.missing_regression = true;
        let queue = RepositoryWorkerQueue::reconstruct(&RepositoryWorkSnapshot {
            pull_requests: vec![candidate],
            issues: vec![],
        });
        assert_eq!(
            queue.select_work(),
            Some(RepositoryWorkSelection {
                pr_number: 25,
                reason: RepositorySelectionReason::MissingContractEvidence,
            })
        );
    }

    #[test]
    fn dependency_blocker_outranks_ordinary_ready_work() {
        let ordinary = pr(20, "ordinary", 20);
        let blocker = pr(30, "blocker", 30);
        let mut dependent = pr(31, "dependent", 31);
        dependent.dependencies.insert(30);
        let queue = RepositoryWorkerQueue::reconstruct(&RepositoryWorkSnapshot {
            pull_requests: vec![ordinary, blocker, dependent],
            issues: vec![],
        });
        assert_eq!(
            queue.select_work(),
            Some(RepositoryWorkSelection {
                pr_number: 30,
                reason: RepositorySelectionReason::DependencyBlocking,
            })
        );
    }

    #[test]
    fn draft_or_unresolved_substantive_finding_never_forms_green_boundary() {
        let mut draft = pr(26, "draft", 26);
        draft.draft = true;
        let mut finding = pr(27, "finding", 27);
        finding.discussions.push(RepositoryDiscussionEvidence {
            id: 9,
            kind: RepositoryDiscussionKind::ConversationComment,
            body: "retain issue traceability".into(),
            substantive: true,
            blocking: false,
            resolved: false,
        });
        let queue = RepositoryWorkerQueue::reconstruct(&RepositoryWorkSnapshot {
            pull_requests: vec![draft, finding],
            issues: vec![],
        });
        assert!(!queue.pull_request(26).unwrap().green_boundary);
        assert!(!queue.pull_request(27).unwrap().green_boundary);
        assert!(!queue.pull_request(27).unwrap().contract_complete);
    }

    #[test]
    fn worker_resume_is_reconstructed_from_repository_snapshot_only() {
        let snapshot = RepositoryWorkSnapshot {
            pull_requests: vec![pr(30, "resume", 30)],
            issues: vec![],
        };
        let serialized = serde_json::to_string(&snapshot).unwrap();
        let restored: RepositoryWorkSnapshot = serde_json::from_str(&serialized).unwrap();
        let first = RepositoryWorkerQueue::reconstruct(&snapshot);
        let resumed = RepositoryWorkerQueue::reconstruct(&restored);
        assert_eq!(first, resumed);
        assert_eq!(
            resumed.select_work(),
            Some(RepositoryWorkSelection {
                pr_number: 30,
                reason: RepositorySelectionReason::NextReady,
            })
        );
    }
}
