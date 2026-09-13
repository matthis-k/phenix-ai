use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::PathBuf};

pub const WORKSPACE_DISCOVERY_DESCRIPTOR_VERSION: u32 = 1;
pub const MAX_WORKSPACE_DISCOVERY_DESCRIPTOR_BYTES: usize = 64 * 1024;
pub const MAX_WORKSPACE_DISCOVERY_REMOTES: usize = 16;
pub const MAX_WORKSPACE_DISCOVERY_TERMS: usize = 256;
pub const MAX_WORKSPACE_DISCOVERY_TERM_BYTES: usize = 64;
pub const MAX_WORKSPACE_DISCOVERY_SCAN: usize = 4_096;
pub const MAX_WORKSPACE_DISCOVERY_PREPARED_CANDIDATES: usize = 3;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceDiscoveryDescriptorV1 {
    pub version: u32,
    pub workspace_id: String,
    pub canonical_root: PathBuf,
    #[serde(default)]
    pub repository_remotes: BTreeSet<String>,
    #[serde(default)]
    pub recall_terms: BTreeSet<String>,
    pub observation_count: u32,
    pub confirmed_recoveries: u32,
    pub last_observed_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceDiscoveryDescriptorError {
    UnsupportedVersion { version: u32 },
    EmptyWorkspaceId,
    RelativeRoot,
    TooManyRepositoryRemotes { requested: usize, allowed: usize },
    TooManyRecallTerms { requested: usize, allowed: usize },
    RecallTermTooLarge { term: String, requested: usize, allowed: usize },
    SerializedDescriptorTooLarge { requested: usize, allowed: usize },
}

impl WorkspaceDiscoveryDescriptorV1 {
    pub fn validate(&self) -> Result<(), WorkspaceDiscoveryDescriptorError> {
        if self.version != WORKSPACE_DISCOVERY_DESCRIPTOR_VERSION {
            return Err(WorkspaceDiscoveryDescriptorError::UnsupportedVersion {
                version: self.version,
            });
        }
        if self.workspace_id.trim().is_empty() {
            return Err(WorkspaceDiscoveryDescriptorError::EmptyWorkspaceId);
        }
        if !self.canonical_root.is_absolute() {
            return Err(WorkspaceDiscoveryDescriptorError::RelativeRoot);
        }
        if self.repository_remotes.len() > MAX_WORKSPACE_DISCOVERY_REMOTES {
            return Err(WorkspaceDiscoveryDescriptorError::TooManyRepositoryRemotes {
                requested: self.repository_remotes.len(),
                allowed: MAX_WORKSPACE_DISCOVERY_REMOTES,
            });
        }
        if self.recall_terms.len() > MAX_WORKSPACE_DISCOVERY_TERMS {
            return Err(WorkspaceDiscoveryDescriptorError::TooManyRecallTerms {
                requested: self.recall_terms.len(),
                allowed: MAX_WORKSPACE_DISCOVERY_TERMS,
            });
        }
        for term in &self.recall_terms {
            let length = term.as_bytes().len();
            if length > MAX_WORKSPACE_DISCOVERY_TERM_BYTES {
                return Err(WorkspaceDiscoveryDescriptorError::RecallTermTooLarge {
                    term: term.clone(),
                    requested: length,
                    allowed: MAX_WORKSPACE_DISCOVERY_TERM_BYTES,
                });
            }
        }
        let serialized = serde_json::to_vec(self).expect("workspace descriptor is serializable");
        if serialized.len() > MAX_WORKSPACE_DISCOVERY_DESCRIPTOR_BYTES {
            return Err(WorkspaceDiscoveryDescriptorError::SerializedDescriptorTooLarge {
                requested: serialized.len(),
                allowed: MAX_WORKSPACE_DISCOVERY_DESCRIPTOR_BYTES,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceDiscoveryQuery {
    pub workspace_ids: BTreeSet<String>,
    pub repository_remotes: BTreeSet<String>,
    pub recall_terms: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceDiscoveryMatch {
    ExactWorkspaceId,
    ExactRepositoryRemote,
    LexicalTerms { matched: u32 },
}

impl WorkspaceDiscoveryMatch {
    const fn class(&self) -> u8 {
        match self {
            Self::ExactWorkspaceId => 3,
            Self::ExactRepositoryRemote => 2,
            Self::LexicalTerms { .. } => 1,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceDiscoveryCandidate {
    pub descriptor: WorkspaceDiscoveryDescriptorV1,
    pub evidence: WorkspaceDiscoveryMatch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceDiscoveryCompleteness {
    Complete,
    Incomplete { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceDiscoveryScanResult {
    pub candidates: Vec<WorkspaceDiscoveryCandidate>,
    pub scanned_descriptors: u32,
    pub invalid_descriptors: u32,
    pub completeness: WorkspaceDiscoveryCompleteness,
}

#[must_use]
pub fn scan_workspace_descriptors(
    descriptors: impl IntoIterator<Item = WorkspaceDiscoveryDescriptorV1>,
    query: &WorkspaceDiscoveryQuery,
) -> WorkspaceDiscoveryScanResult {
    let mut candidates = Vec::new();
    let mut scanned = 0_u32;
    let mut invalid = 0_u32;
    let mut truncated = false;

    for descriptor in descriptors {
        if scanned as usize >= MAX_WORKSPACE_DISCOVERY_SCAN {
            truncated = true;
            break;
        }
        scanned = scanned.saturating_add(1);
        if descriptor.validate().is_err() {
            invalid = invalid.saturating_add(1);
            continue;
        }
        if let Some(evidence) = descriptor_match(&descriptor, query) {
            candidates.push(WorkspaceDiscoveryCandidate {
                descriptor,
                evidence,
            });
        }
    }

    candidates.sort_by(|left, right| {
        right
            .evidence
            .class()
            .cmp(&left.evidence.class())
            .then_with(|| match (&left.evidence, &right.evidence) {
                (
                    WorkspaceDiscoveryMatch::LexicalTerms { matched: left },
                    WorkspaceDiscoveryMatch::LexicalTerms { matched: right },
                ) => right.cmp(left),
                _ => std::cmp::Ordering::Equal,
            })
            .then_with(|| {
                right
                    .descriptor
                    .confirmed_recoveries
                    .cmp(&left.descriptor.confirmed_recoveries)
            })
            .then_with(|| {
                right
                    .descriptor
                    .observation_count
                    .cmp(&left.descriptor.observation_count)
            })
            .then_with(|| left.descriptor.workspace_id.cmp(&right.descriptor.workspace_id))
    });

    WorkspaceDiscoveryScanResult {
        candidates,
        scanned_descriptors: scanned,
        invalid_descriptors: invalid,
        completeness: if truncated {
            WorkspaceDiscoveryCompleteness::Incomplete {
                reason: format!(
                    "workspace descriptor scan exceeded {MAX_WORKSPACE_DISCOVERY_SCAN} entries"
                ),
            }
        } else {
            WorkspaceDiscoveryCompleteness::Complete
        },
    }
}

fn descriptor_match(
    descriptor: &WorkspaceDiscoveryDescriptorV1,
    query: &WorkspaceDiscoveryQuery,
) -> Option<WorkspaceDiscoveryMatch> {
    if query.workspace_ids.contains(&descriptor.workspace_id) {
        return Some(WorkspaceDiscoveryMatch::ExactWorkspaceId);
    }
    if descriptor
        .repository_remotes
        .iter()
        .any(|remote| query.repository_remotes.contains(remote))
    {
        return Some(WorkspaceDiscoveryMatch::ExactRepositoryRemote);
    }
    let matched = descriptor
        .recall_terms
        .intersection(&query.recall_terms)
        .count();
    if matched > 0 {
        Some(WorkspaceDiscoveryMatch::LexicalTerms {
            matched: u32::try_from(matched).unwrap_or(u32::MAX),
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(id: &str, terms: &[&str]) -> WorkspaceDiscoveryDescriptorV1 {
        WorkspaceDiscoveryDescriptorV1 {
            version: 1,
            workspace_id: id.into(),
            canonical_root: PathBuf::from(format!("/work/{id}")),
            repository_remotes: BTreeSet::new(),
            recall_terms: terms.iter().map(|term| (*term).to_owned()).collect(),
            observation_count: 1,
            confirmed_recoveries: 0,
            last_observed_at: 1,
        }
    }

    #[test]
    fn lexical_descriptor_discovery_is_bounded_and_rebuildable() {
        let query = WorkspaceDiscoveryQuery {
            recall_terms: ["phenix".to_owned(), "prs".to_owned()].into_iter().collect(),
            ..WorkspaceDiscoveryQuery::default()
        };
        let result = scan_workspace_descriptors(
            [descriptor("one", &["phenix", "prs"]), descriptor("two", &["other"])],
            &query,
        );
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].descriptor.workspace_id, "one");
        assert_eq!(result.completeness, WorkspaceDiscoveryCompleteness::Complete);
    }

    #[test]
    fn exact_workspace_identity_outranks_lexical_terms() {
        let query = WorkspaceDiscoveryQuery {
            workspace_ids: ["exact".to_owned()].into_iter().collect(),
            recall_terms: ["phenix".to_owned()].into_iter().collect(),
            ..WorkspaceDiscoveryQuery::default()
        };
        let result = scan_workspace_descriptors(
            [descriptor("lexical", &["phenix"]), descriptor("exact", &[])],
            &query,
        );
        assert_eq!(result.candidates[0].descriptor.workspace_id, "exact");
    }
}
