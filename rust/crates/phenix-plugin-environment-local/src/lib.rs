#![forbid(unsafe_code)]

use phenix_core::{
    ArtifactRevision, Authority, ComponentExport, ComponentId, ComponentInterface,
    ComponentManifest, ContentReferenceStore, FileContentReferenceStore, PluginContext,
    PluginExecution, PluginHost, PluginId, PluginInstance, PluginManifest, ServiceContribution,
    ServiceId,
};
use phenix_sdk::{
    environment_service, EnvironmentCommand, EnvironmentDescription, EnvironmentDirEntry,
    EnvironmentFileKind, EnvironmentFilesystemPolicy, EnvironmentInterface, EnvironmentResponse,
    ProcessStreamRecovery,
};
use rustix::{
    fs::{self as rfs, Dir, FileType, Mode, OFlags, ResolveFlags},
    io::Errno,
    process::{kill_process_group, Pid, Signal},
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    os::{
        fd::{AsFd, OwnedFd},
        unix::process::CommandExt,
    },
    path::{Component, Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
};

pub const LOCAL_ENVIRONMENT_PLUGIN: &str = "phenix.environment.local";
const LOCAL_ENVIRONMENT_COMPONENT: &str = "phenix.environment.local";
const MAX_CAPTURE_BYTES: usize = 1024 * 1024;
const MAX_EXACT_CAPTURE_BYTES: usize = 16 * 1024 * 1024;
const LOCAL_FILESYSTEM_POLICY_ENV: &str = "PHENIX_LOCAL_FILESYSTEM_POLICY";

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
    Box::new(LocalEnvironment::configured(
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    ))
}

#[must_use]
pub fn local_environment_factory_for(root: impl Into<PathBuf>) -> Box<dyn PluginInstance> {
    Box::new(LocalEnvironment::new(
        root.into(),
        EnvironmentFilesystemPolicy::Unrestricted,
    ))
}

#[must_use]
pub fn local_environment_factory_for_policy(
    root: impl Into<PathBuf>,
    filesystem_policy: EnvironmentFilesystemPolicy,
) -> Box<dyn PluginInstance> {
    Box::new(LocalEnvironment::new(root.into(), filesystem_policy))
}

struct CaptureBuffer {
    bytes: Vec<u8>,
    truncated: bool,
    total_bytes: u64,
    digest: Sha256,
    exact: Option<Vec<u8>>,
}

impl Default for CaptureBuffer {
    fn default() -> Self {
        Self {
            bytes: Vec::new(),
            truncated: false,
            total_bytes: 0,
            digest: Sha256::new(),
            exact: Some(Vec::new()),
        }
    }
}

impl CaptureBuffer {
    fn push(&mut self, bytes: &[u8]) {
        self.digest.update(bytes);
        self.total_bytes = self
            .total_bytes
            .saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        if let Some(exact) = &mut self.exact {
            if exact.len().saturating_add(bytes.len()) <= MAX_EXACT_CAPTURE_BYTES {
                exact.extend_from_slice(bytes);
            } else {
                self.exact = None;
            }
        }
        let remaining = MAX_CAPTURE_BYTES.saturating_sub(self.bytes.len());
        self.bytes
            .extend_from_slice(&bytes[..bytes.len().min(remaining)]);
        if bytes.len() > remaining {
            self.truncated = true;
        }
    }

    fn take(&mut self) -> CapturedStream {
        let view = std::mem::take(&mut self.bytes);
        let truncated = std::mem::take(&mut self.truncated);
        let total_bytes = std::mem::take(&mut self.total_bytes);
        let digest = std::mem::take(&mut self.digest).finalize();
        let content_identity = format!("sha256:{digest:x}")
            .parse()
            .expect("sha256 digest is a valid artifact revision");
        let exact = self.exact.take();
        self.exact = Some(Vec::new());
        CapturedStream {
            view,
            complete: !truncated,
            total_bytes,
            content_identity,
            exact,
        }
    }
}

struct CapturedStream {
    view: Vec<u8>,
    complete: bool,
    total_bytes: u64,
    content_identity: ArtifactRevision,
    exact: Option<Vec<u8>>,
}

struct CapturedProcessOutput {
    stdout: CapturedStream,
    stderr: CapturedStream,
}

struct PersistentProcess {
    child: Child,
    process_group: Option<Pid>,
    stdin: Option<ChildStdin>,
    stdout: Arc<Mutex<CaptureBuffer>>,
    stderr: Arc<Mutex<CaptureBuffer>>,
    stdout_reader: Option<JoinHandle<()>>,
    stderr_reader: Option<JoinHandle<()>>,
}

impl PersistentProcess {
    fn terminate_tree(&mut self) {
        if let Some(process_group) = self.process_group {
            let _ = kill_process_group(process_group, Signal::KILL);
        }
        let _ = self.child.kill();
    }

    fn finish_readers(&mut self) {
        if let Some(reader) = self.stdout_reader.take() {
            let _ = reader.join();
        }
        if let Some(reader) = self.stderr_reader.take() {
            let _ = reader.join();
        }
    }

    fn take_output(&self) -> Result<CapturedProcessOutput, String> {
        let stdout = self
            .stdout
            .lock()
            .map_err(|_| "local environment stdout capture poisoned".to_owned())?
            .take();
        let stderr = self
            .stderr
            .lock()
            .map_err(|_| "local environment stderr capture poisoned".to_owned())?
            .take();
        Ok(CapturedProcessOutput { stdout, stderr })
    }
}

struct LocalEnvironment {
    root: PathBuf,
    root_fd: Option<OwnedFd>,
    filesystem_policy: Result<EnvironmentFilesystemPolicy, String>,
    bubblewrap_program: PathBuf,
    processes: BTreeMap<String, PersistentProcess>,
    next_process_id: u64,
    process_output_store: FileContentReferenceStore,
}

impl LocalEnvironment {
    fn new(root: PathBuf, filesystem_policy: EnvironmentFilesystemPolicy) -> Self {
        let process_output_store =
            FileContentReferenceStore::new(root.join(".phenix/process-output"));
        Self {
            root,
            root_fd: None,
            filesystem_policy: Ok(filesystem_policy),
            bubblewrap_program: PathBuf::from("bwrap"),
            processes: BTreeMap::new(),
            next_process_id: 1,
            process_output_store,
        }
    }

    fn configured(root: PathBuf) -> Self {
        let filesystem_policy = match std::env::var(LOCAL_FILESYSTEM_POLICY_ENV) {
            Ok(value) => match value.as_str() {
                "unrestricted" | "local" => Ok(EnvironmentFilesystemPolicy::Unrestricted),
                "working-directory-only" | "working-dir" => {
                    Ok(EnvironmentFilesystemPolicy::WorkingDirectoryOnly)
                }
                "host-read-working-directory-write" | "workdir-write" => {
                    Ok(EnvironmentFilesystemPolicy::HostReadWorkingDirectoryWrite)
                }
                _ => Err(format!(
                    "{LOCAL_FILESYSTEM_POLICY_ENV} has unsupported value {value:?}"
                )),
            },
            Err(std::env::VarError::NotPresent) => Ok(EnvironmentFilesystemPolicy::Unrestricted),
            Err(std::env::VarError::NotUnicode(_)) => {
                Err(format!("{LOCAL_FILESYSTEM_POLICY_ENV} must be valid UTF-8"))
            }
        };
        let process_output_store =
            FileContentReferenceStore::new(root.join(".phenix/process-output"));
        Self {
            root,
            root_fd: None,
            filesystem_policy,
            bubblewrap_program: PathBuf::from("bwrap"),
            processes: BTreeMap::new(),
            next_process_id: 1,
            process_output_store,
        }
    }

    fn stream_recovery(&self, stream: &CapturedStream) -> ProcessStreamRecovery {
        let Some(exact) = stream.exact.as_deref() else {
            return ProcessStreamRecovery::Unavailable {
                reason: format!(
                    "process stream exceeded exact-capture quota of {MAX_EXACT_CAPTURE_BYTES} bytes"
                ),
            };
        };
        match self
            .process_output_store
            .put("application/octet-stream", exact)
        {
            Ok(reference)
                if reference.digest == stream.content_identity
                    && u64::try_from(reference.bytes).ok() == Some(stream.total_bytes) =>
            {
                ProcessStreamRecovery::Reference { reference }
            }
            Ok(_) => ProcessStreamRecovery::Unavailable {
                reason: "persisted process stream did not match its captured identity".into(),
            },
            Err(_error) if stream.complete => ProcessStreamRecovery::Inline,
            Err(error) => ProcessStreamRecovery::Unavailable {
                reason: format!("persist exact process stream: {error}"),
            },
        }
    }

    fn filesystem_policy(&self) -> Result<EnvironmentFilesystemPolicy, String> {
        self.filesystem_policy
            .as_ref()
            .copied()
            .map_err(Clone::clone)
    }

    fn resolve(&self, path: &str) -> PathBuf {
        let path = Path::new(path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        }
    }

    fn root_fd(&self) -> Result<&OwnedFd, String> {
        self.root_fd
            .as_ref()
            .ok_or_else(|| "local environment root descriptor is unavailable".to_owned())
    }

    fn restricted_relative(&self, resolved: &Path) -> Result<PathBuf, String> {
        let relative = resolved.strip_prefix(&self.root).map_err(|_| {
            format!(
                "environment filesystem policy denies path outside working directory: {}",
                resolved.display()
            )
        })?;
        for component in relative.components() {
            match component {
                Component::Normal(_) | Component::CurDir => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(format!(
                        "environment filesystem policy denies path escape: {}",
                        resolved.display()
                    ));
                }
            }
        }
        if relative.as_os_str().is_empty() {
            Ok(PathBuf::from("."))
        } else {
            Ok(relative.to_path_buf())
        }
    }

    fn confined_open(
        &self,
        resolved: &Path,
        flags: OFlags,
        mode: Mode,
    ) -> Result<Option<OwnedFd>, String> {
        let relative = self.restricted_relative(resolved)?;
        match rfs::openat2(
            self.root_fd()?.as_fd(),
            &relative,
            flags | OFlags::CLOEXEC,
            mode,
            ResolveFlags::BENEATH | ResolveFlags::NO_MAGICLINKS,
        ) {
            Ok(fd) => Ok(Some(fd)),
            Err(Errno::NOENT) => Ok(None),
            Err(Errno::NOSYS) => Err(
                "restricted local environment requires Linux openat2 for race-safe filesystem access"
                    .into(),
            ),
            Err(error) => Err(format!(
                "confined open {} failed: {error}",
                resolved.display()
            )),
        }
    }

    fn ensure_confined_parents(&self, relative: &Path, create: bool) -> Result<(), String> {
        let Some(parent) = relative.parent() else {
            return Ok(());
        };
        let mut accumulated = PathBuf::new();
        for component in parent.components() {
            let name = match component {
                Component::Normal(name) => name,
                Component::CurDir => continue,
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(format!(
                        "environment filesystem policy denies parent path escape: {}",
                        relative.display()
                    ));
                }
            };
            accumulated.push(name);

            match rfs::openat2(
                self.root_fd()?.as_fd(),
                &accumulated,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                Mode::empty(),
                ResolveFlags::BENEATH | ResolveFlags::NO_MAGICLINKS,
            ) {
                Ok(_) => {}
                Err(Errno::NOENT) if create => {
                    let parent_path = accumulated.parent().unwrap_or_else(|| Path::new("."));
                    let parent_fd = rfs::openat2(
                        self.root_fd()?.as_fd(),
                        parent_path,
                        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                        Mode::empty(),
                        ResolveFlags::BENEATH | ResolveFlags::NO_MAGICLINKS,
                    )
                    .map_err(|error| {
                        format!("open confined parent {}: {error}", parent_path.display())
                    })?;
                    rfs::mkdirat(
                        parent_fd.as_fd(),
                        Path::new(name),
                        Mode::from_raw_mode(0o755),
                    )
                    .map_err(|error| {
                        format!("create confined parent {}: {error}", accumulated.display())
                    })?;
                }
                Err(error) => {
                    return Err(format!(
                        "open confined parent {}: {error}",
                        accumulated.display()
                    ))
                }
            }
        }
        Ok(())
    }

    fn confined_write(
        &self,
        resolved: &Path,
        content: &[u8],
        create_parents: bool,
    ) -> Result<(), String> {
        let relative = self.restricted_relative(resolved)?;
        if relative == Path::new(".") {
            return Err(format!("write path is a directory: {}", resolved.display()));
        }
        self.ensure_confined_parents(&relative, create_parents)?;
        let fd = rfs::openat2(
            self.root_fd()?.as_fd(),
            &relative,
            OFlags::WRONLY | OFlags::CREATE | OFlags::TRUNC | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o666),
            ResolveFlags::BENEATH | ResolveFlags::NO_MAGICLINKS,
        )
        .map_err(|error| format!("confined write open {}: {error}", resolved.display()))?;
        let mut file = fs::File::from(fd);
        file.write_all(content)
            .map_err(|error| format!("confined write {}: {error}", resolved.display()))
    }

    fn requested_working_directory(
        &self,
        working_directory: Option<&str>,
    ) -> Result<PathBuf, String> {
        let mut cwd = working_directory
            .map(|path| self.resolve(path))
            .unwrap_or_else(|| self.root.clone());
        if self.filesystem_policy()? != EnvironmentFilesystemPolicy::Unrestricted {
            cwd = fs::canonicalize(&cwd)
                .map_err(|error| format!("canonicalize restricted working directory: {error}"))?;
        }
        if self.filesystem_policy()? != EnvironmentFilesystemPolicy::Unrestricted
            && !cwd.starts_with(&self.root)
        {
            return Err(format!(
                "restricted local environment working directory escapes root: {}",
                cwd.display()
            ));
        }
        Ok(cwd)
    }

    fn add_parent_dirs(command: &mut Command, path: &Path) {
        let mut parents = path.ancestors().collect::<Vec<_>>();
        parents.reverse();
        for parent in parents {
            if parent.as_os_str().is_empty() || parent == Path::new("/") || parent == path {
                continue;
            }
            command.arg("--dir").arg(parent);
        }
    }

    fn add_runtime_readonly_paths(command: &mut Command) {
        for path in ["/nix/store", "/usr", "/bin", "/lib", "/lib64"] {
            command.arg("--ro-bind-try").arg(path).arg(path);
        }
        command.arg("--dir").arg("/etc");
        for path in [
            "/etc/ld.so.cache",
            "/etc/ld.so.conf",
            "/etc/nsswitch.conf",
            "/etc/resolv.conf",
            "/etc/hosts",
            "/etc/ssl",
        ] {
            command.arg("--ro-bind-try").arg(path).arg(path);
        }
    }

    // All Environment-mediated process creation goes through this method.
    // Restricted policies compile to a Bubblewrap filesystem view before spawn.
    fn process_command(
        &self,
        program: &str,
        arguments: &[String],
        working_directory: Option<&str>,
        environment: &BTreeMap<String, String>,
    ) -> Result<Command, String> {
        let cwd = self.requested_working_directory(working_directory)?;
        if self.filesystem_policy()? == EnvironmentFilesystemPolicy::Unrestricted {
            let mut command = Command::new(program);
            command.args(arguments);
            command.current_dir(cwd);
            command.envs(environment);
            command.process_group(0);
            return Ok(command);
        }

        let mut command = Command::new(&self.bubblewrap_program);
        command
            .arg("--die-with-parent")
            .arg("--new-session")
            .arg("--unshare-user")
            .arg("--unshare-pid");

        let scratch = match self.filesystem_policy()? {
            EnvironmentFilesystemPolicy::Unrestricted => unreachable!(),
            EnvironmentFilesystemPolicy::HostReadWorkingDirectoryWrite => {
                command.arg("--ro-bind").arg("/").arg("/");
                // / is read-only in this view. Mount private scratch below the
                // private /dev mount so Bubblewrap can create the mountpoint
                // without hiding the host's read-only /tmp.
                "/dev/phenix-tmp"
            }
            EnvironmentFilesystemPolicy::WorkingDirectoryOnly => {
                Self::add_parent_dirs(&mut command, &self.root);
                Self::add_runtime_readonly_paths(&mut command);
                "/.phenix-tmp"
            }
        };

        command
            .arg("--proc")
            .arg("/proc")
            .arg("--dev")
            .arg("/dev")
            .arg("--tmpfs")
            .arg(scratch)
            .arg("--bind")
            .arg(&self.root)
            .arg(&self.root);
        command
            .arg("--setenv")
            .arg("TMPDIR")
            .arg(scratch)
            .arg("--chdir")
            .arg(&cwd);

        for (key, value) in environment {
            command.arg("--setenv").arg(key).arg(value);
        }

        command.arg("--").arg(program).args(arguments);
        Ok(command)
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
        self.root = fs::canonicalize(&self.root)
            .map_err(|error| format!("canonicalize local environment root: {error}"))?;
        self.root_fd = Some(
            rfs::open(
                &self.root,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|error| format!("open local environment root: {error}"))?,
        );
        if self.filesystem_policy()? != EnvironmentFilesystemPolicy::Unrestricted {
            rfs::openat2(
                self.root_fd()?.as_fd(),
                ".",
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                Mode::empty(),
                ResolveFlags::BENEATH | ResolveFlags::NO_MAGICLINKS,
            )
            .map_err(|error| {
                format!("restricted local environment requires usable Linux openat2: {error}")
            })?;
            let status = Command::new(&self.bubblewrap_program)
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|error| {
                    format!("restricted local environment requires bubblewrap: {error}")
                })?;
            if !status.success() {
                return Err("restricted local environment bubblewrap probe failed".into());
            }
        }
        self.filesystem_policy()?;
        Ok(())
    }

    fn stop(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        for (_, mut process) in std::mem::take(&mut self.processes) {
            process.stdin.take();
            process.terminate_tree();
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
                    filesystem_policy: self.filesystem_policy()?,
                    persistent_processes: true,
                    pty: false,
                },
            }),
            EnvironmentCommand::Stat { path } => {
                let resolved = self.resolve(&path);
                let kind = if self.filesystem_policy()?
                    == EnvironmentFilesystemPolicy::WorkingDirectoryOnly
                {
                    match self.confined_open(&resolved, OFlags::PATH, Mode::empty())? {
                        Some(fd) => {
                            let stat =
                                rfs::fstat(&fd).map_err(|error| format!("stat {path}: {error}"))?;
                            Some(match FileType::from_raw_mode(stat.st_mode) {
                                FileType::RegularFile => EnvironmentFileKind::File,
                                FileType::Directory => EnvironmentFileKind::Directory,
                                _ => EnvironmentFileKind::Other,
                            })
                        }
                        None => None,
                    }
                } else {
                    match fs::metadata(&resolved) {
                        Ok(metadata) if metadata.is_file() => Some(EnvironmentFileKind::File),
                        Ok(metadata) if metadata.is_dir() => Some(EnvironmentFileKind::Directory),
                        Ok(_) => Some(EnvironmentFileKind::Other),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                        Err(error) => return Err(format!("stat {path}: {error}")),
                    }
                };
                Ok(EnvironmentResponse::Metadata { kind })
            }
            EnvironmentCommand::ReadFile { path } => {
                let resolved = self.resolve(&path);
                let content = if self.filesystem_policy()?
                    == EnvironmentFilesystemPolicy::WorkingDirectoryOnly
                {
                    match self.confined_open(&resolved, OFlags::RDONLY, Mode::empty())? {
                        Some(fd) => {
                            let mut file = fs::File::from(fd);
                            let mut content = Vec::new();
                            file.read_to_end(&mut content)
                                .map_err(|error| format!("read {path}: {error}"))?;
                            Some(content)
                        }
                        None => None,
                    }
                } else {
                    match fs::read(&resolved) {
                        Ok(content) => Some(content),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                        Err(error) => return Err(format!("read {path}: {error}")),
                    }
                };
                Ok(EnvironmentResponse::File { content })
            }
            EnvironmentCommand::WriteFile {
                path,
                content,
                create_parents,
            } => {
                let resolved = self.resolve(&path);
                if self.filesystem_policy()? == EnvironmentFilesystemPolicy::Unrestricted {
                    if create_parents {
                        if let Some(parent) = resolved.parent() {
                            fs::create_dir_all(parent)
                                .map_err(|error| format!("create parent for {path}: {error}"))?;
                        }
                    }
                    fs::write(&resolved, content)
                        .map_err(|error| format!("write {path}: {error}"))?;
                } else {
                    self.confined_write(&resolved, &content, create_parents)?;
                }
                Ok(EnvironmentResponse::Written)
            }
            EnvironmentCommand::ReadDir { path } => {
                let resolved = self.resolve(&path);
                let entries = if self.filesystem_policy()?
                    == EnvironmentFilesystemPolicy::WorkingDirectoryOnly
                {
                    let fd = self
                        .confined_open(
                            &resolved,
                            OFlags::RDONLY | OFlags::DIRECTORY,
                            Mode::empty(),
                        )?
                        .ok_or_else(|| format!("read directory {path}: not found"))?;
                    let mut entries = Vec::new();
                    let dir =
                        Dir::new(fd).map_err(|error| format!("read directory {path}: {error}"))?;
                    for entry in dir {
                        let entry =
                            entry.map_err(|error| format!("read directory {path}: {error}"))?;
                        let name = entry.file_name().to_bytes();
                        if name == b"." || name == b".." {
                            continue;
                        }
                        let kind = match entry.file_type() {
                            FileType::RegularFile => EnvironmentFileKind::File,
                            FileType::Directory => EnvironmentFileKind::Directory,
                            _ => EnvironmentFileKind::Other,
                        };
                        entries.push(EnvironmentDirEntry {
                            path: resolved
                                .join(String::from_utf8_lossy(name).as_ref())
                                .to_string_lossy()
                                .into_owned(),
                            kind,
                        });
                    }
                    entries.sort_by(|left, right| left.path.cmp(&right.path));
                    entries
                } else {
                    let mut entries = fs::read_dir(&resolved)
                        .map_err(|error| format!("read directory {path}: {error}"))?
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|error| error.to_string())?;
                    entries.sort_by_key(|entry| entry.file_name());
                    entries
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
                        .collect::<Result<Vec<_>, String>>()?
                };
                Ok(EnvironmentResponse::Directory { entries })
            }
            EnvironmentCommand::Exec {
                program,
                arguments,
                working_directory,
                environment,
            } => {
                let owns_process_group =
                    self.filesystem_policy()? == EnvironmentFilesystemPolicy::Unrestricted;
                let mut child = self
                    .process_command(
                        &program,
                        &arguments,
                        working_directory.as_deref(),
                        &environment,
                    )?
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .map_err(|error| format!("spawn {program}: {error}"))?;
                let process_group = owns_process_group.then(|| {
                    Pid::from_raw(child.id() as i32)
                        .expect("spawned process id is a non-zero process-group id")
                });
                let stdout = child
                    .stdout
                    .take()
                    .ok_or_else(|| format!("spawn {program}: stdout was not piped"))?;
                let stderr = child
                    .stderr
                    .take()
                    .ok_or_else(|| format!("spawn {program}: stderr was not piped"))?;
                let stdout_capture = Arc::new(Mutex::new(CaptureBuffer::default()));
                let stderr_capture = Arc::new(Mutex::new(CaptureBuffer::default()));
                let stdout_reader = spawn_reader(stdout, Arc::clone(&stdout_capture));
                let stderr_reader = spawn_reader(stderr, Arc::clone(&stderr_capture));
                let mut process = PersistentProcess {
                    child,
                    process_group,
                    stdin: None,
                    stdout: stdout_capture,
                    stderr: stderr_capture,
                    stdout_reader: Some(stdout_reader),
                    stderr_reader: Some(stderr_reader),
                };
                let status = process
                    .child
                    .wait()
                    .map_err(|error| format!("wait {program}: {error}"))?;
                process.terminate_tree();
                process.finish_readers();
                let output = process.take_output()?;
                let stdout_recovery = self.stream_recovery(&output.stdout);
                let stderr_recovery = self.stream_recovery(&output.stderr);
                Ok(EnvironmentResponse::Process {
                    exit_code: status.code().unwrap_or(-1),
                    stdout: output.stdout.view,
                    stderr: output.stderr.view,
                    truncated: !output.stdout.complete || !output.stderr.complete,
                    stdout_complete: output.stdout.complete,
                    stderr_complete: output.stderr.complete,
                    stdout_bytes: Some(output.stdout.total_bytes),
                    stderr_bytes: Some(output.stderr.total_bytes),
                    stdout_content_identity: Some(output.stdout.content_identity),
                    stderr_content_identity: Some(output.stderr.content_identity),
                    stdout_recovery,
                    stderr_recovery,
                })
            }
            EnvironmentCommand::OpenProcess {
                program,
                arguments,
                working_directory,
                environment,
            } => {
                let owns_process_group =
                    self.filesystem_policy()? == EnvironmentFilesystemPolicy::Unrestricted;
                let mut child = self
                    .process_command(
                        &program,
                        &arguments,
                        working_directory.as_deref(),
                        &environment,
                    )?
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .map_err(|error| format!("spawn persistent {program}: {error}"))?;
                let process_group = owns_process_group.then(|| {
                    Pid::from_raw(child.id() as i32)
                        .expect("spawned process id is a non-zero process-group id")
                });
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
                        process_group,
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
                    process.terminate_tree();
                    process.finish_readers();
                }
                let output = process.take_output()?;
                let stdout_recovery = self.stream_recovery(&output.stdout);
                let stderr_recovery = self.stream_recovery(&output.stderr);
                Ok(EnvironmentResponse::ProcessOutput {
                    stdout: output.stdout.view,
                    stderr: output.stderr.view,
                    exit_code,
                    truncated: !output.stdout.complete || !output.stderr.complete,
                    stdout_complete: output.stdout.complete,
                    stderr_complete: output.stderr.complete,
                    stdout_bytes: Some(output.stdout.total_bytes),
                    stderr_bytes: Some(output.stderr.total_bytes),
                    stdout_content_identity: Some(output.stdout.content_identity),
                    stderr_content_identity: Some(output.stderr.content_identity),
                    stdout_recovery,
                    stderr_recovery,
                })
            }
            EnvironmentCommand::CloseProcess { handle } => {
                let mut process = self
                    .processes
                    .remove(&handle)
                    .ok_or_else(|| format!("unknown environment process handle: {handle}"))?;
                process.stdin.take();
                process.terminate_tree();
                let status = process
                    .child
                    .wait()
                    .map_err(|error| format!("close environment process {handle}: {error}"))?;
                process.finish_readers();
                let output = process.take_output()?;
                let stdout_recovery = self.stream_recovery(&output.stdout);
                let stderr_recovery = self.stream_recovery(&output.stderr);
                Ok(EnvironmentResponse::ProcessClosed {
                    stdout: output.stdout.view,
                    stderr: output.stderr.view,
                    exit_code: Some(status.code().unwrap_or(-1)),
                    truncated: !output.stdout.complete || !output.stderr.complete,
                    stdout_complete: output.stdout.complete,
                    stderr_complete: output.stderr.complete,
                    stdout_bytes: Some(output.stdout.total_bytes),
                    stderr_bytes: Some(output.stderr.total_bytes),
                    stdout_content_identity: Some(output.stdout.content_identity),
                    stderr_content_identity: Some(output.stderr.content_identity),
                    stdout_recovery,
                    stderr_recovery,
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

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{Kernel, KernelConfig, PhenixValue, Project};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

    fn invoke_result(
        kernel: &mut Kernel,
        command: EnvironmentCommand,
    ) -> Result<EnvironmentResponse, String> {
        let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
        let output = kernel
            .invoke(&environment_service(), &input, &Authority::default(), None)
            .map_err(|error| error.to_string())?;
        let output: PhenixValue = serde_json::from_slice(&output).unwrap();
        EnvironmentResponse::try_from(Project(&output)).map_err(|error| error.to_string())
    }

    fn invoke(kernel: &mut Kernel, command: EnvironmentCommand) -> EnvironmentResponse {
        invoke_result(kernel, command).unwrap()
    }

    #[test]
    fn capture_buffer_reports_full_stream_size_when_view_is_truncated() {
        let mut capture = CaptureBuffer::default();
        let full = vec![b'x'; MAX_CAPTURE_BYTES + 17];
        capture.push(&full);
        let stream = capture.take();
        assert_eq!(stream.view.len(), MAX_CAPTURE_BYTES);
        assert!(!stream.complete);
        assert_eq!(stream.total_bytes, (MAX_CAPTURE_BYTES + 17) as u64);
        assert_eq!(
            stream.content_identity,
            ArtifactRevision::from_content(&full),
            "stream identity must cover bytes discarded from the bounded view"
        );
        assert_eq!(stream.exact.as_deref(), Some(full.as_slice()));

        capture.push(b"small");
        let stream = capture.take();
        assert_eq!(stream.view, b"small");
        assert!(stream.complete);
        assert_eq!(stream.total_bytes, 5);
        assert_eq!(
            stream.content_identity,
            ArtifactRevision::from_content(b"small")
        );
        assert_eq!(stream.exact.as_deref(), Some(b"small".as_slice()));
    }

    #[test]
    fn exact_capture_quota_fails_recovery_explicitly() {
        let root = temp_root();
        let environment =
            LocalEnvironment::new(root.clone(), EnvironmentFilesystemPolicy::Unrestricted);
        let mut capture = CaptureBuffer::default();
        capture.push(&vec![b'x'; MAX_EXACT_CAPTURE_BYTES + 1]);
        let stream = capture.take();
        assert!(matches!(
            environment.stream_recovery(&stream),
            ProcessStreamRecovery::Unavailable { reason }
                if reason.contains("exact-capture quota")
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn truncated_stream_is_persisted_as_an_exact_reference() {
        let root = temp_root();
        let environment =
            LocalEnvironment::new(root.clone(), EnvironmentFilesystemPolicy::Unrestricted);
        let full = vec![b'x'; MAX_CAPTURE_BYTES + 17];
        let mut capture = CaptureBuffer::default();
        capture.push(&full);
        let stream = capture.take();
        let ProcessStreamRecovery::Reference { reference } = environment.stream_recovery(&stream)
        else {
            panic!("bounded process view must retain an exact reference");
        };
        assert_eq!(reference.digest, ArtifactRevision::from_content(&full));
        assert_eq!(reference.bytes, full.len());
        let recovered = environment
            .process_output_store
            .get(&reference)
            .unwrap()
            .expect("persisted stream remains recoverable");
        assert_eq!(recovered, full);
        let _ = fs::remove_dir_all(root);
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
    fn local_provider_root_is_default_cwd_not_confinement() {
        let root = temp_root();
        let outside = temp_root();
        let outside_read = outside.join("read.txt");
        let outside_direct_write = outside.join("direct-write.txt");
        let outside_process_write = outside.join("process-write.txt");
        fs::write(&outside_read, b"outside").unwrap();

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
                EnvironmentCommand::ReadFile {
                    path: outside_read.to_string_lossy().into_owned(),
                }
            ),
            EnvironmentResponse::File { content: Some(content) } if content == b"outside"
        ));

        assert!(matches!(
            invoke(
                &mut kernel,
                EnvironmentCommand::WriteFile {
                    path: outside_direct_write.to_string_lossy().into_owned(),
                    content: b"direct".to_vec(),
                    create_parents: false,
                }
            ),
            EnvironmentResponse::Written
        ));
        assert_eq!(fs::read(&outside_direct_write).unwrap(), b"direct");

        assert!(matches!(
            invoke(
                &mut kernel,
                EnvironmentCommand::Exec {
                    program: "sh".into(),
                    arguments: vec![
                        "-c".into(),
                        "printf process > \"$1\"".into(),
                        "sh".into(),
                        outside_process_write.to_string_lossy().into_owned(),
                    ],
                    working_directory: None,
                    environment: BTreeMap::new(),
                }
            ),
            EnvironmentResponse::Process { exit_code: 0, .. }
        ));
        assert_eq!(fs::read(&outside_process_write).unwrap(), b"process");

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    fn restricted_kernel(root: &Path, policy: EnvironmentFilesystemPolicy) -> Kernel {
        let manifest = local_environment_manifest();
        let plugin = manifest.id.clone();
        let mut kernel = Kernel::new(KernelConfig::new([manifest]).unwrap());
        let provider_root = root.to_path_buf();
        kernel
            .register_embedded_factory(plugin, move || {
                local_environment_factory_for_policy(provider_root.clone(), policy)
            })
            .unwrap();
        kernel.activate_all().unwrap();
        kernel
    }

    #[cfg(target_os = "linux")]
    fn wait_for_process_exit(pid: u32) {
        let proc_path = PathBuf::from(format!("/proc/{pid}"));
        for _ in 0..100 {
            if !proc_path.exists() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("descendant process {pid} survived Environment cleanup");
    }

    #[cfg(target_os = "linux")]
    fn open_background_child(kernel: &mut Kernel) -> (String, u32) {
        let handle = match invoke(
            kernel,
            EnvironmentCommand::OpenProcess {
                program: "sh".into(),
                arguments: vec![
                    "-c".into(),
                    r#"sleep 60 >/dev/null 2>&1 & child=$!; printf '%s\n' "$child"; wait"#.into(),
                ],
                working_directory: None,
                environment: BTreeMap::new(),
            },
        ) {
            EnvironmentResponse::ProcessOpened { handle } => handle,
            other => panic!("unexpected response: {other:?}"),
        };

        let mut output = Vec::new();
        for _ in 0..100 {
            match invoke(
                kernel,
                EnvironmentCommand::PollProcess {
                    handle: handle.clone(),
                },
            ) {
                EnvironmentResponse::ProcessOutput { stdout, .. } => {
                    output.extend(stdout);
                    if let Some(pid) = String::from_utf8_lossy(&output)
                        .lines()
                        .find_map(|line| line.trim().parse::<u32>().ok())
                    {
                        return (handle, pid);
                    }
                }
                other => panic!("unexpected response: {other:?}"),
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!(
            "persistent process did not report its descendant pid; stdout={:?}",
            String::from_utf8_lossy(&output)
        );
    }

    #[test]
    fn restricted_provider_fails_closed_when_bubblewrap_is_missing() {
        let root = temp_root();
        let manifest = local_environment_manifest();
        let plugin = manifest.id.clone();
        let mut kernel = Kernel::new(KernelConfig::new([manifest]).unwrap());
        let provider_root = root.clone();
        kernel
            .register_embedded_factory(plugin, move || {
                let mut environment = LocalEnvironment::new(
                    provider_root.clone(),
                    EnvironmentFilesystemPolicy::WorkingDirectoryOnly,
                );
                environment.bubblewrap_program = provider_root.join("missing-bubblewrap");
                Box::new(environment)
            })
            .unwrap();

        let error = kernel.activate_all().unwrap_err();
        assert!(
            error.to_string().contains("requires bubblewrap"),
            "unexpected activation error: {error}"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn restricted_policy_is_reported_and_child_environment_cannot_widen_it() {
        let root = temp_root();
        let outside = temp_root();
        let outside_write = outside.join("write.txt");
        let mut kernel = restricted_kernel(
            &root,
            EnvironmentFilesystemPolicy::HostReadWorkingDirectoryWrite,
        );

        assert!(matches!(
            invoke(&mut kernel, EnvironmentCommand::Describe),
            EnvironmentResponse::Description { environment }
                if environment.filesystem_policy
                    == EnvironmentFilesystemPolicy::HostReadWorkingDirectoryWrite
        ));

        let process = invoke(
            &mut kernel,
            EnvironmentCommand::Exec {
                program: "sh".into(),
                arguments: vec![
                    "-c".into(),
                    r#"printf nope > "$1""#.into(),
                    "sh".into(),
                    outside_write.to_string_lossy().into_owned(),
                ],
                working_directory: None,
                environment: BTreeMap::from([(
                    LOCAL_FILESYSTEM_POLICY_ENV.to_owned(),
                    "unrestricted".to_owned(),
                )]),
            },
        );
        assert!(matches!(
            process,
            EnvironmentResponse::Process { exit_code, .. } if exit_code != 0
        ));
        assert!(!outside_write.exists());

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn one_shot_exec_terminates_background_descendants() {
        let root = temp_root();
        let mut kernel = restricted_kernel(&root, EnvironmentFilesystemPolicy::Unrestricted);

        let process = invoke(
            &mut kernel,
            EnvironmentCommand::Exec {
                program: "sh".into(),
                arguments: vec![
                    "-c".into(),
                    r#"sleep 60 >/dev/null 2>&1 & printf '%s\n' "$!""#.into(),
                ],
                working_directory: None,
                environment: BTreeMap::new(),
            },
        );
        let pid = match process {
            EnvironmentResponse::Process {
                exit_code: 0,
                stdout,
                ..
            } => String::from_utf8(stdout)
                .unwrap()
                .trim()
                .parse::<u32>()
                .unwrap(),
            other => panic!("unexpected response: {other:?}"),
        };
        wait_for_process_exit(pid);

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn closing_persistent_process_terminates_descendants() {
        let root = temp_root();
        let mut kernel = restricted_kernel(&root, EnvironmentFilesystemPolicy::Unrestricted);
        let (handle, pid) = open_background_child(&mut kernel);

        assert!(matches!(
            invoke(&mut kernel, EnvironmentCommand::CloseProcess { handle }),
            EnvironmentResponse::ProcessClosed { .. }
        ));
        wait_for_process_exit(pid);

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn provider_stop_terminates_persistent_descendants() {
        let root = temp_root();
        let plugin = local_environment_manifest().id;
        let mut kernel = restricted_kernel(&root, EnvironmentFilesystemPolicy::Unrestricted);
        let (_handle, pid) = open_background_child(&mut kernel);

        kernel.stop(&plugin).unwrap();
        wait_for_process_exit(pid);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn working_directory_only_runtime_dependencies_are_read_only_mounts() {
        let root = temp_root();
        let mut kernel =
            restricted_kernel(&root, EnvironmentFilesystemPolicy::WorkingDirectoryOnly);

        let process = invoke(
            &mut kernel,
            EnvironmentCommand::Exec {
                program: "sh".into(),
                arguments: vec![
                    "-c".into(),
                    r#"found=
while IFS=' ' read -r _ _ _ _ mount_point options _; do
    if [ "$mount_point" = /nix/store ]; then
        case ",$options," in
            *,ro,*) found=1 ;;
        esac
    fi
done < /proc/self/mountinfo
test -n "$found""#
                        .into(),
                ],
                working_directory: None,
                environment: BTreeMap::new(),
            },
        );
        assert!(matches!(
            process,
            EnvironmentResponse::Process { exit_code: 0, .. }
        ));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn host_read_working_directory_write_confines_process_tree() {
        let root = temp_root();
        let outside = temp_root();
        let outside_read = outside.join("read.txt");
        let outside_write = outside.join("write.txt");
        fs::write(&outside_read, b"outside").unwrap();

        let mut kernel = restricted_kernel(
            &root,
            EnvironmentFilesystemPolicy::HostReadWorkingDirectoryWrite,
        );

        assert!(matches!(
            invoke(
                &mut kernel,
                EnvironmentCommand::ReadFile {
                    path: outside_read.to_string_lossy().into_owned(),
                }
            ),
            EnvironmentResponse::File { content: Some(content) } if content == b"outside"
        ));
        assert!(invoke_result(
            &mut kernel,
            EnvironmentCommand::WriteFile {
                path: outside_write.to_string_lossy().into_owned(),
                content: b"nope".to_vec(),
                create_parents: false,
            },
        )
        .unwrap_err()
        .contains("denies path outside working directory"));

        let process = invoke(
            &mut kernel,
            EnvironmentCommand::Exec {
                program: "sh".into(),
                arguments: vec![
                    "-c".into(),
                    r#"cat "$1" >/dev/null && printf ok > inside.txt && (printf nope > "$2")"#
                        .into(),
                    "sh".into(),
                    outside_read.to_string_lossy().into_owned(),
                    outside_write.to_string_lossy().into_owned(),
                ],
                working_directory: None,
                environment: BTreeMap::new(),
            },
        );
        assert!(matches!(
            process,
            EnvironmentResponse::Process { exit_code, .. } if exit_code != 0
        ));
        assert_eq!(fs::read(root.join("inside.txt")).unwrap(), b"ok");
        assert!(!outside_write.exists());

        let scratch = invoke(
            &mut kernel,
            EnvironmentCommand::Exec {
                program: "sh".into(),
                arguments: vec![
                    "-c".into(),
                    r#"printf scratch > "$TMPDIR/value" && cat "$TMPDIR/value""#.into(),
                ],
                working_directory: None,
                environment: BTreeMap::new(),
            },
        );
        assert!(matches!(
            scratch,
            EnvironmentResponse::Process { exit_code: 0, stdout, .. }
                if stdout == b"scratch"
        ));

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn working_directory_only_hides_unrelated_host_data_and_confines_children() {
        let root = temp_root();
        let outside = temp_root();
        let outside_read = outside.join("read.txt");
        fs::write(&outside_read, b"outside").unwrap();

        let mut kernel =
            restricted_kernel(&root, EnvironmentFilesystemPolicy::WorkingDirectoryOnly);

        assert!(invoke_result(
            &mut kernel,
            EnvironmentCommand::ReadFile {
                path: outside_read.to_string_lossy().into_owned(),
            },
        )
        .unwrap_err()
        .contains("denies path outside working directory"));

        let process = invoke(
            &mut kernel,
            EnvironmentCommand::Exec {
                program: "sh".into(),
                arguments: vec![
                    "-c".into(),
                    r#"printf ok > inside.txt; cat "$1" >/dev/null"#.into(),
                    "sh".into(),
                    outside_read.to_string_lossy().into_owned(),
                ],
                working_directory: None,
                environment: BTreeMap::new(),
            },
        );
        assert!(matches!(
            process,
            EnvironmentResponse::Process { exit_code, .. } if exit_code != 0
        ));
        assert_eq!(fs::read(root.join("inside.txt")).unwrap(), b"ok");

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[cfg(unix)]
    #[test]
    fn restricted_policies_block_symlink_escape_for_direct_and_process_writes() {
        use std::os::unix::fs::symlink;

        let root = temp_root();
        let outside = temp_root();
        let outside_file = outside.join("escape.txt");
        symlink(&outside, root.join("escape")).unwrap();

        let mut kernel = restricted_kernel(
            &root,
            EnvironmentFilesystemPolicy::HostReadWorkingDirectoryWrite,
        );

        invoke_result(
            &mut kernel,
            EnvironmentCommand::WriteFile {
                path: root
                    .join("escape/direct.txt")
                    .to_string_lossy()
                    .into_owned(),
                content: b"nope".to_vec(),
                create_parents: false,
            },
        )
        .unwrap_err();
        assert!(!outside.join("direct.txt").exists());

        let process = invoke(
            &mut kernel,
            EnvironmentCommand::Exec {
                program: "sh".into(),
                arguments: vec!["-c".into(), r#"printf nope > escape/process.txt"#.into()],
                working_directory: None,
                environment: BTreeMap::new(),
            },
        );
        assert!(matches!(
            process,
            EnvironmentResponse::Process { exit_code, .. } if exit_code != 0
        ));
        assert!(!outside.join("process.txt").exists());
        assert!(!outside_file.exists());

        let _ = fs::remove_file(root.join("escape"));
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[cfg(unix)]
    #[test]
    fn restricted_direct_write_allows_symlink_that_stays_beneath_root() {
        use std::os::unix::fs::symlink;

        let root = temp_root();
        fs::create_dir_all(root.join("target")).unwrap();
        symlink("target", root.join("inside-link")).unwrap();

        let mut kernel = restricted_kernel(
            &root,
            EnvironmentFilesystemPolicy::HostReadWorkingDirectoryWrite,
        );

        assert!(matches!(
            invoke(
                &mut kernel,
                EnvironmentCommand::WriteFile {
                    path: root
                        .join("inside-link/value.txt")
                        .to_string_lossy()
                        .into_owned(),
                    content: b"inside".to_vec(),
                    create_parents: false,
                },
            ),
            EnvironmentResponse::Written
        ));
        assert_eq!(fs::read(root.join("target/value.txt")).unwrap(), b"inside");

        let _ = fs::remove_file(root.join("inside-link"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn restricted_persistent_process_keeps_descendants_confined() {
        let root = temp_root();
        let outside = temp_root();
        let outside_write = outside.join("persistent.txt");
        let mut kernel = restricted_kernel(
            &root,
            EnvironmentFilesystemPolicy::HostReadWorkingDirectoryWrite,
        );

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

        let command = format!(
            r#"sh -c 'printf inside > nested.txt; printf nope > "$1"' sh '{}'; exit
"#,
            outside_write.to_string_lossy()
        );
        assert!(matches!(
            invoke(
                &mut kernel,
                EnvironmentCommand::WriteProcess {
                    handle: handle.clone(),
                    input: command.into_bytes(),
                },
            ),
            EnvironmentResponse::Written
        ));

        let mut exited = false;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut final_exit = None;
        for _ in 0..100 {
            if let EnvironmentResponse::ProcessOutput {
                stdout: chunk_stdout,
                stderr: chunk_stderr,
                exit_code,
                ..
            } = invoke(
                &mut kernel,
                EnvironmentCommand::PollProcess {
                    handle: handle.clone(),
                },
            ) {
                stdout.extend(chunk_stdout);
                stderr.extend(chunk_stderr);
                if exit_code.is_some() {
                    final_exit = exit_code;
                    exited = true;
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            exited,
            "restricted persistent process did not exit; stdout={:?} stderr={:?}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        );
        assert!(
            root.join("nested.txt").exists(),
            "restricted persistent process did not create in-root file; exit={final_exit:?} stdout={:?} stderr={:?}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        );
        assert_eq!(fs::read(root.join("nested.txt")).unwrap(), b"inside");
        assert!(!outside_write.exists());
        let _ = invoke(&mut kernel, EnvironmentCommand::CloseProcess { handle });

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
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
        let mut output = Vec::new();
        let mut exited = false;
        for _ in 0..100 {
            match invoke(
                &mut kernel,
                EnvironmentCommand::PollProcess {
                    handle: handle.clone(),
                },
            ) {
                EnvironmentResponse::ProcessOutput {
                    stdout, exit_code, ..
                } => {
                    output.extend(stdout);
                    if exit_code.is_some() {
                        exited = true;
                        break;
                    }
                }
                other => panic!("unexpected response: {other:?}"),
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(exited, "persistent process did not exit");
        assert!(String::from_utf8_lossy(&output).contains("persistent"));

        assert!(matches!(
            invoke(&mut kernel, EnvironmentCommand::CloseProcess { handle }),
            EnvironmentResponse::ProcessClosed {
                exit_code: Some(0),
                ..
            }
        ));
    }
}
