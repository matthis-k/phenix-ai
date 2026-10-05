use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

pub const WORKSPACE_DISCOVERY_DESCRIPTOR_VERSION: u32 = 1;
pub const MAX_WORKSPACE_DISCOVERY_DESCRIPTOR_BYTES: usize = 64 * 1024;
pub const MAX_WORKSPACE_DISCOVERY_REMOTES: usize = 16;
pub const MAX_WORKSPACE_DISCOVERY_TERMS: usize = 256;
pub const MAX_WORKSPACE_DISCOVERY_TERM_BYTES: usize = 64;
pub const MAX_WORKSPACE_DISCOVERY_SCAN: usize = 4_096;
pub const MAX_WORKSPACE_DISCOVERY_PREPARED_CANDIDATES: usize = 3;
const DISCOVERY_FILE: &str = "discovery.json";

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
    UnsupportedVersion {
        version: u32,
    },
    EmptyWorkspaceId,
    RelativeRoot,
    TooManyRepositoryRemotes {
        requested: usize,
        allowed: usize,
    },
    TooManyRecallTerms {
        requested: usize,
        allowed: usize,
    },
    RecallTermTooLarge {
        term: String,
        requested: usize,
        allowed: usize,
    },
    SerializedDescriptorTooLarge {
        requested: usize,
        allowed: usize,
    },
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
            return Err(
                WorkspaceDiscoveryDescriptorError::TooManyRepositoryRemotes {
                    requested: self.repository_remotes.len(),
                    allowed: MAX_WORKSPACE_DISCOVERY_REMOTES,
                },
            );
        }
        if self.recall_terms.len() > MAX_WORKSPACE_DISCOVERY_TERMS {
            return Err(WorkspaceDiscoveryDescriptorError::TooManyRecallTerms {
                requested: self.recall_terms.len(),
                allowed: MAX_WORKSPACE_DISCOVERY_TERMS,
            });
        }
        for term in &self.recall_terms {
            let length = term.len();
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
            return Err(
                WorkspaceDiscoveryDescriptorError::SerializedDescriptorTooLarge {
                    requested: serialized.len(),
                    allowed: MAX_WORKSPACE_DISCOVERY_DESCRIPTOR_BYTES,
                },
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceDiscoveryReadResult {
    pub descriptors: Vec<WorkspaceDiscoveryDescriptorV1>,
    pub invalid_descriptors: u32,
    pub scanned_entries: u32,
    pub truncated: bool,
}

#[must_use]
pub fn workspace_discovery_root() -> Option<PathBuf> {
    if let Some(root) = env::var_os("PHENIX_WORKSPACE_DISCOVERY_DIR") {
        return Some(PathBuf::from(root));
    }
    if let Some(state) = env::var_os("XDG_STATE_HOME") {
        return Some(PathBuf::from(state).join("phenix/workspaces"));
    }
    env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state/phenix/workspaces"))
}

#[must_use]
pub fn workspace_key(workspace_id: &str) -> String {
    let digest = Sha256::digest(workspace_id.as_bytes());
    digest
        .iter()
        .take(12)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn observe_local_workspace(
    root: &Path,
    workspace_id: &str,
    canonical_root: &Path,
    repository_remotes: impl IntoIterator<Item = String>,
    recall_terms: impl IntoIterator<Item = String>,
    observed_at: u64,
) -> io::Result<WorkspaceDiscoveryDescriptorV1> {
    let directory = root.join(workspace_key(workspace_id));
    let path = directory.join(DISCOVERY_FILE);
    let mut descriptor = read_descriptor(&path).unwrap_or(WorkspaceDiscoveryDescriptorV1 {
        version: WORKSPACE_DISCOVERY_DESCRIPTOR_VERSION,
        workspace_id: workspace_id.to_owned(),
        canonical_root: canonical_root.to_path_buf(),
        repository_remotes: BTreeSet::new(),
        recall_terms: BTreeSet::new(),
        observation_count: 0,
        confirmed_recoveries: 0,
        last_observed_at: observed_at,
    });

    if descriptor.workspace_id != workspace_id || descriptor.canonical_root != canonical_root {
        descriptor = WorkspaceDiscoveryDescriptorV1 {
            version: WORKSPACE_DISCOVERY_DESCRIPTOR_VERSION,
            workspace_id: workspace_id.to_owned(),
            canonical_root: canonical_root.to_path_buf(),
            repository_remotes: BTreeSet::new(),
            recall_terms: BTreeSet::new(),
            observation_count: 0,
            confirmed_recoveries: 0,
            last_observed_at: observed_at,
        };
    }
    descriptor.repository_remotes.extend(
        repository_remotes
            .into_iter()
            .filter(|value| !value.trim().is_empty()),
    );
    descriptor.recall_terms.extend(
        recall_terms
            .into_iter()
            .filter(|value| !value.trim().is_empty()),
    );
    descriptor.observation_count = descriptor.observation_count.saturating_add(1);
    descriptor.last_observed_at = observed_at;
    descriptor
        .validate()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, format!("{error:?}")))?;

    fs::create_dir_all(&directory)?;
    write_descriptor_atomic(&path, &descriptor)?;
    Ok(descriptor)
}

pub fn read_workspace_descriptors(root: &Path) -> io::Result<WorkspaceDiscoveryReadResult> {
    let mut descriptors = Vec::new();
    let mut invalid_descriptors = 0_u32;
    let mut scanned_entries = 0_u32;
    let mut truncated = false;
    let mut directories = match fs::read_dir(root) {
        Ok(entries) => entries.filter_map(Result::ok).collect::<Vec<_>>(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(WorkspaceDiscoveryReadResult {
                descriptors,
                invalid_descriptors,
                scanned_entries,
                truncated,
            });
        }
        Err(error) => return Err(error),
    };
    directories.sort_by_key(|entry| entry.file_name());

    for entry in directories {
        if scanned_entries as usize >= MAX_WORKSPACE_DISCOVERY_SCAN {
            truncated = true;
            break;
        }
        let path = entry.path().join(DISCOVERY_FILE);
        if !path.is_file() {
            continue;
        }
        scanned_entries = scanned_entries.saturating_add(1);
        match read_descriptor(&path) {
            Some(descriptor) if descriptor.validate().is_ok() => descriptors.push(descriptor),
            _ => invalid_descriptors = invalid_descriptors.saturating_add(1),
        }
    }

    Ok(WorkspaceDiscoveryReadResult {
        descriptors,
        invalid_descriptors,
        scanned_entries,
        truncated,
    })
}

fn read_descriptor(path: &Path) -> Option<WorkspaceDiscoveryDescriptorV1> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() > MAX_WORKSPACE_DISCOVERY_DESCRIPTOR_BYTES {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

fn write_descriptor_atomic(
    path: &Path,
    descriptor: &WorkspaceDiscoveryDescriptorV1,
) -> io::Result<()> {
    let bytes = serde_json::to_vec(descriptor)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "descriptor path has no parent")
    })?;
    let temporary = parent.join(format!(".{DISCOVERY_FILE}.tmp-{}", std::process::id()));

    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
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
            .then_with(|| {
                left.descriptor
                    .workspace_id
                    .cmp(&right.descriptor.workspace_id)
            })
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
            recall_terms: ["phenix".to_owned(), "prs".to_owned()]
                .into_iter()
                .collect(),
            ..WorkspaceDiscoveryQuery::default()
        };
        let result = scan_workspace_descriptors(
            [
                descriptor("one", &["phenix", "prs"]),
                descriptor("two", &["other"]),
            ],
            &query,
        );
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].descriptor.workspace_id, "one");
        assert_eq!(
            result.completeness,
            WorkspaceDiscoveryCompleteness::Complete
        );
    }

    #[test]
    fn descriptor_store_round_trips_bounded_local_discovery() {
        let root =
            std::env::temp_dir().join(format!("phenix-workspace-discovery-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let canonical = root.join("repo");
        fs::create_dir_all(&canonical).unwrap();

        let observed = observe_local_workspace(
            &root,
            "workspace-phenix",
            &canonical,
            ["https://github.com/matthis-k/phenix-ai".to_owned()],
            ["phenix".to_owned(), "prs".to_owned()],
            42,
        )
        .unwrap();
        assert_eq!(observed.observation_count, 1);

        let read = read_workspace_descriptors(&root).unwrap();
        assert_eq!(read.invalid_descriptors, 0);
        assert_eq!(read.descriptors, vec![observed]);

        let metadata = fs::metadata(
            root.join(workspace_key("workspace-phenix"))
                .join(DISCOVERY_FILE),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        }
        let _ = fs::remove_dir_all(root);
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
