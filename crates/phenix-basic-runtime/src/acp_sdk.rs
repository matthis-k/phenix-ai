//! SDK-backed ACP transport. The SDK owns JSON-RPC framing, dispatch and
//! session updates; the Basic controller owns Kernel, graph, sessions and
//! authority on one thread.
use super::*;
use acp::schema::{
    v1::{
        AgentCapabilities, CancelNotification, Implementation, InitializeRequest,
        InitializeResponse, LoadSessionRequest, LoadSessionResponse, NewSessionRequest,
        NewSessionResponse, PromptRequest, PromptResponse, SessionConfigOption,
        SessionConfigOptionCategory, SessionConfigOptionValue, SessionConfigSelectOption,
        SessionNotification, SetSessionConfigOptionRequest, SetSessionConfigOptionResponse,
        StopReason,
    },
    ProtocolVersion,
};
#[cfg(feature = "acp_v2_preview")]
use acp::ConnectTo;
use acp::{Agent, Client, ConnectionTo, Responder};
use agent_client_protocol as acp;
use futures::SinkExt;
use phenix_basic_contracts::ErrorCode;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{sync_channel, Receiver, SyncSender, TrySendError},
    Arc, Mutex,
};
use std::time::{SystemTime, UNIX_EPOCH};

type Reply<T> = Responder<T>;
type Peer = ConnectionTo<Client>;
type PendingPrompts = Mutex<VecDeque<(String, Arc<AtomicBool>)>>;

enum Command {
    Initialize(Box<InitializeRequest>, Reply<InitializeResponse>),
    New(NewSessionRequest, Reply<NewSessionResponse>),
    Load(LoadSessionRequest, Reply<LoadSessionResponse>, Peer),
    SetConfig(
        SetSessionConfigOptionRequest,
        Reply<SetSessionConfigOptionResponse>,
    ),
    Prompt(PromptRequest, Reply<PromptResponse>, Peer, Arc<AtomicBool>),
    Closed,
}

impl Command {
    fn reject(self, error: acp::Error) -> acp::Result<()> {
        match self {
            Self::Initialize(_, reply) => reply.respond_with_error(error),
            Self::New(_, reply) => reply.respond_with_error(error),
            Self::Load(_, reply, _) => reply.respond_with_error(error),
            Self::SetConfig(_, reply) => reply.respond_with_error(error),
            Self::Prompt(_, reply, _, _) => reply.respond_with_error(error),
            Self::Closed => Ok(()),
        }
    }
}

fn submit(sender: &SyncSender<Command>, cmd: Command) -> acp::Result<()> {
    match sender.try_send(cmd) {
        Ok(()) => Ok(()),
        Err(TrySendError::Full(cmd)) => {
            cmd.reject(acp::Error::internal_error().data("ACP controller request queue full"))
        }
        Err(TrySendError::Disconnected(cmd)) => {
            cmd.reject(acp::Error::internal_error().data("ACP controller disconnected"))
        }
    }
}

/// A cancellation targets the earliest admitted prompt for the session, even
/// before the controller starts its root. Unrelated sessions are unaffected.
fn signal_cancel(pending: &PendingPrompts, session: &str) -> acp::Result<()> {
    let queued = pending.lock().map_err(internal)?;
    if let Some((_, flag)) = queued.iter().find(|(owner, _)| owner == session) {
        flag.store(true, Ordering::Release);
    }
    Ok(())
}

fn internal(error: impl ToString) -> acp::Error {
    acp::Error::internal_error().data(error.to_string())
}

// Preserve the canonical BasicError code at the standard JSON-RPC error.data
// boundary. Clients can handle this exact typed value without inspecting
// provider text or matching unstable human-readable diagnostics.
fn internal_basic(problem: BasicError) -> acp::Error {
    acp::Error::internal_error().data(serde_json::json!({
        "format": "phenix.basic.error.v1",
        "code": problem.code,
        "message": problem.message,
    }))
}
fn internal_storage(reason: impl ToString) -> acp::Error {
    internal_basic(BasicError {
        code: ErrorCode::StorageFailed,
        message: reason.to_string(),
    })
}
fn invalid(message: &str) -> acp::Error {
    acp::Error::invalid_params().data(message)
}

/// The native Frontend returns serialized, typed v1 SessionNotification
/// payloads, not JSON-RPC frames. Only the SDK writes protocol envelopes.
fn send_updates(
    kernel: &Kernel,
    graph: Option<&GraphExecution<'_>>,
    session: &str,
    after: &mut u64,
    peer: &Peer,
    disconnected: &AtomicBool,
) -> acp::Result<()> {
    let mut encoded = Vec::new();
    render_new(
        kernel,
        graph,
        "phenix.basic.frontend-acp",
        session,
        after,
        None,
        &mut encoded,
    )
    .map_err(internal)?;
    for line in String::from_utf8(encoded).map_err(internal)?.lines() {
        let notification: SessionNotification = serde_json::from_str(line).map_err(internal)?;
        if peer.send_notification(notification).is_err() {
            // The client may vanish before SDK on_close runs. Never abandon
            // a live graph with a durable owner marker on a broken writer.
            // Treat the lost peer as disconnect; drive cancels and drains.
            disconnected.store(true, Ordering::Release);
            break;
        }
    }
    Ok(())
}

/// Bounded physical stdio adapter built from the SDK's Lines transport.
/// The Phenix reader enforces only byte, UTF-8 and FIFO limits; the official
/// SDK owns every ACP/JSON-RPC envelope and protocol operation.
fn bounded_transport(
    rejected: Arc<AtomicBool>,
    disconnected: Arc<AtomicBool>,
) -> impl acp::ConnectTo<Agent> {
    let (mut incoming_sender, incoming_receiver) =
        futures::channel::mpsc::channel::<std::io::Result<String>>(64);
    std::thread::spawn(move || {
        let mut input = std::io::stdin().lock();
        loop {
            match bounded_input_line(&mut input) {
                Ok(Some(line)) => {
                    if futures::executor::block_on(incoming_sender.send(Ok(line))).is_err() {
                        break;
                    }
                }
                Ok(None) => {
                    // EOF can arrive while the SDK still holds a pending
                    // prompt responder. Inform the kernel-owning controller
                    // now rather than waiting for protocol shutdown.
                    disconnected.store(true, Ordering::Release);
                    break;
                }
                Err(error) => {
                    rejected.store(true, Ordering::Release);
                    disconnected.store(true, Ordering::Release);
                    // Preserve the physical transport error for the SDK,
                    // but cancellation must not wait on a full input queue.
                    let _ = incoming_sender.try_send(Err(error));
                    break;
                }
            }
        }
    });
    let outgoing = futures::sink::unfold(
        blocking::Unblock::new(std::io::stdout()),
        |mut output, line: String| async move {
            use futures::AsyncWriteExt;
            output.write_all(line.as_bytes()).await?;
            output.write_all(b"\n").await?;
            output.flush().await?;
            Ok::<_, std::io::Error>(output)
        },
    );
    acp::Lines::new(outgoing, incoming_receiver)
}

struct Controller {
    composition: Composition,
    sessions: BTreeMap<String, Kernel>,
    offsets: BTreeMap<String, u64>,
    initialized: bool,
    issued: u64,
    queued: Arc<PendingPrompts>,
    closed: Arc<AtomicBool>,
}

impl Controller {
    fn start(
        composition: Composition,
        queued: Arc<PendingPrompts>,
        closed: Arc<AtomicBool>,
    ) -> Self {
        Self {
            composition,
            sessions: BTreeMap::new(),
            offsets: BTreeMap::new(),
            initialized: false,
            issued: 0,
            queued,
            closed,
        }
    }

    fn dispatch(&mut self, command: Command) {
        match command {
            Command::Initialize(request, reply) => {
                let result = if self.initialized {
                    Err(invalid("connection already initialized"))
                } else if request.protocol_version == ProtocolVersion::V0 {
                    // V0 is the pre-release protocol. Keep the connection
                    // uninitialized so the client can retry with stable v1.
                    Err(invalid("unsupported ACP protocol version"))
                } else {
                    // This handler is v1-only. A higher client version may
                    // negotiate down to stable v1, never silently activate
                    // the SDK's separate draft-v2 lifecycle.
                    self.initialized = true;
                    Ok(InitializeResponse::new(ProtocolVersion::V1)
                        .agent_capabilities(AgentCapabilities::new().load_session(true))
                        .agent_info(
                            Implementation::new("phenix-basic", "0.1.0").title("Phenix Basic"),
                        ))
                };
                let _ = reply.respond_with_result(result);
            }
            Command::New(request, reply) => {
                let result = if !self.initialized {
                    Err(invalid("initialize required"))
                } else {
                    self.new_session(request)
                };
                let _ = reply.respond_with_result(result);
            }
            Command::Load(request, reply, peer) => {
                let result = if !self.initialized {
                    Err(invalid("initialize required"))
                } else {
                    self.load_session(request, &peer)
                };
                let _ = reply.respond_with_result(result);
            }
            Command::SetConfig(request, reply) => {
                let result = if !self.initialized {
                    Err(invalid("initialize required"))
                } else {
                    self.set_config_option(request)
                };
                let _ = reply.respond_with_result(result);
            }
            Command::Prompt(request, reply, peer, flag) => {
                let result = if !self.initialized {
                    Err(invalid("initialize required"))
                } else {
                    self.prompt(request, &peer, &flag)
                };
                if let Ok(mut pending) = self.queued.lock() {
                    pending.retain(|(_, current)| !Arc::ptr_eq(current, &flag));
                }
                let _ = reply.respond_with_result(result);
            }
            Command::Closed => self.closed.store(true, Ordering::Release),
        }
    }

    /// The selected Model is an installed composition binding, not client configuration.
    /// ACP's standard session config options let any frontend render the effective
    /// model, even while this P1 composition has only one eligible binding.
    fn model_options(&self) -> Vec<SessionConfigOption> {
        self.composition
            .slots
            .iter()
            .filter(|binding| {
                binding.caller == "phenix.basic.agent"
                    && binding.slot == "Model"
                    && binding.index == 0
            })
            .map(|binding| {
                SessionConfigOption::select(
                    "model",
                    "Model",
                    binding.provider.clone(),
                    vec![SessionConfigSelectOption::new(
                        binding.provider.clone(),
                        binding.provider.clone(),
                    )],
                )
                .category(SessionConfigOptionCategory::Model)
            })
            .collect()
    }

    fn set_config_option(
        &mut self,
        request: SetSessionConfigOptionRequest,
    ) -> acp::Result<SetSessionConfigOptionResponse> {
        if !self.sessions.contains_key(&request.session_id.to_string()) {
            return Err(invalid("unknown ACP session"));
        }
        let chosen = match request.value {
            SessionConfigOptionValue::ValueId { value } => value.to_string(),
            _ => return Err(invalid("unsupported model configuration value")),
        };
        let admitted = self.composition.slots.iter().any(|binding| {
            binding.caller == "phenix.basic.agent"
                && binding.slot == "Model"
                && binding.index == 0
                && binding.provider == chosen
        });
        if request.config_id.to_string() != "model" || !admitted {
            return Err(invalid("unavailable or unauthorized model"));
        }
        // Until dynamic graph routing is installed, selection of the
        // already admitted model is idempotent. A different model is denied.
        Ok(SetSessionConfigOptionResponse::new(self.model_options()))
    }

    fn new_session(&mut self, request: NewSessionRequest) -> acp::Result<NewSessionResponse> {
        if !request.cwd.is_absolute()
            || !request.mcp_servers.is_empty()
            || !request.additional_directories.is_empty()
        {
            return Err(invalid("unsupported ACP session configuration"));
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(internal)?
            .as_nanos();
        self.issued = self
            .issued
            .checked_add(1)
            .ok_or_else(|| internal("session ID overflow"))?;
        let session = format!("s{}_{}_{}", std::process::id(), stamp, self.issued);
        let kernel = scoped_kernel(&self.composition, &session).map_err(internal)?;
        self.sessions.insert(session.clone(), kernel);
        self.offsets.insert(session.clone(), 0);
        Ok(NewSessionResponse::new(session).config_options(self.model_options()))
    }

    fn load_session(
        &mut self,
        request: LoadSessionRequest,
        peer: &Peer,
    ) -> acp::Result<LoadSessionResponse> {
        if !request.cwd.is_absolute()
            || !request.mcp_servers.is_empty()
            || !request.additional_directories.is_empty()
        {
            return Err(invalid("unsupported ACP session load configuration"));
        }
        let session = request.session_id.to_string();
        phenix_basic_contracts::session_resource(&session)
            .map_err(|_| invalid("invalid ACP session ID"))?;
        let kernel = scoped_kernel(&self.composition, &session).map_err(internal)?;
        let snapshot: SessionSnapshot = match invoke(
            &kernel,
            "phenix.basic.session",
            "phenix.basic.session@1/open",
            Scope {
                session: session.clone(),
            },
        ) {
            Ok(Ok(snapshot)) => snapshot,
            Ok(Err(problem)) => return Err(internal_basic(problem)),
            Err(failure) => return Err(internal(failure)),
        };
        if snapshot.revision == 0 && !self.sessions.contains_key(&session) {
            kernel.shutdown().map_err(internal)?;
            return Err(invalid("unknown persisted ACP session"));
        }
        if let Some(previous) = self.sessions.remove(&session) {
            previous.shutdown().map_err(internal)?;
        }
        self.sessions.insert(session.clone(), kernel);
        let offset = self.offsets.entry(session.clone()).or_default();
        *offset = 0;
        let loaded = self.sessions.get(&session).expect("just inserted");
        send_updates(loaded, None, &session, offset, peer, &self.closed)?;
        Ok(LoadSessionResponse::new().config_options(self.model_options()))
    }

    fn prompt(
        &mut self,
        request: PromptRequest,
        peer: &Peer,
        cancelled: &AtomicBool,
    ) -> acp::Result<PromptResponse> {
        let session = request.session_id.to_string();
        let Some(kernel) = self.sessions.get(&session) else {
            return Err(invalid("unknown ACP session"));
        };
        // The SDK has already decoded the JSON-RPC envelope into a typed
        // PromptRequest. The native frontend maps only that schema payload
        // into Phenix's typed Submission action.
        let submission: Submission = invoke(
            kernel,
            "phenix.basic.frontend-acp",
            "phenix.basic.frontend@1/decode",
            ClientInput {
                session: session.clone(),
                line: serde_json::json!({"prompt": request}).to_string(),
            },
        )
        .map_err(internal)?
        .map_err(|cause| {
            if cause.code == ErrorCode::InvalidInput {
                invalid(&cause.message)
            } else {
                internal_basic(cause)
            }
        })?;
        let mut owner = session_lock(&session).map_err(internal_storage)?;
        recover_owned_run(kernel, &session, &mut owner).map_err(internal_storage)?;
        let execution =
            begin_submission(kernel, submission, basic_scope(&self.composition, &session))
                .map_err(internal)?;
        let offset = self.offsets.entry(session.clone()).or_default();
        Self::drive(
            kernel,
            execution,
            &session,
            &mut owner,
            offset,
            peer,
            cancelled,
            &self.closed,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn drive(
        kernel: &Kernel,
        mut execution: GraphExecution<'_>,
        session: &str,
        owner: &mut std::fs::File,
        after: &mut u64,
        peer: &Peer,
        cancel_flag: &AtomicBool,
        closed: &AtomicBool,
    ) -> acp::Result<PromptResponse> {
        let mut marked = false;
        let mut stopping = false;
        loop {
            if !stopping
                && (closed.load(Ordering::Acquire) || cancel_flag.swap(false, Ordering::AcqRel))
            {
                execution.cancel().map_err(internal)?;
                stopping = true;
            }
            let status = execution.pump(|_| {}).map_err(internal)?;
            if !marked {
                marked = mark_admitted_run(kernel, &execution, session, owner).map_err(internal)?;
            }
            if !closed.load(Ordering::Acquire) {
                send_updates(kernel, Some(&execution), session, after, peer, closed)?;
            }
            if status == GraphStatus::Finished {
                let cancelled = finish_submission(&execution).map_err(internal)?;
                drop(execution);
                if cancelled {
                    record_cancelled(kernel, session).map_err(internal)?;
                    if !closed.load(Ordering::Acquire) {
                        send_updates(kernel, None, session, after, peer, closed)?;
                    }
                }
                set_owner_marker(owner, None).map_err(internal)?;
                return Ok(PromptResponse::new(if cancelled {
                    StopReason::Cancelled
                } else {
                    StopReason::EndTurn
                }));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn serve(mut self, receiver: Receiver<Command>) -> Result<(), String> {
        while !self.closed.load(Ordering::Acquire) {
            match receiver.recv_timeout(Duration::from_millis(20)) {
                Ok(command) => self.dispatch(command),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        for (_, kernel) in self.sessions {
            kernel.shutdown().map_err(|e| format!("{e:?}"))?;
        }
        Ok(())
    }
}

pub(super) fn run(composition: &Composition) -> Result<(), String> {
    let (tx, receiver) = sync_channel::<Command>(64);
    let closed = Arc::new(AtomicBool::new(false));
    // A prompt cancellation flag is registered before admission, closing
    // the race between queued requests and the first graph pump.
    let queued: Arc<PendingPrompts> = Arc::new(Mutex::new(VecDeque::new()));
    let controller = Controller::start(
        composition.clone(),
        Arc::clone(&queued),
        Arc::clone(&closed),
    );
    // Kernel is deliberately !Send. Keep the controller on the caller
    // thread and move only the SDK wire dispatcher to a worker.

    let initialize = tx.clone();
    let new = tx.clone();
    let load = tx.clone();
    let set_config = tx.clone();
    let prompt = tx.clone();
    let on_close = tx.clone();
    let pending_prompts = Arc::clone(&queued);
    let cancel_pending = Arc::clone(&queued);
    let close_flag = Arc::clone(&closed);
    let connection = Agent
        .builder()
        .name("phenix-basic")
        .on_receive_request(
            async move |request: InitializeRequest, reply, _peer| {
                submit(&initialize, Command::Initialize(Box::new(request), reply))
            },
            acp::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: NewSessionRequest, reply, _peer| {
                submit(&new, Command::New(request, reply))
            },
            acp::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: LoadSessionRequest, reply, peer| {
                submit(&load, Command::Load(request, reply, peer))
            },
            acp::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: SetSessionConfigOptionRequest, reply, _peer| {
                submit(&set_config, Command::SetConfig(request, reply))
            },
            acp::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: PromptRequest, reply, peer| {
                let flag = Arc::new(AtomicBool::new(false));
                {
                    let mut pending = pending_prompts.lock().map_err(internal)?;
                    pending.push_back((request.session_id.to_string(), Arc::clone(&flag)));
                }
                match prompt.try_send(Command::Prompt(request, reply, peer, Arc::clone(&flag))) {
                    Ok(()) => Ok(()),
                    Err(problem) => {
                        pending_prompts
                            .lock()
                            .map_err(internal)?
                            .retain(|(_, current)| !Arc::ptr_eq(current, &flag));
                        match problem {
                            TrySendError::Full(command) => {
                                command.reject(internal("ACP controller request queue full"))
                            }
                            TrySendError::Disconnected(command) => {
                                command.reject(internal("ACP controller disconnected"))
                            }
                        }
                    }
                }
            },
            acp::on_receive_request!(),
        )
        .on_receive_notification(
            async move |notice: CancelNotification, _peer| {
                signal_cancel(&cancel_pending, &notice.session_id.to_string())
            },
            acp::on_receive_notification!(),
        )
        .on_close(move |_peer| async move {
            close_flag.store(true, Ordering::Release);
            let _ = on_close.try_send(Command::Closed);
            Ok(())
        });
    // The official router is deliberately opt-in while R1-ACP5b is incomplete.
    // This preview accepts v2 initialization without advertising sessions or
    // exposing a partial v2 prompt lifecycle. Production keeps the tested
    // stable-v1 host and its unchanged protocol-version rejection behavior.
    #[cfg(feature = "acp_v2_preview")]
    let connection = Agent.protocol_router().with_v1(connection).with_v2(
        Agent
            .v2()
            .name("phenix-basic-v2-preview")
            .on_receive_request(
                async |request: acp::schema::v2::InitializeRequest,
                       reply: Responder<acp::schema::v2::InitializeResponse>,
                       _peer: acp::V2ConnectionTo<Client>| {
                    if request.protocol_version != ProtocolVersion::V2 {
                        return reply.respond_with_error(invalid("unsupported ACP v2 version"));
                    }
                    reply.respond(acp::schema::v2::InitializeResponse::new(
                        ProtocolVersion::V2,
                        acp::schema::v2::Implementation::new("phenix-basic", "0.1.0"),
                    ))
                },
                acp::on_receive_request!(),
            ),
    );

    let rejected = Arc::new(AtomicBool::new(false));
    let transport_rejected = Arc::clone(&rejected);
    let transport_closed = Arc::clone(&closed);
    let wire = std::thread::spawn(move || {
        let result = futures::executor::block_on(connection.connect_to(bounded_transport(
            transport_rejected,
            Arc::clone(&transport_closed),
        )))
        .map_err(|e| format!("{e:?}"));
        // A transport error does not always invoke the SDK's on_close hook.
        // Wake the single-thread kernel controller on EOF or a bad frame.
        transport_closed.store(true, Ordering::Release);
        result
    });
    let controller_result = controller.serve(receiver);
    closed.store(true, Ordering::Release);
    drop(tx);
    let transport_result = wire
        .join()
        .map_err(|_| "ACP transport panicked".to_string())?;
    controller_result?;
    transport_result?;
    if rejected.load(Ordering::Acquire) {
        return Err("ACP stdio frame rejected".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_cancel_targets_first_matching_session() {
        let oldest = Arc::new(AtomicBool::new(false));
        let another = Arc::new(AtomicBool::new(false));
        let second = Arc::new(AtomicBool::new(false));
        let queue = Mutex::new(VecDeque::from([
            ("s1".to_string(), Arc::clone(&oldest)),
            ("s2".to_string(), Arc::clone(&another)),
            ("s1".to_string(), Arc::clone(&second)),
        ]));
        signal_cancel(&queue, "unknown").unwrap();
        assert!(!oldest.load(Ordering::Acquire));
        signal_cancel(&queue, "s1").unwrap();
        assert!(oldest.load(Ordering::Acquire));
        assert!(!another.load(Ordering::Acquire));
        assert!(!second.load(Ordering::Acquire));
        queue.lock().unwrap().pop_front();
        signal_cancel(&queue, "s1").unwrap();
        assert!(second.load(Ordering::Acquire));
        assert!(!another.load(Ordering::Acquire));
    }
}
