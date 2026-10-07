use phenix_core::{
    Authority, ComponentInterface, DurableSchema, PermissionId, PluginContext, PluginExecution,
    PluginHost, PluginId, PluginInstance, PluginManifest, ResourceNamespace, SdkClient,
    ServiceContribution, ServiceId, TransactionOp,
};
use phenix_sdk::{
    EnvironmentCommand, EnvironmentFileKind, EnvironmentInterface, EnvironmentResponse,
    ProcessStreamRecovery, WORKSPACE_SERVICE, WorkspaceCapabilities, WorkspaceCommand,
    WorkspaceCommitReceipt, WorkspaceCommittedFile, WorkspaceEntry, WorkspaceEntryKind,
    WorkspaceFileVersion, WorkspaceInterface, WorkspaceProjectFile, WorkspaceResponse,
    WorkspaceSearchMatch, WorkspaceVersionConflict, WorkspaceWrite, WorkspaceWriteAtomicity,
    WorkspaceWrittenFile,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
};

const WORKSPACE_PLUGIN: &str = "phenix.workspace";
const WORKSPACE_READ: &str = "workspace.read";
const WORKSPACE_WRITE: &str = "workspace.write";
const WORKSPACE_SHELL: &str = "workspace.shell";
const WORKSPACE_GIT: &str = "workspace.git";
const MAX_PROCESS_MODEL_VIEW_BYTES: usize = 64 * 1024;
const WORKSPACE_STATE_NAMESPACE: &str = "phenix.workspace.state";
const PERSISTENCE_SCHEMA: &str = "kernel.persistence.schema";
const PERSISTENCE_READ: &str = "kernel.persistence.read";
const PERSISTENCE_WRITE: &str = "kernel.persistence.write";

struct WorkspaceSdk<'host, 'runtime> {
    environment: SdkClient<'host, 'runtime, EnvironmentInterface>,
}

type WorkspaceContext<'host, 'runtime, 'state> =
    PluginContext<'host, 'runtime, WorkspaceSdk<'host, 'runtime>, (), &'state Path>;

fn context<'host, 'runtime, 'state>(
    host: &'host PluginHost<'runtime>,
    root: &'state Path,
) -> WorkspaceContext<'host, 'runtime, 'state> {
    PluginContext::new(
        host,
        WorkspaceSdk {
            environment: SdkClient::new(host, crate::workspace_component_id()),
        },
        (),
        root,
    )
}

#[must_use]
pub fn workspace_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(WORKSPACE_PLUGIN).expect("static plugin id is valid"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: phenix_core::ServiceRole::Terminal,
            service: workspace_service(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        resource_namespaces: vec![workspace_state_namespace()],
        maximum_authority: Authority::new([
            capability(WORKSPACE_READ),
            capability(WORKSPACE_WRITE),
            capability(WORKSPACE_SHELL),
            capability(WORKSPACE_GIT),
            capability(PERSISTENCE_SCHEMA),
            capability(PERSISTENCE_READ),
            capability(PERSISTENCE_WRITE),
        ]),
    }
}

#[must_use]
pub fn workspace_factory() -> Box<dyn PluginInstance> {
    Box::new(WorkspacePlugin::new(
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    ))
}

#[must_use]
pub fn workspace_factory_for(root: impl Into<PathBuf>) -> Box<dyn PluginInstance> {
    Box::new(WorkspacePlugin::new(root.into()))
}

#[must_use]
pub fn workspace_service() -> ServiceId {
    ServiceId::parse(WORKSPACE_SERVICE).expect("static service id is valid")
}

fn capability(value: &str) -> PermissionId {
    PermissionId::parse(value).expect("static capability is valid")
}

fn workspace_state_namespace() -> ResourceNamespace {
    ResourceNamespace::parse(WORKSPACE_STATE_NAMESPACE)
        .expect("static workspace state namespace is valid")
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WorkspaceCommitState {
    Prepared,
    Committed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct WorkspaceCommitJournalFile {
    path: String,
    before_version: WorkspaceFileVersion,
    content: String,
    version: WorkspaceFileVersion,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct WorkspaceCommitJournal {
    operation_id: String,
    intent_identity: String,
    state: WorkspaceCommitState,
    files: Vec<WorkspaceCommitJournalFile>,
}

impl WorkspaceCommitJournal {
    fn receipt(&self) -> WorkspaceCommitReceipt {
        WorkspaceCommitReceipt {
            operation_id: self.operation_id.clone(),
            intent_identity: self.intent_identity.clone(),
            files: self
                .files
                .iter()
                .map(|file| WorkspaceCommittedFile {
                    path: file.path.clone(),
                    before_version: file.before_version.clone(),
                    version: file.version.clone(),
                })
                .collect(),
        }
    }
}

struct WorkspacePlugin {
    root: PathBuf,
}

impl WorkspacePlugin {
    fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl PluginInstance for WorkspacePlugin {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        // The workspace root is an Environment-namespace path. Validating it with
        // host filesystem APIs would make remote/container providers impossible.
        let context = context(host, &self.root);
        context
            .kernel
            .register_durable_schema(&DurableSchema::new(workspace_state_namespace(), 1))
            .map_err(|error| error.to_string())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &workspace_service() {
            return Err(format!("unsupported workspace service: {service}"));
        }
        let context = context(host, &self.root);
        let interface = WorkspaceInterface::interface_id();
        let command = context
            .kernel
            .decode_projected::<WorkspaceCommand>(&interface, input)
            .map_err(|error| error.to_string())?;
        let response = handle(&context, command)?;
        context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string())
    }
}

fn handle(
    context: &WorkspaceContext<'_, '_, '_>,
    command: WorkspaceCommand,
) -> Result<WorkspaceResponse, String> {
    match command {
        WorkspaceCommand::Capabilities => capabilities(context),
        WorkspaceCommand::Read { path } => read(context, path),
        WorkspaceCommand::Write {
            path,
            content,
            expected_version,
        } => write(context, path, content, expected_version),
        WorkspaceCommand::WriteBatch { writes } => write_batch(context, writes),
        WorkspaceCommand::ReadContentReference { reference } => {
            require(context, WORKSPACE_READ)?;
            match environment(
                context,
                EnvironmentCommand::ReadContentReference { reference },
            )? {
                EnvironmentResponse::ReferencedContent {
                    content: Some(content),
                } => Ok(WorkspaceResponse::ReferencedContent { content }),
                EnvironmentResponse::ReferencedContent { content: None } => {
                    Err("workspace content reference is unavailable".into())
                }
                other => Err(format!(
                    "environment returned unexpected content-reference response: {other:?}"
                )),
            }
        }
        WorkspaceCommand::CommitBatch {
            operation_id,
            writes,
        } => commit_batch(context, operation_id, writes),
        WorkspaceCommand::Search {
            needle,
            path,
            case_sensitive,
        } => search(context, needle, path, case_sensitive),
        WorkspaceCommand::List { path, recursive } => list(context, path, recursive),
        WorkspaceCommand::DiscoverProjectFiles {
            working_directory,
            root_markers,
            file_names,
        } => discover_project_files(context, working_directory, root_markers, file_names),
        WorkspaceCommand::ReadBytes { path } => read_bytes(context, path),
        WorkspaceCommand::WriteBytes {
            path,
            content,
            expected_version,
        } => write_bytes(context, path, content, expected_version),
        WorkspaceCommand::Exec {
            program,
            arguments,
            working_directory,
            environment,
        } => exec(context, program, arguments, working_directory, environment),
        WorkspaceCommand::Shell { command } => {
            if command.trim().is_empty() {
                return Err("shell command must not be empty".into());
            }
            process(context, "bash", &["-c".into(), command], WORKSPACE_SHELL)
        }
        WorkspaceCommand::Git { arguments } => process(context, "git", &arguments, WORKSPACE_GIT),
    }
}

fn resolve(context: &WorkspaceContext<'_, '_, '_>, input: &str) -> Result<PathBuf, String> {
    let input = Path::new(input);
    if input.is_absolute() {
        return Err("workspace paths must be relative".into());
    }
    let mut relative = PathBuf::new();
    for component in input.components() {
        match component {
            Component::Normal(value) => relative.push(value),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err("workspace path escapes the configured root".into());
            }
        }
    }
    Ok(context.plugin.state.join(relative))
}

fn require(context: &WorkspaceContext<'_, '_, '_>, value: &str) -> Result<(), String> {
    let capability = capability(value);
    if context.call.authority.permits(&capability) {
        Ok(())
    } else {
        Err(format!("workspace authority denied: {value}"))
    }
}

fn environment(
    context: &WorkspaceContext<'_, '_, '_>,
    command: EnvironmentCommand,
) -> Result<EnvironmentResponse, String> {
    context
        .sdk
        .environment
        .invoke_projected::<EnvironmentCommand, EnvironmentResponse>(&command)
        .map_err(|error| error.to_string())
}

fn environment_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn capabilities(context: &WorkspaceContext<'_, '_, '_>) -> Result<WorkspaceResponse, String> {
    let EnvironmentResponse::Description { environment } =
        environment(context, EnvironmentCommand::Describe)?
    else {
        return Err("environment returned unexpected description response".into());
    };
    Ok(WorkspaceResponse::Capabilities {
        capabilities: WorkspaceCapabilities {
            write_atomicity: WorkspaceWriteAtomicity::PreconditionCheckedSequential,
            recoverable_commit_atomicity: environment
                .atomic_file_replace
                .then_some(WorkspaceWriteAtomicity::CrashRecoverable),
        },
    })
}

fn require_recoverable_commit(
    context: &WorkspaceContext<'_, '_, '_>,
) -> Result<Option<WorkspaceResponse>, String> {
    let EnvironmentResponse::Description { environment } =
        environment(context, EnvironmentCommand::Describe)?
    else {
        return Err("environment returned unexpected description response".into());
    };
    if environment.atomic_file_replace {
        Ok(None)
    } else {
        Ok(Some(WorkspaceResponse::UnsupportedAtomicScope {
            requested: WorkspaceWriteAtomicity::CrashRecoverable,
            available: WorkspaceWriteAtomicity::PreconditionCheckedSequential,
        }))
    }
}

fn read(context: &WorkspaceContext<'_, '_, '_>, path: String) -> Result<WorkspaceResponse, String> {
    require(context, WORKSPACE_READ)?;
    let resolved = resolve(context, &path)?;
    let response = environment(
        context,
        EnvironmentCommand::ReadFile {
            path: environment_path(&resolved),
        },
    )?;
    let EnvironmentResponse::File {
        content: Some(bytes),
    } = response
    else {
        return Err(format!("read {path}: file not found"));
    };
    let version = version_for_bytes(&bytes);
    let content = String::from_utf8(bytes)
        .map_err(|_| format!("workspace read requires UTF-8 text: {path}"))?;
    Ok(WorkspaceResponse::Read {
        path,
        content,
        version,
    })
}

fn read_bytes(
    context: &WorkspaceContext<'_, '_, '_>,
    path: String,
) -> Result<WorkspaceResponse, String> {
    require(context, WORKSPACE_READ)?;
    let resolved = resolve(context, &path)?;
    let response = environment(
        context,
        EnvironmentCommand::ReadFile {
            path: environment_path(&resolved),
        },
    )?;
    let EnvironmentResponse::File {
        content: Some(content),
    } = response
    else {
        return Err(format!("read {path}: file not found"));
    };
    let version = version_for_bytes(&content);
    Ok(WorkspaceResponse::ReadBytes {
        path,
        content,
        version,
    })
}

fn write(
    context: &WorkspaceContext<'_, '_, '_>,
    path: String,
    content: String,
    expected_version: WorkspaceFileVersion,
) -> Result<WorkspaceResponse, String> {
    require(context, WORKSPACE_WRITE)?;
    let resolved = resolve(context, &path)?;
    let observed = inspect_version(context, &resolved, &path)?;
    if observed != expected_version {
        return Err(format!(
            "workspace version conflict for {path}: expected {expected_version:?}, observed {observed:?}"
        ));
    }
    write_resolved(context, &resolved, &path, &content)?;
    Ok(WorkspaceResponse::Written {
        path,
        version: version_for_bytes(content.as_bytes()),
    })
}

fn write_bytes(
    context: &WorkspaceContext<'_, '_, '_>,
    path: String,
    content: Vec<u8>,
    expected_version: WorkspaceFileVersion,
) -> Result<WorkspaceResponse, String> {
    require(context, WORKSPACE_WRITE)?;
    let resolved = resolve(context, &path)?;
    let observed = inspect_version(context, &resolved, &path)?;
    let desired = version_for_bytes(&content);
    if observed != expected_version && observed != desired {
        return Ok(WorkspaceResponse::VersionConflict {
            conflicts: vec![WorkspaceVersionConflict {
                path,
                expected_version,
                observed_version: observed,
            }],
        });
    }
    if observed != desired {
        write_resolved_bytes(context, &resolved, &path, &content)?;
    }
    Ok(WorkspaceResponse::Written {
        path,
        version: desired,
    })
}

fn write_batch(
    context: &WorkspaceContext<'_, '_, '_>,
    writes: Vec<WorkspaceWrite>,
) -> Result<WorkspaceResponse, String> {
    require(context, WORKSPACE_WRITE)?;
    if writes.is_empty() {
        return Err("workspace write batch must not be empty".into());
    }

    let mut paths = BTreeSet::new();
    let mut prepared = Vec::with_capacity(writes.len());
    let mut conflicts = Vec::new();
    for write in writes {
        if !paths.insert(write.path.clone()) {
            return Err(format!(
                "workspace write batch contains duplicate path: {}",
                write.path
            ));
        }
        let resolved = resolve(context, &write.path)?;
        let observed = inspect_version(context, &resolved, &write.path)?;
        let desired = version_for_bytes(write.content.as_bytes());
        if observed != write.expected_version && observed != desired {
            conflicts.push(WorkspaceVersionConflict {
                path: write.path.clone(),
                expected_version: write.expected_version.clone(),
                observed_version: observed.clone(),
            });
        }
        prepared.push((write, resolved, observed, desired));
    }

    if !conflicts.is_empty() {
        return Ok(WorkspaceResponse::VersionConflict { conflicts });
    }

    let mut files = Vec::with_capacity(prepared.len());
    for (write, resolved, observed, desired) in prepared {
        if observed != desired {
            write_resolved(context, &resolved, &write.path, &write.content)?;
        }
        files.push(WorkspaceWrittenFile {
            path: write.path,
            version: desired,
        });
    }
    Ok(WorkspaceResponse::WrittenBatch { files })
}

fn commit_batch(
    context: &WorkspaceContext<'_, '_, '_>,
    operation_id: String,
    writes: Vec<WorkspaceWrite>,
) -> Result<WorkspaceResponse, String> {
    require(context, WORKSPACE_WRITE)?;
    if operation_id.trim().is_empty() {
        return Err("workspace commit operation id must not be empty".into());
    }
    if writes.is_empty() {
        return Err("workspace commit batch must not be empty".into());
    }
    let intent_identity = commit_intent_identity(&writes)?;
    if let Some(mut journal) = read_commit_journal(context, &operation_id)? {
        if journal.intent_identity != intent_identity {
            return Err(format!(
                "workspace commit operation {operation_id} is already bound to another intent"
            ));
        }
        if journal.state == WorkspaceCommitState::Prepared {
            if let Some(response) = require_recoverable_commit(context)? {
                return Ok(response);
            }
            recover_commit(context, &mut journal)?;
        }
        return Ok(WorkspaceResponse::CommittedBatch {
            receipt: journal.receipt(),
        });
    }

    if let Some(response) = require_recoverable_commit(context)? {
        return Ok(response);
    }

    let mut paths = BTreeSet::new();
    let mut files = Vec::with_capacity(writes.len());
    let mut conflicts = Vec::new();
    for write in writes {
        if !paths.insert(write.path.clone()) {
            return Err(format!(
                "workspace commit batch contains duplicate path: {}",
                write.path
            ));
        }
        let resolved = resolve(context, &write.path)?;
        let observed = inspect_version(context, &resolved, &write.path)?;
        if observed != write.expected_version {
            conflicts.push(WorkspaceVersionConflict {
                path: write.path,
                expected_version: write.expected_version,
                observed_version: observed,
            });
            continue;
        }
        files.push(WorkspaceCommitJournalFile {
            path: write.path,
            before_version: observed,
            version: version_for_bytes(write.content.as_bytes()),
            content: write.content,
        });
    }
    if !conflicts.is_empty() {
        return Ok(WorkspaceResponse::VersionConflict { conflicts });
    }

    let mut journal = WorkspaceCommitJournal {
        operation_id,
        intent_identity,
        state: WorkspaceCommitState::Prepared,
        files,
    };
    persist_commit_journal(context, None, &journal)?;
    recover_commit(context, &mut journal)?;
    Ok(WorkspaceResponse::CommittedBatch {
        receipt: journal.receipt(),
    })
}

fn commit_intent_identity(writes: &[WorkspaceWrite]) -> Result<String, String> {
    let encoded = serde_json::to_vec(writes).map_err(|error| error.to_string())?;
    Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
}

fn commit_journal_key(context: &WorkspaceContext<'_, '_, '_>, operation_id: &str) -> String {
    let root = environment_path(context.plugin.state);
    let root_identity = format!("{:x}", Sha256::digest(root.as_bytes()));
    let operation_identity = format!("{:x}", Sha256::digest(operation_id.as_bytes()));
    format!("commit/{root_identity}/{operation_identity}")
}

fn read_commit_journal(
    context: &WorkspaceContext<'_, '_, '_>,
    operation_id: &str,
) -> Result<Option<WorkspaceCommitJournal>, String> {
    let key = commit_journal_key(context, operation_id);
    context
        .kernel
        .read_durable(&workspace_state_namespace(), &key)
        .map_err(|error| error.to_string())?
        .map(|value| {
            let journal: WorkspaceCommitJournal =
                serde_json::from_slice(&value).map_err(|error| error.to_string())?;
            if journal.operation_id != operation_id {
                return Err("workspace commit journal identity collision".into());
            }
            Ok(journal)
        })
        .transpose()
}

fn persist_commit_journal(
    context: &WorkspaceContext<'_, '_, '_>,
    expected: Option<&WorkspaceCommitJournal>,
    journal: &WorkspaceCommitJournal,
) -> Result<(), String> {
    let key = commit_journal_key(context, &journal.operation_id);
    let expected = expected
        .map(serde_json::to_vec)
        .transpose()
        .map_err(|error| error.to_string())?;
    let value = serde_json::to_vec(journal).map_err(|error| error.to_string())?;
    context
        .kernel
        .transact_durable(
            &workspace_state_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: key.clone(),
                    expected,
                },
                TransactionOp::Put { key, value },
            ],
        )
        .map_err(|error| error.to_string())
}

fn recover_commit(
    context: &WorkspaceContext<'_, '_, '_>,
    journal: &mut WorkspaceCommitJournal,
) -> Result<(), String> {
    if journal.state == WorkspaceCommitState::Committed {
        return Ok(());
    }

    for file in &journal.files {
        let resolved = resolve(context, &file.path)?;
        let observed = inspect_version(context, &resolved, &file.path)?;
        if observed == file.version {
            continue;
        }
        if observed != file.before_version {
            return Err(format!(
                "workspace commit recovery conflict for {}: expected preimage {:?} or target {:?}, observed {:?}",
                file.path, file.before_version, file.version, observed
            ));
        }
        write_resolved(context, &resolved, &file.path, &file.content)?;
        let confirmed = inspect_version(context, &resolved, &file.path)?;
        if confirmed != file.version {
            return Err(format!(
                "workspace commit write verification failed for {}: expected {:?}, observed {:?}",
                file.path, file.version, confirmed
            ));
        }
    }

    let prepared = journal.clone();
    journal.state = WorkspaceCommitState::Committed;
    persist_commit_journal(context, Some(&prepared), journal)
}

fn inspect_version(
    context: &WorkspaceContext<'_, '_, '_>,
    resolved: &Path,
    path: &str,
) -> Result<WorkspaceFileVersion, String> {
    match environment(
        context,
        EnvironmentCommand::ReadFile {
            path: environment_path(resolved),
        },
    )? {
        EnvironmentResponse::File {
            content: Some(bytes),
        } => Ok(version_for_bytes(&bytes)),
        EnvironmentResponse::File { content: None } => Ok(WorkspaceFileVersion::Absent),
        other => Err(format!(
            "inspect {path}: environment returned unexpected response {other:?}"
        )),
    }
}

fn write_resolved(
    context: &WorkspaceContext<'_, '_, '_>,
    resolved: &Path,
    path: &str,
    content: &str,
) -> Result<(), String> {
    write_resolved_bytes(context, resolved, path, content.as_bytes())
}

fn write_resolved_bytes(
    context: &WorkspaceContext<'_, '_, '_>,
    resolved: &Path,
    path: &str,
    content: &[u8],
) -> Result<(), String> {
    match environment(
        context,
        EnvironmentCommand::WriteFile {
            path: environment_path(resolved),
            content: content.to_vec(),
            create_parents: true,
        },
    )? {
        EnvironmentResponse::Written => Ok(()),
        other => Err(format!(
            "write {path}: environment returned unexpected response {other:?}"
        )),
    }
}

fn discover_project_files(
    context: &WorkspaceContext<'_, '_, '_>,
    working_directory: String,
    root_markers: Vec<String>,
    file_names: Vec<String>,
) -> Result<WorkspaceResponse, String> {
    require(context, WORKSPACE_READ)?;
    if file_names.is_empty() {
        return Err("project discovery requires at least one file name".into());
    }
    for value in root_markers.iter().chain(file_names.iter()) {
        validate_project_discovery_name(value)?;
    }

    let working_directory = PathBuf::from(working_directory);
    let working_directory = if working_directory.is_absolute() {
        working_directory
    } else {
        context.plugin.state.join(working_directory)
    };

    let mut project_root = None;
    if !root_markers.is_empty() {
        for directory in working_directory.ancestors() {
            let mut matched = false;
            for marker in &root_markers {
                let candidate = directory.join(marker);
                match environment(
                    context,
                    EnvironmentCommand::Stat {
                        path: environment_path(&candidate),
                    },
                ) {
                    Ok(EnvironmentResponse::Metadata { kind: Some(_) }) => {
                        matched = true;
                        break;
                    }
                    Ok(EnvironmentResponse::Metadata { kind: None }) | Err(_) => {}
                    Ok(other) => {
                        return Err(format!(
                            "project discovery stat {} returned unexpected response {other:?}",
                            candidate.display()
                        ));
                    }
                }
            }
            if matched {
                project_root = Some(directory.to_path_buf());
                break;
            }
        }
    }
    let project_root = project_root.unwrap_or_else(|| working_directory.clone());

    let relative = working_directory.strip_prefix(&project_root).map_err(|_| {
        "project discovery working directory is not beneath the discovered root".to_owned()
    })?;
    let mut directories = vec![project_root.clone()];
    let mut current = project_root.clone();
    for component in relative.components() {
        match component {
            Component::Normal(value) => {
                current.push(value);
                directories.push(current.clone());
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(
                    "project discovery working directory contains invalid components".into(),
                );
            }
        }
    }

    let mut files = Vec::new();
    for directory in directories {
        for file_name in &file_names {
            let candidate = directory.join(file_name);
            match environment(
                context,
                EnvironmentCommand::Stat {
                    path: environment_path(&candidate),
                },
            )? {
                EnvironmentResponse::Metadata {
                    kind: Some(EnvironmentFileKind::File),
                } => {}
                EnvironmentResponse::Metadata { .. } => continue,
                other => {
                    return Err(format!(
                        "project discovery stat {} returned unexpected response {other:?}",
                        candidate.display()
                    ));
                }
            }

            let response = environment(
                context,
                EnvironmentCommand::ReadFile {
                    path: environment_path(&candidate),
                },
            )?;
            let EnvironmentResponse::File { content } = response else {
                return Err(format!(
                    "project discovery read {} returned unexpected response {response:?}",
                    candidate.display()
                ));
            };
            let Some(content) = content else {
                continue;
            };
            let content = String::from_utf8_lossy(&content).into_owned();
            let relative = candidate.strip_prefix(&project_root).map_err(|_| {
                format!(
                    "project discovery result escaped root: {}",
                    candidate.display()
                )
            })?;
            files.push(WorkspaceProjectFile {
                path: relative.to_string_lossy().into_owned(),
                content,
            });
            break;
        }
    }

    Ok(WorkspaceResponse::ProjectFiles {
        root: environment_path(&project_root),
        files,
    })
}

fn validate_project_discovery_name(value: &str) -> Result<(), String> {
    if value.contains(['/', '\\', '\0', ':']) {
        return Err(format!(
            "project discovery names must be portable single path components: {value}"
        ));
    }

    let mut components = Path::new(value).components();
    let Some(Component::Normal(_)) = components.next() else {
        return Err(format!(
            "project discovery names must be portable single path components: {value}"
        ));
    };
    if components.next().is_some() {
        return Err(format!(
            "project discovery names must be portable single path components: {value}"
        ));
    }
    Ok(())
}

fn list(
    context: &WorkspaceContext<'_, '_, '_>,
    path: Option<String>,
    recursive: bool,
) -> Result<WorkspaceResponse, String> {
    require(context, WORKSPACE_READ)?;
    let relative = path.unwrap_or_else(|| ".".into());
    let root = resolve(context, &relative)?;
    let mut entries = Vec::new();
    list_path(
        context,
        context.plugin.state,
        &root,
        recursive,
        true,
        &mut entries,
    )?;
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(WorkspaceResponse::List { entries })
}

fn list_path(
    context: &WorkspaceContext<'_, '_, '_>,
    workspace_root: &Path,
    path: &Path,
    recursive: bool,
    is_root: bool,
    entries: &mut Vec<WorkspaceEntry>,
) -> Result<(), String> {
    if path.file_name().is_some_and(|name| name == ".git") {
        return Ok(());
    }
    let kind = match environment(
        context,
        EnvironmentCommand::Stat {
            path: environment_path(path),
        },
    )? {
        EnvironmentResponse::Metadata { kind } => kind,
        other => {
            return Err(format!(
                "list {}: environment returned unexpected response {other:?}",
                path.display()
            ));
        }
    };
    let Some(kind) = kind else {
        return Ok(());
    };
    if !is_root {
        let relative = path.strip_prefix(workspace_root).unwrap_or(path);
        entries.push(WorkspaceEntry {
            path: relative.to_string_lossy().into_owned(),
            kind: match kind {
                EnvironmentFileKind::File => WorkspaceEntryKind::File,
                EnvironmentFileKind::Directory => WorkspaceEntryKind::Directory,
                EnvironmentFileKind::Other => WorkspaceEntryKind::Other,
            },
        });
    }
    if kind != EnvironmentFileKind::Directory {
        return Ok(());
    }
    let response = environment(
        context,
        EnvironmentCommand::ReadDir {
            path: environment_path(path),
        },
    )?;
    let EnvironmentResponse::Directory { entries: children } = response else {
        return Err(format!(
            "list {}: environment returned non-directory response",
            path.display()
        ));
    };
    for child in children {
        if child.kind == EnvironmentFileKind::Other {
            continue;
        }
        let child_path = workspace_directory_child(workspace_root, path, &child.path)?;
        if recursive {
            list_path(context, workspace_root, &child_path, true, false, entries)?;
        } else {
            let relative = child_path
                .strip_prefix(workspace_root)
                .unwrap_or(&child_path);
            entries.push(WorkspaceEntry {
                path: relative.to_string_lossy().into_owned(),
                kind: match child.kind {
                    EnvironmentFileKind::File => WorkspaceEntryKind::File,
                    EnvironmentFileKind::Directory => WorkspaceEntryKind::Directory,
                    EnvironmentFileKind::Other => WorkspaceEntryKind::Other,
                },
            });
        }
    }
    Ok(())
}

fn search(
    context: &WorkspaceContext<'_, '_, '_>,
    needle: String,
    path: Option<String>,
    case_sensitive: bool,
) -> Result<WorkspaceResponse, String> {
    require(context, WORKSPACE_READ)?;
    if needle.is_empty() {
        return Err("workspace search needle must not be empty".into());
    }
    let relative = path.unwrap_or_else(|| ".".into());
    let root = resolve(context, &relative)?;
    let mut matches = Vec::new();
    search_path(
        context,
        context.plugin.state,
        &root,
        &needle,
        case_sensitive,
        &mut matches,
    )?;
    matches.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.line.cmp(&right.line))
    });
    Ok(WorkspaceResponse::Search { matches })
}

fn search_path(
    context: &WorkspaceContext<'_, '_, '_>,
    workspace_root: &Path,
    path: &Path,
    needle: &str,
    case_sensitive: bool,
    matches: &mut Vec<WorkspaceSearchMatch>,
) -> Result<(), String> {
    if path.file_name().is_some_and(|name| name == ".git") {
        return Ok(());
    }
    let kind = match environment(
        context,
        EnvironmentCommand::Stat {
            path: environment_path(path),
        },
    )? {
        EnvironmentResponse::Metadata { kind } => kind,
        other => {
            return Err(format!(
                "search {}: environment returned unexpected response {other:?}",
                path.display()
            ));
        }
    };
    match kind {
        Some(EnvironmentFileKind::Directory) => {
            let response = environment(
                context,
                EnvironmentCommand::ReadDir {
                    path: environment_path(path),
                },
            )?;
            let EnvironmentResponse::Directory { entries } = response else {
                return Err(format!(
                    "search {}: environment returned non-directory response",
                    path.display()
                ));
            };
            for entry in entries {
                if entry.kind == EnvironmentFileKind::Other {
                    continue;
                }
                let child = workspace_directory_child(workspace_root, path, &entry.path)?;
                search_path(
                    context,
                    workspace_root,
                    &child,
                    needle,
                    case_sensitive,
                    matches,
                )?;
            }
            Ok(())
        }
        Some(EnvironmentFileKind::File) => {
            let response = environment(
                context,
                EnvironmentCommand::ReadFile {
                    path: environment_path(path),
                },
            )?;
            let EnvironmentResponse::File {
                content: Some(bytes),
            } = response
            else {
                return Ok(());
            };
            let Ok(content) = String::from_utf8(bytes) else {
                return Ok(());
            };
            let query = if case_sensitive {
                needle.to_owned()
            } else {
                needle.to_lowercase()
            };
            for (index, line) in content.lines().enumerate() {
                let candidate = if case_sensitive {
                    line.to_owned()
                } else {
                    line.to_lowercase()
                };
                if candidate.contains(&query) {
                    let relative = path.strip_prefix(workspace_root).unwrap_or(path);
                    matches.push(WorkspaceSearchMatch {
                        path: relative.to_string_lossy().into_owned(),
                        line: u64::try_from(index)
                            .ok()
                            .and_then(|line| line.checked_add(1))
                            .ok_or_else(|| "workspace search line overflow".to_owned())?,
                        text: line.to_owned(),
                    });
                }
            }
            Ok(())
        }
        Some(EnvironmentFileKind::Other) | None => Ok(()),
    }
}

fn workspace_directory_child(
    workspace_root: &Path,
    parent: &Path,
    environment_path: &str,
) -> Result<PathBuf, String> {
    let environment_path = Path::new(environment_path);
    let relative = environment_path.strip_prefix(parent).map_err(|_| {
        format!(
            "environment returned directory entry outside requested directory: {}",
            environment_path.display()
        )
    })?;
    let mut components = relative.components();
    let Some(Component::Normal(name)) = components.next() else {
        return Err(format!(
            "environment returned invalid directory entry: {}",
            environment_path.display()
        ));
    };
    if components.next().is_some() {
        return Err(format!(
            "environment returned non-child directory entry: {}",
            environment_path.display()
        ));
    }

    let child = parent.join(name);
    child.strip_prefix(workspace_root).map_err(|_| {
        format!(
            "environment returned directory entry outside workspace root: {}",
            environment_path.display()
        )
    })?;
    Ok(child)
}

fn exec(
    context: &WorkspaceContext<'_, '_, '_>,
    program: String,
    arguments: Vec<String>,
    working_directory: Option<String>,
    environment_values: BTreeMap<String, String>,
) -> Result<WorkspaceResponse, String> {
    if program.trim().is_empty() {
        return Err("workspace exec program must not be empty".into());
    }
    let working_directory = match working_directory {
        Some(path) => resolve(context, &path)?,
        None => context.plugin.state.to_path_buf(),
    };
    process_in(
        context,
        &program,
        &arguments,
        &working_directory,
        environment_values,
        WORKSPACE_SHELL,
    )
}

fn process(
    context: &WorkspaceContext<'_, '_, '_>,
    program: &str,
    args: &[String],
    capability: &str,
) -> Result<WorkspaceResponse, String> {
    process_in(
        context,
        program,
        args,
        context.plugin.state,
        BTreeMap::new(),
        capability,
    )
}

fn process_in(
    context: &WorkspaceContext<'_, '_, '_>,
    program: &str,
    args: &[String],
    working_directory: &Path,
    environment_values: BTreeMap<String, String>,
    capability: &str,
) -> Result<WorkspaceResponse, String> {
    require(context, capability)?;
    match environment(
        context,
        EnvironmentCommand::Exec {
            program: program.to_owned(),
            arguments: args.to_vec(),
            working_directory: Some(environment_path(working_directory)),
            environment: environment_values,
        },
    )? {
        EnvironmentResponse::Process {
            exit_code,
            stdout,
            stderr,
            stdout_complete,
            stderr_complete,
            stdout_bytes,
            stderr_bytes,
            stdout_content_identity,
            stderr_content_identity,
            stdout_recovery,
            stderr_recovery,
            ..
        } => {
            let (stdout, stdout_view_complete) = process_model_view(stdout, &stdout_recovery);
            let (stderr, stderr_view_complete) = process_model_view(stderr, &stderr_recovery);
            Ok(WorkspaceResponse::Process {
                exit_code,
                stdout,
                stderr,
                stdout_complete: stdout_complete && stdout_view_complete,
                stderr_complete: stderr_complete && stderr_view_complete,
                stdout_bytes,
                stderr_bytes,
                stdout_content_identity,
                stderr_content_identity,
                stdout_recovery: Box::new(stdout_recovery),
                stderr_recovery: Box::new(stderr_recovery),
            })
        }
        other => Err(format!(
            "environment returned unexpected process response: {other:?}"
        )),
    }
}

fn process_model_view(bytes: Vec<u8>, recovery: &ProcessStreamRecovery) -> (String, bool) {
    let may_collapse = matches!(recovery, ProcessStreamRecovery::Reference { .. });
    if !may_collapse || bytes.len() <= MAX_PROCESS_MODEL_VIEW_BYTES {
        return (String::from_utf8_lossy(&bytes).into_owned(), true);
    }
    (
        String::from_utf8_lossy(&bytes[..MAX_PROCESS_MODEL_VIEW_BYTES]).into_owned(),
        false,
    )
}

fn version_for_bytes(bytes: &[u8]) -> WorkspaceFileVersion {
    WorkspaceFileVersion::Present {
        content_hash: format!("{:x}", Sha256::digest(bytes)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace_component_manifest;
    use phenix_core::{
        ComponentExport, ComponentId, ComponentManifest, Kernel, KernelConfig, PhenixValue,
        Project, ResolvedGeneration, ResolvedGenerationActivation,
    };
    use phenix_plugin_environment_local::{
        local_environment_component_manifest, local_environment_factory_for,
        local_environment_manifest,
    };
    use phenix_sdk::{ProcessStreamRecovery, environment_service};
    use std::{
        fs,
        process::Command,
        sync::{Arc, Mutex},
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_workspace(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("phenix-{name}-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn fixture_environment_manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId::parse("fixture.environment").unwrap(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: vec![ServiceContribution {
                role: phenix_core::ServiceRole::Terminal,
                service: environment_service(),
                priority: 200,
                required_authority: Authority::default(),
            }],
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        }
    }

    fn fixture_environment_component_manifest() -> ComponentManifest {
        ComponentManifest {
            listeners: Vec::new(),
            id: ComponentId::parse("fixture.environment").unwrap(),
            owner: fixture_environment_manifest().id,
            imports: Vec::new(),
            exports: vec![ComponentExport {
                interface: EnvironmentInterface::interface_id(),
                schema: EnvironmentInterface::schema(),
                priority: 200,
                required_authority: Authority::default(),
            }],
            maximum_authority: Authority::default(),
        }
    }

    struct FixtureEnvironment {
        commands: Arc<Mutex<Vec<EnvironmentCommand>>>,
    }

    impl PluginInstance for FixtureEnvironment {
        fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
            Ok(())
        }

        fn invoke(
            &mut self,
            service: &ServiceId,
            input: &[u8],
            host: &PluginHost<'_>,
        ) -> Result<Vec<u8>, String> {
            if service != &environment_service() {
                return Err(format!(
                    "unsupported fixture environment service: {service}"
                ));
            }

            let context = PluginContext::new(host, (), (), ());
            let command = context
                .kernel
                .decode_projected::<EnvironmentCommand>(
                    &EnvironmentInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            self.commands
                .lock()
                .map_err(|_| "fixture environment command log poisoned".to_owned())?
                .push(command.clone());

            let response = match command {
                EnvironmentCommand::Describe => EnvironmentResponse::Description {
                    environment: phenix_sdk::EnvironmentDescription {
                        provider: "fixture.environment".into(),
                        filesystem_policy: phenix_sdk::EnvironmentFilesystemPolicy::Unrestricted,
                        atomic_file_replace: false,
                        persistent_processes: false,
                        pty: false,
                    },
                },
                EnvironmentCommand::Stat { path } if path.ends_with(".git") => {
                    return Err("fixture marker stat denied".into());
                }
                EnvironmentCommand::Stat { path } => EnvironmentResponse::Metadata {
                    kind: path
                        .ends_with("AGENTS.md")
                        .then_some(EnvironmentFileKind::File),
                },
                EnvironmentCommand::ReadFile { path } => EnvironmentResponse::File {
                    content: if path.ends_with("input.txt") {
                        Some(b"virtual-content".to_vec())
                    } else if path.ends_with("AGENTS.md") {
                        Some(b"fixture project rules".to_vec())
                    } else {
                        None
                    },
                },
                EnvironmentCommand::WriteFile { .. } => EnvironmentResponse::Written,
                EnvironmentCommand::ReadContentReference { reference } => {
                    EnvironmentResponse::ReferencedContent {
                        content: (reference.media_type == "application/x-phenix-fixture")
                            .then(|| b"fixture-recovered".to_vec()),
                    }
                }
                EnvironmentCommand::Exec { .. } => EnvironmentResponse::Process {
                    exit_code: 0,
                    stdout: b"fixture-process".to_vec(),
                    stderr: Vec::new(),
                    truncated: false,
                    stdout_complete: true,
                    stderr_complete: true,
                    stdout_bytes: Some(b"fixture-process".len() as u64),
                    stderr_bytes: Some(0),
                    stdout_content_identity: Some(phenix_core::ArtifactRevision::from_content(
                        b"fixture-process",
                    )),
                    stderr_content_identity: Some(phenix_core::ArtifactRevision::from_content(b"")),
                    stdout_recovery: ProcessStreamRecovery::Inline,
                    stderr_recovery: ProcessStreamRecovery::Inline,
                },
                other => return Err(format!("unexpected fixture environment command: {other:?}")),
            };

            context
                .kernel
                .encode_value(&response)
                .map_err(|error| error.to_string())
        }
    }

    struct EscapingDirectoryEnvironment {
        commands: Arc<Mutex<Vec<EnvironmentCommand>>>,
        root: PathBuf,
        outside: PathBuf,
    }

    impl PluginInstance for EscapingDirectoryEnvironment {
        fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
            Ok(())
        }

        fn invoke(
            &mut self,
            service: &ServiceId,
            input: &[u8],
            host: &PluginHost<'_>,
        ) -> Result<Vec<u8>, String> {
            if service != &environment_service() {
                return Err(format!(
                    "unsupported escaping fixture environment service: {service}"
                ));
            }

            let context = PluginContext::new(host, (), (), ());
            let command = context
                .kernel
                .decode_projected::<EnvironmentCommand>(
                    &EnvironmentInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            self.commands
                .lock()
                .map_err(|_| "escaping fixture command log poisoned".to_owned())?
                .push(command.clone());

            let response = match command {
                EnvironmentCommand::Stat { path } if Path::new(&path) == self.root.as_path() => {
                    EnvironmentResponse::Metadata {
                        kind: Some(EnvironmentFileKind::Directory),
                    }
                }
                EnvironmentCommand::ReadDir { path } if Path::new(&path) == self.root.as_path() => {
                    EnvironmentResponse::Directory {
                        entries: vec![phenix_sdk::EnvironmentDirEntry {
                            path: self.outside.to_string_lossy().into_owned(),
                            kind: EnvironmentFileKind::File,
                        }],
                    }
                }
                EnvironmentCommand::Stat { path } if Path::new(&path) == self.outside.as_path() => {
                    EnvironmentResponse::Metadata {
                        kind: Some(EnvironmentFileKind::File),
                    }
                }
                EnvironmentCommand::ReadFile { path }
                    if Path::new(&path) == self.outside.as_path() =>
                {
                    EnvironmentResponse::File {
                        content: Some(b"needle outside".to_vec()),
                    }
                }
                other => {
                    return Err(format!(
                        "unexpected escaping fixture environment command: {other:?}"
                    ));
                }
            };

            context
                .kernel
                .encode_value(&response)
                .map_err(|error| error.to_string())
        }
    }

    fn kernel(root: PathBuf) -> Kernel {
        let workspace = workspace_manifest();
        let workspace_id = workspace.id.clone();
        let environment = local_environment_manifest();
        let environment_id = environment.id.clone();
        let resolved = ResolvedGeneration::resolve(
            [workspace.clone(), environment.clone()],
            [
                workspace_component_manifest(),
                local_environment_component_manifest(),
            ],
            [],
            &workspace.maximum_authority,
        )
        .unwrap();
        let mut kernel = Kernel::new(KernelConfig::new([workspace, environment]).unwrap());
        kernel.activate_resolved_generation(&resolved).unwrap();

        let workspace_root = root.clone();
        kernel
            .register_embedded_factory(workspace_id, move || {
                workspace_factory_for(workspace_root.clone())
            })
            .unwrap();
        kernel
            .register_embedded_factory(environment_id, move || {
                local_environment_factory_for(root.clone())
            })
            .unwrap();
        kernel.activate_all().unwrap();
        kernel
    }

    fn authority(values: &[&str]) -> Authority {
        Authority::new(values.iter().map(|value| capability(value)))
    }

    fn invoke(
        kernel: &mut Kernel,
        command: WorkspaceCommand,
        authority: &Authority,
    ) -> Result<WorkspaceResponse, String> {
        let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
        let output = kernel
            .invoke(&workspace_service(), &input, authority, None)
            .map_err(|error| error.to_string())?;
        {
            let output: PhenixValue =
                serde_json::from_slice(&output).map_err(|error| error.to_string())?;
            WorkspaceResponse::try_from(Project(&output)).map_err(|error| error.to_string())
        }
    }

    #[test]
    fn project_file_discovery_ignores_failed_root_marker_probes() {
        let workspace = workspace_manifest();
        let workspace_id = workspace.id.clone();
        let environment = fixture_environment_manifest();
        let environment_id = environment.id.clone();
        let resolved = ResolvedGeneration::resolve(
            [workspace.clone(), environment.clone()],
            [
                workspace_component_manifest(),
                fixture_environment_component_manifest(),
            ],
            [],
            &workspace.maximum_authority,
        )
        .unwrap();
        let mut kernel = Kernel::new(KernelConfig::new([workspace, environment]).unwrap());
        kernel.activate_resolved_generation(&resolved).unwrap();

        let virtual_root = PathBuf::from("/phenix-fixture-environment-only/project");
        let workspace_root = virtual_root.clone();
        kernel
            .register_embedded_factory(workspace_id, move || {
                workspace_factory_for(workspace_root.clone())
            })
            .unwrap();
        let commands = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&commands);
        kernel
            .register_embedded_factory(environment_id, move || {
                Box::new(FixtureEnvironment {
                    commands: Arc::clone(&recorded),
                })
            })
            .unwrap();
        kernel.activate_all().unwrap();

        let response = invoke(
            &mut kernel,
            WorkspaceCommand::DiscoverProjectFiles {
                working_directory: virtual_root.to_string_lossy().into_owned(),
                root_markers: vec![".git".into()],
                file_names: vec!["AGENTS.override.md".into(), "AGENTS.md".into()],
            },
            &authority(&[WORKSPACE_READ]),
        )
        .unwrap();

        assert_eq!(
            response,
            WorkspaceResponse::ProjectFiles {
                root: virtual_root.to_string_lossy().into_owned(),
                files: vec![WorkspaceProjectFile {
                    path: "AGENTS.md".into(),
                    content: "fixture project rules".into(),
                }],
            }
        );
        assert!(commands.lock().unwrap().iter().any(|command| matches!(
            command,
            EnvironmentCommand::Stat { path } if path.ends_with(".git")
        )));
    }

    #[test]
    fn project_file_discovery_walks_ancestors_and_only_reads_the_cwd_chain() {
        let outer = temp_workspace("project-discovery");
        let repository = outer.join("repo");
        let cwd = repository.join("src/deep");
        fs::create_dir_all(repository.join(".git")).unwrap();
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(repository.join("sibling")).unwrap();
        fs::create_dir_all(cwd.join("descendant")).unwrap();
        fs::write(repository.join("AGENTS.md"), "root").unwrap();
        fs::write(repository.join("src/AGENTS.override.md"), "src").unwrap();
        fs::write(repository.join("sibling/AGENTS.md"), "sibling").unwrap();
        fs::write(cwd.join("descendant/AGENTS.md"), "descendant").unwrap();

        let mut kernel = kernel(cwd.clone());
        let response = invoke(
            &mut kernel,
            WorkspaceCommand::DiscoverProjectFiles {
                working_directory: cwd.to_string_lossy().into_owned(),
                root_markers: vec![".git".into()],
                file_names: vec!["AGENTS.override.md".into(), "AGENTS.md".into()],
            },
            &authority(&[WORKSPACE_READ]),
        )
        .unwrap();

        assert_eq!(
            response,
            WorkspaceResponse::ProjectFiles {
                root: repository.to_string_lossy().into_owned(),
                files: vec![
                    WorkspaceProjectFile {
                        path: "AGENTS.md".into(),
                        content: "root".into(),
                    },
                    WorkspaceProjectFile {
                        path: "src/AGENTS.override.md".into(),
                        content: "src".into(),
                    },
                ],
            }
        );

        let _ = fs::remove_dir_all(outer);
    }

    #[test]
    fn project_file_discovery_empty_markers_stays_at_cwd() {
        let outer = temp_workspace("project-discovery-cwd-only");
        let repository = outer.join("repo");
        let cwd = repository.join("src");
        fs::create_dir_all(repository.join(".git")).unwrap();
        fs::create_dir_all(&cwd).unwrap();
        fs::write(repository.join("AGENTS.md"), "root").unwrap();
        fs::write(cwd.join("AGENTS.md"), "cwd").unwrap();

        let mut kernel = kernel(cwd.clone());
        let response = invoke(
            &mut kernel,
            WorkspaceCommand::DiscoverProjectFiles {
                working_directory: cwd.to_string_lossy().into_owned(),
                root_markers: Vec::new(),
                file_names: vec!["AGENTS.override.md".into(), "AGENTS.md".into()],
            },
            &authority(&[WORKSPACE_READ]),
        )
        .unwrap();

        assert_eq!(
            response,
            WorkspaceResponse::ProjectFiles {
                root: cwd.to_string_lossy().into_owned(),
                files: vec![WorkspaceProjectFile {
                    path: "AGENTS.md".into(),
                    content: "cwd".into(),
                }],
            }
        );

        let _ = fs::remove_dir_all(outer);
    }

    #[test]
    fn project_file_discovery_skips_non_file_override() {
        let root = temp_workspace("project-discovery-non-file-override");
        fs::create_dir_all(root.join("AGENTS.override.md")).unwrap();
        fs::write(root.join("AGENTS.md"), "project rules").unwrap();

        let mut kernel = kernel(root.clone());
        let response = invoke(
            &mut kernel,
            WorkspaceCommand::DiscoverProjectFiles {
                working_directory: root.to_string_lossy().into_owned(),
                root_markers: Vec::new(),
                file_names: vec!["AGENTS.override.md".into(), "AGENTS.md".into()],
            },
            &authority(&[WORKSPACE_READ]),
        )
        .unwrap();

        assert_eq!(
            response,
            WorkspaceResponse::ProjectFiles {
                root: root.to_string_lossy().into_owned(),
                files: vec![WorkspaceProjectFile {
                    path: "AGENTS.md".into(),
                    content: "project rules".into(),
                }],
            }
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn project_file_discovery_uses_lossy_utf8() {
        let root = temp_workspace("project-discovery-lossy-utf8");
        fs::write(root.join("AGENTS.md"), b"project\xff rules").unwrap();

        let mut kernel = kernel(root.clone());
        let response = invoke(
            &mut kernel,
            WorkspaceCommand::DiscoverProjectFiles {
                working_directory: root.to_string_lossy().into_owned(),
                root_markers: Vec::new(),
                file_names: vec!["AGENTS.override.md".into(), "AGENTS.md".into()],
            },
            &authority(&[WORKSPACE_READ]),
        )
        .unwrap();

        assert_eq!(
            response,
            WorkspaceResponse::ProjectFiles {
                root: root.to_string_lossy().into_owned(),
                files: vec![WorkspaceProjectFile {
                    path: "AGENTS.md".into(),
                    content: "project\u{FFFD} rules".into(),
                }],
            }
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn project_file_discovery_rejects_path_fallback_names() {
        let root = temp_workspace("project-discovery-invalid-name");
        let mut kernel = kernel(root.clone());
        let error = invoke(
            &mut kernel,
            WorkspaceCommand::DiscoverProjectFiles {
                working_directory: root.to_string_lossy().into_owned(),
                root_markers: vec![".git".into()],
                file_names: vec!["../CLAUDE.md".into()],
            },
            &authority(&[WORKSPACE_READ]),
        )
        .unwrap_err();
        assert!(error.contains("single path components"));

        for invalid in [r"nested\\CLAUDE.md", "C:CLAUDE.md"] {
            let error = invoke(
                &mut kernel,
                WorkspaceCommand::DiscoverProjectFiles {
                    working_directory: root.to_string_lossy().into_owned(),
                    root_markers: vec![".git".into()],
                    file_names: vec![invalid.into()],
                },
                &authority(&[WORKSPACE_READ]),
            )
            .unwrap_err();
            assert!(error.contains("single path components"));
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn process_model_view_collapses_only_with_exact_recovery() {
        let large = vec![b'x'; MAX_PROCESS_MODEL_VIEW_BYTES + 11];
        let reference = phenix_core::ContentReference {
            digest: phenix_core::ArtifactRevision::from_content(&large),
            media_type: "application/octet-stream".into(),
            bytes: large.len(),
            locator: phenix_core::ContentLocator::File {
                path: "sha256/fixture".into(),
            },
        };
        let (view, complete) = process_model_view(
            large.clone(),
            &ProcessStreamRecovery::Reference { reference },
        );
        assert_eq!(view.len(), MAX_PROCESS_MODEL_VIEW_BYTES);
        assert!(!complete);

        let (view, complete) = process_model_view(large.clone(), &ProcessStreamRecovery::Inline);
        assert_eq!(view.len(), large.len());
        assert!(complete);

        let (view, complete) = process_model_view(
            large.clone(),
            &ProcessStreamRecovery::Unavailable {
                reason: "fixture".into(),
            },
        );
        assert_eq!(view.len(), large.len());
        assert!(complete);
    }

    #[test]
    fn workspace_root_is_an_environment_namespace_path() {
        let environment_root = temp_workspace("environment-root");
        let workspace_root = environment_root.join("provider-only-workspace");
        assert!(!workspace_root.exists());

        let workspace = workspace_manifest();
        let workspace_id = workspace.id.clone();
        let environment = local_environment_manifest();
        let environment_id = environment.id.clone();
        let resolved = ResolvedGeneration::resolve(
            [workspace.clone(), environment.clone()],
            [
                workspace_component_manifest(),
                local_environment_component_manifest(),
            ],
            [],
            &workspace.maximum_authority,
        )
        .unwrap();
        let mut kernel = Kernel::new(KernelConfig::new([workspace, environment]).unwrap());
        kernel.activate_resolved_generation(&resolved).unwrap();

        kernel
            .register_embedded_factory(workspace_id, move || {
                workspace_factory_for(workspace_root.clone())
            })
            .unwrap();
        let cleanup_root = environment_root.clone();
        kernel
            .register_embedded_factory(environment_id, move || {
                local_environment_factory_for(environment_root.clone())
            })
            .unwrap();

        kernel.activate_all().unwrap();
        let _ = fs::remove_dir_all(cleanup_root);
    }

    #[test]
    fn workspace_routes_io_and_processes_through_replaceable_environment() {
        let workspace = workspace_manifest();
        let workspace_id = workspace.id.clone();
        let environment = fixture_environment_manifest();
        let environment_id = environment.id.clone();
        let resolved = ResolvedGeneration::resolve(
            [workspace.clone(), environment.clone()],
            [
                workspace_component_manifest(),
                fixture_environment_component_manifest(),
            ],
            [],
            &workspace.maximum_authority,
        )
        .unwrap();
        let mut kernel = Kernel::new(KernelConfig::new([workspace, environment]).unwrap());
        kernel.activate_resolved_generation(&resolved).unwrap();

        let virtual_root = PathBuf::from("/phenix-fixture-environment-only/project");
        assert!(!virtual_root.exists());
        kernel
            .register_embedded_factory(workspace_id, move || {
                workspace_factory_for(virtual_root.clone())
            })
            .unwrap();

        let commands = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&commands);
        kernel
            .register_embedded_factory(environment_id, move || {
                Box::new(FixtureEnvironment {
                    commands: Arc::clone(&recorded),
                })
            })
            .unwrap();
        kernel.activate_all().unwrap();

        let read = invoke(
            &mut kernel,
            WorkspaceCommand::Read {
                path: "input.txt".into(),
            },
            &authority(&[WORKSPACE_READ]),
        )
        .unwrap();
        assert!(matches!(
            read,
            WorkspaceResponse::Read { content, .. } if content == "virtual-content"
        ));

        let recovery_reference = phenix_core::ContentReference::new(
            b"fixture-recovered",
            "application/x-phenix-fixture",
            phenix_core::ContentLocator::Service {
                service: "fixture.environment".into(),
                resource: "process-output".into(),
            },
        );
        assert!(matches!(
            invoke(
                &mut kernel,
                WorkspaceCommand::ReadContentReference {
                    reference: recovery_reference.clone(),
                },
                &authority(&[WORKSPACE_READ]),
            )
            .unwrap(),
            WorkspaceResponse::ReferencedContent { content }
                if content == b"fixture-recovered"
        ));

        assert!(matches!(
            invoke(
                &mut kernel,
                WorkspaceCommand::Write {
                    path: "new.txt".into(),
                    content: "new".into(),
                    expected_version: WorkspaceFileVersion::Absent,
                },
                &authority(&[WORKSPACE_WRITE]),
            )
            .unwrap(),
            WorkspaceResponse::Written { .. }
        ));
        assert!(matches!(
            invoke(
                &mut kernel,
                WorkspaceCommand::ReadBytes {
                    path: "input.txt".into(),
                },
                &authority(&[WORKSPACE_READ]),
            )
            .unwrap(),
            WorkspaceResponse::ReadBytes { content, .. }
                if content == b"virtual-content"
        ));
        let binary = vec![0, 0xff, b'P', b'H', b'X'];
        assert!(matches!(
            invoke(
                &mut kernel,
                WorkspaceCommand::WriteBytes {
                    path: "artifact.bin".into(),
                    content: binary.clone(),
                    expected_version: WorkspaceFileVersion::Absent,
                },
                &authority(&[WORKSPACE_WRITE]),
            )
            .unwrap(),
            WorkspaceResponse::Written { .. }
        ));
        assert!(matches!(
            invoke(
                &mut kernel,
                WorkspaceCommand::Exec {
                    program: "builder".into(),
                    arguments: vec!["--literal".into(), "a b;$(not-shell)".into()],
                    working_directory: Some("build/stage".into()),
                    environment: BTreeMap::from([("BUILD_MODE".into(), "release;literal".into())]),
                },
                &authority(&[WORKSPACE_SHELL]),
            )
            .unwrap(),
            WorkspaceResponse::Process { exit_code: 0, .. }
        ));

        let shell = invoke(
            &mut kernel,
            WorkspaceCommand::Shell {
                command: "printf shell".into(),
            },
            &authority(&[WORKSPACE_SHELL]),
        )
        .unwrap();
        assert!(matches!(
            shell,
            WorkspaceResponse::Process {
                exit_code: 0,
                ref stdout_content_identity,
                ref stderr_content_identity,
                ref stdout_recovery,
                ref stderr_recovery,
                ..
            } if stdout_content_identity.as_ref()
                    == Some(&phenix_core::ArtifactRevision::from_content(b"fixture-process"))
                && stderr_content_identity.as_ref()
                    == Some(&phenix_core::ArtifactRevision::from_content(b""))
                && stdout_recovery.as_ref() == &ProcessStreamRecovery::Inline
                && stderr_recovery.as_ref() == &ProcessStreamRecovery::Inline
        ));
        assert!(matches!(
            invoke(
                &mut kernel,
                WorkspaceCommand::Git {
                    arguments: vec!["status".into()],
                },
                &authority(&[WORKSPACE_GIT]),
            )
            .unwrap(),
            WorkspaceResponse::Process { exit_code: 0, .. }
        ));

        let commands = commands.lock().unwrap();
        assert!(commands.iter().any(|command| matches!(
            command,
            EnvironmentCommand::ReadFile { path }
                if path == "/phenix-fixture-environment-only/project/input.txt"
        )));
        assert!(commands.iter().any(|command| matches!(
            command,
            EnvironmentCommand::WriteFile { path, content, .. }
                if path == "/phenix-fixture-environment-only/project/new.txt"
                    && content == b"new"
        )));
        assert!(commands.iter().any(|command| matches!(
            command,
            EnvironmentCommand::ReadContentReference { reference }
                if reference == &recovery_reference
        )));
        assert!(commands.iter().any(|command| matches!(
            command,
            EnvironmentCommand::WriteFile { path, content, .. }
                if path == "/phenix-fixture-environment-only/project/artifact.bin"
                    && content == &binary
        )));
        assert!(commands.iter().any(|command| matches!(
            command,
            EnvironmentCommand::Exec {
                program,
                arguments,
                working_directory: Some(working_directory),
                environment,
            } if program == "builder"
                && arguments == &vec!["--literal".to_owned(), "a b;$(not-shell)".to_owned()]
                && working_directory == "/phenix-fixture-environment-only/project/build/stage"
                && environment.get("BUILD_MODE").map(String::as_str) == Some("release;literal")
        )));
        assert!(commands.iter().any(|command| matches!(
            command,
            EnvironmentCommand::Exec {
                program,
                working_directory: Some(working_directory),
                ..
            } if program == "bash"
                && working_directory == "/phenix-fixture-environment-only/project"
        )));
        assert!(commands.iter().any(|command| matches!(
            command,
            EnvironmentCommand::Exec {
                program,
                working_directory: Some(working_directory),
                ..
            } if program == "git"
                && working_directory == "/phenix-fixture-environment-only/project"
        )));
    }

    #[test]
    fn provider_without_atomic_replace_rejects_recoverable_commit_before_write() {
        let workspace = workspace_manifest();
        let workspace_id = workspace.id.clone();
        let environment = fixture_environment_manifest();
        let environment_id = environment.id.clone();
        let resolved = ResolvedGeneration::resolve(
            [workspace.clone(), environment.clone()],
            [
                workspace_component_manifest(),
                fixture_environment_component_manifest(),
            ],
            [],
            &workspace.maximum_authority,
        )
        .unwrap();
        let mut kernel = Kernel::new(KernelConfig::new([workspace, environment]).unwrap());
        kernel.activate_resolved_generation(&resolved).unwrap();

        let virtual_root = PathBuf::from("/phenix-fixture-environment-only/project");
        kernel
            .register_embedded_factory(workspace_id, move || {
                workspace_factory_for(virtual_root.clone())
            })
            .unwrap();
        let commands = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&commands);
        kernel
            .register_embedded_factory(environment_id, move || {
                Box::new(FixtureEnvironment {
                    commands: Arc::clone(&recorded),
                })
            })
            .unwrap();
        kernel.activate_all().unwrap();

        let capabilities = invoke(
            &mut kernel,
            WorkspaceCommand::Capabilities,
            &authority(&[WORKSPACE_WRITE]),
        )
        .unwrap();
        assert!(matches!(
            capabilities,
            WorkspaceResponse::Capabilities {
                capabilities: WorkspaceCapabilities {
                    recoverable_commit_atomicity: None,
                    ..
                }
            }
        ));

        let commit = authority(&[WORKSPACE_WRITE, PERSISTENCE_READ, PERSISTENCE_WRITE]);
        let response = invoke(
            &mut kernel,
            WorkspaceCommand::CommitBatch {
                operation_id: "operation-1".into(),
                writes: vec![WorkspaceWrite {
                    path: "new.txt".into(),
                    content: "new".into(),
                    expected_version: WorkspaceFileVersion::Absent,
                }],
            },
            &commit,
        )
        .unwrap();
        assert_eq!(
            response,
            WorkspaceResponse::UnsupportedAtomicScope {
                requested: WorkspaceWriteAtomicity::CrashRecoverable,
                available: WorkspaceWriteAtomicity::PreconditionCheckedSequential,
            }
        );

        let commands = commands.lock().unwrap();
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, EnvironmentCommand::Describe))
        );
        assert!(
            !commands
                .iter()
                .any(|command| matches!(command, EnvironmentCommand::WriteFile { .. }))
        );
    }

    #[test]
    fn search_rejects_environment_directory_entries_outside_workspace_root() {
        let workspace = workspace_manifest();
        let workspace_id = workspace.id.clone();
        let environment = fixture_environment_manifest();
        let environment_id = environment.id.clone();
        let resolved = ResolvedGeneration::resolve(
            [workspace.clone(), environment.clone()],
            [
                workspace_component_manifest(),
                fixture_environment_component_manifest(),
            ],
            [],
            &workspace.maximum_authority,
        )
        .unwrap();
        let mut kernel = Kernel::new(KernelConfig::new([workspace, environment]).unwrap());
        kernel.activate_resolved_generation(&resolved).unwrap();

        let root = PathBuf::from("/virtual/project");
        let outside = PathBuf::from("/virtual/outside/secret.txt");
        let workspace_root = root.clone();
        kernel
            .register_embedded_factory(workspace_id, move || {
                workspace_factory_for(workspace_root.clone())
            })
            .unwrap();

        let commands = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&commands);
        kernel
            .register_embedded_factory(environment_id, move || {
                Box::new(EscapingDirectoryEnvironment {
                    commands: Arc::clone(&recorded),
                    root: root.clone(),
                    outside: outside.clone(),
                })
            })
            .unwrap();
        kernel.activate_all().unwrap();

        let error = invoke(
            &mut kernel,
            WorkspaceCommand::Search {
                needle: "needle".into(),
                path: None,
                case_sensitive: true,
            },
            &authority(&[WORKSPACE_READ]),
        )
        .unwrap_err();
        assert!(
            error.contains("outside requested directory"),
            "unexpected search error: {error}"
        );

        let commands = commands.lock().unwrap();
        assert!(!commands.iter().any(|command| matches!(
            command,
            EnvironmentCommand::Stat { path } | EnvironmentCommand::ReadFile { path }
                if path == "/virtual/outside/secret.txt"
        )));
    }

    #[test]
    fn read_write_use_exact_versions_and_cannot_escape_workspace() {
        let root = temp_workspace("workspace-versions");
        fs::write(root.join("input.txt"), "one\n").unwrap();
        let mut kernel = kernel(root.clone());
        let read = authority(&[WORKSPACE_READ]);
        let write = authority(&[WORKSPACE_WRITE]);
        let version = match invoke(
            &mut kernel,
            WorkspaceCommand::Read {
                path: "input.txt".into(),
            },
            &read,
        )
        .unwrap()
        {
            WorkspaceResponse::Read { version, .. } => version,
            other => panic!("unexpected response: {other:?}"),
        };
        invoke(
            &mut kernel,
            WorkspaceCommand::Write {
                path: "input.txt".into(),
                content: "two\n".into(),
                expected_version: version.clone(),
            },
            &write,
        )
        .unwrap();
        let conflict = invoke(
            &mut kernel,
            WorkspaceCommand::Write {
                path: "input.txt".into(),
                content: "three\n".into(),
                expected_version: version,
            },
            &write,
        )
        .unwrap_err();
        assert!(conflict.contains("version conflict"));
        assert!(
            invoke(
                &mut kernel,
                WorkspaceCommand::Read {
                    path: "../outside".into(),
                },
                &read,
            )
            .unwrap_err()
            .contains("escapes")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn batch_write_rejects_the_whole_version_set_before_mutation() {
        let root = temp_workspace("workspace-batch-conflict");
        fs::write(root.join("a.txt"), "old-a").unwrap();
        fs::write(root.join("b.txt"), "old-b").unwrap();
        let mut kernel = kernel(root.clone());
        let read = authority(&[WORKSPACE_READ]);
        let write = authority(&[WORKSPACE_WRITE]);
        let a_version = match invoke(
            &mut kernel,
            WorkspaceCommand::Read {
                path: "a.txt".into(),
            },
            &read,
        )
        .unwrap()
        {
            WorkspaceResponse::Read { version, .. } => version,
            other => panic!("unexpected response: {other:?}"),
        };
        let stale_b = WorkspaceFileVersion::Present {
            content_hash: "stale".into(),
        };
        let response = invoke(
            &mut kernel,
            WorkspaceCommand::WriteBatch {
                writes: vec![
                    WorkspaceWrite {
                        path: "a.txt".into(),
                        content: "new-a".into(),
                        expected_version: a_version,
                    },
                    WorkspaceWrite {
                        path: "b.txt".into(),
                        content: "new-b".into(),
                        expected_version: stale_b,
                    },
                ],
            },
            &write,
        )
        .unwrap();
        assert!(matches!(
            response,
            WorkspaceResponse::VersionConflict { ref conflicts } if conflicts.len() == 1
        ));
        assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "old-a");
        assert_eq!(fs::read_to_string(root.join("b.txt")).unwrap(), "old-b");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn batch_write_is_idempotent_after_the_desired_version_exists() {
        let root = temp_workspace("workspace-batch-idempotent");
        fs::write(root.join("a.txt"), "old").unwrap();
        let mut kernel = kernel(root.clone());
        let read = authority(&[WORKSPACE_READ]);
        let write = authority(&[WORKSPACE_WRITE]);
        let version = match invoke(
            &mut kernel,
            WorkspaceCommand::Read {
                path: "a.txt".into(),
            },
            &read,
        )
        .unwrap()
        {
            WorkspaceResponse::Read { version, .. } => version,
            other => panic!("unexpected response: {other:?}"),
        };
        let command = WorkspaceCommand::WriteBatch {
            writes: vec![WorkspaceWrite {
                path: "a.txt".into(),
                content: "new".into(),
                expected_version: version,
            }],
        };
        assert!(matches!(
            invoke(&mut kernel, command.clone(), &write).unwrap(),
            WorkspaceResponse::WrittenBatch { .. }
        ));
        assert!(matches!(
            invoke(&mut kernel, command, &write).unwrap(),
            WorkspaceResponse::WrittenBatch { .. }
        ));
        assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "new");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recoverable_commit_retries_return_the_same_receipt() {
        let root = temp_workspace("workspace-commit-retry");
        fs::write(root.join("a.txt"), "old-a").unwrap();
        fs::write(root.join("b.txt"), "old-b").unwrap();
        let mut kernel = kernel(root.clone());
        let read = authority(&[WORKSPACE_READ]);
        let write = authority(&[WORKSPACE_WRITE, PERSISTENCE_READ, PERSISTENCE_WRITE]);

        let read_version = |kernel: &mut Kernel, path: &str| match invoke(
            kernel,
            WorkspaceCommand::Read { path: path.into() },
            &read,
        )
        .unwrap()
        {
            WorkspaceResponse::Read { version, .. } => version,
            other => panic!("unexpected response: {other:?}"),
        };
        let a_version = read_version(&mut kernel, "a.txt");
        let b_version = read_version(&mut kernel, "b.txt");

        let capabilities = invoke(&mut kernel, WorkspaceCommand::Capabilities, &write).unwrap();
        assert!(matches!(
            capabilities,
            WorkspaceResponse::Capabilities {
                capabilities: WorkspaceCapabilities {
                    recoverable_commit_atomicity: Some(WorkspaceWriteAtomicity::CrashRecoverable),
                    ..
                }
            }
        ));

        let command = WorkspaceCommand::CommitBatch {
            operation_id: "semantic-edit-1".into(),
            writes: vec![
                WorkspaceWrite {
                    path: "a.txt".into(),
                    content: "new-a".into(),
                    expected_version: a_version,
                },
                WorkspaceWrite {
                    path: "b.txt".into(),
                    content: "new-b".into(),
                    expected_version: b_version,
                },
            ],
        };
        let first = invoke(&mut kernel, command.clone(), &write).unwrap();
        let second = invoke(&mut kernel, command, &write).unwrap();
        assert_eq!(first, second);
        let WorkspaceResponse::CommittedBatch { receipt } = first else {
            panic!("expected committed batch");
        };
        assert_eq!(receipt.operation_id, "semantic-edit-1");
        assert_eq!(receipt.files.len(), 2);
        assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "new-a");
        assert_eq!(fs::read_to_string(root.join("b.txt")).unwrap(), "new-b");

        let conflicting_retry = invoke(
            &mut kernel,
            WorkspaceCommand::CommitBatch {
                operation_id: "semantic-edit-1".into(),
                writes: vec![WorkspaceWrite {
                    path: "a.txt".into(),
                    content: "different".into(),
                    expected_version: WorkspaceFileVersion::Absent,
                }],
            },
            &write,
        )
        .unwrap_err();
        assert!(conflicting_retry.contains("already bound to another intent"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn list_is_deterministic_recursive_and_excludes_git_metadata() {
        let root = temp_workspace("workspace-list");
        fs::create_dir_all(root.join("src/nested")).unwrap();
        fs::create_dir_all(root.join(".git/objects")).unwrap();
        fs::write(root.join("AGENTS.md"), "root rules\n").unwrap();
        fs::write(root.join("src/nested/SKILL.md"), "skill\n").unwrap();
        fs::write(root.join(".git/objects/ignored"), "ignored\n").unwrap();

        let mut kernel = kernel(root.clone());
        let response = invoke(
            &mut kernel,
            WorkspaceCommand::List {
                path: None,
                recursive: true,
            },
            &authority(&[WORKSPACE_READ]),
        )
        .unwrap();
        let WorkspaceResponse::List { entries } = response else {
            panic!("unexpected response: {response:?}");
        };
        let paths = entries
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec!["AGENTS.md", "src", "src/nested", "src/nested/SKILL.md"]
        );
        assert!(!paths.iter().any(|path| path.starts_with(".git")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn search_is_deterministic_and_read_authority_cannot_write() {
        let root = temp_workspace("workspace-search");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/b.rs"), "needle b\n").unwrap();
        fs::write(root.join("src/a.rs"), "needle a\n").unwrap();
        let mut kernel = kernel(root.clone());
        let read = authority(&[WORKSPACE_READ]);
        let response = invoke(
            &mut kernel,
            WorkspaceCommand::Search {
                needle: "needle".into(),
                path: Some("src".into()),
                case_sensitive: true,
            },
            &read,
        )
        .unwrap();
        match response {
            WorkspaceResponse::Search { matches } => {
                assert_eq!(matches.len(), 2);
                assert_eq!(matches[0].path, "src/a.rs");
                assert_eq!(matches[1].path, "src/b.rs");
            }
            other => panic!("unexpected response: {other:?}"),
        }
        let denied = invoke(
            &mut kernel,
            WorkspaceCommand::Write {
                path: "new.txt".into(),
                content: "no".into(),
                expected_version: WorkspaceFileVersion::Absent,
            },
            &read,
        )
        .unwrap_err();
        assert!(denied.contains("authority denied"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn git_runs_through_the_same_replaceable_workspace_service() {
        let root = temp_workspace("workspace-git");
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap();
        let mut kernel = kernel(root.clone());
        let response = invoke(
            &mut kernel,
            WorkspaceCommand::Git {
                arguments: vec!["status".into(), "--porcelain".into()],
            },
            &authority(&[WORKSPACE_GIT]),
        )
        .unwrap();
        assert!(matches!(
            response,
            WorkspaceResponse::Process { exit_code: 0, .. }
        ));
        let _ = fs::remove_dir_all(root);
    }
}
