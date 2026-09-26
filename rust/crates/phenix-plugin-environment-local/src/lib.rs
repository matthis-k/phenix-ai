#![forbid(unsafe_code)]

use phenix_core::{
    Authority, ComponentExport, ComponentId, ComponentInterface, ComponentManifest, PluginContext,
    PluginExecution, PluginHost, PluginId, PluginInstance, PluginManifest, ServiceContribution,
    ServiceId,
};
use phenix_sdk::{
    environment_service, EnvironmentCommand, EnvironmentDescription, EnvironmentDirEntry,
    EnvironmentFileKind, EnvironmentInterface, EnvironmentResponse,
};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
};

pub const LOCAL_ENVIRONMENT_PLUGIN: &str = "phenix.environment.local";
const LOCAL_ENVIRONMENT_COMPONENT: &str = "phenix.environment.local";
const MAX_CAPTURE_BYTES: usize = 1024 * 1024;

#[must_use]
pub fn local_environment_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(LOCAL_ENVIRONMENT_PLUGIN).expect("static plugin id is valid"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: phenix_core::ServiceRole::Terminal,
            service: environment_service(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

#[must_use]
pub fn local_environment_component_id() -> ComponentId {
    ComponentId::parse(LOCAL_ENVIRONMENT_COMPONENT).expect("static component id is valid")
}

#[must_use]
pub fn local_environment_component_manifest() -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: local_environment_component_id(),
        owner: local_environment_manifest().id,
        imports: Vec::new(),
        exports: vec![ComponentExport {
            interface: EnvironmentInterface::interface_id(),
            schema: EnvironmentInterface::schema(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        maximum_authority: Authority::default(),
    }
}

#[must_use]
pub fn local_environment_factory() -> Box<dyn PluginInstance> {
    Box::new(LocalEnvironment::new(
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    ))
}

#[must_use]
pub fn local_environment_factory_for(root: impl Into<PathBuf>) -> Box<dyn PluginInstance> {
    Box::new(LocalEnvironment::new(root.into()))
}

#[derive(Default)]
struct CaptureBuffer {
    bytes: Vec<u8>,
    truncated: bool,
}

impl CaptureBuffer {
    fn push(&mut self, bytes: &[u8]) {
        let remaining = MAX_CAPTURE_BYTES.saturating_sub(self.bytes.len());
        self.bytes
            .extend_from_slice(&bytes[..bytes.len().min(remaining)]);
        if bytes.len() > remaining {
            self.truncated = true;
        }
    }

    fn take(&mut self) -> (Vec<u8>, bool) {
        let bytes = std::mem::take(&mut self.bytes);
        let truncated = std::mem::take(&mut self.truncated);
        (bytes, truncated)
    }
}

struct PersistentProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Arc<Mutex<CaptureBuffer>>,
    stderr: Arc<Mutex<CaptureBuffer>>,
    stdout_reader: Option<JoinHandle<()>>,
    stderr_reader: Option<JoinHandle<()>>,
}

impl PersistentProcess {
    fn finish_readers(&mut self) {
        if let Some(reader) = self.stdout_reader.take() {
            let _ = reader.join();
        }
        if let Some(reader) = self.stderr_reader.take() {
            let _ = reader.join();
        }
    }

    fn take_output(&self) -> Result<(Vec<u8>, Vec<u8>, bool), String> {
        let (stdout, stdout_truncated) = self
            .stdout
            .lock()
            .map_err(|_| "local environment stdout capture poisoned".to_owned())?
            .take();
        let (stderr, stderr_truncated) = self
            .stderr
            .lock()
            .map_err(|_| "local environment stderr capture poisoned".to_owned())?
            .take();
        Ok((stdout, stderr, stdout_truncated || stderr_truncated))
    }
}

struct LocalEnvironment {
    root: PathBuf,
    processes: BTreeMap<String, PersistentProcess>,
    next_process_id: u64,
}

impl LocalEnvironment {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            processes: BTreeMap::new(),
            next_process_id: 1,
        }
    }

    fn resolve(&self, path: &str) -> PathBuf {
        let path = Path::new(path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        }
    }

    fn command(
        &self,
        program: &str,
        arguments: &[String],
        working_directory: Option<&str>,
        environment: &BTreeMap<String, String>,
    ) -> Command {
        let mut command = Command::new(program);
        command.args(arguments);
        command.current_dir(
            working_directory
                .map(|path| self.resolve(path))
                .unwrap_or_else(|| self.root.clone()),
        );
        command.envs(environment);
        command
    }
}

impl PluginInstance for LocalEnvironment {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        if !self.root.is_dir() {
            return Err(format!(
                "local environment root is not a directory: {}",
                self.root.display()
            ));
        }
        Ok(())
    }

    fn stop(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        for (_, mut process) in std::mem::take(&mut self.processes) {
            let _ = process.child.kill();
            let _ = process.child.wait();
            process.finish_readers();
        }
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &environment_service() {
            return Err(format!("unsupported environment service: {service}"));
        }
        let context = PluginContext::new(host, (), (), ());
        let command = context
            .kernel
            .decode_projected::<EnvironmentCommand>(&EnvironmentInterface::interface_id(), input)
            .map_err(|error| error.to_string())?;
        let response = self.handle(command)?;
        context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string())
    }
}

impl LocalEnvironment {
    fn handle(&mut self, command: EnvironmentCommand) -> Result<EnvironmentResponse, String> {
        match command {
            EnvironmentCommand::Describe => Ok(EnvironmentResponse::Description {
                environment: EnvironmentDescription {
                    provider: LOCAL_ENVIRONMENT_PLUGIN.into(),
                    persistent_processes: true,
                    pty: false,
                },
            }),
            EnvironmentCommand::Stat { path } => {
                let resolved = self.resolve(&path);
                let kind = match fs::metadata(&resolved) {
                    Ok(metadata) if metadata.is_file() => Some(EnvironmentFileKind::File),
                    Ok(metadata) if metadata.is_dir() => Some(EnvironmentFileKind::Directory),
                    Ok(_) => Some(EnvironmentFileKind::Other),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => return Err(format!("stat {path}: {error}")),
                };
                Ok(EnvironmentResponse::Metadata { kind })
            }
            EnvironmentCommand::ReadFile { path } => {
                let resolved = self.resolve(&path);
                let content = match fs::read(&resolved) {
                    Ok(content) => Some(content),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => return Err(format!("read {path}: {error}")),
                };
                Ok(EnvironmentResponse::File { content })
            }
            EnvironmentCommand::WriteFile {
                path,
                content,
                create_parents,
            } => {
                let resolved = self.resolve(&path);
                if create_parents {
                    if let Some(parent) = resolved.parent() {
                        fs::create_dir_all(parent)
                            .map_err(|error| format!("create parent for {path}: {error}"))?;
                    }
                }
                fs::write(&resolved, content).map_err(|error| format!("write {path}: {error}"))?;
                Ok(EnvironmentResponse::Written)
            }
            EnvironmentCommand::ReadDir { path } => {
                let resolved = self.resolve(&path);
                let mut entries = fs::read_dir(&resolved)
                    .map_err(|error| format!("read directory {path}: {error}"))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| error.to_string())?;
                entries.sort_by_key(|entry| entry.file_name());
                let entries = entries
                    .into_iter()
                    .map(|entry| {
                        let metadata = entry.metadata().map_err(|error| error.to_string())?;
                        let kind = if metadata.is_file() {
                            EnvironmentFileKind::File
                        } else if metadata.is_dir() {
                            EnvironmentFileKind::Directory
                        } else {
                            EnvironmentFileKind::Other
                        };
                        Ok(EnvironmentDirEntry {
                            path: entry.path().to_string_lossy().into_owned(),
                            kind,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(EnvironmentResponse::Directory { entries })
            }
            EnvironmentCommand::Exec {
                program,
                arguments,
                working_directory,
                environment,
            } => {
                let output = self
                    .command(
                        &program,
                        &arguments,
                        working_directory.as_deref(),
                        &environment,
                    )
                    .output()
                    .map_err(|error| format!("spawn {program}: {error}"))?;
                let (stdout, stdout_truncated) = bounded(output.stdout);
                let (stderr, stderr_truncated) = bounded(output.stderr);
                Ok(EnvironmentResponse::Process {
                    exit_code: output.status.code().unwrap_or(-1),
                    stdout,
                    stderr,
                    truncated: stdout_truncated || stderr_truncated,
                })
            }
            EnvironmentCommand::OpenProcess {
                program,
                arguments,
                working_directory,
                environment,
            } => {
                let mut child = self
                    .command(
                        &program,
                        &arguments,
                        working_directory.as_deref(),
                        &environment,
                    )
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .map_err(|error| format!("spawn persistent {program}: {error}"))?;
                let stdin = child.stdin.take();
                let stdout = child
                    .stdout
                    .take()
                    .ok_or_else(|| "persistent process stdout was not piped".to_owned())?;
                let stderr = child
                    .stderr
                    .take()
                    .ok_or_else(|| "persistent process stderr was not piped".to_owned())?;
                let stdout_capture = Arc::new(Mutex::new(CaptureBuffer::default()));
                let stderr_capture = Arc::new(Mutex::new(CaptureBuffer::default()));
                let stdout_reader = spawn_reader(stdout, Arc::clone(&stdout_capture));
                let stderr_reader = spawn_reader(stderr, Arc::clone(&stderr_capture));
                let handle = format!("process-{}", self.next_process_id);
                self.next_process_id = self.next_process_id.saturating_add(1);
                self.processes.insert(
                    handle.clone(),
                    PersistentProcess {
                        child,
                        stdin,
                        stdout: stdout_capture,
                        stderr: stderr_capture,
                        stdout_reader: Some(stdout_reader),
                        stderr_reader: Some(stderr_reader),
                    },
                );
                Ok(EnvironmentResponse::ProcessOpened { handle })
            }
            EnvironmentCommand::WriteProcess { handle, input } => {
                let process = self
                    .processes
                    .get_mut(&handle)
                    .ok_or_else(|| format!("unknown environment process handle: {handle}"))?;
                let stdin = process
                    .stdin
                    .as_mut()
                    .ok_or_else(|| format!("environment process stdin is closed: {handle}"))?;
                stdin
                    .write_all(&input)
                    .and_then(|_| stdin.flush())
                    .map_err(|error| format!("write environment process {handle}: {error}"))?;
                Ok(EnvironmentResponse::Written)
            }
            EnvironmentCommand::PollProcess { handle } => {
                let process = self
                    .processes
                    .get_mut(&handle)
                    .ok_or_else(|| format!("unknown environment process handle: {handle}"))?;
                let exit_code = process
                    .child
                    .try_wait()
                    .map_err(|error| format!("poll environment process {handle}: {error}"))?
                    .map(|status| status.code().unwrap_or(-1));
                if exit_code.is_some() {
                    process.stdin.take();
                    process.finish_readers();
                }
                let (stdout, stderr, truncated) = process.take_output()?;
                Ok(EnvironmentResponse::ProcessOutput {
                    stdout,
                    stderr,
                    exit_code,
                    truncated,
                })
            }
            EnvironmentCommand::CloseProcess { handle } => {
                let mut process = self
                    .processes
                    .remove(&handle)
                    .ok_or_else(|| format!("unknown environment process handle: {handle}"))?;
                process.stdin.take();
                let status = match process.child.try_wait() {
                    Ok(Some(status)) => Some(status),
                    Ok(None) => {
                        let _ = process.child.kill();
                        process.child.wait().ok()
                    }
                    Err(error) => {
                        return Err(format!("close environment process {handle}: {error}"))
                    }
                };
                process.finish_readers();
                let (stdout, stderr, truncated) = process.take_output()?;
                Ok(EnvironmentResponse::ProcessClosed {
                    stdout,
                    stderr,
                    exit_code: status.map(|status| status.code().unwrap_or(-1)),
                    truncated,
                })
            }
        }
    }
}

fn spawn_reader(
    mut reader: impl Read + Send + 'static,
    capture: Arc<Mutex<CaptureBuffer>>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if let Ok(mut capture) = capture.lock() {
                        capture.push(&buffer[..read]);
                    } else {
                        break;
                    }
                }
            }
        }
    })
}

fn bounded(bytes: Vec<u8>) -> (Vec<u8>, bool) {
    if bytes.len() <= MAX_CAPTURE_BYTES {
        (bytes, false)
    } else {
        (bytes[..MAX_CAPTURE_BYTES].to_vec(), true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{Kernel, KernelConfig, PhenixValue, Project};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "phenix-local-environment-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn invoke(kernel: &mut Kernel, command: EnvironmentCommand) -> EnvironmentResponse {
        let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
        let output = kernel
            .invoke(&environment_service(), &input, &Authority::default(), None)
            .unwrap();
        let output: PhenixValue = serde_json::from_slice(&output).unwrap();
        EnvironmentResponse::try_from(Project(&output)).unwrap()
    }

    #[test]
    fn local_provider_owns_filesystem_and_process_execution() {
        let root = temp_root();
        let manifest = local_environment_manifest();
        let plugin = manifest.id.clone();
        let mut kernel = Kernel::new(KernelConfig::new([manifest]).unwrap());
        let provider_root = root.clone();
        kernel
            .register_embedded_factory(plugin, move || {
                local_environment_factory_for(provider_root.clone())
            })
            .unwrap();
        kernel.activate_all().unwrap();

        assert!(matches!(
            invoke(
                &mut kernel,
                EnvironmentCommand::WriteFile {
                    path: "value.txt".into(),
                    content: b"one".to_vec(),
                    create_parents: true,
                }
            ),
            EnvironmentResponse::Written
        ));
        assert!(matches!(
            invoke(
                &mut kernel,
                EnvironmentCommand::ReadFile {
                    path: "value.txt".into(),
                }
            ),
            EnvironmentResponse::File { content: Some(content) } if content == b"one"
        ));
        assert!(matches!(
            invoke(
                &mut kernel,
                EnvironmentCommand::Exec {
                    program: "sh".into(),
                    arguments: vec!["-c".into(), "printf two > value.txt".into()],
                    working_directory: None,
                    environment: BTreeMap::new(),
                }
            ),
            EnvironmentResponse::Process { exit_code: 0, .. }
        ));
        assert_eq!(fs::read(root.join("value.txt")).unwrap(), b"two");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_provider_supports_persistent_process_handles() {
        let root = temp_root();
        let manifest = local_environment_manifest();
        let plugin = manifest.id.clone();
        let mut kernel = Kernel::new(KernelConfig::new([manifest]).unwrap());
        kernel
            .register_embedded_factory(plugin, move || local_environment_factory_for(root.clone()))
            .unwrap();
        kernel.activate_all().unwrap();

        let handle = match invoke(
            &mut kernel,
            EnvironmentCommand::OpenProcess {
                program: "sh".into(),
                arguments: Vec::new(),
                working_directory: None,
                environment: BTreeMap::new(),
            },
        ) {
            EnvironmentResponse::ProcessOpened { handle } => handle,
            other => panic!("unexpected response: {other:?}"),
        };
        assert!(matches!(
            invoke(
                &mut kernel,
                EnvironmentCommand::WriteProcess {
                    handle: handle.clone(),
                    input: b"printf persistent\nexit\n".to_vec(),
                }
            ),
            EnvironmentResponse::Written
        ));
        let closed = invoke(&mut kernel, EnvironmentCommand::CloseProcess { handle });
        assert!(matches!(
            closed,
            EnvironmentResponse::ProcessClosed { stdout, .. }
                if String::from_utf8_lossy(&stdout).contains("persistent")
        ));
    }
}
