//! Terminal and JSONL Basic product host. Plugins perform domain work;
//! only this frontend executable reads stdin and submits graph actions.
use phenix_basic_contracts::{
    BasicError, ClientInput, EventPage, JournalQuery, ObserverCheckpoint, ObserverRequest,
    RenderRequest, Submission,
};
use phenix_contract::{Portable, ScopedRequest, Value};
use phenix_kernel::{
    graph::GraphStatus,
    resolve::{Composition, Prepared, Selection},
    Kernel, Outcome,
};
use std::{
    env,
    io::{self, BufRead, Write},
    path::Path,
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
            "phenix.basic.agent",
            "phenix.basic.output@1/read",
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
                "phenix.basic.agent",
                "phenix.basic.output@1/checkpoint_observer",
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
    if input.cancel {
        // The controller must retain an active GraphExecution for cancellation.
        // This synchronous input loop cannot admit a second control packet yet.
        return Err("no active pending graph execution to cancel".into());
    }
    if input.action != "phenix.basic.submit" {
        return Err("frontend selected an unknown Basic action".into());
    }
    let value = Value::decode(&input.payload.0).map_err(|e| format!("{e:?}"))?;
    let mut execution = kernel
        .begin_action_scoped(&input.action, &value, scope)
        .map_err(|e| format!("{e:?}"))?;
    while let GraphStatus::Pending | GraphStatus::Draining =
        execution.pump(|_| {}).map_err(|e| format!("{e:?}"))?
    {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let steps = execution.steps();
    if steps.is_empty() {
        return Err("prompt action did not activate a graph node".into());
    }
    for step in steps {
        if let Outcome::Domain(value) = &step.outcome {
            let cause = BasicError::from_value(value.clone()).map_err(|e| format!("{e:?}"))?;
            return Err(format!("{:?}: {}", cause.code, cause.message));
        }
    }
    Ok(())
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
    let kernel = Prepared::prepare(composition.clone())
        .map_err(|e| format!("{e:?}"))?
        .load()
        .map_err(|e| format!("{e:?}"))?;
    let frontend = match args[1].as_str() {
        "terminal" => "phenix.basic.frontend-terminal",
        "jsonl" => "phenix.basic.frontend-jsonl",
        "acp" => "phenix.basic.frontend-acp",
        _ => unreachable!(),
    };
    // The first product fixture binds one authenticated session. Later
    // frontends will ask the controller to construct a scope per session.
    let session = "s1";
    let mut observer: ObserverCheckpoint = completed(invoke(
        &kernel,
        "phenix.basic.agent",
        "phenix.basic.output@1/observer",
        ObserverRequest {
            session: session.into(),
            observer: args[1].clone(),
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
    for line in io::stdin().lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        let decoded = completed(invoke::<Submission>(
            &kernel,
            frontend,
            "phenix.basic.frontend@1/decode",
            ClientInput {
                session: session.into(),
                line,
            },
        ))?;
        // A domain failure is returned to the caller rather than forging
        // a successful answer. Persisted failure events can still be replayed.
        let result = submit(&kernel, decoded, basic_scope(&composition, session));
        render_new(
            &kernel,
            frontend,
            session,
            &mut after,
            Some(&mut observer),
            &mut stdout,
        )?;
        result?;
    }
    kernel.shutdown().map_err(|e| format!("{e:?}"))
}
fn main() {
    if let Err(err) = run() {
        eprintln!("phenix-basic: {err}");
        std::process::exit(1);
    }
}
