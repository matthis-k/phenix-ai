//! Session workspace selection, independent of process cwd and filesystem grants.
//! A selected directory is metadata, not permission to access that directory.
use phenix_sdk::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct WorkspaceQuery {
    pub session: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub session: String,
    pub cwd: Option<String>,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct WorkspaceUpdate {
    pub session: String,
    pub cwd: Option<String>,
    pub expected_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum WorkspaceProblem {
    InvalidInput,
    Denied,
    Conflict,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct WorkspaceError {
    pub problem: WorkspaceProblem,
    pub message: String,
}

#[contract(id = "phenix.workspace@1")]
pub trait Workspace {
    fn get(&self, request: WorkspaceQuery) -> Result<WorkspaceSnapshot, WorkspaceError>;
    fn set(&self, request: WorkspaceUpdate) -> Result<WorkspaceSnapshot, WorkspaceError>;
}

#[plugin(
    id = "phenix.workspace-contracts",
    version = "1.0.0",
    profile = "definitions",
    define(Workspace)
)]
pub mod definitions {
    use super::Workspace;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_workspace_revisions_round_trip() {
        let update = WorkspaceUpdate {
            session: "s1".into(),
            cwd: Some("/work/project".into()),
            expected_revision: 42,
        };
        assert_eq!(
            WorkspaceUpdate::from_value(update.to_value()).unwrap(),
            update
        );
    }
}
