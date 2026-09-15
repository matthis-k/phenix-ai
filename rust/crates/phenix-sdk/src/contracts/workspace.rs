use phenix_core::{ComponentInterface, InterfaceId, InterfaceSchema};
use phenix_sdk_macros::PhenixValue;
use serde::{Deserialize, Serialize};

pub const WORKSPACE_SERVICE: &str = "phenix.workspace@1";

pub struct WorkspaceInterface;

impl ComponentInterface for WorkspaceInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(WORKSPACE_SERVICE).expect("static workspace interface id is valid")
    }

    fn schema() -> InterfaceSchema {
        InterfaceSchema::of::<WorkspaceCommand, WorkspaceResponse>()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum WorkspaceFileVersion {
    Absent,
    Present { content_hash: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
pub struct WorkspaceWrite {
    pub path: String,
    pub content: String,
    pub expected_version: WorkspaceFileVersion,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
pub struct WorkspaceWrittenFile {
    pub path: String,
    pub version: WorkspaceFileVersion,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
pub struct WorkspaceVersionConflict {
    pub path: String,
    pub expected_version: WorkspaceFileVersion,
    pub observed_version: WorkspaceFileVersion,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
pub struct WorkspaceSearchMatch {
    pub path: String,
    pub line: u64,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum WorkspaceCommand {
    Read {
        path: String,
    },
    Write {
        path: String,
        content: String,
        expected_version: WorkspaceFileVersion,
    },
    WriteBatch {
        writes: Vec<WorkspaceWrite>,
    },
    Search {
        needle: String,
        path: Option<String>,
        case_sensitive: bool,
    },
    Shell {
        command: String,
    },
    Git {
        arguments: Vec<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case")]
pub enum WorkspaceResponse {
    Read {
        path: String,
        content: String,
        version: WorkspaceFileVersion,
    },
    Written {
        path: String,
        version: WorkspaceFileVersion,
    },
    WrittenBatch {
        files: Vec<WorkspaceWrittenFile>,
    },
    VersionConflict {
        conflicts: Vec<WorkspaceVersionConflict>,
    },
    Search {
        matches: Vec<WorkspaceSearchMatch>,
    },
    Process {
        exit_code: i32,
        stdout: String,
        stderr: String,
    },
}
