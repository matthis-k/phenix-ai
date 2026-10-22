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
    bind_http_model, validate_model_selection, AuthAttemptQuery, AuthAttemptState, AuthChallenge,
    AuthQuery, AuthStart, AuthenticationKind, AuthenticationStatus, CredentialIdentity,
    CredentialImport, CredentialSecret, HttpModelBinding, HttpModelInstall, ModelPage, ModelQuery,
    ModelSelection, ProviderDescriptor, ProviderDetailsQuery, ProviderError, ProviderErrorCode,
    ProviderPage, ProviderQuery,
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

/// The provider contract owns the method and attempt lifecycle. No API key,
/// OAuth token, transport authority, or caller-supplied provider artifact is
/// accepted by any of these ACP control-plane requests.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, acp::JsonRpcRequest)]
#[request(method = "_phenix/providers/auth/begin", response = serde_json::Value)]
#[serde(deny_unknown_fields)]
struct ProviderAuthBeginRequest {
    scope: String,
    provider_id: String,
    connection_id: String,
    method_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, acp::JsonRpcRequest)]
#[request(method = "_phenix/providers/auth/poll", response = serde_json::Value)]
#[serde(deny_unknown_fields)]
struct ProviderAuthPollRequest {
    scope: String,
    provider_id: String,
    connection_id: String,
    attempt_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, acp::JsonRpcRequest)]
#[request(method = "_phenix/providers/auth/cancel", response = serde_json::Value)]
#[serde(deny_unknown_fields)]
struct ProviderAuthCancelRequest {
    scope: String,
    provider_id: String,
    connection_id: String,
    attempt_id: String,
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

/// The only accepted write authority comes from a trusted launcher-owned
/// composition, not a Lua feature flag or an arbitrary provider catalog.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, acp::JsonRpcRequest)]
#[request(method = "_phenix/credentials/capabilities", response = serde_json::Value)]
#[serde(deny_unknown_fields)]
struct CredentialCapabilitiesRequest {}

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
    fn identity(&self) -> Result<CredentialIdentity, ProviderError> {
        CredentialIdentity::new(
            self.provider_id.clone(),
            self.connection_id.clone(),
            self.profile_id.clone(),
        )
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
    ProviderAuthBegin(ProviderAuthBeginRequest, Reply<serde_json::Value>),
    ProviderAuthPoll(ProviderAuthPollRequest, Reply<serde_json::Value>),
    ProviderAuthCancel(ProviderAuthCancelRequest, Reply<serde_json::Value>),
    ProviderModels(ProviderModelsRequest, Reply<serde_json::Value>),
    CredentialCapabilities(CredentialCapabilitiesRequest, Reply<serde_json::Value>),
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
            Self::ProviderAuthBegin(_, reply) => reply.respond_with_error(error),
            Self::ProviderAuthPoll(_, reply) => reply.respond_with_error(error),
            Self::ProviderAuthCancel(_, reply) => reply.respond_with_error(error),
            Self::ProviderModels(_, reply) => reply.respond_with_error(error),
            Self::CredentialCapabilities(_, reply) => reply.respond_with_error(error),
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

/// Preserve the native provider's typed denial code without forwarding
/// provider prose (which may contain identity or credential fragments).
fn provider_rejection(problem: ProviderError) -> acp::Error {
    acp::Error::invalid_params().data(serde_json::json!({
        "format": "phenix.provider.error.v1",
        "code": problem.code,
    }))
}

// Trusted catalog reads are made through the launcher's already-admitted
// control-plane kernel. Selector values never select a provider implementation
// or create new native grants.
/// Complete one provider-owned model catalog before admitting a choice.
/// A cursor is only pagination state, never a revision or authorization token.
#[derive(Default)]
struct ModelPageAccumulator {
    models: Vec<phenix_provider_contracts::ProviderModel>,
    revision: Option<u64>,
    cursors: std::collections::BTreeSet<String>,
    pages: usize,
}

impl ModelPageAccumulator {
    fn push(&mut self, page: ModelPage, profile: &Option<String>) -> acp::Result<Option<String>> {
        if self.pages >= 64
            || page.revision == 0
            || self.revision.is_some_and(|prev| prev != page.revision)
            || &page.profile_id != profile
            || page.models.len() > 256
            || self
                .models
                .len()
                .checked_add(page.models.len())
                .is_none_or(|count| count > 4096)
        {
            return Err(invalid(
                "provider catalog page changed or exceeded its budget",
            ));
        }
        self.pages += 1;
        self.revision = Some(page.revision);
        let next = page.next_cursor;
        if let Some(cursor) = &next {
            if page.models.is_empty()
                || cursor.is_empty()
                || cursor.len() > 1024
                || !self.cursors.insert(cursor.clone())
            {
                return Err(invalid("invalid or repeated provider catalog cursor"));
            }
        }
        self.models.extend(page.models);
        Ok(next)
    }

    fn complete(self, profile: Option<String>) -> acp::Result<ModelPage> {
        Ok(ModelPage {
            profile_id: profile,
            models: self.models,
            next_cursor: None,
            revision: self
                .revision
                .ok_or_else(|| invalid("missing model catalog"))?,
        })
    }
}

impl ProviderManagement {
    fn read<Q: Portable, R: Portable>(
        &self,
        interface: &str,
        operation: &str,
        query: &Q,
    ) -> acp::Result<R> {
        let root = self
            .kernel
            .admit_root(Selection {
                interface: interface.into(),
                operation: operation.into(),
                provider: self.selected.provider.clone(),
            })
            .map_err(internal)?;
        let value = match root.invoke(&query.to_value()).map_err(internal)? {
            Outcome::Success(value) => value,
            Outcome::Domain(value) => {
                let problem = ProviderError::from_value(value).map_err(internal)?;
                return Err(provider_rejection(problem));
            }
        };
        R::from_value(value).map_err(internal)
    }

    /// Controller-only preflight. Never treat UI-discovered models or a
    /// preflight result as an executable kernel grant. The later native
    /// installation must recheck auth/registration, resolve the exact secret
    /// and use the same session kernel.
    #[allow(dead_code)]
    fn current_http_binding(&self, desired: &ModelSelection) -> acp::Result<HttpModelBinding> {
        const SCOPE: &str = "user";

        let listed: ProviderPage = self.read(
            "phenix.provider.catalog@1",
            "list",
            &ProviderQuery {
                scope: SCOPE.into(),
            },
        )?;
        if listed.revision == 0 {
            return Err(invalid("provider list has no current revision"));
        }
        let mut candidates = listed
            .providers
            .iter()
            .filter(|p| p.id == desired.provider_id);
        let provided = candidates
            .next()
            .ok_or_else(|| invalid("unknown selected provider"))?;
        if candidates.next().is_some() {
            return Err(invalid("ambiguous selected provider"));
        }
        let described: ProviderDescriptor = self.read(
            "phenix.provider.catalog@1",
            "describe",
            &ProviderDetailsQuery {
                scope: SCOPE.into(),
                provider_id: desired.provider_id.clone(),
            },
        )?;
        if provided != &described {
            return Err(invalid("provider registration changed during selection"));
        }
        let auth: AuthenticationStatus = self.read(
            "phenix.provider.auth@1",
            "status",
            &AuthQuery {
                scope: SCOPE.into(),
                provider_id: desired.provider_id.clone(),
                connection_id: desired.connection_id.clone(),
            },
        )?;

        let mut collected = ModelPageAccumulator::default();
        let mut cursor: Option<String> = None;
        loop {
            let page: ModelPage = self.read(
                "phenix.provider.catalog@1",
                "models",
                &ModelQuery {
                    scope: SCOPE.into(),
                    provider_id: desired.provider_id.clone(),
                    connection_id: desired.connection_id.clone(),
                    profile_id: desired.profile_id.clone(),
                    cursor: cursor.clone(),
                    limit: 256,
                },
            )?;
            cursor = collected.push(page, &desired.profile_id)?;
            if cursor.is_none() {
                let complete = collected.complete(desired.profile_id.clone())?;
                let checked = validate_model_selection(&described, &auth, &complete, desired, true)
                    .map_err(provider_rejection)?;
                return bind_http_model(&described, checked).map_err(provider_rejection);
            }
        }
    }
}

/// A non-secret ACP option assembled from CURRENT plugin-owned provider
/// evidence, never from an arbitrary frontend model list.
#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeModelOffer {
    id: String,
    label: String,
    selected: ModelSelection,
}

/// The complete provider/connection/account/protocol/endpoint identity is
/// carried by the ACP option ID. JSON is already the protocol's typed wire
/// representation; this selector is NOT an authority token. On submission,
/// compare the whole ID with current runtime-generated offers, not decoded
/// client claims. Distinct account choices must never alias one model name.
fn native_model_option_id(selection: &ModelSelection, revision: u64) -> acp::Result<String> {
    let mut fields = serde_json::to_value(selection).map_err(internal)?;
    let object = fields
        .as_object_mut()
        .ok_or_else(|| internal("invalid native selection"))?;
    object.insert("session_revision".into(), serde_json::json!(revision));
    serde_json::to_string(&fields)
        .map(|value| format!("phenix.native-model@1:{value}"))
        .map_err(internal)
}

/// Classify a stale menu without treating client JSON as authority. Only
/// current runtime-issued catalog choices ever reach native installation.
fn stale_native_model_choice(chosen: &str, current_revision: u64) -> bool {
    const PREFIX: &str = "phenix.native-model@1:";
    if chosen.len() > 4096 {
        return false;
    }
    chosen
        .strip_prefix(PREFIX)
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .and_then(|json| {
            json.get("session_revision")
                .and_then(|value| value.as_u64())
        })
        .is_some_and(|revision| revision < current_revision)
}

fn stale_model_conflict() -> acp::Error {
    acp::Error::invalid_params().data(serde_json::json!({
        "format": "phenix.basic.error.v1",
        "code": ErrorCode::Conflict,
        "message": "model selection changed; refresh the ACP session model menu",
    }))
}

/// A prior revision choice is idempotent ONLY when it selected the exact
/// current model. A stale choice for another model is rejected on admission.
fn is_previous_native_selection(
    chosen: &str,
    current: &ModelSelection,
    revision: u64,
) -> acp::Result<bool> {
    let Some(previous) = revision.checked_sub(1) else {
        return Ok(false);
    };
    Ok(native_model_option_id(current, previous)? == chosen)
}

/// One controller model-menu refresh must have a global bound, not one
/// budget independently repeated for every provider and connection.
fn charge_model_option_budget(used: &mut usize, maximum: usize) -> acp::Result<()> {
    if *used >= maximum {
        return Err(invalid(
            "provider model selection exceeded discovery budget",
        ));
    }
    *used += 1;
    Ok(())
}

/// Only explicitly unavailable/not-yet-discovered provider catalogs are
/// absent choices. A transport/authority denial, malformed typed reply or
/// conflicting revision must not be confused with an empty model list.
fn is_missing_model_discovery(code: &ProviderErrorCode) -> bool {
    matches!(
        code,
        ProviderErrorCode::Unavailable
            | ProviderErrorCode::AuthenticationRequired
            | ProviderErrorCode::NotFound
    )
}

impl ProviderManagement {
    fn optional_model_page(&self, query: &ModelQuery) -> acp::Result<Option<ModelPage>> {
        let root = self
            .kernel
            .admit_root(Selection {
                interface: "phenix.provider.catalog@1".into(),
                operation: "models".into(),
                provider: self.selected.provider.clone(),
            })
            .map_err(internal)?;
        match root.invoke(&query.to_value()).map_err(internal)? {
            Outcome::Success(value) => ModelPage::from_value(value).map(Some).map_err(internal),
            Outcome::Domain(value) => {
                let problem = ProviderError::from_value(value).map_err(internal)?;
                if is_missing_model_discovery(&problem.code) {
                    Ok(None)
                } else {
                    Err(provider_rejection(problem))
                }
            }
        }
    }

    /// Enumerate only models executable with the launcher's exact credential
    /// identity (or explicit no-auth). A stored key/profile is not itself a
    /// capability. Each choice is checked again before native install.
    fn authorized_http_offers(
        &self,
        resolvers: &[ModelCredentialResolver],
    ) -> acp::Result<Vec<NativeModelOffer>> {
        let listed: ProviderPage = self.read(
            "phenix.provider.catalog@1",
            "list",
            &ProviderQuery {
                scope: "user".into(),
            },
        )?;
        if listed.revision == 0 || listed.providers.len() > 32 {
            return Err(invalid(
                "provider selection catalog is invalid or too large",
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut offered = Vec::new();
        let mut inspected_connections = 0;
        let mut inspected_pages = 0;
        for provider in listed.providers {
            if provider.connections.len() > 32 || !ids.insert(provider.id.clone()) {
                return Err(invalid("ambiguous or oversized provider selection"));
            }
            let exact: ProviderDescriptor = self.read(
                "phenix.provider.catalog@1",
                "describe",
                &ProviderDetailsQuery {
                    scope: "user".into(),
                    provider_id: provider.id.clone(),
                },
            )?;
            if exact != provider {
                return Err(invalid("provider changed while listing model options"));
            }
            let mut connections = std::collections::BTreeSet::new();
            for connection in &exact.connections {
                charge_model_option_budget(&mut inspected_connections, 32)?;
                if !connections.insert(&connection.id) {
                    return Err(invalid("duplicate provider connection"));
                }
                let profiles = if connection.auth_methods.is_empty() {
                    vec![None]
                } else {
                    // Each resolver corresponds to one independently granted
                    // exact resource. Named accounts on the same connection
                    // are separate catalog/authorization selections.
                    resolvers
                        .iter()
                        .filter(|source| {
                            source.identity.provider_id() == exact.id
                                && source.identity.connection_id() == connection.id
                        })
                        .map(|source| Some(source.identity.profile_id().to_owned()))
                        .collect::<Vec<_>>()
                };
                if profiles.is_empty() {
                    continue;
                }
                let auth: AuthenticationStatus = self.read(
                    "phenix.provider.auth@1",
                    "status",
                    &AuthQuery {
                        scope: "user".into(),
                        provider_id: exact.id.clone(),
                        connection_id: connection.id.clone(),
                    },
                )?;
                for profile in profiles {
                    if let Some(profile_id) = profile.as_deref() {
                        if !auth.profile_is_valid_for(&exact.id, connection, profile_id) {
                            continue;
                        }
                    }

                    let mut collected = ModelPageAccumulator::default();
                    let mut cursor = None;
                    let full = loop {
                        charge_model_option_budget(&mut inspected_pages, 128)?;
                        let Some(page) = self.optional_model_page(&ModelQuery {
                            scope: "user".into(),
                            provider_id: exact.id.clone(),
                            connection_id: connection.id.clone(),
                            profile_id: profile.clone(),
                            cursor: cursor.clone(),
                            limit: 256,
                        })?
                        else {
                            // Registration without discovery is not a selectable
                            // model or a reason to break unrelated ACP sessions.
                            break None;
                        };
                        cursor = collected.push(page, &profile)?;
                        if cursor.is_none() {
                            break Some(collected.complete(profile.clone())?);
                        }
                    };
                    let Some(full) = full else {
                        continue;
                    };
                    for model in &full.models {
                        if !model.selectable || model.connection_id != connection.id {
                            continue;
                        }
                        let desired = ModelSelection {
                            provider_id: exact.id.clone(),
                            connection_id: connection.id.clone(),
                            profile_id: profile.clone(),
                            model_id: model.id.clone(),
                            protocol_id: model.protocol_id.clone(),
                            endpoint_id: model.endpoint_id.clone(),
                            catalog_revision: full.revision,
                        };
                        let Ok(admitted) =
                            validate_model_selection(&exact, &auth, &full, &desired, true)
                        else {
                            continue;
                        };
                        let Ok(binding) = bind_http_model(&exact, admitted) else {
                            continue;
                        };
                        // No OAuth/plugin-specific HTTP transport is implemented
                        // here; never advertise it as an executable choice.
                        if !matches!(
                            binding.authentication_kind(),
                            None | Some(AuthenticationKind::ApiKey)
                        ) {
                            continue;
                        }
                        if binding.credential_identity().is_some()
                            && !resolvers
                                .iter()
                                .any(|r| binding.credential_identity() == Some(&r.identity))
                        {
                            continue;
                        }
                        if offered.len() >= 128 {
                            return Err(invalid("too many selectable HTTP models"));
                        }
                        offered.push(NativeModelOffer {
                            id: native_model_option_id(&desired, 0)?,
                            label: format!(
                                "{} / {} / {}",
                                exact.label, connection.label, model.label
                            ),
                            selected: desired,
                        });
                    }
                }
            }
        }
        let mut distinct = std::collections::BTreeSet::new();
        if offered.iter().any(|entry| !distinct.insert(&entry.id)) {
            return Err(invalid("duplicate native model option"));
        }
        Ok(offered)
    }
}

struct CredentialManagement {
    kernel: Kernel,
    importer: Selection,
    resource: String,
    identity: CredentialIdentity,
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
    let identity = CredentialIdentity::from_resource(resource)
        .map_err(|_| "invalid launcher credential resource")?;
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
        identity,
    })
}

/// A separate launcher-authorized model-secret resolution root. This kernel
/// is not selected from ACP request parameters and exposes no ACP method.
/// The ordinary Basic session retains only its session/property resources.
struct ModelCredentialResolver {
    kernel: Kernel,
    selected: Selection,
    identity: CredentialIdentity,
}

fn validate_model_resolver_spec(spec: &Composition) -> Result<CredentialIdentity, String> {
    const CONTRACT: &str = "phenix.provider.credentials@1";
    const RESOLVE: &str = "phenix.provider.credentials@1/resolve";
    let resource = spec
        .resource
        .as_deref()
        .ok_or("model resolver requires an explicit credential resource")?;
    let identity = CredentialIdentity::from_resource(resource)
        .map_err(|_| "invalid model resolver credential resource")?;
    let allowed_resource = std::collections::BTreeSet::from([resource.to_string()]);
    if spec.entry.interface != CONTRACT
        || spec.entry.operation != "resolve"
        || spec.authority.operations != std::collections::BTreeSet::from([RESOLVE.into()])
        || spec.authority.resources != allowed_resource
        || spec.grants.len() != 1
        || !spec.slots.is_empty()
        || !spec.excluded.is_empty()
        || !spec.substitutions.is_empty()
    {
        return Err("model resolver requires exactly one resolve-only root authority".into());
    }
    let grant = &spec.grants[0];
    if grant.caller != "root"
        || grant.interface != CONTRACT
        || grant.operation != "resolve"
        || grant.resources != allowed_resource
    {
        return Err("model resolver grant must match the selected credential resource".into());
    }
    Ok(identity)
}

fn prepare_model_resolver(spec: &Composition) -> Result<ModelCredentialResolver, String> {
    let identity = validate_model_resolver_spec(spec)?;
    let kernel = Prepared::prepare(spec.clone())
        .map_err(|_| "invalid model resolver composition".to_string())?
        .load()
        .map_err(|_| "cannot load model resolver composition".to_string())?;
    Ok(ModelCredentialResolver {
        kernel,
        selected: spec.entry.clone(),
        identity,
    })
}

impl ModelCredentialResolver {
    /// Available only to the trusted controller after checking fresh model
    /// binding and authorization. No ACP command can call this method.
    #[allow(dead_code)]
    fn resolve_for(&self, identity: &CredentialIdentity) -> Result<CredentialSecret, String> {
        if *identity != self.identity {
            return Err("model credential identity is not launcher-authorized".into());
        }
        let root = self
            .kernel
            .admit_root(self.selected.clone())
            .map_err(|_| "model credential resolution is not admitted".to_string())?;
        match root
            .invoke(&identity.to_value())
            .map_err(|_| "model credential resolution failed".to_string())?
        {
            Outcome::Success(value) => CredentialSecret::from_value(value)
                .map_err(|_| "invalid credential resolution response".into()),
            Outcome::Domain(_) => Err("model credential resolution denied".into()),
        }
    }
}

/// P1 installs exactly one authoritative Model slot. Multiple mappings
/// (including identical duplicates) are not a selectable model catalog:
/// client model IDs must never resolve through first-match routing.
fn installed_model_provider(composition: &Composition) -> Option<&str> {
    let mut bindings = composition.slots.iter().filter(|binding| {
        binding.caller == "phenix.basic.agent" && binding.slot == "Model" && binding.index == 0
    });
    let selected = bindings.next()?;
    if bindings.next().is_some() {
        return None;
    }
    Some(selected.provider.as_str())
}

// HttpModelInstall's Portable encoding contains a temporary copy of the
// resolved credential. Scrub all embedded Bytes on every success/error path.
// CredentialSecret owns and zeroizes the original independently.
fn scrub_portable_secret_bytes(value: &mut Value) {
    match value {
        Value::Bytes(bytes) => bytes.fill(0),
        Value::List(items) => {
            for item in items {
                scrub_portable_secret_bytes(item);
            }
        }
        Value::Map(fields) => {
            for item in fields.values_mut() {
                scrub_portable_secret_bytes(item);
            }
        }
        _ => {}
    }
}

/// Only the launcher-installed exact session storage root may opt in to
/// persisted selected-model intent. This does not infer authority from an ACP
/// request or an unrelated model slot.
fn has_model_selection_storage(spec: &Composition) -> bool {
    spec.grants.iter().any(|grant| {
        grant.caller == "root"
            && grant.interface == "phenix.basic.storage@1"
            && grant.operation == "read"
    })
}

/// The ACP config-option response does not grant model routing. It may only
/// acknowledge the runtime-advertised effective model; a forged/stale value
/// must not be silently interpreted as a switch.
fn exact_current_model(offered: Option<&str>, requested: &str) -> bool {
    !requested.is_empty() && offered == Some(requested)
}

/// Normal Basic stays on its launcher-installed static Model. The opt-in
/// selected HTTP composition must have all three exact native root operations
/// before it can present a mutable model picker.
fn can_switch_native_model(spec: &Composition) -> bool {
    let Some(resource) = spec.resource.as_deref() else {
        return false;
    };
    if installed_model_provider(spec).is_none() || !spec.authority.resources.contains(resource) {
        return false;
    }
    [
        ("phenix.basic.storage@1", "read"),
        ("phenix.basic.storage@1", "transact"),
        ("phenix.provider.http-model-session@1", "install"),
    ]
    .into_iter()
    .all(|(interface, operation)| {
        if !spec
            .authority
            .operations
            .contains(&format!("{interface}/{operation}"))
        {
            return false;
        }
        spec.grants
            .iter()
            .filter(|grant| {
                grant.caller == "root"
                    && grant.interface == interface
                    && grant.operation == operation
                    && grant.resources.len() == 1
                    && grant.resources.contains(resource)
            })
            .count()
            == 1
    })
}

struct Controller {
    composition: Composition,
    management: Option<ProviderManagement>,
    credentials: Vec<CredentialManagement>,
    resolvers: Vec<ModelCredentialResolver>,
    sessions: BTreeMap<String, Kernel>,
    offsets: BTreeMap<String, u64>,
    selected_http_seen: std::collections::BTreeSet<String>,
    initialized: bool,
    issued: u64,
    queued: Arc<PendingPrompts>,
    closed: Arc<AtomicBool>,
}

impl Controller {
    fn start(
        composition: Composition,
        management: Option<ProviderManagement>,
        credentials: Vec<CredentialManagement>,
        resolvers: Vec<ModelCredentialResolver>,
        queued: Arc<PendingPrompts>,
        closed: Arc<AtomicBool>,
    ) -> Self {
        Self {
            composition,
            management,
            credentials,
            resolvers,
            sessions: BTreeMap::new(),
            offsets: BTreeMap::new(),
            selected_http_seen: std::collections::BTreeSet::new(),
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
            Command::ProviderAuthBegin(request, reply) => {
                let result = if !self.initialized || self.closed.load(Ordering::Acquire) {
                    Err(invalid("active initialized ACP connection required"))
                } else {
                    self.provider_auth_begin(request)
                };
                let _ = reply.respond_with_result(result);
            }
            Command::ProviderAuthPoll(request, reply) => {
                let result = if !self.initialized || self.closed.load(Ordering::Acquire) {
                    Err(invalid("active initialized ACP connection required"))
                } else {
                    self.provider_auth_poll(request)
                };
                let _ = reply.respond_with_result(result);
            }
            Command::ProviderAuthCancel(request, reply) => {
                let result = if !self.initialized || self.closed.load(Ordering::Acquire) {
                    Err(invalid("active initialized ACP connection required"))
                } else {
                    self.provider_auth_cancel(request)
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
            Command::CredentialCapabilities(_, reply) => {
                let result = if !self.initialized || self.closed.load(Ordering::Acquire) {
                    Err(invalid("active initialized ACP connection required"))
                } else {
                    self.credential_capabilities()
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
        let result: R = management.read(interface, operation, query)?;
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

    fn provider_auth_begin(
        &self,
        request: ProviderAuthBeginRequest,
    ) -> acp::Result<serde_json::Value> {
        if request.scope != "user"
            || request.provider_id.is_empty()
            || request.connection_id.is_empty()
            || request.method_id.is_empty()
            || request.provider_id.len() > 128
            || request.connection_id.len() > 128
            || request.method_id.len() > 128
        {
            return Err(invalid("invalid provider authentication method"));
        }
        self.provider_read::<_, AuthChallenge>(
            "phenix.provider.auth@1",
            "begin",
            &AuthStart {
                scope: request.scope,
                provider_id: request.provider_id,
                connection_id: request.connection_id,
                method_id: request.method_id,
            },
        )
    }

    fn provider_auth_attempt(
        &self,
        scope: String,
        provider_id: String,
        connection_id: String,
        attempt_id: String,
        operation: &str,
    ) -> acp::Result<serde_json::Value> {
        if scope != "user"
            || provider_id.is_empty()
            || connection_id.is_empty()
            || attempt_id.is_empty()
            || provider_id.len() > 128
            || connection_id.len() > 128
            || attempt_id.len() > 256
            || attempt_id.chars().any(char::is_control)
        {
            return Err(invalid("invalid provider authentication attempt"));
        }
        // The attempt selector is not proof of authentication or a grant:
        // only the launcher-owned provider plugin can complete this operation.
        self.provider_read::<_, AuthAttemptState>(
            "phenix.provider.auth@1",
            operation,
            &AuthAttemptQuery {
                scope,
                provider_id,
                connection_id,
                attempt_id,
            },
        )
    }

    fn provider_auth_poll(
        &self,
        request: ProviderAuthPollRequest,
    ) -> acp::Result<serde_json::Value> {
        self.provider_auth_attempt(
            request.scope,
            request.provider_id,
            request.connection_id,
            request.attempt_id,
            "poll",
        )
    }

    fn provider_auth_cancel(
        &self,
        request: ProviderAuthCancelRequest,
    ) -> acp::Result<serde_json::Value> {
        self.provider_auth_attempt(
            request.scope,
            request.provider_id,
            request.connection_id,
            request.attempt_id,
            "cancel",
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

    fn credential_capabilities(&self) -> acp::Result<serde_json::Value> {
        if self.credentials.is_empty() {
            return Err(acp::Error::method_not_found());
        }
        let profiles = self
            .credentials
            .iter()
            .map(|item| {
                serde_json::json!({
                    "provider_id": item.identity.provider_id(),
                    "connection_id": item.identity.connection_id(),
                    "profile_id": item.identity.profile_id(),
                    "import": true,
                    "revoke": true,
                })
            })
            .collect::<Vec<_>>();
        // Preserve the accepted single-account capability shape. When
        // several accounts are granted, enumerate instead of advertising a
        // misleading single selected account or broad wildcard authority.
        if profiles.len() == 1 {
            return Ok(profiles.into_iter().next().expect("one profile"));
        }
        Ok(serde_json::json!({ "profiles": profiles }))
    }

    fn credential_operation(
        &self,
        operation: &str,
        identity: &CredentialIdentity,
        value: &Value,
    ) -> acp::Result<serde_json::Value> {
        if self.credentials.is_empty() {
            return Err(acp::Error::method_not_found());
        }
        // Identity is a selector, not a grant. The lookup returns one
        // distinct, already validated launcher-owned exact-resource kernel.
        let resource = identity
            .resource()
            .map_err(|_| invalid("invalid credential identity"))?;
        let management = self
            .credentials
            .iter()
            .find(|item| item.resource == resource)
            .ok_or_else(|| invalid("credential identity is not authorized"))?;
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
        let identity = CredentialIdentity::new(
            request.provider_id.clone(),
            request.connection_id.clone(),
            request.profile_id.clone(),
        )
        .map_err(|_| invalid("invalid credential identity"))?;
        // Check the identity before allocating/copying the supplied secret.
        if self.credentials.is_empty() {
            return Err(acp::Error::method_not_found());
        }
        let resource = identity
            .resource()
            .map_err(|_| invalid("invalid credential identity"))?;
        if !self
            .credentials
            .iter()
            .any(|item| item.resource == resource)
        {
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
        let identity = request
            .identity()
            .map_err(|_| invalid("invalid credential identity"))?;
        self.credential_operation("revoke", &identity, &identity.to_value())
    }

    /// Read the effective model for THIS session, not the static installed
    /// transport plugin name when the native session has a persisted selection.
    /// Storage contains only intent: provider-owned catalog/auth is checked
    /// again before disclosing a selectable ACP model. A missing/invalid
    /// selection never borrows a different session's model or authority.
    fn current_model_id(&self, session: &str) -> acp::Result<Option<String>> {
        let kernel = self
            .sessions
            .get(session)
            .ok_or_else(|| invalid("unknown ACP session"))?;
        let installed = installed_model_provider(&self.composition);
        if has_model_selection_storage(&self.composition) {
            let recorded = super::model_selection_state::read(kernel, &self.composition, session)
                .map_err(internal)?;
            match recorded {
                Some((selection, _revision)) => {
                    // The actual installed Model slot remains launcher-owned.
                    // Validation alone cannot authorize an absent native slot.
                    installed.ok_or_else(|| invalid("installed Model slot is ambiguous"))?;
                    let management = self
                        .management
                        .as_ref()
                        .ok_or_else(|| invalid("selected model management is unavailable"))?;
                    let binding = management.current_http_binding(&selection)?;
                    return Ok(Some(binding.model_id().to_owned()));
                }
                None if self.selected_http_seen.contains(session) => {
                    return Err(invalid("selected model record was withdrawn"));
                }
                None => {}
            }
        }
        Ok(installed.map(str::to_owned))
    }

    /// The UI only receives session-effective options from the authorized
    /// provider catalog. The option ID contains the entire non-secret native
    /// selection identity, NOT an executable capability.
    fn model_options(&self, session: &str) -> acp::Result<Vec<SessionConfigOption>> {
        let effective = self.current_model_id(session)?;
        let mut options = Vec::new();
        let chosen = if can_switch_native_model(&self.composition) && self.management.is_some() {
            let kernel = self
                .sessions
                .get(session)
                .ok_or_else(|| invalid("unknown ACP session"))?;
            let recorded = super::model_selection_state::read(kernel, &self.composition, session)
                .map_err(internal)?;
            let management = self
                .management
                .as_ref()
                .ok_or_else(|| invalid("selected model management is unavailable"))?;
            let native = management.authorized_http_offers(&self.resolvers)?;
            let revision = recorded.as_ref().map_or(0, |(_, revision)| *revision);
            let chosen = match &recorded {
                Some((selection, _)) => native_model_option_id(selection, revision)?,
                None => effective.clone().unwrap_or_default(),
            };
            if recorded.is_none() {
                if let Some(legacy) = effective.as_ref() {
                    options.push(SessionConfigSelectOption::new(
                        legacy.clone(),
                        legacy.clone(),
                    ));
                }
            }
            for entry in native {
                options.push(SessionConfigSelectOption::new(
                    native_model_option_id(&entry.selected, revision)?,
                    entry.label,
                ));
            }
            if !options
                .iter()
                .any(|option| option.value.to_string() == chosen)
            {
                return Err(invalid("effective selected model is no longer offered"));
            }
            chosen
        } else {
            let Some(effective) = effective else {
                return Ok(Vec::new());
            };
            options.push(SessionConfigSelectOption::new(
                effective.clone(),
                effective.clone(),
            ));
            effective
        };
        Ok(vec![SessionConfigOption::select(
            "model", "Model", chosen, options,
        )
        .category(SessionConfigOptionCategory::Model)])
    }

    fn set_config_option(
        &mut self,
        request: SetSessionConfigOptionRequest,
    ) -> acp::Result<SetSessionConfigOptionResponse> {
        let session = request.session_id.to_string();
        let chosen = match request.value {
            SessionConfigOptionValue::ValueId { value } => value.to_string(),
            _ => return Err(invalid("unsupported model configuration value")),
        };
        if request.config_id.to_string() != "model" {
            return Err(invalid("unsupported configuration option"));
        }

        let kernel = self
            .sessions
            .get(&session)
            .ok_or_else(|| invalid("unknown ACP session"))?;
        let previous = if can_switch_native_model(&self.composition) && self.management.is_some() {
            super::model_selection_state::read(kernel, &self.composition, &session)
                .map_err(internal)?
        } else {
            None
        };
        let expected = previous.as_ref().map_or(0, |(_, revision)| *revision);
        let current = match &previous {
            Some((selection, _)) => Some(native_model_option_id(selection, expected)?),
            None => self.current_model_id(&session)?,
        };
        let idempotent = match &previous {
            Some((selected, revision)) => {
                is_previous_native_selection(&chosen, selected, *revision)?
            }
            None => false,
        };
        if exact_current_model(current.as_deref(), &chosen) || idempotent {
            return Ok(SetSessionConfigOptionResponse::new(
                self.model_options(&session)?,
            ));
        }
        if !can_switch_native_model(&self.composition) || self.management.is_none() {
            return Err(invalid("unavailable or unauthorized model"));
        }

        // Only a freshly enumerated option may request mutation. No generic
        // client-supplied JSON deserialization can create provider authority.
        let management = self
            .management
            .as_ref()
            .ok_or_else(|| invalid("selected model management is unavailable"))?;
        let offers = management.authorized_http_offers(self.resolver.as_ref())?;
        let mut found = Vec::new();
        for entry in offers {
            if native_model_option_id(&entry.selected, expected)? == chosen {
                found.push(entry.selected);
            }
        }
        if found.len() != 1 {
            if stale_native_model_choice(&chosen, expected) {
                return Err(stale_model_conflict());
            }
            return Err(invalid("ambiguous or unauthorized model choice"));
        }
        let desired = found.pop().ok_or_else(|| invalid("model unavailable"))?;
        // After admission, remember any attempt to install a native Model:
        // even a later failed CAS must not silently route a no-record prompt
        // via the legacy environment while an uncommitted Model is installed.
        self.selected_http_seen.insert(session.clone());
        self.install_current_http_model(&session, &desired)?;
        let checked = management.current_http_binding(&desired)?;
        super::model_selection_state::compare_and_swap(
            self.sessions
                .get(&session)
                .ok_or_else(|| invalid("unknown selected model session"))?,
            &self.composition,
            &session,
            &checked,
            expected,
        )
        .map_err(|_| invalid("selected model revision conflict or persistence denied"))?;
        let confirmed = super::model_selection_state::read(
            self.sessions
                .get(&session)
                .ok_or_else(|| invalid("unknown selected model session"))?,
            &self.composition,
            &session,
        )
        .map_err(internal)?;
        if confirmed.as_ref().map(|(stored, _)| stored) != Some(&desired) {
            return Err(invalid(
                "selected model CAS receipt did not become effective",
            ));
        }
        Ok(SetSessionConfigOptionResponse::new(
            self.model_options(&session)?,
        ))
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
        let options = self.model_options(&session)?;
        Ok(NewSessionResponse::new(session).config_options(options))
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
        Ok(LoadSessionResponse::new().config_options(self.model_options(&session)?))
    }

    /// Controller-only, same-session Model authorization. A persisted choice
    /// is an intent: every new turn must revalidate it, resolve exactly its
    /// currently granted credential and install that native HTTP transport.
    /// A denied or stale choice prevents the turn; it never falls back to
    /// environment Model routing.
    fn install_current_http_model(
        &self,
        session: &str,
        desired: &ModelSelection,
    ) -> acp::Result<()> {
        let management = self
            .management
            .as_ref()
            .ok_or_else(|| invalid("current provider management is unavailable"))?;
        let binding = management.current_http_binding(desired)?;
        let credential = match (binding.credential_identity(), binding.authentication_kind()) {
            (None, None) => None,
            (Some(identity), Some(AuthenticationKind::ApiKey)) => {
                // Scope derivation is not a grant. The separate model resolver
                // kernel must also possess the exact launcher-owned root.
                binding
                    .resolution_scope_for(identity)
                    .map_err(provider_rejection)?;
                let resolver = self
                    .resolvers
                    .iter()
                    .find(|source| source.identity == *identity)
                    .ok_or_else(|| invalid("selected API-key resolver is not installed"))?;
                let secret = resolver
                    .resolve_for(identity)
                    .map_err(|_| invalid("selected API-key resolution was denied"))?;
                Some((identity.clone(), secret))
            }
            _ => return Err(invalid("selected model authentication is unsupported")),
        };
        // Refuse changed registration/revocation while resolving the secret.
        // Neither the original choice nor the secret-store reply are grants.
        if management.current_http_binding(desired)? != binding {
            return Err(invalid(
                "selected model authentication changed before install",
            ));
        }
        let install = HttpModelInstall::from_checked(&binding, session.into(), credential, None)
            .map_err(provider_rejection)?;
        let kernel = self
            .sessions
            .get(session)
            .ok_or_else(|| invalid("unknown selected model session"))?;
        let provider = installed_model_provider(&self.composition)
            .ok_or_else(|| invalid("installed Model slot is missing or ambiguous"))?;
        let root = kernel
            .admit_root(Selection {
                interface: "phenix.provider.http-model-session@1".into(),
                operation: "install".into(),
                provider: provider.into(),
            })
            .map_err(|_| invalid("selected Model install grant is unavailable"))?;
        let mut payload = install.to_value();
        let result = root.invoke(&payload);
        scrub_portable_secret_bytes(&mut payload);
        match result.map_err(internal)? {
            Outcome::Success(value) => <() as Portable>::from_value(value).map_err(internal),
            Outcome::Domain(value) => {
                let problem = ProviderError::from_value(value).map_err(internal)?;
                Err(provider_rejection(problem))
            }
        }
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
        if has_model_selection_storage(&self.composition) {
            match super::model_selection_state::read(kernel, &self.composition, &session)
                .map_err(internal)?
            {
                Some((stored, _revision)) => {
                    // A once-selected native transport cannot silently return
                    // to legacy preset routing when its record disappears.
                    self.selected_http_seen.insert(session.clone());
                    self.install_current_http_model(&session, &stored)?;
                }
                None if self.selected_http_seen.contains(&session) => {
                    return Err(invalid("selected model record was withdrawn"));
                }
                None => {}
            }
        }
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
        for credential in self.credentials {
            credential
                .kernel
                .shutdown()
                .map_err(|_| "credential cleanup failed")?;
        }
        for resolver in self.resolvers {
            resolver
                .kernel
                .shutdown()
                .map_err(|_| "model resolver cleanup failed")?;
        }
        Ok(())
    }
}

pub(super) fn run(
    composition: &Composition,
    management: Option<&Composition>,
    credentials: &[Composition],
    resolvers: &[Composition],
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
    if credentials.len() > 32 || resolvers.len() > 32 {
        return Err("too many authorized credential compositions".into());
    }
    let credentials = credentials
        .iter()
        .map(prepare_credentials)
        .collect::<Result<Vec<_>, _>>()?;
    let resolvers = resolvers
        .iter()
        .map(prepare_model_resolver)
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen = std::collections::BTreeSet::new();
    if credentials
        .iter()
        .any(|item| !seen.insert(item.resource.clone()))
    {
        return Err("duplicate credential root".into());
    }
    seen.clear();
    if resolvers.iter().any(|item| {
        !seen.insert(
            item.identity
                .clone()
                .resource()
                .expect("validated credential root"),
        )
    }) {
        return Err("duplicate model credential root".into());
    }
    let (tx, receiver) = sync_channel::<Command>(64);
    let closed = Arc::new(AtomicBool::new(false));
    // A prompt cancellation flag is registered before admission, closing
    // the race between queued requests and the first graph pump.
    let queued: Arc<PendingPrompts> = Arc::new(Mutex::new(VecDeque::new()));
    let controller = Controller::start(
        composition.clone(),
        management,
        credentials,
        resolvers,
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
    let provider_auth_begin = tx.clone();
    let provider_auth_poll = tx.clone();
    let provider_auth_cancel = tx.clone();
    let provider_models = tx.clone();
    let credential_capabilities = tx.clone();
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
            async move |request: ProviderAuthBeginRequest, reply, _peer| {
                submit(
                    &provider_auth_begin,
                    Command::ProviderAuthBegin(request, reply),
                )
            },
            acp::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ProviderAuthPollRequest, reply, _peer| {
                submit(
                    &provider_auth_poll,
                    Command::ProviderAuthPoll(request, reply),
                )
            },
            acp::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ProviderAuthCancelRequest, reply, _peer| {
                submit(
                    &provider_auth_cancel,
                    Command::ProviderAuthCancel(request, reply),
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
            async move |request: CredentialCapabilitiesRequest, reply, _peer| {
                submit(
                    &credential_capabilities,
                    Command::CredentialCapabilities(request, reply),
                )
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
    fn model_picker_budgets_all_connections_and_pages_globally() {
        let mut inspected = 0;
        assert!(charge_model_option_budget(&mut inspected, 2).is_ok());
        assert!(charge_model_option_budget(&mut inspected, 2).is_ok());
        assert!(charge_model_option_budget(&mut inspected, 2).is_err());
        assert_eq!(inspected, 2);
    }

    #[test]
    fn absent_discovery_never_becomes_a_false_model_offer() {
        assert!(is_missing_model_discovery(&ProviderErrorCode::Unavailable));
        assert!(is_missing_model_discovery(
            &ProviderErrorCode::AuthenticationRequired
        ));
        assert!(is_missing_model_discovery(&ProviderErrorCode::NotFound));
        assert!(!is_missing_model_discovery(&ProviderErrorCode::Denied));
        assert!(!is_missing_model_discovery(&ProviderErrorCode::Conflict));
        assert!(!is_missing_model_discovery(
            &ProviderErrorCode::InvalidInput
        ));
    }

    #[test]
    fn native_model_ids_bind_provider_account_transport_and_catalog_revision() {
        let selection = ModelSelection {
            provider_id: "provider".into(),
            connection_id: "api".into(),
            profile_id: Some("personal".into()),
            model_id: "model".into(),
            protocol_id: "openai-responses".into(),
            endpoint_id: "response".into(),
            catalog_revision: 7,
        };
        let id = native_model_option_id(&selection, 0).unwrap();
        assert!(id.starts_with("phenix.native-model@1:"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                id.strip_prefix("phenix.native-model@1:").unwrap()
            )
            .unwrap()["session_revision"],
            0
        );
        assert!(!id.contains("Bearer "));
        let mut other = selection.clone();
        other.profile_id = Some("work".into());
        assert_ne!(id, native_model_option_id(&other, 0).unwrap());
        other = selection.clone();
        other.protocol_id = "anthropic-messages".into();
        assert_ne!(id, native_model_option_id(&other, 0).unwrap());
        other = selection.clone();
        other.catalog_revision = 8;
        assert_ne!(id, native_model_option_id(&other, 0).unwrap());
    }

    #[test]
    fn stale_menu_is_typed_conflict_but_forged_and_future_choices_are_not() {
        let chosen = serde_json::json!({
            "provider_id": "fixture", "model_id": "model", "session_revision": 1
        });
        let id = format!("phenix.native-model@1:{chosen}");
        assert!(stale_native_model_choice(&id, 2));
        assert!(!stale_native_model_choice(&id, 1));
        assert!(!stale_native_model_choice(&id, 0));
        assert!(!stale_native_model_choice(
            "phenix.native-model@1:{\"provider_id\":\"forged\"}",
            2,
        ));
        assert!(!stale_native_model_choice("unrelated", 2));
    }

    #[test]
    fn model_choices_are_scoped_to_session_storage_revision() {
        let selection = ModelSelection {
            provider_id: "provider".into(),
            connection_id: "api".into(),
            profile_id: None,
            model_id: "a".into(),
            protocol_id: "openai-chat".into(),
            endpoint_id: "chat".into(),
            catalog_revision: 7,
        };
        let initial = native_model_option_id(&selection, 0).unwrap();
        assert_ne!(initial, native_model_option_id(&selection, 1).unwrap());
        assert!(is_previous_native_selection(&initial, &selection, 1).unwrap());
        assert!(!is_previous_native_selection(&initial, &selection, 2).unwrap());
        let mut other = selection.clone();
        other.model_id = "b".into();
        assert!(!is_previous_native_selection(&initial, &other, 1).unwrap());
    }

    #[test]
    fn mutation_requires_all_exact_launcher_roots() {
        let mut spec: Composition = serde_json::from_value(serde_json::json!({
            "format": "phenix.composition.v3",
            "entry": {"interface": "phenix.basic.input@1", "operation": "submit",
                      "provider": "phenix.basic.agent"},
            "artifacts": [], "definitions": [],
            "slots": [{
                "caller": "phenix.basic.agent", "slot": "Model", "index": 0,
                "provider": "phenix.model.http", "export": "Model"
            }],
            "grants": [], "excluded": [], "substitutions": [],
            "authority": {"operations": [], "resources": []},
            "resource": "phenix.session.s1"
        }))
        .unwrap();
        assert!(!can_switch_native_model(&spec));
        for (interface, operation) in [
            ("phenix.basic.storage@1", "read"),
            ("phenix.basic.storage@1", "transact"),
            ("phenix.provider.http-model-session@1", "install"),
        ] {
            spec.grants.push(
                serde_json::from_value(serde_json::json!({
                    "caller": "root", "interface": interface, "operation": operation,
                    "resources": ["phenix.session.s1"]
                }))
                .unwrap(),
            );
        }
        assert!(!can_switch_native_model(&spec));
        spec.authority.resources.insert("phenix.session.s1".into());
        for operation in [
            "phenix.basic.storage@1/read",
            "phenix.basic.storage@1/transact",
            "phenix.provider.http-model-session@1/install",
        ] {
            spec.authority.operations.insert(operation.into());
        }
        assert!(can_switch_native_model(&spec));
        spec.authority
            .operations
            .remove("phenix.basic.storage@1/transact");
        assert!(!can_switch_native_model(&spec));
        spec.authority
            .operations
            .insert("phenix.basic.storage@1/transact".into());
        spec.grants.push(spec.grants[0].clone());
        assert!(!can_switch_native_model(&spec));
    }

    #[test]
    fn acp_effective_model_idempotency_rejects_foreign_or_missing_choices() {
        assert!(exact_current_model(
            Some("selected-real-model"),
            "selected-real-model"
        ));
        assert!(!exact_current_model(
            Some("phenix.model.http"),
            "selected-real-model"
        ));
        assert!(!exact_current_model(
            Some("selected-real-model"),
            "other-account-model"
        ));
        assert!(!exact_current_model(Some("selected-real-model"), ""));
        assert!(!exact_current_model(None, "selected-real-model"));
    }

    #[test]
    fn only_one_exact_model_slot_is_advertisable_or_selectable() {
        let mut spec: Composition = serde_json::from_value(serde_json::json!({
            "format": "phenix.composition.v3",
            "entry": {"interface": "phenix.basic.input@1", "operation": "submit",
                      "provider": "phenix.basic.agent"},
            "artifacts": [], "definitions": [],
            "slots": [{
                "caller": "phenix.basic.agent", "slot": "Model", "index": 0,
                "provider": "phenix.model.http", "export": "Model"
            }],
            "grants": [], "excluded": [], "substitutions": [],
            "authority": {"operations": [], "resources": []},
            "resource": "phenix.session.s1"
        }))
        .unwrap();

        assert_eq!(installed_model_provider(&spec), Some("phenix.model.http"));
        // An unrelated plugin slot does not change the installed model.
        let mut other = spec.slots[0].clone();
        other.caller = "phenix.plugin.context".into();
        spec.slots.push(other);
        assert_eq!(installed_model_provider(&spec), Some("phenix.model.http"));

        // A second matching Model slot is ambiguous even if identical.
        spec.slots.push(spec.slots[0].clone());
        assert_eq!(installed_model_provider(&spec), None);
        spec.slots.pop();

        // A distinct conflicting Model provider must fail closed as well.
        let mut conflict = spec.slots[0].clone();
        conflict.provider = "phenix.model.other".into();
        spec.slots.push(conflict);
        assert_eq!(installed_model_provider(&spec), None);
        spec.slots
            .retain(|slot| slot.caller != "phenix.basic.agent");
        assert_eq!(installed_model_provider(&spec), None);
    }

    #[test]
    fn provider_model_pages_bound_revision_cursor_profile_and_size() {
        use phenix_provider_contracts::{ModelOrigin, ProviderModel};

        let fixture = |revision, profile: Option<&str>, next: Option<&str>| ModelPage {
            profile_id: profile.map(str::to_owned),
            models: vec![ProviderModel {
                id: "selected".into(),
                label: "Selected".into(),
                connection_id: "local".into(),
                protocol_id: "openai-chat".into(),
                endpoint_id: "chat".into(),
                capabilities: vec![],
                origin: ModelOrigin::Declared,
                selectable: true,
            }],
            next_cursor: next.map(str::to_owned),
            revision,
        };
        let mut acc = ModelPageAccumulator::default();
        assert_eq!(
            acc.push(fixture(7, Some("a"), Some("cursor-1")), &Some("a".into()))
                .unwrap(),
            Some("cursor-1".into())
        );
        assert!(acc
            .push(fixture(7, Some("a"), Some("cursor-1")), &Some("a".into()))
            .is_err());
        assert!(acc
            .push(fixture(8, Some("a"), None), &Some("a".into()))
            .is_err());
        assert!(acc
            .push(fixture(7, Some("other"), None), &Some("a".into()))
            .is_err());
        assert!(acc
            .push(fixture(7, Some("a"), Some("")), &Some("a".into()))
            .is_err());

        let mut clean = ModelPageAccumulator::default();
        assert_eq!(
            clean
                .push(fixture(7, Some("a"), None), &Some("a".into()))
                .unwrap(),
            None
        );
        let page = clean.complete(Some("a".into())).unwrap();
        assert_eq!(page.models.len(), 1);
        assert_eq!(page.revision, 7);
        assert_eq!(page.next_cursor, None);

        let mut huge = ModelPageAccumulator::default();
        let mut oversized = fixture(7, None, None);
        oversized.models = vec![oversized.models[0].clone(); 257];
        assert!(huge.push(oversized, &None).is_err());
        let mut empty_continuation = fixture(7, None, Some("cursor"));
        empty_continuation.models.clear();
        assert!(huge.push(empty_continuation, &None).is_err());
        huge.pages = 64;
        assert!(huge.push(fixture(7, None, None), &None).is_err());
    }

    #[test]
    fn native_model_credential_value_is_scrubbed_recursively() {
        let mut payload = Value::Map(std::collections::BTreeMap::from([
            (
                "auth".into(),
                Value::List(vec![Value::Map(std::collections::BTreeMap::from([(
                    "secret".into(),
                    Value::Bytes(b"fixture-private-api-key".to_vec()),
                )]))]),
            ),
            ("model".into(), Value::String("selected".into())),
        ]));
        scrub_portable_secret_bytes(&mut payload);
        let Value::Map(fields) = payload else {
            panic!("expected map");
        };
        let Value::List(auth) = &fields["auth"] else {
            panic!("expected auth list");
        };
        let Value::Map(inner) = &auth[0] else {
            panic!("expected auth map");
        };
        assert_eq!(inner["secret"], Value::Bytes(vec![0; 23]));
        assert_eq!(fields["model"], Value::String("selected".into()));
    }

    #[test]
    fn private_model_resolver_composition_has_one_exact_resolve_only_grant() {
        let identity = CredentialIdentity::new("openai", "api", "personal").unwrap();
        let resource = identity.resource().unwrap();
        let contract = "phenix.provider.credentials@1";
        let spec: Composition = serde_json::from_value(serde_json::json!({
            "format": "phenix.composition.v3",
            "entry": {"interface": contract, "operation": "resolve",
                      "provider": "phenix.provider.credential-files"},
            "artifacts": [], "definitions": [], "slots": [],
            "grants": [{"caller": "root", "interface": contract,
                        "operation": "resolve", "resources": [resource]}],
            "excluded": [], "substitutions": [],
            "authority": {
                "operations": ["phenix.provider.credentials@1/resolve"],
                "resources": [resource]
            },
            "resource": resource
        }))
        .unwrap();
        assert_eq!(validate_model_resolver_spec(&spec).unwrap(), identity);

        let mut excessive = spec.clone();
        excessive
            .authority
            .operations
            .insert("phenix.provider.credentials@1/import".into());
        assert!(validate_model_resolver_spec(&excessive).is_err());

        let mut excessive = spec.clone();
        excessive.grants.push(excessive.grants[0].clone());
        assert!(validate_model_resolver_spec(&excessive).is_err());

        let mut foreign = spec.clone();
        foreign.resource = Some(
            CredentialIdentity::new("openai", "api", "work")
                .unwrap()
                .resource()
                .unwrap(),
        );
        assert!(validate_model_resolver_spec(&foreign).is_err());

        let mut forged = spec.clone();
        forged.entry.operation = "revoke".into();
        assert!(validate_model_resolver_spec(&forged).is_err());
    }

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
