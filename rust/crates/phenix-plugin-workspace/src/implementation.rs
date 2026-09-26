use phenix_core::{
    Authority, CapabilityId, ComponentInterface, PluginContext, PluginExecution, PluginHost,
    PluginId, PluginInstance, PluginManifest, SdkClient, ServiceContribution, ServiceId,
};
use phenix_sdk::{
    EnvironmentCommand, EnvironmentFileKind, EnvironmentInterface, EnvironmentResponse,
    WorkspaceCommand, WorkspaceFileVersion, WorkspaceInterface, WorkspaceResponse,
    WorkspaceSearchMatch, WorkspaceVersionConflict, WorkspaceWrite, WorkspaceWrittenFile,
    WORKSPACE_SERVICE,
};
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
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::new([
            capability(WORKSPACE_READ),
            capability(WORKSPACE_WRITE),
            capability(WORKSPACE_SHELL),
            capability(WORKSPACE_GIT),
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

fn capability(value: &str) -> CapabilityId {
    CapabilityId::parse(value).expect("static capability is valid")
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
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        // The workspace root is an Environment-namespace path. Validating it with
        // host filesystem APIs would make remote/container providers impossible.
        Ok(())
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
        WorkspaceCommand::Read { path } => read(context, path),
        WorkspaceCommand::Write {
            path,
            content,
            expected_version,
        } => write(context, path, content, expected_version),
        WorkspaceCommand::WriteBatch { writes } => write_batch(context, writes),
        WorkspaceCommand::Search {
            needle,
            path,
            case_sensitive,
        } => search(context, needle, path, case_sensitive),
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
    match environment(
        context,
        EnvironmentCommand::WriteFile {
            path: environment_path(resolved),
            content: content.as_bytes().to_vec(),
            create_parents: true,
        },
    )? {
        EnvironmentResponse::Written => Ok(()),
        other => Err(format!(
            "write {path}: environment returned unexpected response {other:?}"
        )),
    }
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
            ))
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
                search_path(
                    context,
                    workspace_root,
                    Path::new(&entry.path),
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

fn process(
    context: &WorkspaceContext<'_, '_, '_>,
    program: &str,
    args: &[String],
    capability: &str,
) -> Result<WorkspaceResponse, String> {
    require(context, capability)?;
    match environment(
        context,
        EnvironmentCommand::Exec {
            program: program.to_owned(),
            arguments: args.to_vec(),
            working_directory: Some(environment_path(context.plugin.state)),
            environment: BTreeMap::new(),
        },
    )? {
        EnvironmentResponse::Process {
            exit_code,
            stdout,
            stderr,
            ..
        } => Ok(WorkspaceResponse::Process {
            exit_code,
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        }),
        other => Err(format!(
            "environment returned unexpected process response: {other:?}"
        )),
    }
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
        Kernel, KernelConfig, PhenixValue, Project, ResolvedHarness, ResolvedHarnessActivation,
    };
    use phenix_plugin_environment_local::{
        local_environment_component_manifest, local_environment_factory_for,
        local_environment_manifest,
    };
    use std::{
        fs,
        process::Command,
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

    fn kernel(root: PathBuf) -> Kernel {
        let workspace = workspace_manifest();
        let workspace_id = workspace.id.clone();
        let environment = local_environment_manifest();
        let environment_id = environment.id.clone();
        let resolved = ResolvedHarness::resolve(
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
        kernel.activate_resolved_harness(&resolved).unwrap();

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
    fn workspace_root_is_an_environment_namespace_path() {
        let environment_root = temp_workspace("environment-root");
        let workspace_root = environment_root.join("provider-only-workspace");
        assert!(!workspace_root.exists());

        let workspace = workspace_manifest();
        let workspace_id = workspace.id.clone();
        let environment = local_environment_manifest();
        let environment_id = environment.id.clone();
        let resolved = ResolvedHarness::resolve(
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
        kernel.activate_resolved_harness(&resolved).unwrap();

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
        assert!(invoke(
            &mut kernel,
            WorkspaceCommand::Read {
                path: "../outside".into(),
            },
            &read,
        )
        .unwrap_err()
        .contains("escapes"));
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
