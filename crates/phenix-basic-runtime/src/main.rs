//! Terminal and JSONL Basic product host. Plugins perform domain work;
//! only this frontend executable reads stdin and submits graph actions.

mod acp_sdk;
mod model_selection_state;
mod provider_deployment;
mod provider_management;
mod selected_provider;
use phenix_basic_contracts::{
    BasicError, ClientInput, Event, EventKind, EventPage, JournalAppend, JournalQuery,
    ObserverCheckpoint, ObserverRequest, RenderRequest, RunResult, RunStatus, Scope,
    SessionSnapshot, Submission,
};
use phenix_contract::{Portable, ScopedRequest, Value};
use phenix_kernel::{
    graph::{GraphCompletion, GraphExecution, GraphStatus, GraphStep},
    resolve::{Composition, Prepared, Selection},
    Kernel, Outcome,
};
use std::{
    collections::{BTreeMap, VecDeque},
    env,
    io::{self, BufRead, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    time::Duration,
};

/// The product host owns the XDG session database path. Nix wrappers only
/// select the preset's file name; the Storage plugin owns its schema and I/O.
/// An explicit absolute PHENIX_BASIC_SQLITE_PATH always overrides the preset.
fn sqlite_default_name(name: Option<&str>) -> Result<&str, String> {
    let selected = name.unwrap_or("basic.sqlite");
    // All installed Basic ACP launchers select one fixed journal basename;
    // reject paths and unknown names, but never reject our own auth presets.
    match selected {
        "basic.sqlite"
        | "basic-http.sqlite"
        | "basic-openai-api.sqlite"
        | "basic-chatgpt.sqlite"
        | "basic-p1-auth.sqlite" => Ok(selected),
        _ => Err("invalid Basic SQLite preset name".into()),
    }
}

fn sqlite_state_base(xdg: Option<PathBuf>, home: Option<PathBuf>) -> Result<PathBuf, String> {
    // Use the same pure policy as the selected Environment provider.
    phenix_environment_contracts::local::state_root(
        &phenix_environment_contracts::local::DirectoryInputs {
            home,
            state: xdg,
            ..Default::default()
        },
    )
    .map_err(|_| "Basic SQLite requires absolute XDG_STATE_HOME or HOME".into())
}

fn ensure_default_sqlite_path() -> Result<(), String> {
    let database = if let Some(explicit) = env::var_os("PHENIX_BASIC_SQLITE_PATH") {
        PathBuf::from(explicit)
    } else {
        let name = env::var("PHENIX_BASIC_SQLITE_DEFAULT_NAME").ok();
        let file_name = sqlite_default_name(name.as_deref())?;
        let base = sqlite_state_base(
            env::var_os("XDG_STATE_HOME").map(PathBuf::from),
            env::var_os("HOME").map(PathBuf::from),
        )?;
        base.join("phenix").join(file_name)
    };
    if !database.is_absolute() {
        return Err("PHENIX_BASIC_SQLITE_PATH must be absolute".into());
    }
    let parent = database
        .parent()
        .ok_or("PHENIX_BASIC_SQLITE_PATH must name a file")?;
    // The wrapper uses umask 077. A direct runtime launch also creates newly
    // needed state directories privately, without changing existing owners
    // or permissions on the caller's parent directories.
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        builder.mode(0o700);
        builder
            .create(parent)
            .map_err(|err| format!("SQLite state directory: {err}"))?;
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(parent).map_err(|err| format!("SQLite state directory: {err}"))?;
    env::set_var("PHENIX_BASIC_SQLITE_PATH", database);
    Ok(())
}

fn invoke<T: Portable>(
    kernel: &Kernel,
    provider: &str,
    operation: &str,
    request: impl Portable,
) -> Result<Result<T, BasicError>, String> {
    let (interface, name) = operation
        .split_once('/')
        .ok_or_else(|| format!("invalid operation {operation}"))?;
    let selection = Selection {
        interface: interface.into(),
        operation: name.into(),
        provider: provider.into(),
    };
    let root = kernel.admit_root(selection).map_err(|e| format!("{e:?}"))?;
    match root
        .invoke(&request.to_value())
        .map_err(|e| format!("{e:?}"))?
    {
        Outcome::Success(value) => Ok(Ok(T::from_value(value).map_err(|e| format!("{e:?}"))?)),
        Outcome::Domain(value) => Ok(Err(
            BasicError::from_value(value).map_err(|e| format!("{e:?}"))?
        )),
    }
}

fn completed<T>(result: Result<Result<T, BasicError>, String>) -> Result<T, String> {
    result?.map_err(|e| format!("{:?}: {}", e.code, e.message))
}

/// Use the pending graph's authenticated root instead of admitting another
/// root while its tickets are live. The kernel still checks the selected
/// operation, exact scope and ordinary root grants.
fn invoke_graph<T: Portable>(
    execution: &GraphExecution<'_>,
    provider: &str,
    operation: &str,
    request: impl Portable,
) -> Result<Result<T, BasicError>, String> {
    let (interface, name) = operation
        .split_once('/')
        .ok_or_else(|| format!("invalid operation {operation}"))?;
    let selected = Selection {
        interface: interface.into(),
        operation: name.into(),
        provider: provider.into(),
    };
    match execution
        .invoke_selected(&selected, &request.to_value())
        .map_err(|e| format!("{e:?}"))?
    {
        Outcome::Success(value) => Ok(Ok(T::from_value(value).map_err(|e| format!("{e:?}"))?)),
        Outcome::Domain(value) => Ok(Err(
            BasicError::from_value(value).map_err(|e| format!("{e:?}"))?
        )),
    }
}

fn invoke_observed<T: Portable>(
    kernel: &Kernel,
    execution: Option<&GraphExecution<'_>>,
    provider: &str,
    operation: &str,
    request: impl Portable,
) -> Result<Result<T, BasicError>, String> {
    match execution {
        Some(graph) => invoke_graph(graph, provider, operation, request),
        None => invoke(kernel, provider, operation, request),
    }
}

fn render_new(
    kernel: &Kernel,
    execution: Option<&GraphExecution<'_>>,
    frontend: &str,
    session: &str,
    after: &mut u64,
    observer: Option<&mut ObserverCheckpoint>,
    sink: impl Write,
) -> Result<(), String> {
    render_new_gated(
        kernel, execution, frontend, session, after, observer, None, sink,
    )
}

fn hold_terminals(events: Vec<Event>, held: &mut Vec<Event>) -> Vec<Event> {
    events
        .into_iter()
        .filter_map(|event| {
            if matches!(
                event.kind,
                EventKind::Answer | EventKind::Cancelled | EventKind::Failure
            ) {
                held.push(event);
                None
            } else {
                Some(event)
            }
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn render_new_gated(
    kernel: &Kernel,
    execution: Option<&GraphExecution<'_>>,
    frontend: &str,
    session: &str,
    after: &mut u64,
    mut observer: Option<&mut ObserverCheckpoint>,
    mut held: Option<&mut Vec<Event>>,
    mut sink: impl Write,
) -> Result<(), String> {
    loop {
        let page: EventPage = completed(invoke_observed(
            kernel,
            execution,
            "phenix.basic.event-journal",
            "phenix.basic.journal@1/read",
            JournalQuery {
                session: session.into(),
                after: *after,
                limit: 128,
            },
        ))?;
        if page.events.is_empty() {
            break;
        }
        if page.next <= *after {
            return Err("journal cursor did not advance".into());
        }
        let events = match held.as_deref_mut() {
            Some(held) => hold_terminals(page.events, held),
            None => page.events,
        };
        let lines: Vec<String> = completed(invoke_observed(
            kernel,
            execution,
            frontend,
            "phenix.basic.frontend@1/render",
            RenderRequest {
                session: session.into(),
                events,
            },
        ))?;
        for line in lines {
            writeln!(sink, "{line}").map_err(|e| e.to_string())?;
        }
        sink.flush().map_err(|e| e.to_string())?;
        if let Some(checkpoint) = observer
            .as_deref_mut()
            .filter(|_| held.as_ref().is_none_or(|events| events.is_empty()))
        {
            *checkpoint = completed(invoke_observed(
                kernel,
                execution,
                "phenix.basic.event-journal",
                "phenix.basic.journal@1/checkpoint_observer",
                ObserverCheckpoint {
                    after: page.next,
                    ..checkpoint.clone()
                },
            ))?;
        }
        *after = page.next;
    }
    sink.flush().map_err(|e| e.to_string())
}

fn basic_scope(composition: &Composition, session: &str) -> ScopedRequest {
    let resource = format!("phenix.session.{session}");
    let mut authority = composition.authority.clone();
    // A Basic prompt is confined to its authenticated session, but graph
    // contributions also need the M1 property identities explicitly granted
    // by the trusted composition. Never pass unrelated session/project
    // resources into this root. The kernel still checks the exact property's
    // set/get/watch grants and its selected owner on every state effect.
    authority
        .resources
        .retain(|granted| granted == &resource || granted.contains(".property."));
    authority.resources.insert(resource.clone());
    ScopedRequest {
        authority,
        resource: Some(resource),
    }
}

/// Start an admitted graph without blocking the frontend's input controller.
mod session_lifecycle;
use session_lifecycle::{OwnershipError, SessionAdmission, SessionOwnership};

/// Recognize the selected Basic product's public terminal result rather than
/// depending on the private ID or placement of the final graph node.
/// InvokeEach child outcomes are not independent completed Basic turns.
fn completed_basic_turn(steps: &[GraphStep]) -> bool {
    steps.iter().any(|step| {
        step.branch.is_none()
            && matches!(
                &step.outcome,
                Outcome::Success(value) if RunResult::from_value(value.clone()).is_ok()
            )
    })
}

/// Child-invocation errors are part of the tool batch's typed outcome and may
/// be reconciled by the parent. Only non-branched domain failures are terminal
/// errors for the product host.
fn first_unhandled_domain_error(steps: &[GraphStep]) -> Result<Option<BasicError>, String> {
    for step in steps {
        if step.branch.is_some() {
            continue;
        }
        if let Outcome::Domain(value) = &step.outcome {
            return BasicError::from_value(value.clone())
                .map(Some)
                .map_err(|error| format!("{error:?}"));
        }
    }
    Ok(None)
}

fn finish_submission(execution: &GraphExecution<'_>) -> Result<bool, String> {
    let cancelled = match execution.completion() {
        Some(GraphCompletion::Success | GraphCompletion::Domain) => false,
        Some(GraphCompletion::Cancelled) => true,
        Some(GraphCompletion::Failure(failure)) => return Err(format!("{failure:?}")),
        None => return Err("graph is not finished".into()),
    };
    if execution.steps().is_empty() && !cancelled {
        return Err("prompt action did not activate a graph node".into());
    }
    if let Some(cause) = first_unhandled_domain_error(execution.steps())? {
        return Err(format!("{:?}: {}", cause.code, cause.message));
    }
    if cfg!(feature = "graph-v2") && !cancelled && !completed_basic_turn(execution.steps()) {
        return Err("graph completed without a final Basic answer".into());
    }
    Ok(cancelled)
}

/// A failed Model turn or caller-selected run limit checkpoints a Failed
/// session and journals a Failure event. Keep Terminal/JSONL alive for
/// later queued prompts; Storage, admission and other domain failures
/// remain fatal. ACP retains its SDK request-error path.
fn finish_stream_submission(execution: &GraphExecution<'_>) -> Result<bool, String> {
    if matches!(execution.completion(), Some(GraphCompletion::Domain)) {
        if completed_basic_turn(execution.steps()) {
            return finish_submission(execution);
        }
        if let Some(cause) = first_unhandled_domain_error(execution.steps())? {
            if cause.code == phenix_basic_contracts::ErrorCode::ModelFailed
                || cause.code == phenix_basic_contracts::ErrorCode::TurnLimit
            {
                return Ok(false);
            }
            return Err(format!("{:?}: {}", cause.code, cause.message));
        }
        return Err("domain graph finished without a domain outcome".into());
    }
    finish_submission(execution)
}

/// The kernel's cancel callback has no Storage authority. Once its tickets
/// physically drain, the host records the explicit user cancellation through
/// the separately authorized Session and Journal providers.
fn record_cancelled(kernel: &Kernel, session: &str) -> Result<(), String> {
    let mut snapshot: SessionSnapshot = completed(invoke(
        kernel,
        "phenix.basic.session",
        "phenix.basic.session@1/open",
        Scope {
            session: session.into(),
        },
    ))?;
    if snapshot.status != RunStatus::Running
        || (!cfg!(feature = "graph-v2") && !snapshot.pending.is_empty())
    {
        return Err("cancelled run has unresolved session work".into());
    }
    if cfg!(feature = "graph-v2") && !snapshot.pending.is_empty() {
        snapshot = completed(invoke(
            kernel,
            "phenix.basic.tool-receipts",
            "phenix.basic.tool-receipts@1/recover",
            phenix_basic_contracts::GraphToolRecovery {
                session: session.into(),
                run: snapshot.run.clone(),
                status: RunStatus::Cancelled,
            },
        ))?;
    } else {
        snapshot.status = RunStatus::Cancelled;
        snapshot = completed(invoke(
            kernel,
            "phenix.basic.session",
            "phenix.basic.session@1/checkpoint",
            snapshot,
        ))?;
    }
    completed(invoke::<u64>(
        kernel,
        "phenix.basic.event-journal",
        "phenix.basic.journal@1/append",
        JournalAppend {
            session: session.into(),
            events: vec![Event {
                sequence: 0,
                session: session.into(),
                run: snapshot.run,
                kind: EventKind::Cancelled,
                call_id: String::new(),
                text: String::new(),
                data: phenix_sdk::Blob(vec![]),
            }],
        },
    ))?;
    Ok(())
}

/// Cooperative, local-process ownership for the duration of one active
/// graph. The file descriptor is the guard: the operating system releases
/// its advisory lock if the host process exits unexpectedly.
///
/// This is admission protection, not a recovery authorization. A fresh
/// process must still not rewrite a durable Running snapshot solely because
/// it acquired a lock; older or noncooperating hosts may lack this protocol.
fn session_lock(session: &str) -> Result<std::fs::File, OwnershipError> {
    let path = env::var("PHENIX_BASIC_SQLITE_PATH")
        .map_err(|_| "PHENIX_BASIC_SQLITE_PATH is required for session ownership".to_string())?;
    if path.is_empty() || path == ":memory:" || path.starts_with("file:") {
        return Err(OwnershipError::Storage(
            "session ownership requires a file-backed SQLite path".into(),
        ));
    }
    lock_session_file(Path::new(&path), session)
}

fn lock_session_file(database: &Path, session: &str) -> Result<std::fs::File, OwnershipError> {
    phenix_basic_contracts::session_resource(session)
        .map_err(|e| format!("{:?}: {}", e.code, e.message))?;
    let full = match std::fs::canonicalize(database) {
        Ok(path) => path,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let parent = database
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            let name = database
                .file_name()
                .ok_or_else(|| "SQLite path has no file name".to_string())?;
            std::fs::canonicalize(parent)
                .map_err(|e| e.to_string())?
                .join(name)
        }
        Err(error) => return Err(OwnershipError::Storage(error.to_string())),
    };
    let mut suffix = String::with_capacity(session.len() * 2);
    for byte in session.bytes() {
        use std::fmt::Write as _;
        write!(suffix, "{byte:02x}").expect("writing to String cannot fail");
    }
    let mut lock_path = full.into_os_string();
    lock_path.push(".phenix-session-");
    lock_path.push(suffix);
    lock_path.push(".lock");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(std::path::PathBuf::from(lock_path))
        .map_err(|e| e.to_string())?;
    file.try_lock().map_err(|error| match error {
        std::fs::TryLockError::WouldBlock => OwnershipError::Busy,
        std::fs::TryLockError::Error(error) => OwnershipError::Storage(error.to_string()),
    })?;
    Ok(file)
}

// A durable marker attests that a lock-aware host checkpointed this exact
// run. A fresh holder may recover only after it owns the matching OS lock.
// No marker, invalid marker or mismatched run leaves Running untouched.
const OWNER_MARKER: &str = "phenix.basic.owner.v1\n";
const RECOVERY_MARKER: &str = "phenix.basic.recovery.v1\n";
const INTERRUPTED_MESSAGE: &str = "run interrupted after owner process exited";

fn owner_marker(file: &mut std::fs::File) -> Result<String, String> {
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut value = String::new();
    file.take(256)
        .read_to_string(&mut value)
        .map_err(|e| e.to_string())?;
    Ok(value)
}

fn set_marker(file: &mut std::fs::File, prefix: &str, run: Option<&str>) -> Result<(), String> {
    file.set_len(0).map_err(|e| e.to_string())?;
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    if let Some(run) = run {
        file.write_all(prefix.as_bytes())
            .map_err(|e| e.to_string())?;
        file.write_all(run.as_bytes()).map_err(|e| e.to_string())?;
    }
    file.sync_all().map_err(|e| e.to_string())
}

fn set_owner_marker(file: &mut std::fs::File, run: Option<&str>) -> Result<(), String> {
    set_marker(file, OWNER_MARKER, run)
}

fn verified_marker(marker: &str, prefix: &str, run: &str) -> bool {
    !run.is_empty() && marker == format!("{prefix}{run}")
}

fn verified_owner(marker: &str, run: &str) -> bool {
    verified_marker(marker, OWNER_MARKER, run)
}

fn verified_recovery(marker: &str, run: &str) -> bool {
    verified_marker(marker, RECOVERY_MARKER, run)
}

// A recovery checkpoint and journal append are separate transactions.
// Scan the durable journal to make recovery retries idempotent under the
// exclusive local session lock.
fn recovery_event_exists(kernel: &Kernel, session: &str, run: &str) -> Result<bool, String> {
    let mut after = 0;
    loop {
        let page: EventPage = completed(invoke(
            kernel,
            "phenix.basic.event-journal",
            "phenix.basic.journal@1/read",
            JournalQuery {
                session: session.into(),
                after,
                limit: 128,
            },
        ))?;
        if page.events.iter().any(|event| {
            event.run == run
                && event.kind == EventKind::Failure
                && event.text == INTERRUPTED_MESSAGE
        }) {
            return Ok(true);
        }
        if page.events.is_empty() {
            return Ok(false);
        }
        if page.next <= after {
            return Err("recovery journal cursor did not advance".into());
        }
        after = page.next;
    }
}

fn recover_owned_run(
    kernel: &Kernel,
    session: &str,
    owner: &mut std::fs::File,
) -> Result<(), String> {
    let marker = owner_marker(owner)?;
    let mut snapshot: SessionSnapshot = completed(invoke(
        kernel,
        "phenix.basic.session",
        "phenix.basic.session@1/open",
        Scope {
            session: session.into(),
        },
    ))?;
    let recovering = verified_recovery(&marker, &snapshot.run);
    let receipt_cleanup = cfg!(feature = "graph-v2")
        && snapshot.status == RunStatus::Indeterminate
        && !snapshot.pending.is_empty()
        && (verified_owner(&marker, &snapshot.run) || recovering);
    if snapshot.status == RunStatus::Running || receipt_cleanup {
        if !verified_owner(&marker, &snapshot.run) && !recovering {
            return Err("running session has no matching fenced owner marker".into());
        }
        // Record recovery intent before the terminal checkpoint. A crash
        // after that checkpoint resumes the missing journal append.
        if !recovering {
            set_marker(owner, RECOVERY_MARKER, Some(&snapshot.run))?;
        }
        if cfg!(feature = "graph-v2") && !snapshot.pending.is_empty() {
            snapshot = completed(invoke(
                kernel,
                "phenix.basic.tool-receipts",
                "phenix.basic.tool-receipts@1/recover",
                phenix_basic_contracts::GraphToolRecovery {
                    session: session.into(),
                    run: snapshot.run.clone(),
                    status: RunStatus::Failed,
                },
            ))?;
        } else {
            snapshot.status = if snapshot.pending.is_empty() {
                RunStatus::Failed
            } else {
                RunStatus::Indeterminate
            };
            snapshot = completed(invoke(
                kernel,
                "phenix.basic.session",
                "phenix.basic.session@1/checkpoint",
                snapshot,
            ))?;
        }
    } else if !recovering {
        if marker.starts_with(RECOVERY_MARKER) {
            return Err("recovery marker does not match durable session run".into());
        }
        return set_owner_marker(owner, None);
    }

    if !matches!(
        snapshot.status,
        RunStatus::Failed | RunStatus::Indeterminate
    ) {
        return Err("recovery marker has an unexpected session status".into());
    }

    if !recovery_event_exists(kernel, session, &snapshot.run)? {
        completed(invoke::<u64>(
            kernel,
            "phenix.basic.event-journal",
            "phenix.basic.journal@1/append",
            JournalAppend {
                session: session.into(),
                events: vec![Event {
                    sequence: 0,
                    session: session.into(),
                    run: snapshot.run,
                    kind: EventKind::Failure,
                    call_id: String::new(),
                    text: INTERRUPTED_MESSAGE.into(),
                    data: phenix_sdk::Blob(vec![]),
                }],
            },
        ))?;
    }
    // A second crash is safe: the recovery marker survives until both
    // durable writes complete and the next holder checks the journal.
    set_owner_marker(owner, None)
}

// Graph admission does not run the first node. Wait until a pump has
// committed Running before recording its durable owner.
fn mark_admitted_run(
    kernel: &Kernel,
    graph: &GraphExecution<'_>,
    session: &str,
    owner: &mut std::fs::File,
) -> Result<bool, String> {
    let snapshot: SessionSnapshot = completed(invoke_observed(
        kernel,
        Some(graph),
        "phenix.basic.session",
        "phenix.basic.session@1/open",
        Scope {
            session: session.into(),
        },
    ))?;
    if snapshot.status == RunStatus::Running && !snapshot.run.is_empty() {
        set_owner_marker(owner, Some(&snapshot.run))?;
        Ok(true)
    } else {
        Ok(false)
    }
}

// This limit applies before JSON, ACP or Terminal parsing. A bounded
// channel alone cannot prevent BufRead::lines from allocating an unbounded
// buffer for an input that never contains a newline.
const MAX_STDIN_LINE_BYTES: usize = 1024 * 1024;

fn bounded_input_line(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut line = Vec::new();
    let mut too_long = false;
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            if line.is_empty() && !too_long {
                return Ok(None);
            }
            break;
        }
        let newline = buffer.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(buffer.len(), |position| position + 1);
        if !too_long {
            if line.len().saturating_add(consumed) > MAX_STDIN_LINE_BYTES {
                too_long = true;
            } else {
                line.extend_from_slice(&buffer[..consumed]);
            }
        }
        reader.consume(consumed);
        if newline.is_some() {
            break;
        }
    }
    if too_long {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Basic stdin frame exceeds 1 MiB",
        ));
    }
    if line.last() == Some(&b'\n') {
        line.pop();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
    }
    String::from_utf8(line)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// A reader owns stdin. Only the controller thread touches Kernel, Root, or
/// GraphExecution. This keeps root admission and cancellation on one thread.
fn input_channel() -> Receiver<Result<String, String>> {
    let (sender, receiver) = mpsc::sync_channel(64);
    std::thread::spawn(move || {
        let mut input = io::stdin().lock();
        loop {
            match bounded_input_line(&mut input) {
                Ok(Some(line)) => {
                    if sender.send(Ok(line)).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let _ = sender.send(Err(error.to_string()));
                    break;
                }
            }
        }
    });
    receiver
}

/// Terminal and JSONL share one controller. Follow-up prompts are queued and
/// cancel packets target only the currently active root.
fn run_console(
    composition: &Composition,
    frontend: &str,
    observer_id: &str,
    session: &str,
) -> Result<(), String> {
    let kernel = scoped_kernel(composition, session)?;
    let mut observer: ObserverCheckpoint = completed(invoke(
        &kernel,
        "phenix.basic.event-journal",
        "phenix.basic.journal@1/observer",
        ObserverRequest {
            session: session.into(),
            observer: observer_id.into(),
        },
    ))?;
    let mut after = observer.after;
    let mut stdout = io::stdout().lock();
    render_new(
        &kernel,
        None,
        frontend,
        session,
        &mut after,
        Some(&mut observer),
        &mut stdout,
    )?;

    let receiver = input_channel();
    let mut queue = VecDeque::<Submission>::new();
    let mut active: Option<GraphExecution<'_>> = None;
    let mut active_lock: Option<SessionAdmission> = None;
    let mut active_marked = false;
    let mut eof = false;
    loop {
        let packet = if eof {
            None
        } else if active.is_some() || !queue.is_empty() {
            match receiver.try_recv() {
                Ok(value) => Some(value),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    eof = true;
                    None
                }
            }
        } else {
            match receiver.recv() {
                Ok(value) => Some(value),
                Err(_) => {
                    eof = true;
                    None
                }
            }
        };
        if let Some(line) = packet {
            let submission = completed(invoke_observed::<Submission>(
                &kernel,
                active.as_ref(),
                frontend,
                "phenix.basic.frontend@1/decode",
                ClientInput {
                    session: session.into(),
                    line: line?,
                },
            ))?;
            if submission.is_cancel() {
                let current = active
                    .as_mut()
                    .ok_or("no active graph execution to cancel")?;
                current.cancel().map_err(|e| format!("{e:?}"))?;
            } else {
                if queue.len() >= 32 {
                    return Err("too many queued Basic prompts".into());
                }
                queue.push_back(submission);
            }
        }

        if active.is_none() {
            if let Some(next) = queue.pop_front() {
                let owner = SessionAdmission::acquire(&kernel, session, next)?;
                let execution = owner.begin(&kernel, basic_scope(composition, session))?;
                active_marked = false;
                active_lock = Some(owner);
                active = Some(execution);
            } else if eof {
                break;
            }
        }
        if let Some(execution) = active.as_mut() {
            let status = execution.pump(|_| {}).map_err(|e| format!("{e:?}"))?;
            if !active_marked {
                if let Some(owner) = active_lock.as_mut() {
                    active_marked = owner.mark(&kernel, execution, session)?;
                }
            }
            render_new(
                &kernel,
                Some(execution),
                frontend,
                session,
                &mut after,
                Some(&mut observer),
                &mut stdout,
            )?;
            if status == GraphStatus::Finished {
                let outcome = finish_stream_submission(execution);
                active = None;
                if outcome? {
                    record_cancelled(&kernel, session)?;
                    render_new(
                        &kernel,
                        None,
                        frontend,
                        session,
                        &mut after,
                        Some(&mut observer),
                        &mut stdout,
                    )?;
                }
                // Clear the durable marker before releasing the OS lock.
                // A crashed process instead leaves the marker for recovery.
                if let Some(ref mut owner) = active_lock {
                    owner.complete()?;
                }
                drop(active_lock.take());
            } else {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
    kernel.shutdown().map_err(|e| format!("{e:?}"))
}

fn jsonl_session(line: &str) -> Result<String, String> {
    let frame: serde_json::Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
    let session = frame
        .get("session")
        .and_then(|value| value.as_str())
        .ok_or("JSONL session is required")?;
    phenix_basic_contracts::session_resource(session)
        .map_err(|e| format!("{:?}: {}", e.code, e.message))?;
    Ok(session.into())
}

fn jsonl_request_failure(
    session: &str,
    message: String,
    mut sink: impl Write,
) -> Result<(), String> {
    let problem = BasicError {
        code: phenix_basic_contracts::ErrorCode::ModelFailed,
        message,
    };
    // Sequence zero is an unjournaled host outcome, not a durable event cursor.
    let event = Event {
        sequence: 0,
        session: session.into(),
        run: String::new(),
        kind: EventKind::Failure,
        call_id: String::new(),
        text: problem.message.clone(),
        data: phenix_basic_contracts::encode(&problem).map_err(|error| format!("{error:?}"))?,
    };
    serde_json::to_writer(&mut sink, &event).map_err(|error| error.to_string())?;
    writeln!(sink).map_err(|error| error.to_string())?;
    sink.flush().map_err(|error| error.to_string())
}

fn discard_jsonl_terminals(
    checkpoint: Option<ObserverCheckpoint>,
    terminals: &[Event],
    persist: impl FnOnce(ObserverCheckpoint) -> Result<(), String>,
) -> Result<(), String> {
    let Some(after) = terminals.iter().map(|event| event.sequence).max() else {
        return Ok(());
    };
    let mut checkpoint = checkpoint.ok_or("missing JSONL terminal observer checkpoint")?;
    checkpoint.after = checkpoint.after.max(after);
    persist(checkpoint)
}

#[cfg(test)]
mod jsonl_outcome_tests {
    use super::*;

    fn event(kind: EventKind, sequence: u64) -> Event {
        Event {
            sequence,
            session: "s1".into(),
            run: "run-1".into(),
            kind,
            call_id: String::new(),
            text: "streamed".into(),
            data: phenix_contract::Blob(vec![]),
        }
    }

    #[test]
    fn only_terminal_events_wait_for_release() {
        let mut held = Vec::new();
        let streaming = hold_terminals(
            vec![
                event(EventKind::TextDelta, 1),
                event(EventKind::Answer, 2),
                event(EventKind::Cancelled, 3),
                event(EventKind::Failure, 4),
            ],
            &mut held,
        );
        assert_eq!(streaming.len(), 1);
        assert_eq!(streaming[0].kind, EventKind::TextDelta);
        assert_eq!(held.len(), 3);
        let result: Result<(), &str> =
            selected_provider::finalize(Ok(()), || Err("release failed"), || {});
        assert!(result.is_err());
        // The caller discards held journal terminals and emits the host Failure.
        let mut wire = Vec::new();
        jsonl_request_failure("s1", "release failed".into(), &mut wire).unwrap();
        let failure: Event = serde_json::from_slice(&wire).unwrap();
        assert_eq!(failure.kind, EventKind::Failure);
        assert_eq!(failure.sequence, 0);
        let problem: BasicError = phenix_basic_contracts::decode(&failure.data).unwrap();
        assert_eq!(problem.code, phenix_basic_contracts::ErrorCode::ModelFailed);
        assert!(!String::from_utf8(wire).unwrap().contains("Answer"));
    }

    #[test]
    fn jsonl_resolver_failure_does_not_export_native_prose() {
        let error = selected_provider::internal(phenix_contract::Failure::new(
            phenix_contract::Code::PluginFailed,
            "credential-wire-sentinel",
        ));
        let mut wire = Vec::new();
        jsonl_request_failure("s1", error.to_string(), &mut wire).unwrap();
        let encoded = String::from_utf8(wire).unwrap();
        assert!(!encoded.contains("credential-wire-sentinel"));
        assert!(encoded.contains("PluginFailed"));
    }

    #[test]
    fn discarded_terminals_advance_the_existing_observer_before_quarantine() {
        let terminals = vec![event(EventKind::Answer, 7), event(EventKind::Cancelled, 9)];
        let checkpoint = ObserverCheckpoint {
            session: "s1".into(),
            observer: "jsonl".into(),
            after: 6,
            revision: 3,
        };
        let persisted = std::cell::Cell::new(0);
        let suppression = discard_jsonl_terminals(Some(checkpoint), &terminals, |next| {
            assert_eq!(next.session, "s1");
            assert_eq!(next.observer, "jsonl");
            assert_eq!(next.revision, 3);
            persisted.set(next.after);
            Ok(())
        });
        let cleanup: Result<(), String> = Err("release failed".into());
        let result = selected_provider::finalize(
            Ok(()),
            || cleanup.and(suppression),
            || {
                assert_eq!(persisted.get(), 9);
            },
        );
        assert_eq!(result.unwrap_err(), "release failed");
        assert!(!terminals
            .iter()
            .any(|event| event.sequence > persisted.get()));
    }

    #[test]
    fn failed_terminal_suppression_quarantines_without_replacing_primary_error() {
        for primary in [Ok(()), Err("primary turn failure".to_owned())] {
            let checkpoint = ObserverCheckpoint {
                session: "s2".into(),
                observer: "jsonl".into(),
                after: 1,
                revision: 1,
            };
            let suppression =
                discard_jsonl_terminals(Some(checkpoint), &[event(EventKind::Answer, 2)], |_| {
                    Err("checkpoint failed".to_owned())
                });
            let quarantined = std::cell::Cell::new(false);
            let expected = primary
                .clone()
                .err()
                .unwrap_or_else(|| "checkpoint failed".into());
            let result =
                selected_provider::finalize(primary, || suppression, || quarantined.set(true));
            assert!(quarantined.get());
            assert_eq!(result.unwrap_err(), expected);
        }
        assert!(
            discard_jsonl_terminals(None, &[event(EventKind::Answer, 2)], |_| panic!(
                "missing observer"
            ))
            .is_err()
        );
    }
}

/// Reuse the ACP session-scoped composition for JSONL. A single controller
/// executes each turn to completion; other sessions' prompts remain queued.
/// Each session has its own journal and observer checkpoint in Storage.
fn run_jsonl(
    composition: &Composition,
    management_specs: &[Composition],
    resolver_specs: &[Composition],
) -> Result<(), String> {
    const FRONTEND: &str = "phenix.basic.frontend-jsonl";
    const OBSERVER: &str = "jsonl";
    let receiver = input_channel();
    let mut queued = VecDeque::<String>::new();
    let mut output = io::stdout().lock();
    let mut sessions = BTreeMap::<String, Kernel>::new();
    let management = provider_management::ProviderManagementGroup::prepare(management_specs)?;
    let resolvers = selected_provider::prepare_resolvers(resolver_specs)?;
    let mut selected = selected_provider::SelectedState::default();
    sessions.insert("s1".into(), scoped_kernel(composition, "s1")?);

    // Retain the existing no-input behavior: replay undelivered events from
    // the default session when a JSONL host starts solely as an observer.
    if let Some(_ownership) = match SessionOwnership::acquire("s1") {
        Ok(owner) => Some(owner),
        // Startup observation is optional; explicit requests still require ownership.
        Err(OwnershipError::Busy) => None,
        Err(error) => return Err(error.into()),
    } {
        let kernel = sessions.get("s1").expect("default session initialized");
        let mut observer: ObserverCheckpoint = completed(invoke(
            kernel,
            "phenix.basic.event-journal",
            "phenix.basic.journal@1/observer",
            ObserverRequest {
                session: "s1".into(),
                observer: OBSERVER.into(),
            },
        ))?;
        let mut after = observer.after;
        render_new(
            kernel,
            None,
            FRONTEND,
            "s1",
            &mut after,
            Some(&mut observer),
            &mut output,
        )?;
    }

    loop {
        let line = if let Some(line) = queued.pop_front() {
            line
        } else {
            match receiver.recv() {
                Ok(line) => line?,
                Err(_) => break,
            }
        };
        let session = jsonl_session(&line)?;
        let request_result = (|| -> Result<(), String> {
            selected.check(&session)?;
            if !sessions.contains_key(&session) {
                if sessions.len() >= 32 {
                    return Err("too many active JSONL sessions".into());
                }
                sessions.insert(session.clone(), scoped_kernel(composition, &session)?);
            }
            let kernel = sessions.get(&session).expect("session initialized");
            let input: Submission = completed(invoke(
                kernel,
                FRONTEND,
                "phenix.basic.frontend@1/decode",
                ClientInput {
                    session: session.clone(),
                    line,
                },
            ))?;
            let idle_cancel = input.is_cancel();
            let mut observation: (Vec<Event>, Option<ObserverCheckpoint>) = (Vec::new(), None);
            let result = selected.run_with(
                kernel,
                composition,
                management.as_ref(),
                &resolvers,
                &session,
                input,
                &mut observation,
                |(terminals, terminal_checkpoint), admission, installation| -> Result<(), String> {
                    let observer = terminal_checkpoint.insert(completed(invoke(
                        kernel,
                        "phenix.basic.event-journal",
                        "phenix.basic.journal@1/observer",
                        ObserverRequest {
                            session: session.clone(),
                            observer: OBSERVER.into(),
                        },
                    ))?);
                    let mut after = observer.after;
                    render_new(
                        kernel,
                        None,
                        FRONTEND,
                        &session,
                        &mut after,
                        Some(&mut *observer),
                        &mut output,
                    )?;
                    if idle_cancel {
                        // Replay is authorized observation, not recovery or agent execution.
                        return Ok(());
                    }
                    let owner = admission.expect("prompt admission acquired");
                    let execution = owner.begin(kernel, basic_scope(composition, &session))?;
                    let mut execution =
                        selected_provider::Execution::new(execution, kernel, installation);
                    let mut marked = false;
                    loop {
                        // Leave unprocessed requests in the bounded channel when the
                        // local queue fills. Do not abandon an admitted graph.
                        for _ in 0..32 {
                            if queued.len() >= 32 {
                                break;
                            }
                            let pending_line = match receiver.try_recv() {
                                Ok(line) => line?,
                                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
                            };
                            let target = match jsonl_session(&pending_line) {
                                Ok(target) => target,
                                Err(_) => {
                                    // Report malformed input only after the active root
                                    // finishes; it must not abandon pending physical work.
                                    queued.push_back(pending_line);
                                    continue;
                                }
                            };
                            let packet: serde_json::Value =
                                serde_json::from_str(&pending_line).map_err(|e| e.to_string())?;
                            if packet.get("type").and_then(|value| value.as_str()) == Some("cancel")
                            {
                                if target == session {
                                    let cancellation: Submission = completed(invoke_graph(
                                        &execution,
                                        FRONTEND,
                                        "phenix.basic.frontend@1/decode",
                                        ClientInput {
                                            session: session.clone(),
                                            line: pending_line,
                                        },
                                    ))?;
                                    if cancellation.is_cancel() {
                                        execution.cancel()?;
                                    }
                                }
                            } else {
                                queued.push_back(pending_line);
                            }
                        }
                        let status = execution.pump(|_| {}).map_err(|e| format!("{e:?}"))?;
                        if !marked {
                            marked = owner.mark(kernel, &execution, &session)?;
                        }
                        render_new_gated(
                            kernel,
                            Some(&execution),
                            FRONTEND,
                            &session,
                            &mut after,
                            Some(&mut *observer),
                            Some(&mut *terminals),
                            &mut output,
                        )?;
                        if status == GraphStatus::Finished {
                            execution.finish();
                            let cancelled = finish_stream_submission(&execution)?;
                            drop(execution);
                            if cancelled {
                                record_cancelled(kernel, &session)?;
                                render_new_gated(
                                    kernel,
                                    None,
                                    FRONTEND,
                                    &session,
                                    &mut after,
                                    Some(&mut *observer),
                                    Some(&mut *terminals),
                                    &mut output,
                                )?;
                            }
                            owner.complete()?;
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    if !terminals.is_empty() {
                        observer.after = after;
                    }
                    Ok(())
                },
                |(terminals, terminal_checkpoint), result, cleanup| {
                    // Streaming Model/limit failures are journaled outcomes, not errors
                    // from finish_stream_submission. They still outrank a release fault.
                    if result.is_ok() && cleanup.is_err() {
                        if let Some(failure) = terminals
                            .iter()
                            .find(|event| event.kind == EventKind::Failure)
                        {
                            *result = Err(failure.text.clone());
                        }
                    }
                    // Discarded terminals must also be consumed durably by this JSONL
                    // observer, otherwise restart/idle replay could publish false success.
                    // Keep ownership and the kernel until the suppression write finishes.
                    let suppression = if result.is_err() || cleanup.is_err() {
                        discard_jsonl_terminals(
                            terminal_checkpoint.take(),
                            terminals,
                            |checkpoint| {
                                let _: ObserverCheckpoint = completed(invoke(
                                    kernel,
                                    "phenix.basic.event-journal",
                                    "phenix.basic.journal@1/checkpoint_observer",
                                    checkpoint,
                                ))?;
                                Ok(())
                            },
                        )
                    } else {
                        Ok(())
                    };
                    if suppression.is_err() {
                        eprintln!(
                            "JSONL terminal suppression checkpoint failed; session quarantined"
                        );
                    }
                    suppression
                },
            );
            if selected.is_quarantined(&session) {
                if let Some(kernel) = sessions.remove(&session) {
                    let _ = kernel.shutdown();
                }
            }
            result?;
            let (terminals, terminal_checkpoint) = observation;
            if !terminals.is_empty() {
                let kernel = sessions
                    .get(&session)
                    .expect("successful cleanup retains session");
                let lines: Vec<String> = completed(invoke(
                    kernel,
                    FRONTEND,
                    "phenix.basic.frontend@1/render",
                    RenderRequest {
                        session: session.clone(),
                        events: terminals,
                    },
                ))?;
                for line in lines {
                    writeln!(output, "{line}").map_err(|error| error.to_string())?;
                }
                output.flush().map_err(|error| error.to_string())?;
                if let Some(checkpoint) = terminal_checkpoint {
                    let _: ObserverCheckpoint = completed(invoke(
                        kernel,
                        "phenix.basic.event-journal",
                        "phenix.basic.journal@1/checkpoint_observer",
                        checkpoint,
                    ))?;
                }
            }
            Ok(())
        })();
        if let Err(error) = request_result {
            jsonl_request_failure(&session, error, &mut output)?;
        }
    }
    for (_, kernel) in sessions {
        kernel.shutdown().map_err(|e| format!("{e:?}"))?;
    }
    if let Some(management) = management {
        management.shutdown()?;
    }
    for resolver in resolvers {
        resolver
            .kernel
            .shutdown()
            .map_err(selected_provider::internal)?;
    }
    Ok(())
}

/// ACP connection controller. Each session owns a fresh M1 kernel admission
/// with the requested resource scope. Artifacts and resolver stay unchanged.
fn scoped_composition(composition: &Composition, resource: &str) -> Composition {
    let mut spec = composition.clone();
    spec.resource = Some(resource.into());
    // Retain only property resources already declared by the trusted
    // composition. A session root must never inherit another session's
    // grants, but reactive graph nodes still need their separately granted
    // M1 property identities within this same pinned generation.
    let property_resources: std::collections::BTreeSet<_> = spec
        .authority
        .resources
        .iter()
        .filter(|name| name.contains(".property."))
        .cloned()
        .collect();
    spec.authority.resources = property_resources.clone();
    spec.authority.resources.insert(resource.into());
    for grant in &mut spec.grants {
        let retained = grant.resources.intersection(&property_resources).cloned();
        grant.resources = retained.collect();
        grant.resources.insert(resource.into());
    }
    spec
}

fn scoped_kernel(composition: &Composition, session: &str) -> Result<Kernel, String> {
    let resource = phenix_basic_contracts::session_resource(session)
        .map_err(|e| format!("{:?}: {}", e.code, e.message))?;
    let spec = scoped_composition(composition, &resource);
    prepare_agent(spec)
        .map_err(|e| format!("{e:?}"))?
        .load()
        .map_err(|e| format!("{e:?}"))
}

#[cfg(not(feature = "graph-v2"))]
fn prepare_agent(spec: Composition) -> Result<Prepared, phenix_contract::Failure> {
    Prepared::prepare(spec)
}

#[cfg(feature = "graph-v2")]
fn prepare_agent(spec: Composition) -> Result<Prepared, phenix_contract::Failure> {
    use phenix_contract::graph::v2::{Classification, IntegrityClass, PortSecurity};
    use phenix_kernel::graph::v2::{
        OperationSecurity, SourceOwnerPolicy, TrustedCompositionPolicy,
    };
    let label = PortSecurity {
        confidentiality: Classification::Confidential,
        integrity: IntegrityClass::Untrusted,
    };
    let mut policy = TrustedCompositionPolicy::default();
    policy.source_owners.insert(
        "phenix.basic.agent".into(),
        SourceOwnerPolicy {
            owner_ceiling: Classification::Confidential,
            composition_ceiling: Classification::Confidential,
        },
    );
    policy
        .sources
        .insert("phenix.basic.agent/prompt".into(), label);
    for key in [
        "phenix.basic.turn-prepare@1/prepare",
        "phenix.basic.graph-turn@1/model",
        "phenix.basic.graph-turn@1/decide",
        "phenix.basic.graph-turn@1/tool",
        "phenix.basic.graph-turn@1/finish",
        "phenix.basic.graph-tool-turn@1/admit",
        "phenix.basic.graph-tool-turn@1/reconcile",
    ] {
        policy.operations.insert(
            ("phenix.basic.agent".into(), key.into()),
            OperationSecurity {
                input: label,
                ok: label,
                err: label,
            },
        );
    }
    for (provider, keys) in [
        (
            "phenix.basic.tools",
            &[
                "phenix.basic.graph-tools@1/plan",
                "phenix.basic.graph-tools@1/collect",
                "phenix.basic.graph-tool-executor@1/invoke",
            ][..],
        ),
        (
            "phenix.tool.pending-a",
            &["phenix.basic.graph-tool-executor@1/invoke"][..],
        ),
        (
            "phenix.tool.pending-b",
            &["phenix.basic.graph-tool-executor@1/invoke"][..],
        ),
    ] {
        for key in keys {
            policy.operations.insert(
                (provider.into(), (*key).into()),
                OperationSecurity {
                    input: label,
                    ok: label,
                    err: label,
                },
            );
        }
    }
    for node in ["plan", "join", "collect"] {
        policy.topology.insert((
            "phenix.basic.agent".into(),
            format!("phenix.basic.tools/{node}"),
        ));
    }
    for (from, port, to, input) in [
        (
            "phenix.basic.agent/admit",
            "ok",
            "phenix.basic.tools/plan",
            "in",
        ),
        (
            "phenix.basic.agent/admit",
            "ok",
            "phenix.basic.tools/join",
            "batch",
        ),
        (
            "phenix.basic.tools/collect",
            "ok",
            "phenix.basic.agent/tool",
            "in",
        ),
        (
            "phenix.basic.tools/plan",
            "ok",
            "phenix.basic.tools/each",
            "in",
        ),
        (
            "phenix.basic.tools/each",
            "out",
            "phenix.basic.tools/join",
            "outcomes",
        ),
    ] {
        policy.delivery.insert(
            (from.into(), port.into(), to.into(), input.into()),
            phenix_kernel::graph::v2::FlowGrant {
                resources: spec.authority.resources.clone(),
                maximum_classification: Classification::Confidential,
            },
        );
    }
    Prepared::prepare_graph_v2(spec, &policy)
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 || !["terminal", "jsonl", "acp"].contains(&args[1].as_str()) {
        return Err("usage: phenix-basic-runtime terminal|jsonl|acp COMPOSITION.json [--session SESSION | --provider-management MANAGEMENT.json] [--provider-credentials CREDENTIALS.json]... [--provider-model-resolver RESOLVER.json]... [--provider-bundle DEPLOYMENT.json]".into());
    }
    ensure_default_sqlite_path()?;

    // Management compositions are launcher-owned. Client requests cannot
    // supply a package path, composition or native authority grant.
    let mut management: Vec<Composition> = Vec::new();
    let mut credentials: Vec<Composition> = Vec::new();
    let mut resolvers: Vec<Composition> = Vec::new();
    if args[1] == "acp" || args[1] == "jsonl" {
        if !(args.len() - 3).is_multiple_of(2) {
            return Err("ACP management flags require a composition file".into());
        }
        let mut has_bundle = false;
        for flag in args[3..].chunks_exact(2) {
            let data = std::fs::read(&flag[1]).map_err(|e| e.to_string())?;
            if flag[0] == "--provider-bundle" {
                if has_bundle
                    || !management.is_empty()
                    || !credentials.is_empty()
                    || !resolvers.is_empty()
                {
                    return Err("provider bundle cannot be mixed with manual provider roots".into());
                }
                let deployment: provider_deployment::ProviderDeployment =
                    serde_json::from_slice(&data).map_err(|e| e.to_string())?;
                let composed = deployment.compose()?;
                management.push(composed.management);
                if let Some(control) = composed.credentials {
                    credentials.push(control);
                }
                if let Some(resolver) = composed.resolver {
                    resolvers.push(resolver);
                }
                has_bundle = true;
                continue;
            }
            if has_bundle {
                return Err("provider bundle cannot be mixed with manual provider roots".into());
            }
            let spec: Composition = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
            match flag[0].as_str() {
                "--provider-management" => {
                    management.push(spec);
                }
                "--provider-credentials" => credentials.push(spec),
                "--provider-model-resolver" => resolvers.push(spec),
                _ => return Err("unknown ACP management flag".into()),
            }
        }
    }
    let terminal_session = if args[1] == "terminal" && args.len() == 5 && args[3] == "--session" {
        args[4].as_str()
    } else if args.len() == 3 || args[1] == "acp" || args[1] == "jsonl" {
        "s1"
    } else {
        return Err("terminal session arguments are invalid".into());
    };
    let path = Path::new(&args[2]);
    let config = std::fs::read(path).map_err(|e| e.to_string())?;
    // Source/build policies belong to the product host. The kernel receives
    // the sole v3 composition format containing verified package paths only.
    let source_config: serde_json::Value =
        serde_json::from_slice(&config).map_err(|e| e.to_string())?;
    let prepared = if source_config.get("plugin_sources").is_some()
        || source_config.get("builders").is_some()
    {
        if source_config.get("format").and_then(|v| v.as_str()) != Some("phenix.composition.v3") {
            return Err("source declarations require phenix.composition.v3".into());
        }
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let scratch = std::env::temp_dir().join(format!(
            "phenix-plugin-sources-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir(&scratch).map_err(|e| e.to_string())?;
        let output = scratch.join("composition.json");
        let preparer = std::env::var_os("PHENIX_PLUGIN_PREPARER")
            .unwrap_or_else(|| "phenix-plugin-sources".into());
        let status = std::process::Command::new(preparer)
            .arg(path)
            .arg(&output)
            .status();
        let result = match status {
            Ok(status) if status.success() => std::fs::read(&output).map_err(|e| e.to_string()),
            Ok(status) => Err(format!("plugin source preparation exited with {status}")),
            Err(error) => Err(format!("cannot invoke plugin source preparer: {error}")),
        };
        let _ = std::fs::remove_dir_all(scratch);
        result?
    } else {
        config
    };
    let composition: Composition = serde_json::from_slice(&prepared).map_err(|e| e.to_string())?;
    match args[1].as_str() {
        "acp" => acp_sdk::run(&composition, &management, &credentials, &resolvers),
        "jsonl" => run_jsonl(&composition, &management, &resolvers),
        "terminal" => run_console(
            &composition,
            "phenix.basic.frontend-terminal",
            "terminal",
            terminal_session,
        ),
        _ => unreachable!(),
    }
}
fn main() {
    if let Err(err) = run() {
        eprintln!("phenix-basic: {err}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod session_lock_tests {
    use super::{
        lock_session_file, owner_marker, set_marker, set_owner_marker, verified_owner,
        verified_recovery, OWNER_MARKER, RECOVERY_MARKER,
    };
    use super::{scoped_composition, sqlite_default_name, sqlite_state_base};
    use phenix_contract::Authority;
    use phenix_kernel::resolve::{Composition, ScopedGrant, Selection};
    use std::collections::BTreeSet;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn sqlite_state_base_obeys_xdg_precedence_and_unset_semantics() {
        use std::path::PathBuf;
        let home = Some(PathBuf::from("/home/user"));
        assert_eq!(
            sqlite_state_base(Some("/state".into()), home.clone()).unwrap(),
            PathBuf::from("/state")
        );
        for xdg in [None, Some("".into()), Some("relative".into())] {
            assert_eq!(
                sqlite_state_base(xdg, home.clone()).unwrap(),
                PathBuf::from("/home/user/.local/state")
            );
        }
        for home in [None, Some("".into()), Some("relative".into())] {
            assert!(sqlite_state_base(None, home.clone()).is_err());
            assert_eq!(
                sqlite_state_base(Some("/state".into()), home).unwrap(),
                PathBuf::from("/state")
            );
        }
    }

    #[test]
    fn sqlite_preset_identity_is_bounded_and_cannot_be_a_path() {
        assert_eq!(sqlite_default_name(None).unwrap(), "basic.sqlite");
        assert_eq!(
            sqlite_default_name(Some("basic.sqlite")).unwrap(),
            "basic.sqlite"
        );
        assert_eq!(
            sqlite_default_name(Some("basic-http.sqlite")).unwrap(),
            "basic-http.sqlite"
        );
        for invalid in [
            "",
            "../other.sqlite",
            "/tmp/other.sqlite",
            "basic-other.sqlite",
        ] {
            assert!(sqlite_default_name(Some(invalid)).is_err());
        }
    }

    #[test]
    fn session_root_preserves_explicit_property_grants_but_never_foreign_sessions() {
        let session = "phenix.session.s1";
        let foreign = "phenix.session.s2";
        let property = "phenix.basic.agent.property.pending_model_request";
        let grant = ScopedGrant {
            caller: "root".into(),
            interface: "phenix.basic.input@1".into(),
            operation: "submit".into(),
            resources: [session.into(), foreign.into(), property.into()].into(),
        };
        let original = Composition {
            format: "phenix.composition.v3".into(),
            artifacts: vec![],
            definitions: vec![],
            entry: Selection {
                provider: "phenix.basic.agent".into(),
                interface: "phenix.basic.input@1".into(),
                operation: "submit".into(),
            },
            slots: vec![],
            grants: vec![grant],
            excluded: BTreeSet::new(),
            substitutions: vec![],
            authority: Authority {
                operations: ["phenix.basic.input@1/submit".into()].into(),
                resources: [session.into(), foreign.into(), property.into()].into(),
            },
            resource: Some(session.into()),
        };
        let scoped = scoped_composition(&original, session);
        assert_eq!(
            scoped.authority.resources,
            [session.into(), property.into()].into()
        );
        assert_eq!(
            scoped.grants[0].resources,
            [session.into(), property.into()].into()
        );
        assert!(!scoped.authority.resources.contains(foreign));
        let foreign_scoped = scoped_composition(&original, foreign);
        assert_eq!(
            foreign_scoped.authority.resources,
            [foreign.into(), property.into()].into()
        );
        assert!(!foreign_scoped.authority.resources.contains(session));
        assert_eq!(original.grants[0].resources.len(), 3, "original immutable");
    }

    #[test]
    fn recovery_requires_matching_versioned_run_marker() {
        assert!(verified_owner(&format!("{OWNER_MARKER}run-4"), "run-4"));
        assert!(!verified_owner("", "run-4"));
        assert!(!verified_owner("run-4", "run-4"));
        assert!(!verified_owner(&format!("{OWNER_MARKER}run-3"), "run-4"));
        assert!(!verified_owner(&format!("{OWNER_MARKER}run-4"), ""));
        assert!(!verified_owner(&format!("{OWNER_MARKER}run-4\\n"), "run-4"));
        assert!(verified_recovery(
            &format!("{RECOVERY_MARKER}run-4"),
            "run-4"
        ));
        assert!(!verified_recovery(
            &format!("{RECOVERY_MARKER}run-3"),
            "run-4"
        ));
        assert!(!verified_owner(&format!("{RECOVERY_MARKER}run-4"), "run-4"));
    }

    #[test]
    fn independent_sessions_and_lock_release() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "phenix-session-lock-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let database = root.join("basic.db");

        let mut first = lock_session_file(&database, "s1").unwrap();
        assert!(matches!(
            lock_session_file(&database, "s1"),
            Err(super::OwnershipError::Busy)
        ));
        assert!(matches!(
            lock_session_file(&root.join("missing/basic.db"), "s1"),
            Err(super::OwnershipError::Storage(_))
        ));
        let second = lock_session_file(&database, "s2").unwrap();

        assert_eq!(owner_marker(&mut first).unwrap(), "");
        set_owner_marker(&mut first, Some("run-3")).unwrap();
        assert_eq!(
            owner_marker(&mut first).unwrap(),
            format!("{OWNER_MARKER}run-3"),
        );

        drop(first);
        let mut resumed = lock_session_file(&database, "s1").unwrap();
        assert_eq!(
            owner_marker(&mut resumed).unwrap(),
            format!("{OWNER_MARKER}run-3"),
        );
        set_marker(&mut resumed, RECOVERY_MARKER, Some("run-3")).unwrap();
        assert_eq!(
            owner_marker(&mut resumed).unwrap(),
            format!("{RECOVERY_MARKER}run-3"),
        );

        set_owner_marker(&mut resumed, None).unwrap();
        assert_eq!(owner_marker(&mut resumed).unwrap(), "");
        drop(resumed);
        drop(second);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod sqlite_preset_tests {
    use super::sqlite_default_name;

    #[test]
    fn all_installed_basic_launchers_have_admitted_journals() {
        assert_eq!(sqlite_default_name(None).unwrap(), "basic.sqlite");
        for basename in [
            "basic.sqlite",
            "basic-http.sqlite",
            "basic-openai-api.sqlite",
            "basic-chatgpt.sqlite",
            "basic-p1-auth.sqlite",
        ] {
            assert_eq!(sqlite_default_name(Some(basename)).unwrap(), basename);
        }
        for invalid in [
            "",
            "../basic.sqlite",
            "basic/credentials.sqlite",
            "basic-p1-auth.sqlite/../foreign",
            "unknown.sqlite",
            "/tmp/basic.sqlite",
        ] {
            assert!(sqlite_default_name(Some(invalid)).is_err(), "{invalid}");
        }
    }
}

#[cfg(test)]
mod bounded_stdio_tests {
    use super::bounded_input_line;
    use std::io::{Cursor, ErrorKind};

    #[test]
    fn preserves_newlines_utf8_crlf_and_final_partial_line() {
        let mut input = Cursor::new("first\r\n☃\nlast".as_bytes());
        assert_eq!(
            bounded_input_line(&mut input).unwrap().as_deref(),
            Some("first")
        );
        assert_eq!(
            bounded_input_line(&mut input).unwrap().as_deref(),
            Some("☃")
        );
        assert_eq!(
            bounded_input_line(&mut input).unwrap().as_deref(),
            Some("last")
        );
        assert_eq!(bounded_input_line(&mut input).unwrap(), None);
    }

    #[test]
    fn drains_oversized_frame_without_buffering_it() {
        let mut input = Cursor::new(
            [
                vec![b'x'; super::MAX_STDIN_LINE_BYTES + 7],
                b"\nnext\n".to_vec(),
            ]
            .concat(),
        );
        assert_eq!(
            bounded_input_line(&mut input).unwrap_err().kind(),
            ErrorKind::InvalidData,
        );
        assert_eq!(
            bounded_input_line(&mut input).unwrap().as_deref(),
            Some("next")
        );
    }

    #[test]
    fn rejects_invalid_utf8_without_lossy_replacement() {
        let mut input = Cursor::new([0xff, b'\n']);
        assert_eq!(
            bounded_input_line(&mut input).unwrap_err().kind(),
            ErrorKind::InvalidData,
        );
    }
}

#[cfg(test)]
mod graph_outcome_contract_tests {
    use super::{completed_basic_turn, first_unhandled_domain_error};
    use phenix_basic_contracts::{error, ErrorCode, ModelReply, RunResult, Usage};
    use phenix_contract::Portable;
    use phenix_kernel::{graph::GraphStep, Outcome};

    fn step(node: &str, branch: Option<u32>, outcome: Outcome) -> GraphStep {
        GraphStep {
            root: 7,
            activation: Some(2),
            branch,
            node: node.into(),
            outcome,
        }
    }

    #[test]
    fn final_answer_is_detected_by_public_result_not_private_node_name() {
        let result = RunResult {
            session: "session".into(),
            run: "run".into(),
            answer: "done".into(),
            model_turns: 1,
            tool_calls: 0,
            usage: Usage::default(),
        };
        assert!(completed_basic_turn(&[step(
            "renamed.agent/any-node",
            None,
            Outcome::Success(result.to_value()),
        )]));
        assert!(!completed_basic_turn(&[step(
            "renamed.tools/child",
            Some(0),
            Outcome::Success(result.to_value()),
        )]));
        assert!(!completed_basic_turn(&[step(
            "renamed.agent/model",
            None,
            Outcome::Success(
                ModelReply {
                    text: "not a final answer".into(),
                    calls: Vec::new(),
                    usage: Usage::default(),
                }
                .to_value(),
            ),
        )]));
    }

    #[test]
    fn tool_child_domain_error_is_not_host_terminal_failure() {
        let child = step(
            "any.tools/executor",
            Some(1),
            Outcome::Domain(error(ErrorCode::ToolFailed, "recovered").to_value()),
        );
        assert!(first_unhandled_domain_error(&[child]).unwrap().is_none());
        let root = step(
            "any.agent/operation",
            None,
            Outcome::Domain(error(ErrorCode::ModelFailed, "terminal").to_value()),
        );
        assert_eq!(
            first_unhandled_domain_error(&[root]).unwrap().unwrap().code,
            ErrorCode::ModelFailed
        );
    }
}
