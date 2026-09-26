use phenix_core::{ComponentInterface, InterfaceId, InterfaceSchema};
use phenix_sdk_macros::PhenixValue;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const ENVIRONMENT_SERVICE: &str = "phenix.environment@1";

pub struct EnvironmentInterface;

impl ComponentInterface for EnvironmentInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(ENVIRONMENT_SERVICE).expect("static environment interface id is valid")
    }

    fn schema() -> InterfaceSchema {
        InterfaceSchema::of::<EnvironmentCommand, EnvironmentResponse>()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentFileKind {
    File,
    Directory,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
pub struct EnvironmentDirEntry {
    pub path: String,
    pub kind: EnvironmentFileKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
pub struct EnvironmentDescription {
    pub provider: String,
    pub persistent_processes: bool,
    pub pty: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum EnvironmentCommand {
    Describe,
    Stat {
        path: String,
    },
    ReadFile {
        path: String,
    },
    WriteFile {
        path: String,
        content: Vec<u8>,
        create_parents: bool,
    },
    ReadDir {
        path: String,
    },
    Exec {
        program: String,
        arguments: Vec<String>,
        working_directory: Option<String>,
        environment: BTreeMap<String, String>,
    },
    OpenProcess {
        program: String,
        arguments: Vec<String>,
        working_directory: Option<String>,
        environment: BTreeMap<String, String>,
    },
    WriteProcess {
        handle: String,
        input: Vec<u8>,
    },
    PollProcess {
        handle: String,
    },
    CloseProcess {
        handle: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case")]
pub enum EnvironmentResponse {
    Description {
        environment: EnvironmentDescription,
    },
    Metadata {
        kind: Option<EnvironmentFileKind>,
    },
    File {
        content: Option<Vec<u8>>,
    },
    Written,
    Directory {
        entries: Vec<EnvironmentDirEntry>,
    },
    Process {
        exit_code: i32,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        truncated: bool,
    },
    ProcessOpened {
        handle: String,
    },
    ProcessOutput {
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        exit_code: Option<i32>,
        truncated: bool,
    },
    ProcessClosed {
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        exit_code: Option<i32>,
        truncated: bool,
    },
}

#[must_use]
pub fn environment_service() -> phenix_core::ServiceId {
    phenix_core::ServiceId::parse(ENVIRONMENT_SERVICE)
        .expect("static environment service id is valid")
}
