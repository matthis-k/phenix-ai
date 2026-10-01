use super::environment::ProcessStreamRecovery;
use phenix_core::{
    ArtifactRevision, ComponentInterface, ContentReference, InterfaceId, InterfaceSchema,
};
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
#[serde(deny_unknown_fields)]
pub struct WorkspaceCommittedFile {
    pub path: String,
    pub before_version: WorkspaceFileVersion,
    pub version: WorkspaceFileVersion,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceCommitReceipt {
    pub operation_id: String,
    pub intent_identity: String,
    pub files: Vec<WorkspaceCommittedFile>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceEntryKind {
    File,
    Directory,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
pub struct WorkspaceEntry {
    pub path: String,
    pub kind: WorkspaceEntryKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceWriteAtomicity {
    PreconditionCheckedSequential,
    CrashRecoverable,
    SnapshotAtomic,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
pub struct WorkspaceCapabilities {
    pub write_atomicity: WorkspaceWriteAtomicity,
    #[serde(default)]
    pub recoverable_commit_atomicity: Option<WorkspaceWriteAtomicity>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum WorkspaceCommand {
    Capabilities,
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
    ReadContentReference {
        reference: ContentReference,
    },
    CommitBatch {
        operation_id: String,
        writes: Vec<WorkspaceWrite>,
    },
    Search {
        needle: String,
        path: Option<String>,
        case_sensitive: bool,
    },
    List {
        path: Option<String>,
        recursive: bool,
    },
    Shell {
        command: String,
    },
    Git {
        arguments: Vec<String>,
    },
}

const fn complete_capture() -> bool {
    true
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case")]
pub enum WorkspaceResponse {
    Capabilities {
        capabilities: WorkspaceCapabilities,
    },
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
    CommittedBatch {
        receipt: WorkspaceCommitReceipt,
    },
    UnsupportedAtomicScope {
        requested: WorkspaceWriteAtomicity,
        available: WorkspaceWriteAtomicity,
    },
    VersionConflict {
        conflicts: Vec<WorkspaceVersionConflict>,
    },
    ReferencedContent {
        content: Vec<u8>,
    },
    Search {
        matches: Vec<WorkspaceSearchMatch>,
    },
    List {
        entries: Vec<WorkspaceEntry>,
    },
    Process {
        exit_code: i32,
        stdout: String,
        stderr: String,
        #[serde(default = "complete_capture")]
        stdout_complete: bool,
        #[serde(default = "complete_capture")]
        stderr_complete: bool,
        #[serde(default)]
        stdout_bytes: Option<u64>,
        #[serde(default)]
        stderr_bytes: Option<u64>,
        #[serde(default)]
        stdout_content_identity: Option<ArtifactRevision>,
        #[serde(default)]
        stderr_content_identity: Option<ArtifactRevision>,
        stdout_recovery: Box<ProcessStreamRecovery>,
        stderr_recovery: Box<ProcessStreamRecovery>,
    },
}
