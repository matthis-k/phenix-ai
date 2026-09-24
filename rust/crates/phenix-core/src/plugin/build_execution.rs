use crate::{Authority, PluginArtifact, PluginBuildPlan};
use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

const MAX_EVIDENCE_ENTRIES: usize = 64;
const MAX_EVIDENCE_ENTRY_BYTES: usize = 4_096;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PluginBuildEvidence {
    provenance: Vec<String>,
    diagnostics: Vec<String>,
}

impl PluginBuildEvidence {
    #[must_use]
    pub fn bounded(provenance: Vec<String>, diagnostics: Vec<String>) -> Self {
        Self {
            provenance: bound_entries(provenance),
            diagnostics: bound_entries(diagnostics),
        }
    }

    pub fn provenance(&self) -> &[String] {
        &self.provenance
    }

    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }
}

fn bound_entries(entries: Vec<String>) -> Vec<String> {
    entries
        .into_iter()
        .take(MAX_EVIDENCE_ENTRIES)
        .map(|mut entry| {
            if entry.len() > MAX_EVIDENCE_ENTRY_BYTES {
                let boundary = entry
                    .char_indices()
                    .map(|(index, _)| index)
                    .take_while(|index| *index <= MAX_EVIDENCE_ENTRY_BYTES)
                    .last()
                    .unwrap_or(0);
                entry.truncate(boundary);
            }
            entry
        })
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginBuildOutput {
    locator: String,
    content: Vec<u8>,
}

impl PluginBuildOutput {
    #[must_use]
    pub fn new(locator: String, content: Vec<u8>) -> Self {
        Self { locator, content }
    }

    pub fn locator(&self) -> &str {
        &self.locator
    }

    pub fn content(&self) -> &[u8] {
        &self.content
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginBuildExecution {
    pub output: Option<PluginBuildOutput>,
    pub evidence: PluginBuildEvidence,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginBuildFailure {
    pub message: String,
    pub evidence: PluginBuildEvidence,
}

impl Display for PluginBuildFailure {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for PluginBuildFailure {}

/// Executes a plan in isolated staging with only the supplied authority and
/// explicit environment. Steps run in order and only the declared output may
/// be returned.
pub trait PluginBuildExecutor {
    fn execute(
        &mut self,
        plan: &PluginBuildPlan,
        effective_authority: &Authority,
    ) -> Result<PluginBuildExecution, PluginBuildFailure>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginArtifactStoreError {
    pub message: String,
}

impl Display for PluginArtifactStoreError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for PluginArtifactStoreError {}

/// Content-addressed artifact storage used before a candidate can reach
/// runtime resolution. Ready artifacts are verified; built bytes are stored
/// under the core-computed revision.
pub trait PluginArtifactStore {
    fn preflight(&mut self) -> Result<(), PluginArtifactStoreError>;

    fn verify_ready(&mut self, artifact: &PluginArtifact) -> Result<(), PluginArtifactStoreError>;

    fn store_built(
        &mut self,
        artifact: &PluginArtifact,
        content: &[u8],
    ) -> Result<(), PluginArtifactStoreError>;
}
