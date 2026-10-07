#![forbid(unsafe_code)]

mod credentials;
mod oauth;
mod providers;
mod schema_adapter;

use credentials::{CredentialStore, StoredCredential};
use futures::StreamExt;
use genai::Client as ProviderClient;
use genai::chat::{
    ChatMessage, ChatOptions, ChatRequest, ChatStreamEvent, ReasoningEffort, Tool, ToolResponse,
};
use genai::resolver::AuthResolver;
use phenix_domain::{
    AuthenticationInput, AuthenticationMethodDescriptor, AuthenticationMethodId,
    AuthenticationMethodKind, AuthenticationState, InferenceEffort, InferenceOptions,
    ModelAdapterCatalog, ModelAdapterId, ModelDescriptor, ModelId, ModelProviderId, ModelTarget,
    SessionId,
};
use phenix_model_adapter::{
    ModelAdapter, ModelAdapterError, ModelAdapterFeatures, ModelAdapterHost, ModelEvent,
    ModelExecutionRequest, ModelSession, ModelSessionRequest, PreparedToolSurface, ToolInvocation,
    ToolPresentation,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub const MODEL_ADAPTER_ID: &str = "phenix";

fn parse_configured_model(value: &str) -> Result<ModelTarget, ModelAdapterError> {
    let (provider, model) = value.split_once('/').ok_or_else(|| {
        ModelAdapterError::Protocol(format!(
            "Phenix model selection {value:?} must be provider/model"
        ))
    })?;
    if provider.trim().is_empty() || model.trim().is_empty() {
        return Err(ModelAdapterError::Protocol(format!(
            "Phenix model selection {value:?} must be provider/model"
        )));
    }
    let target = ModelTarget {
        adapter: ModelAdapterId::parse(MODEL_ADAPTER_ID)
            .map_err(|error| ModelAdapterError::Protocol(error.to_string()))?,
        provider: ModelProviderId::parse(provider)
            .map_err(|error| ModelAdapterError::Protocol(error.to_string()))?,
        model: ModelId::parse(model)
            .map_err(|error| ModelAdapterError::Protocol(error.to_string()))?,
        inference: InferenceOptions::default(),
    };
    validate_model_target(&target)?;
    Ok(target)
}

fn model_wire_value(target: &ModelTarget) -> String {
    format!("{}/{}", target.provider, target.model)
}

fn validate_model_target(target: &ModelTarget) -> Result<(), ModelAdapterError> {
    if target.adapter.as_str() != MODEL_ADAPTER_ID {
        return Err(ModelAdapterError::Unsupported(format!(
            "Phenix adapter cannot serve target adapter {}",
            target.adapter
        )));
    }
    if providers::is_gateway_provider(&target.provider) {
        providers::validate_gateway_model(&target.provider, &target.model)
    } else {
        providers::genai_model(&target.provider, &target.model).map(|_| ())
    }
}

fn provider_reasoning_effort(effort: &InferenceEffort) -> ReasoningEffort {
    match effort {
        InferenceEffort::None => ReasoningEffort::None,
        InferenceEffort::Minimal => ReasoningEffort::Minimal,
        InferenceEffort::Low => ReasoningEffort::Low,
        InferenceEffort::Medium => ReasoningEffort::Medium,
        InferenceEffort::High => ReasoningEffort::High,
        InferenceEffort::ExtraHigh => ReasoningEffort::XHigh,
        InferenceEffort::Max => ReasoningEffort::Max,
    }
}

fn provider_execution_error(context: &str, error: impl Display) -> ModelAdapterError {
    let message = error.to_string();
    let normalized = message.to_ascii_lowercase();
    let overflow = [
        "context_length_exceeded",
        "maximum context length",
        "context window",
        "too many tokens",
        "input is too long",
        "prompt is too long",
    ]
    .iter()
    .any(|needle| normalized.contains(needle));
    if overflow {
        ModelAdapterError::ContextOverflow(format!("{context}: {message}"))
    } else {
        ModelAdapterError::Transport(format!("{context}: {message}"))
    }
}

fn dispatch_tool_call<T: serde::Serialize + ?Sized>(
    tools: &PreparedToolSurface,
    host: &mut dyn ModelAdapterHost,
    fn_name: &str,
    fn_arguments: &T,
) -> Result<String, ModelAdapterError> {
    let Some(descriptor) = tools
        .callables()
        .iter()
        .find(|descriptor| descriptor.id.as_str() == fn_name)
    else {
        return Ok(json!({
            "error": format!("unknown or unavailable Phenix tool {fn_name:?}")
        })
        .to_string());
    };
    let arguments = match serde_json::to_value(fn_arguments) {
        Ok(arguments) => arguments,
        Err(error) => {
            return Ok(json!({
                "error": format!("cannot encode tool arguments: {error}")
            })
            .to_string());
        }
    };
    let arguments = match schema_adapter::model_tool_arguments(&descriptor.input_schema, arguments) {
        Ok(arguments) => arguments,
        Err(error) => {
            return Ok(json!({
                "error": format!("invalid tool arguments: {error}")
            })
            .to_string());
        }
    };
    let arguments_json = serde_json::to_string(&arguments)
        .map_err(|error| ModelAdapterError::Protocol(format!(
            "cannot encode projected tool arguments: {error}"
        )))?;
    match host.invoke_tool(ToolInvocation {
        callable: descriptor.id.clone(),
        arguments_json,
    }) {
        Ok(result) if result.success => Ok(result.output),
        Ok(result) => Ok(json!({ "error": result.output }).to_string()),
        Err(ModelAdapterError::Protocol(error)) => {
            Ok(json!({ "error": format!("tool dispatch failed: {error}") }).to_string())
        }
        Err(error) => Err(error),
    }
}

pub struct NativeModelAdapter {
    runtime: Arc<tokio::runtime::Runtime>,
    provider: Arc<ProviderClient>,
    codex_provider: Arc<ProviderClient>,
    credentials: CredentialStore,
    models: Vec<ModelTarget>,
    max_tool_rounds: Option<NonZeroUsize>,
    persistent_sessions: BTreeMap<SessionId, Arc<NativeModelSession>>,
}

impl NativeModelAdapter {
    pub fn from_environment() -> Result<Self, ModelAdapterError> {
        let credentials = CredentialStore::discover().map_err(ModelAdapterError::Protocol)?;
        let resolver_store = credentials.clone();
        let auth_resolver =
            AuthResolver::from_resolver_fn(move |model| resolver_store.auth_for_model(model));
        let provider = ProviderClient::builder()
            .with_auth_resolver(auth_resolver)
            .build();
        let codex_oauth = oauth::CodexOAuth::new(credentials.clone());
        let codex_auth_resolver = AuthResolver::from_resolver_async_fn(
            move |_model| -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<
                                Option<genai::resolver::AuthData>,
                                genai::resolver::Error,
                            >,
                        > + Send,
                >,
            > {
                let codex_oauth = codex_oauth.clone();
                Box::pin(async move {
                    codex_oauth
                        .auth_data()
                        .await
                        .map_err(genai::resolver::Error::Custom)
                })
            },
        );
        let codex_provider = ProviderClient::builder()
            .with_auth_resolver(codex_auth_resolver)
            .build();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| {
                ModelAdapterError::Transport(format!(
                    "cannot start native model adapter Tokio runtime: {error}"
                ))
            })?;
        Ok(Self {
            runtime: Arc::new(runtime),
            provider: Arc::new(provider),
            codex_provider: Arc::new(codex_provider),
            credentials,
            models: configured_models()?,
            max_tool_rounds: configured_max_tool_rounds()?,
            persistent_sessions: BTreeMap::new(),
        })
    }

    fn validate_request(&self, request: &ModelSessionRequest) -> Result<(), ModelAdapterError> {
        validate_model_target(&request.model)?;
        if !request.tools.is_empty()
            && request.tools.presentation() != Some(ToolPresentation::Native)
        {
            return Err(ModelAdapterError::Unsupported(
                "Phenix adapter requires native runtime tool presentation".to_owned(),
            ));
        }
        Ok(())
    }

    fn new_session(&self, request: ModelSessionRequest) -> Arc<NativeModelSession> {
        Arc::new(NativeModelSession {
            runtime: Arc::clone(&self.runtime),
            provider: Arc::clone(&self.provider),
            codex_provider: Arc::clone(&self.codex_provider),
            credentials: self.credentials.clone(),
            model: Mutex::new(request.model),
            tools: Mutex::new(request.tools),
            max_tool_rounds: self.max_tool_rounds,
            history: Mutex::new(Vec::new()),
            active: Mutex::new(false),
            cancelled: AtomicBool::new(false),
        })
    }
}

impl ModelAdapter for NativeModelAdapter {
    fn features(&self) -> ModelAdapterFeatures {
        ModelAdapterFeatures {
            tool_presentations: BTreeSet::from([ToolPresentation::Native]),
            images: false,
            persistent_sessions: true,
        }
    }

    fn catalog(&mut self) -> Result<ModelAdapterCatalog, ModelAdapterError> {
        let adapter = ModelAdapterId::parse(MODEL_ADAPTER_ID)
            .map_err(|error| ModelAdapterError::Protocol(error.to_string()))?;
        let auth_providers = self
            .models
            .iter()
            .filter_map(|target| providers::canonical_auth_provider(&target.provider))
            .collect::<BTreeSet<_>>();
        let models = self
            .models
            .iter()
            .map(|target| model_descriptor(&self.credentials, target))
            .collect::<Result<Vec<_>, ModelAdapterError>>()?;
        let mut authentication_methods = Vec::new();
        if auth_providers.contains(oauth::PROVIDER) {
            authentication_methods.push(AuthenticationMethodDescriptor {
                id: AuthenticationMethodId::parse(oauth::PROVIDER)
                    .map_err(|error| ModelAdapterError::Protocol(error.to_string()))?,
                adapter: adapter.clone(),
                provider: ModelProviderId::parse(oauth::PROVIDER)
                    .map_err(|error| ModelAdapterError::Protocol(error.to_string()))?,
                kind: AuthenticationMethodKind::Agent,
                name: "OpenAI Codex (ChatGPT OAuth)".to_owned(),
                description: Some("Browser OAuth for ChatGPT subscription access".to_owned()),
                selectable: true,
            });
        }
        for provider in &auth_providers {
            if *provider == oauth::PROVIDER || !providers::is_api_key_auth_provider(provider) {
                continue;
            }
            authentication_methods.push(AuthenticationMethodDescriptor {
                id: AuthenticationMethodId::parse(*provider)
                    .map_err(|error| ModelAdapterError::Protocol(error.to_string()))?,
                adapter: adapter.clone(),
                provider: ModelProviderId::parse(*provider)
                    .map_err(|error| ModelAdapterError::Protocol(error.to_string()))?,
                kind: AuthenticationMethodKind::ApiKey,
                name: providers::environment_name(provider)
                    .expect("known API-key provider has a name")
                    .to_owned(),
                description: providers::environment_description(provider).map(str::to_owned),
                selectable: true,
            });
        }
        let mut any_authenticated = false;
        for provider in &auth_providers {
            let provider_id = ModelProviderId::parse(*provider)
                .map_err(|error| ModelAdapterError::Protocol(error.to_string()))?;
            if provider_has_valid_auth(&self.credentials, &provider_id)? {
                any_authenticated = true;
                break;
            }
        }
        let authentication_state = if auth_providers.is_empty() {
            AuthenticationState::NotRequired
        } else if any_authenticated {
            // Authentication is provider-specific while the ACP catalog exposes one
            // adapter-wide state. Treat the adapter as usable once any configured route
            // has credentials, and let the selected provider report its own missing key.
            AuthenticationState::Authenticated
        } else {
            AuthenticationState::Required
        };
        Ok(ModelAdapterCatalog {
            adapter,
            models,
            authentication_state,
            authentication_methods,
        })
    }

    fn authenticate(&mut self, method: &AuthenticationMethodId) -> Result<(), ModelAdapterError> {
        if method.as_str() != oauth::PROVIDER {
            return Err(ModelAdapterError::Unsupported(format!(
                "Phenix adapter authentication method {method} requires typed authentication input"
            )));
        }
        self.runtime
            .block_on(oauth::login(&self.credentials))
            .map_err(ModelAdapterError::Transport)
    }

    fn authenticate_with_input(
        &mut self,
        method: &AuthenticationMethodId,
        input: Option<&AuthenticationInput>,
    ) -> Result<(), ModelAdapterError> {
        if method.as_str() == oauth::PROVIDER {
            if input.is_some() {
                return Err(ModelAdapterError::Protocol(
                    "OpenAI Codex OAuth does not accept an API-key payload".to_owned(),
                ));
            }
            return self.authenticate(method);
        }
        if !providers::is_api_key_auth_provider(method.as_str()) {
            return Err(ModelAdapterError::Unsupported(format!(
                "Phenix adapter does not expose authentication method {method}"
            )));
        }
        let Some(AuthenticationInput::ApiKey { secret }) = input else {
            return Err(ModelAdapterError::Protocol(format!(
                "Phenix authentication method {method} requires an API key"
            )));
        };
        self.credentials
            .save_api_key(method.as_str(), secret)
            .map_err(ModelAdapterError::Protocol)
    }

    fn open_session(
        &mut self,
        request: ModelSessionRequest,
    ) -> Result<Arc<dyn ModelSession>, ModelAdapterError> {
        self.validate_request(&request)?;
        Ok(self.new_session(request))
    }

    fn open_persistent_session(
        &mut self,
        session_id: &SessionId,
        request: ModelSessionRequest,
    ) -> Result<Arc<dyn ModelSession>, ModelAdapterError> {
        self.validate_request(&request)?;
        if let Some(session) = self.persistent_sessions.get(session_id) {
            session.set_request(request.model, request.tools)?;
            return Ok(session.clone());
        }
        let session = self.new_session(request);
        self.persistent_sessions
            .insert(session_id.clone(), session.clone());
        Ok(session)
    }

    fn close_persistent_session(
        &mut self,
        session_id: &SessionId,
    ) -> Result<(), ModelAdapterError> {
        self.persistent_sessions.remove(session_id);
        Ok(())
    }
}

struct NativeModelSession {
    runtime: Arc<tokio::runtime::Runtime>,
    provider: Arc<ProviderClient>,
    codex_provider: Arc<ProviderClient>,
    credentials: CredentialStore,
    model: Mutex<ModelTarget>,
    tools: Mutex<PreparedToolSurface>,
    max_tool_rounds: Option<NonZeroUsize>,
    history: Mutex<Vec<ChatMessage>>,
    active: Mutex<bool>,
    cancelled: AtomicBool,
}

impl NativeModelSession {
    fn set_request(
        &self,
        model: ModelTarget,
        tools: PreparedToolSurface,
    ) -> Result<(), ModelAdapterError> {
        *self
            .model
            .lock()
            .map_err(|_| ModelAdapterError::Protocol("Phenix model lock poisoned".to_owned()))? =
            model;
        *self
            .tools
            .lock()
            .map_err(|_| ModelAdapterError::Protocol("Phenix tool lock poisoned".to_owned()))? =
            tools;
        Ok(())
    }

    async fn execute_turn(
        &self,
        prompt: String,
        host: &mut dyn ModelAdapterHost,
    ) -> Result<Vec<ChatMessage>, ModelAdapterError> {
        let model = self
            .model
            .lock()
            .map_err(|_| ModelAdapterError::Protocol("Phenix model lock poisoned".to_owned()))?
            .clone();
        let tools = self
            .tools
            .lock()
            .map_err(|_| ModelAdapterError::Protocol("Phenix tool lock poisoned".to_owned()))?
            .clone();
        let mut history = self
            .history
            .lock()
            .map_err(|_| ModelAdapterError::Protocol("Phenix history lock poisoned".to_owned()))?
            .clone();
        history.push(ChatMessage::user(prompt));

        let provider = if model.provider.as_str() == oauth::PROVIDER {
            &self.codex_provider
        } else {
            &self.provider
        };
        let provider_target =
            match providers::gateway_target(&self.credentials, &model.provider, &model.model)? {
                Some(target) => target,
                None => {
                    let provider_model = providers::genai_model(&model.provider, &model.model)?;
                    provider
                        .resolve_service_target(provider_model)
                        .await
                        .map_err(|error| {
                            ModelAdapterError::Transport(format!(
                                "cannot resolve provider target for {}: {error}",
                                model_wire_value(&model)
                            ))
                        })?
                }
            };
        let tool_definitions = tools
            .callables()
            .iter()
            .map(|descriptor| {
                let schema = schema_adapter::model_tool_json_schema(&descriptor.input_schema)?;
                Ok(Tool::new(descriptor.id.as_str())
                    .with_description(descriptor.description.clone())
                    .with_schema(schema))
            })
            .collect::<Result<Vec<_>, ModelAdapterError>>()?;
        let reasoning_effort = model
            .inference
            .effort
            .as_ref()
            .map(provider_reasoning_effort);
        let mut tool_rounds = 0usize;

        loop {
            if let Some(limit) = self.max_tool_rounds
                && tool_rounds >= limit.get()
            {
                return Err(ModelAdapterError::Protocol(format!(
                    "provider exceeded {limit} consecutive tool rounds"
                )));
            }
            tool_rounds += 1;
            if self.cancelled.load(Ordering::Acquire) {
                return Ok(history);
            }
            let request = ChatRequest::new(history.clone()).with_tools(tool_definitions.clone());
            let mut options = ChatOptions::default()
                .with_capture_content(true)
                .with_capture_reasoning_content(true)
                .with_capture_tool_calls(true);
            if let Some(effort) = reasoning_effort.clone() {
                options = options.with_reasoning_effort(effort);
            }
            let mut stream = provider
                .exec_chat_stream(provider_target.clone(), request, Some(&options))
                .await
                .map_err(|error| provider_execution_error("provider request failed", error))?;
            let mut captured = None;
            while let Some(event) = stream.stream.next().await {
                if self.cancelled.load(Ordering::Acquire) {
                    return Ok(history);
                }
                match event
                    .map_err(|error| provider_execution_error("provider stream failed", error))?
                {
                    ChatStreamEvent::Chunk(chunk) => {
                        host.emit(ModelEvent::ContentDelta(chunk.content))?;
                    }
                    ChatStreamEvent::ReasoningChunk(chunk) => {
                        host.emit(ModelEvent::ReasoningDelta(chunk.content))?;
                    }
                    ChatStreamEvent::End(end) => captured = end.captured_content,
                    _ => {}
                }
            }
            let content = captured.unwrap_or_default();
            let tool_calls = content
                .tool_calls()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>();
            history.push(ChatMessage::assistant(content));
            if tool_calls.is_empty() {
                return Ok(history);
            }

            let mut responses = Vec::new();
            for call in tool_calls {
                let output = dispatch_tool_call(&tools, host, &call.fn_name, &call.fn_arguments)?;
                responses.push(ToolResponse::new(call.call_id, output));
            }
            history.push(ChatMessage::from(responses));
        }
    }
}

impl ModelSession for NativeModelSession {
    fn execute(
        &self,
        request: ModelExecutionRequest,
        host: &mut dyn ModelAdapterHost,
    ) -> Result<(), ModelAdapterError> {
        {
            let mut active = self.active.lock().map_err(|_| {
                ModelAdapterError::Protocol("Phenix active lock poisoned".to_owned())
            })?;
            if *active {
                return Err(ModelAdapterError::Protocol(
                    "Phenix adapter session is already executing".to_owned(),
                ));
            }
            *active = true;
        }
        self.cancelled.store(false, Ordering::Release);
        let result = self
            .runtime
            .block_on(self.execute_turn(request.prompt, host));
        if let Ok(mut active) = self.active.lock() {
            *active = false;
        }
        if let Ok(history) = &result {
            *self.history.lock().map_err(|_| {
                ModelAdapterError::Protocol("Phenix history lock poisoned".to_owned())
            })? = history.clone();
        }
        result.map(|_| ())
    }

    fn cancel(&self, _execution_id: &phenix_domain::ExecutionId) -> Result<(), ModelAdapterError> {
        self.cancelled.store(true, Ordering::Release);
        Ok(())
    }
}

fn provider_has_valid_auth(
    credentials: &CredentialStore,
    provider: &ModelProviderId,
) -> Result<bool, ModelAdapterError> {
    let Some(provider) = providers::canonical_auth_provider(provider) else {
        // Supported providers without an auth adapter, such as local Ollama,
        // do not require credentials and remain selectable.
        return Ok(true);
    };
    let stored = credentials
        .resolve(provider)
        .map_err(ModelAdapterError::Protocol)?;
    if provider == oauth::PROVIDER {
        return Ok(matches!(stored, Some(StoredCredential::OAuth { .. })));
    }
    if providers::is_api_key_auth_provider(provider) {
        let stored_key = matches!(
            stored,
            Some(StoredCredential::ApiKey { ref secret }) if !secret.trim().is_empty()
        );
        return Ok(stored_key || providers::environment_authenticated(provider));
    }
    Ok(false)
}

fn model_descriptor(
    credentials: &CredentialStore,
    target: &ModelTarget,
) -> Result<ModelDescriptor, ModelAdapterError> {
    Ok(ModelDescriptor {
        target: target.clone(),
        name: model_wire_value(target),
        selectable: provider_has_valid_auth(credentials, &target.provider)?,
        context_capacity: None,
    })
}

fn configured_max_tool_rounds() -> Result<Option<NonZeroUsize>, ModelAdapterError> {
    let value = std::env::var("PHENIX_MAX_TOOL_ROUNDS").ok();
    parse_max_tool_rounds(value.as_deref())
}

fn parse_max_tool_rounds(value: Option<&str>) -> Result<Option<NonZeroUsize>, ModelAdapterError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let parsed = value.parse::<usize>().map_err(|error| {
        ModelAdapterError::Protocol(format!(
            "PHENIX_MAX_TOOL_ROUNDS must be a positive integer: {error}"
        ))
    })?;
    NonZeroUsize::new(parsed).map(Some).ok_or_else(|| {
        ModelAdapterError::Protocol("PHENIX_MAX_TOOL_ROUNDS must be greater than zero".to_owned())
    })
}

fn configured_models() -> Result<Vec<ModelTarget>, ModelAdapterError> {
    let source = std::env::var("PHENIX_MODELS")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| std::env::var("PHENIX_MODEL").ok())
        .unwrap_or_else(|| providers::DEFAULT_MODELS.join(","));
    let mut seen = BTreeSet::new();
    let mut models = Vec::new();
    for value in source
        .split([',', ';', '\n'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let target = parse_configured_model(value)?;
        if seen.insert((target.provider.clone(), target.model.clone())) {
            models.push(target);
        }
    }
    if models.is_empty() {
        return Err(ModelAdapterError::Protocol(
            "Phenix model catalog must contain at least one provider/model".to_owned(),
        ));
    }
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_domain::{
        CallableDescriptor, CallableFeatureSet, CallableId, CallableKind, CallablePolicy,
        PhenixSchema,
    };
    use phenix_model_adapter::{ToolProvision, ToolResult};
    use serde_json::json;

    #[test]
    fn provider_context_overflow_is_typed() {
        assert!(matches!(
            provider_execution_error(
                "provider request failed",
                "context_length_exceeded: maximum context length reached"
            ),
            ModelAdapterError::ContextOverflow(_)
        ));
        assert!(matches!(
            provider_execution_error("provider request failed", "connection reset"),
            ModelAdapterError::Transport(_)
        ));
    }

    #[test]
    fn model_catalog_marks_provider_auth_selectability() {
        use std::fs;
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "phenix-model-auth-test-{}-{unique}",
            std::process::id()
        ));
        let credentials = CredentialStore {
            path: root.join("credentials.json"),
        };
        let codex = parse_configured_model("openai-codex/gpt-test").unwrap();
        let local = parse_configured_model("ollama/local-test").unwrap();

        assert!(!model_descriptor(&credentials, &codex).unwrap().selectable);
        assert!(model_descriptor(&credentials, &local).unwrap().selectable);

        credentials
            .save_oauth(
                oauth::PROVIDER,
                StoredCredential::OAuth {
                    access_token: "access".to_owned(),
                    refresh_token: "refresh".to_owned(),
                    id_token: "id".to_owned(),
                    account_id: "account".to_owned(),
                    expires_at: u64::MAX,
                },
            )
            .unwrap();
        assert!(model_descriptor(&credentials, &codex).unwrap().selectable);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn model_identity_remains_nominal_and_rejects_aliases() {
        let target = parse_configured_model("openai-codex/gpt-5.6-sol").unwrap();
        assert_eq!(target.adapter.as_str(), MODEL_ADAPTER_ID);
        assert_eq!(target.provider.as_str(), "openai-codex");
        assert_eq!(target.model.as_str(), "gpt-5.6-sol");
        assert_eq!(
            providers::genai_model(&target.provider, &target.model).unwrap(),
            "openai_resp::gpt-5.6-sol"
        );
        for alias in [
            "openai/gpt-5.6-sol",
            "openai-responses/gpt-5.6-sol",
            "google/gemini-test",
            "opencode/model",
            "openrouter/model",
        ] {
            assert!(parse_configured_model(alias).is_err(), "alias {alias}");
        }
    }

    #[test]
    fn native_backend_negotiates_native_tools() {
        let features = ModelAdapterFeatures {
            tool_presentations: BTreeSet::from([ToolPresentation::Native]),
            images: false,
            persistent_sessions: true,
        };
        let surface = ToolProvision::default().prepare(&features).unwrap();
        assert!(surface.is_empty());
        assert!(features.persistent_sessions);
    }

    #[test]
    fn tool_round_limit_is_opt_in() {
        assert_eq!(parse_max_tool_rounds(None).unwrap(), None);
        assert_eq!(parse_max_tool_rounds(Some("  ")).unwrap(), None);
        assert_eq!(parse_max_tool_rounds(Some("7")).unwrap().unwrap().get(), 7);
        assert!(matches!(
            parse_max_tool_rounds(Some("0")),
            Err(ModelAdapterError::Protocol(_))
        ));
        assert!(matches!(
            parse_max_tool_rounds(Some("many")),
            Err(ModelAdapterError::Protocol(_))
        ));
    }

    #[test]
    fn reasoning_effort_is_canonical_core_domain() {
        assert!(matches!(
            provider_reasoning_effort(&InferenceEffort::High),
            ReasoningEffort::High
        ));
        assert!(matches!(
            provider_reasoning_effort(&InferenceEffort::ExtraHigh),
            ReasoningEffort::XHigh
        ));
    }

    fn test_tool_surface() -> PreparedToolSurface {
        ToolProvision {
            callables: vec![CallableDescriptor {
                id: CallableId::parse("read").unwrap(),
                kind: CallableKind::Tool,
                description: "test read".to_owned(),
                input_schema: PhenixSchema::Map(Box::new(PhenixSchema::Any)),
                output_schema: PhenixSchema::Map(Box::new(PhenixSchema::Any)),
                features: CallableFeatureSet::default(),
                policy: CallablePolicy::default(),
            }],
        }
        .prepare(&ModelAdapterFeatures {
            tool_presentations: BTreeSet::from([ToolPresentation::Native]),
            images: false,
            persistent_sessions: false,
        })
        .unwrap()
    }

    struct TestToolHost {
        result: Result<ToolResult, ModelAdapterError>,
        calls: usize,
    }

    impl ModelAdapterHost for TestToolHost {
        fn emit(&mut self, _event: ModelEvent) -> Result<(), ModelAdapterError> {
            Ok(())
        }

        fn invoke_tool(
            &mut self,
            _invocation: ToolInvocation,
        ) -> Result<ToolResult, ModelAdapterError> {
            self.calls += 1;
            self.result.clone()
        }
    }

    fn tool_surface_with_schema(id: &str, input_schema: PhenixSchema) -> PreparedToolSurface {
        ToolProvision {
            callables: vec![CallableDescriptor {
                id: CallableId::parse(id).unwrap(),
                kind: CallableKind::Tool,
                description: "test tool".to_owned(),
                input_schema,
                output_schema: PhenixSchema::String,
                features: CallableFeatureSet::default(),
                policy: CallablePolicy::default(),
            }],
        }
        .prepare(&ModelAdapterFeatures {
            tool_presentations: BTreeSet::from([ToolPresentation::Native]),
            images: false,
            persistent_sessions: false,
        })
        .unwrap()
    }

    #[derive(Default)]
    struct CapturingToolHost {
        invocation: Option<ToolInvocation>,
    }

    impl ModelAdapterHost for CapturingToolHost {
        fn emit(&mut self, _event: ModelEvent) -> Result<(), ModelAdapterError> {
            Ok(())
        }

        fn invoke_tool(
            &mut self,
            invocation: ToolInvocation,
        ) -> Result<ToolResult, ModelAdapterError> {
            self.invocation = Some(invocation);
            Ok(ToolResult {
                output: "ok".to_owned(),
                success: true,
            })
        }
    }

    #[test]
    fn native_dispatch_projects_model_object_arguments_back_to_canonical_inputs() {
        let mut host = CapturingToolHost::default();
        let unit_tools = tool_surface_with_schema("unit", PhenixSchema::Unit);
        assert_eq!(
            dispatch_tool_call(&unit_tools, &mut host, "unit", &json!({})).unwrap(),
            "ok"
        );
        assert_eq!(
            host.invocation.take().unwrap().arguments_json,
            "null",
            "unit input must reach the runtime as canonical JSON null"
        );

        let scalar_tools = tool_surface_with_schema("scalar", PhenixSchema::U64);
        assert_eq!(
            dispatch_tool_call(
                &scalar_tools,
                &mut host,
                "scalar",
                &json!({"value": 7})
            )
            .unwrap(),
            "ok"
        );
        assert_eq!(
            host.invocation.take().unwrap().arguments_json,
            "7",
            "scalar envelope must be removed before runtime dispatch"
        );
    }

    #[test]
    fn faulty_tool_calls_are_returned_to_the_model_and_transport_failures_remain_fatal() {
        let tools = test_tool_surface();
        let mut host = TestToolHost {
            result: Ok(ToolResult {
                output: "missing file".to_owned(),
                success: false,
            }),
            calls: 0,
        };
        let failed =
            dispatch_tool_call(&tools, &mut host, "read", &json!({"path": "missing"})).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&failed).unwrap()["error"],
            "missing file"
        );
        assert_eq!(host.calls, 1);

        host.result = Ok(ToolResult {
            output: "recovered".to_owned(),
            success: true,
        });
        let recovered =
            dispatch_tool_call(&tools, &mut host, "read", &json!({"path": "valid"})).unwrap();
        assert_eq!(recovered, "recovered");
        assert_eq!(
            host.calls, 2,
            "a failed tool call must not poison later calls"
        );

        let unknown = dispatch_tool_call(&tools, &mut host, "made_up_tool", &json!({})).unwrap();
        assert!(unknown.contains("unknown or unavailable Phenix tool"));
        assert_eq!(host.calls, 2, "unknown tools must not reach the host");

        let mut protocol_host = TestToolHost {
            result: Err(ModelAdapterError::Protocol("bad tool request".to_owned())),
            calls: 0,
        };
        let protocol = dispatch_tool_call(&tools, &mut protocol_host, "read", &json!({})).unwrap();
        assert!(protocol.contains("tool dispatch failed"));

        let mut transport_host = TestToolHost {
            result: Err(ModelAdapterError::Transport(
                "persistence unavailable".to_owned(),
            )),
            calls: 0,
        };
        assert!(matches!(
            dispatch_tool_call(&tools, &mut transport_host, "read", &json!({})),
            Err(ModelAdapterError::Transport(_))
        ));
    }
}
