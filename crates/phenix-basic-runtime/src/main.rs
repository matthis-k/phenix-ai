//! Terminal and JSONL Basic product host. Plugins perform domain work;
//! only this frontend executable reads stdin and submits graph actions.
use phenix_basic_contracts::{
    BasicError, ClientInput, Event, EventKind, EventPage, JournalAppend, JournalQuery,
    ObserverCheckpoint, ObserverRequest, RenderRequest, RunStatus, Scope, SessionSnapshot,
    Submission,
};
use phenix_contract::{Portable, ScopedRequest, Value};
use phenix_kernel::{
    graph::{GraphCompletion, GraphExecution, GraphStatus},
    resolve::{Composition, Prepared, Selection},
    Kernel, Outcome,
};
use std::{
    collections::VecDeque,
    env,
    io::{self, BufRead, Write},
    path::Path,
    sync::mpsc::{self, Receiver, TryRecvError},
    time::Duration,
};

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

fn render_new(
    kernel: &Kernel,
    frontend: &str,
    session: &str,
    after: &mut u64,
    mut observer: Option<&mut ObserverCheckpoint>,
    mut sink: impl Write,
) -> Result<(), String> {
    loop {
        let page: EventPage = completed(invoke(
            kernel,
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
        let lines: Vec<String> = completed(invoke(
            kernel,
            frontend,
            "phenix.basic.frontend@1/render",
            RenderRequest {
                session: session.into(),
                events: page.events,
            },
        ))?;
        for line in lines {
            writeln!(sink, "{line}").map_err(|e| e.to_string())?;
        }
        sink.flush().map_err(|e| e.to_string())?;
        if let Some(checkpoint) = observer.as_deref_mut() {
            *checkpoint = completed(invoke(
                kernel,
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
    authority.resources = [resource.clone()].into();
    ScopedRequest {
        authority,
        resource: Some(resource),
    }
}

fn submit(kernel: &Kernel, input: Submission, scope: ScopedRequest) -> Result<(), String> {
    let mut execution = begin_submission(kernel, input, scope)?;
    while let GraphStatus::Pending | GraphStatus::Draining =
        execution.pump(|_| {}).map_err(|e| format!("{e:?}"))?
    {
        std::thread::sleep(Duration::from_millis(5));
    }
    finish_submission(&execution).map(|_| ())
}


/// Start an admitted graph without blocking the frontend's input controller.
fn begin_submission<'a>(
    kernel: &'a Kernel,
    input: Submission,
    scope: ScopedRequest,
) -> Result<GraphExecution<'a>, String> {
    if input.cancel {
        return Err("no active graph execution to cancel".into());
    }
    if input.action != "phenix.basic.submit" {
        return Err("frontend selected an unknown Basic action".into());
    }
    let value = Value::decode(&input.payload.0).map_err(|e| format!("{e:?}"))?;
    kernel
        .begin_action_scoped(&input.action, &value, scope)
        .map_err(|e| format!("{e:?}"))
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
    for step in execution.steps() {
        if let Outcome::Domain(value) = &step.outcome {
            let cause = BasicError::from_value(value.clone()).map_err(|e| format!("{e:?}"))?;
            return Err(format!("{:?}: {}", cause.code, cause.message));
        }
    }
    Ok(cancelled)
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
    if snapshot.status != RunStatus::Running || !snapshot.pending.is_empty() {
        return Err("cancelled run has unresolved session work".into());
    }
    snapshot.status = RunStatus::Cancelled;
    let snapshot: SessionSnapshot = completed(invoke(
        kernel,
        "phenix.basic.session",
        "phenix.basic.session@1/checkpoint",
        snapshot,
    ))?;
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

/// A reader owns stdin. Only the controller thread touches Kernel, Root, or
/// GraphExecution. This keeps root admission and cancellation on one thread.
fn input_channel() -> Receiver<Result<String, String>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            if sender.send(line.map_err(|e| e.to_string())).is_err() {
                break;
            }
        }
    });
    receiver
}

/// Terminal and JSONL share one controller. Follow-up prompts are queued and
/// cancel packets target only the currently active root.
fn run_console(composition: &Composition, frontend: &str, observer_id: &str) -> Result<(), String> {
    let kernel = Prepared::prepare(composition.clone())
        .map_err(|e| format!("{e:?}"))?
        .load()
        .map_err(|e| format!("{e:?}"))?;
    let session = "s1";
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
        frontend,
        session,
        &mut after,
        Some(&mut observer),
        &mut stdout,
    )?;

    let receiver = input_channel();
    let mut queue = VecDeque::<Submission>::new();
    let mut active: Option<GraphExecution<'_>> = None;
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
            let submission = completed(invoke::<Submission>(
                &kernel,
                frontend,
                "phenix.basic.frontend@1/decode",
                ClientInput {
                    session: session.into(),
                    line: line?,
                },
            ))?;
            if submission.cancel {
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
                active = Some(begin_submission(
                    &kernel,
                    next,
                    basic_scope(composition, session),
                )?);
            } else if eof {
                break;
            }
        }
        if let Some(execution) = active.as_mut() {
            let status = execution.pump(|_| {}).map_err(|e| format!("{e:?}"))?;
            render_new(
                &kernel,
                frontend,
                session,
                &mut after,
                Some(&mut observer),
                &mut stdout,
            )?;
            if status == GraphStatus::Finished {
                let outcome = finish_submission(execution);
                active = None;
                if outcome? {
                    record_cancelled(&kernel, session)?;
                    render_new(
                        &kernel,
                        frontend,
                        session,
                        &mut after,
                        Some(&mut observer),
                        &mut stdout,
                    )?;
                }
            } else {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
    kernel.shutdown().map_err(|e| format!("{e:?}"))
}

/// ACP connection controller. Each session owns a fresh M1 kernel admission
/// with the requested resource scope. Artifacts and resolver stay unchanged.
fn scoped_kernel(composition: &Composition, session: &str) -> Result<Kernel, String> {
    phenix_basic_contracts::session_resource(session)
        .map_err(|e| format!("{:?}: {}", e.code, e.message))?;
    let resource = format!("phenix.session.{session}");
    let mut spec = composition.clone();
    spec.resource = Some(resource.clone());
    spec.authority.resources = [resource.clone()].into();
    for grant in &mut spec.grants {
        grant.resources = [resource.clone()].into();
    }
    Prepared::prepare(spec)
        .map_err(|e| format!("{e:?}"))?
        .load()
        .map_err(|e| format!("{e:?}"))
}

fn run_acp(composition: &Composition, mut output: impl Write) -> Result<(), String> {
    use std::collections::BTreeMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    let mut initialized = false;
    let mut sessions: BTreeMap<String, Kernel> = BTreeMap::new();
    let mut offsets: BTreeMap<String, u64> = BTreeMap::new();
    let mut issued = 0_u64;

    for line in io::stdin().lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        let frame: serde_json::Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let id = frame.get("id").cloned().unwrap_or(serde_json::Value::Null);
        let method = frame.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let result: Result<serde_json::Value, (i32, String)> = (|| {
            match method {
                "initialize" => {
                    if initialized {
                        Err((-32600, "connection already initialized".into()))
                    } else {
                        initialized = true;
                        Ok(serde_json::json!({
                            "protocolVersion": 1,
                            "agentCapabilities": {
                                "loadSession": true,
                                "promptCapabilities": {
                                    "image": false,
                                    "audio": false,
                                    "embeddedContext": false
                                }
                            },
                            "agentInfo": {
                                "name": "phenix-basic",
                                "title": "Phenix Basic",
                                "version": "0.1.0"
                            },
                            "authMethods": []
                        }))
                    }
                }
                "session/new" if initialized => {
                    let cwd = frame.pointer("/params/cwd").and_then(|v| v.as_str());
                    let servers = frame
                        .pointer("/params/mcpServers")
                        .and_then(|v| v.as_array());
                    if cwd.is_none_or(|cwd| !Path::new(cwd).is_absolute()) {
                        Err((-32602, "absolute cwd required".into()))
                    } else if servers.is_none_or(|servers| !servers.is_empty()) {
                        Err((-32602, "MCP servers are not implemented".into()))
                    } else {
                        let stamp = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map_err(|e| (-32000, e.to_string()))?
                            .as_nanos();
                        issued += 1;
                        let session = format!("s{}_{}_{}", std::process::id(), stamp, issued);
                        let instance =
                            scoped_kernel(composition, &session).map_err(|e| (-32000, e))?;
                        sessions.insert(session.clone(), instance);
                        offsets.insert(session.clone(), 0);
                        Ok(serde_json::json!({"sessionId":session}))
                    }
                }
                "session/load" if initialized => {
                    let session = frame.pointer("/params/sessionId").and_then(|v| v.as_str());
                    let cwd = frame.pointer("/params/cwd").and_then(|v| v.as_str());
                    let servers = frame
                        .pointer("/params/mcpServers")
                        .and_then(|v| v.as_array());
                    if cwd.is_none_or(|cwd| !Path::new(cwd).is_absolute())
                        || servers.is_none_or(|servers| !servers.is_empty())
                    {
                        Err((-32602, "invalid ACP session load configuration".into()))
                    } else if let Some(session) = session {
                        if let Some(previous) = sessions.remove(session) {
                            previous
                                .shutdown()
                                .map_err(|e| (-32000, format!("{e:?}")))?;
                        }
                        let instance =
                            scoped_kernel(composition, session).map_err(|e| (-32000, e))?;
                        sessions.insert(session.to_string(), instance);
                        let current = sessions.get(session).expect("session admitted");
                        let offset = offsets.entry(session.to_string()).or_default();
                        *offset = 0;
                        render_new(
                            current,
                            "phenix.basic.frontend-acp",
                            session,
                            offset,
                            None,
                            &mut output,
                        )
                        .map_err(|e| (-32000, e))?;
                        Ok(serde_json::json!({}))
                    } else {
                        Err((-32602, "sessionId required".into()))
                    }
                }
                "session/prompt" if initialized => {
                    let session = frame.pointer("/params/sessionId").and_then(|v| v.as_str());
                    if let Some((session, kernel)) = session
                        .and_then(|session| sessions.get(session).map(|kernel| (session, kernel)))
                    {
                        let decoded = completed(invoke::<Submission>(
                            kernel,
                            "phenix.basic.frontend-acp",
                            "phenix.basic.frontend@1/decode",
                            ClientInput {
                                session: session.into(),
                                line,
                            },
                        ));
                        decoded
                            .and_then(|submission| {
                                submit(kernel, submission, basic_scope(composition, session))
                            })
                            .map_err(|e| (-32000, e))?;
                        let offset = offsets.entry(session.to_string()).or_default();
                        render_new(
                            kernel,
                            "phenix.basic.frontend-acp",
                            session,
                            offset,
                            None,
                            &mut output,
                        )
                        .map_err(|e| (-32000, e))?;
                        Ok(serde_json::json!({"stopReason":"end_turn"}))
                    } else {
                        Err((-32602, "unknown sessionId".into()))
                    }
                }
                "session/cancel" if initialized => {
                    // The synchronous model path cannot consume a cancel notification
                    // while blocked. The caller has no live pending graph handle.
                    // Do not claim successful cancellation.
                    if let Some(session) =
                        frame.pointer("/params/sessionId").and_then(|v| v.as_str())
                    {
                        if !sessions.contains_key(session) {
                            eprintln!("phenix-basic: unknown ACP cancellation session");
                        }
                    }
                    if id.is_null() {
                        Ok(serde_json::json!({}))
                    } else {
                        Err((-32600, "cancel must be a notification".into()))
                    }
                }
                _ => Err((-32601, "method unavailable in this Basic host".into())),
            }
        })();
        if id.is_null() {
            continue;
        }
        let reply = match result {
            Ok(value) => serde_json::json!({
                "jsonrpc":"2.0",
                "id":id,
                "result":value
            }),
            Err((code, message)) => serde_json::json!({
                "jsonrpc":"2.0",
                "id":id,
                "error":{"code":code,"message":message}
            }),
        };
        writeln!(output, "{reply}").map_err(|e| e.to_string())?;
        output.flush().map_err(|e| e.to_string())?;
    }
    for (_, kernel) in sessions {
        kernel.shutdown().map_err(|e| format!("{e:?}"))?;
    }
    Ok(())
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 || !["terminal", "jsonl", "acp"].contains(&args[1].as_str()) {
        return Err("usage: phenix-basic-runtime terminal|jsonl|acp COMPOSITION.json".into());
    }
    let path = Path::new(&args[2]);
    let config = std::fs::read(path).map_err(|e| e.to_string())?;
    let composition: Composition = serde_json::from_slice(&config).map_err(|e| e.to_string())?;
    if args[1] == "acp" {
        return run_acp(&composition, io::stdout().lock());
    }
    let frontend = match args[1].as_str() {
        "terminal" => "phenix.basic.frontend-terminal",
        "jsonl" => "phenix.basic.frontend-jsonl",
        _ => unreachable!(),
    };
    run_console(&composition, frontend, &args[1])
}
fn main() {
    if let Err(err) = run() {
        eprintln!("phenix-basic: {err}");
        std::process::exit(1);
    }
}
