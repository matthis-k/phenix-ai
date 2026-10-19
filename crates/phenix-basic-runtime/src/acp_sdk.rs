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
use phenix_provider_contracts::{
    AuthQuery, AuthenticationStatus, CredentialIdentity, CredentialImport, CredentialSecret,
    ModelPage, ModelQuery, ProviderError, ProviderPage, ProviderQuery,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{sync_channel, Receiver, SyncSender, TrySendError},
    Arc, Mutex,
};
use std::time::{SystemTime, UNIX_EPOCH};

type Reply<T> = Responder<T>;
type Peer = ConnectionTo<Client>;
type PendingPrompts = Mutex<VecDeque<(String, Arc<AtomicBool>)>>;

/// ACP v1 extension request: the official SDK owns its JSON-RPC dispatch.
/// The separate launcher-owned native kernel grant is the only authority.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, acp::JsonRpcRequest)]
#[request(method = "_phenix/providers/list", response = serde_json::Value)]
#[serde(deny_unknown_fields)]
struct ProviderListRequest {
    scope: String,
}

/// Read-only provider queries retain exact provider/connection/profile
/// selectors, while the controller fixes the resource and grants.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, acp::JsonRpcRequest)]
#[request(method = "_phenix/providers/auth/status", response = serde_json::Value)]
#[serde(deny_unknown_fields)]
struct ProviderAuthStatusRequest {
    scope: String,
    provider_id: String,
    connection_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, acp::JsonRpcRequest)]
#[request(method = "_phenix/providers/models", response = serde_json::Value)]
#[serde(deny_unknown_fields)]
struct ProviderModelsRequest {
    scope: String,
    provider_id: String,
    connection_id: String,
    profile_id: Option<String>,
    cursor: Option<String>,
    limit: u64,
}

/// Secret-bearing ACP extension. The SDK owns framing; the trusted launcher
/// fixes the only credential resource this connection may modify.
#[derive(Clone, serde::Serialize, serde::Deserialize, acp::JsonRpcRequest)]
#[request(method = "_phenix/credentials/import", response = serde_json::Value)]
#[serde(deny_unknown_fields)]
struct CredentialImportRequest {
    provider_id: String,
    connection_id: String,
    profile_id: String,
    secret: String,
}

impl std::fmt::Debug for CredentialImportRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialImportRequest")
            .field("provider_id", &self.provider_id)
            .field("connection_id", &self.connection_id)
            .field("profile_id", &self.profile_id)
            .field("secret", &"[REDACTED]")
            .finish()
    }
}

impl Drop for CredentialImportRequest {
    fn drop(&mut self) {
        std::mem::take(&mut self.secret).into_bytes().fill(0);
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, acp::JsonRpcRequest)]
#[request(method = "_phenix/credentials/revoke", response = serde_json::Value)]
#[serde(deny_unknown_fields)]
struct CredentialRevokeRequest {
    provider_id: String,
    connection_id: String,
    profile_id: String,
}

impl CredentialRevokeRequest {
    fn identity(&self) -> CredentialIdentity {
        CredentialIdentity {
            provider_id: self.provider_id.clone(),
            connection_id: self.connection_id.clone(),
            profile_id: self.profile_id.clone(),
        }
    }
}

enum Command {
    Initialize(Box<InitializeRequest>, Reply<InitializeResponse>),
    New(NewSessionRequest, Reply<NewSessionResponse>),
    Load(LoadSessionRequest, Reply<LoadSessionResponse>, Peer),
    SetConfig(
        SetSessionConfigOptionRequest,
        Reply<SetSessionConfigOptionResponse>,
    ),
    ProviderList(ProviderListRequest, Reply<serde_json::Value>),
    ProviderAuthStatus(ProviderAuthStatusRequest, Reply<serde_json::Value>),
    ProviderModels(ProviderModelsRequest, Reply<serde_json::Value>),
    CredentialImport(CredentialImportRequest, Reply<serde_json::Value>),
    CredentialRevoke(CredentialRevokeRequest, Reply<serde_json::Value>),
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
            Self::ProviderList(_, reply) => reply.respond_with_error(error),
            Self::ProviderAuthStatus(_, reply) => reply.respond_with_error(error),
            Self::ProviderModels(_, reply) => reply.respond_with_error(error),
            Self::CredentialImport(_, reply) => reply.respond_with_error(error),
            Self::CredentialRevoke(_, reply) => reply.respond_with_error(error),
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

struct ProviderManagement {
    kernel: Kernel,
    selected: Selection,
}

struct CredentialManagement {
    kernel: Kernel,
    importer: Selection,
    resource: String,
}

/// The credential composition is a separate control-plane grant, never an
/// operation from the ordinary Basic session or provider-list root.
fn prepare_credentials(spec: &Composition) -> Result<CredentialManagement, String> {
    const CONTRACT: &str = "phenix.provider.credentials@1";
    let resource = spec
        .resource
        .as_deref()
        .filter(|value| value.starts_with("phenix.credentials."))
        .ok_or("credential management requires one explicit credential resource")?;
    let operations = std::collections::BTreeSet::from([
        format!("{CONTRACT}/import"),
        format!("{CONTRACT}/revoke"),
    ]);
    if spec.entry.interface != CONTRACT
        || spec.entry.operation != "import"
        || spec.authority.operations != operations
        || spec.authority.resources != std::collections::BTreeSet::from([resource.to_string()])
        || spec.grants.len() != 2
        || !spec.slots.is_empty()
        || !spec.excluded.is_empty()
        || !spec.substitutions.is_empty()
    {
        return Err("credential management composition has unexpected authority".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for grant in &spec.grants {
        if grant.caller != "root"
            || grant.interface != CONTRACT
            || !["import", "revoke"].contains(&grant.operation.as_str())
            || grant.resources != std::collections::BTreeSet::from([resource.to_string()])
            || !seen.insert(grant.operation.as_str())
        {
            return Err("credential management requires exact root grants".into());
        }
    }
    if seen.len() != 2 {
        return Err("credential management requires import and revoke grants".into());
    }
    let kernel = Prepared::prepare(spec.clone())
        .map_err(|_| "invalid credential management composition".to_string())?
        .load()
        .map_err(|_| "cannot load credential management composition".to_string())?;
    Ok(CredentialManagement {
        kernel,
        importer: spec.entry.clone(),
        resource: resource.into(),
    })
}

struct Controller {
    composition: Composition,
    management: Option<ProviderManagement>,
    credentials: Option<CredentialManagement>,
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
        management: Option<ProviderManagement>,
        credentials: Option<CredentialManagement>,
        queued: Arc<PendingPrompts>,
        closed: Arc<AtomicBool>,
    ) -> Self {
        Self {
            composition,
            management,
            credentials,
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
            Command::ProviderList(request, reply) => {
                let result = if !self.initialized {
                    Err(invalid("initialize required"))
                } else {
                    self.provider_list(request)
                };
                let _ = reply.respond_with_result(result);
            }
            Command::ProviderAuthStatus(request, reply) => {
                let result = if !self.initialized {
                    Err(invalid("initialize required"))
                } else {
                    self.provider_auth_status(request)
                };
                let _ = reply.respond_with_result(result);
            }
            Command::ProviderModels(request, reply) => {
                let result = if !self.initialized {
                    Err(invalid("initialize required"))
                } else {
                    self.provider_models(request)
                };
                let _ = reply.respond_with_result(result);
            }
            Command::CredentialImport(request, reply) => {
                let result = if !self.initialized || self.closed.load(Ordering::Acquire) {
                    Err(invalid("active initialized ACP connection required"))
                } else {
                    self.credential_import(request)
                };
                let _ = reply.respond_with_result(result);
            }
            Command::CredentialRevoke(request, reply) => {
                let result = if !self.initialized || self.closed.load(Ordering::Acquire) {
                    Err(invalid("active initialized ACP connection required"))
                } else {
                    self.credential_revoke(request)
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

    // The launcher preselects provider implementation and grants. These
    // operations can never infer model entitlement from a catalog entry.
    fn provider_read<Q: Portable, R: Portable + serde::Serialize>(
        &self,
        interface: &str,
        operation: &str,
        query: &Q,
    ) -> acp::Result<serde_json::Value> {
        let management = self
            .management
            .as_ref()
            .ok_or_else(acp::Error::method_not_found)?;
        let selection = Selection {
            interface: interface.into(),
            operation: operation.into(),
            provider: management.selected.provider.clone(),
        };
        let root = management.kernel.admit_root(selection).map_err(internal)?;
        let value = match root.invoke(&query.to_value()).map_err(internal)? {
            Outcome::Success(value) => value,
            Outcome::Domain(value) => {
                let problem = ProviderError::from_value(value).map_err(internal)?;
                return Err(acp::Error::invalid_params().data(serde_json::json!({
                    "format": "phenix.provider.error.v1",
                    "code": problem.code,
                })));
            }
        };
        let result = R::from_value(value).map_err(internal)?;
        serde_json::to_value(result).map_err(internal)
    }

    fn provider_list(&self, request: ProviderListRequest) -> acp::Result<serde_json::Value> {
        if request.scope != "user" {
            return Err(invalid("provider query must request user scope only"));
        }
        self.provider_read::<_, ProviderPage>(
            "phenix.provider.catalog@1",
            "list",
            &ProviderQuery {
                scope: request.scope,
            },
        )
    }

    fn provider_auth_status(
        &self,
        request: ProviderAuthStatusRequest,
    ) -> acp::Result<serde_json::Value> {
        if request.scope != "user"
            || request.provider_id.is_empty()
            || request.connection_id.is_empty()
        {
            return Err(invalid("invalid provider authentication query"));
        }
        self.provider_read::<_, AuthenticationStatus>(
            "phenix.provider.auth@1",
            "status",
            &AuthQuery {
                scope: request.scope,
                provider_id: request.provider_id,
                connection_id: request.connection_id,
            },
        )
    }

    fn provider_models(&self, request: ProviderModelsRequest) -> acp::Result<serde_json::Value> {
        if request.scope != "user"
            || request.provider_id.is_empty()
            || request.connection_id.is_empty()
            || request.limit == 0
            || request.limit > 256
            || request
                .cursor
                .as_ref()
                .is_some_and(|cursor| cursor.len() > 1024)
        {
            return Err(invalid("invalid provider model query"));
        }
        self.provider_read::<_, ModelPage>(
            "phenix.provider.catalog@1",
            "models",
            &ModelQuery {
                scope: request.scope,
                provider_id: request.provider_id,
                connection_id: request.connection_id,
                profile_id: request.profile_id,
                cursor: request.cursor,
                limit: request.limit,
            },
        )
    }

    fn credential_operation(
        &self,
        operation: &str,
        identity: &CredentialIdentity,
        value: &Value,
    ) -> acp::Result<serde_json::Value> {
        let management = self
            .credentials
            .as_ref()
            .ok_or_else(acp::Error::method_not_found)?;
        // Identity is an untrusted selector. It never widens the fixed
        // launcher-owned credential resource or its root grants.
        if identity.resource().ok().as_deref() != Some(management.resource.as_str()) {
            return Err(invalid("credential identity is not authorized"));
        }
        let selected = Selection {
            operation: operation.into(),
            ..management.importer.clone()
        };
        let root = management
            .kernel
            .admit_root(selected)
            .map_err(|_| internal("credential operation not admitted"))?;
        match root
            .invoke(value)
            .map_err(|_| internal("credential operation failed"))?
        {
            Outcome::Success(_) => Ok(serde_json::json!({ "ok": true })),
            Outcome::Domain(value) => {
                let error = ProviderError::from_value(value)
                    .map_err(|_| internal("credential operation failed"))?;
                Err(acp::Error::invalid_params().data(serde_json::json!({
                    "format": "phenix.provider.error.v1",
                    "code": error.code,
                })))
            }
        }
    }

    fn credential_import(
        &self,
        mut request: CredentialImportRequest,
    ) -> acp::Result<serde_json::Value> {
        let identity = CredentialIdentity {
            provider_id: request.provider_id.clone(),
            connection_id: request.connection_id.clone(),
            profile_id: request.profile_id.clone(),
        };
        // Check the identity before allocating/copying the supplied secret.
        let management = self
            .credentials
            .as_ref()
            .ok_or_else(acp::Error::method_not_found)?;
        if identity.resource().ok().as_deref() != Some(management.resource.as_str()) {
            return Err(invalid("credential identity is not authorized"));
        }
        let secret = CredentialSecret::new(std::mem::take(&mut request.secret).into_bytes())
            .map_err(|_| invalid("invalid credential secret"))?;
        let payload = CredentialImport {
            identity: identity.clone(),
            secret,
        };
        let mut value = payload.to_value();
        let response = self.credential_operation("import", &identity, &value);
        // The canonical Value cannot own CredentialSecret's Drop behavior.
        // Overwrite its serialized payload copy even on domain or host denial.
        if let Value::Map(fields) = &mut value {
            if let Some(Value::Bytes(bytes)) = fields.get_mut("secret") {
                bytes.fill(0);
            }
        }
        response
    }

    fn credential_revoke(
        &self,
        request: CredentialRevokeRequest,
    ) -> acp::Result<serde_json::Value> {
        let identity = request.identity();
        self.credential_operation("revoke", &identity, &identity.to_value())
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
        if let Some(management) = self.management {
            management.kernel.shutdown().map_err(|e| format!("{e:?}"))?;
        }
        if let Some(credentials) = self.credentials {
            credentials
                .kernel
                .shutdown()
                .map_err(|_| "credential cleanup failed")?;
        }
        Ok(())
    }
}

pub(super) fn run(
    composition: &Composition,
    management: Option<&Composition>,
    credentials: Option<&Composition>,
) -> Result<(), String> {
    let management = management
        .map(|spec| -> Result<ProviderManagement, String> {
            if spec.entry.interface != "phenix.provider.catalog@1" || spec.entry.operation != "list"
            {
                return Err("ACP management composition must select provider catalog list".into());
            }
            let selected = spec.entry.clone();
            let kernel = Prepared::prepare(spec.clone())
                .map_err(|e| format!("{e:?}"))?
                .load()
                .map_err(|e| format!("{e:?}"))?;
            Ok(ProviderManagement { kernel, selected })
        })
        .transpose()?;
    let credentials = credentials.map(prepare_credentials).transpose()?;
    let (tx, receiver) = sync_channel::<Command>(64);
    let closed = Arc::new(AtomicBool::new(false));
    // A prompt cancellation flag is registered before admission, closing
    // the race between queued requests and the first graph pump.
    let queued: Arc<PendingPrompts> = Arc::new(Mutex::new(VecDeque::new()));
    let controller = Controller::start(
        composition.clone(),
        management,
        credentials,
        Arc::clone(&queued),
        Arc::clone(&closed),
    );
    // Kernel is deliberately !Send. Keep the controller on the caller
    // thread and move only the SDK wire dispatcher to a worker.

    let initialize = tx.clone();
    let new = tx.clone();
    let load = tx.clone();
    let set_config = tx.clone();
    let provider_list = tx.clone();
    let provider_auth_status = tx.clone();
    let provider_models = tx.clone();
    let credential_import = tx.clone();
    let credential_revoke = tx.clone();
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
            async move |request: ProviderListRequest, reply, _peer| {
                submit(&provider_list, Command::ProviderList(request, reply))
            },
            acp::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ProviderAuthStatusRequest, reply, _peer| {
                submit(
                    &provider_auth_status,
                    Command::ProviderAuthStatus(request, reply),
                )
            },
            acp::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ProviderModelsRequest, reply, _peer| {
                submit(&provider_models, Command::ProviderModels(request, reply))
            },
            acp::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: CredentialImportRequest, reply, _peer| {
                submit(
                    &credential_import,
                    Command::CredentialImport(request, reply),
                )
            },
            acp::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: CredentialRevokeRequest, reply, _peer| {
                submit(
                    &credential_revoke,
                    Command::CredentialRevoke(request, reply),
                )
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
