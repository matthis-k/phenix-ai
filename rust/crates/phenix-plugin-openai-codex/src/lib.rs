#![forbid(unsafe_code)]

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use phenix_core::{
    model_inference_service, Authority, CapabilityId, ComponentExport, ComponentId,
    ComponentInterface, ComponentManifest, ModelInferenceInterface, ModelInferenceRequest,
    ModelInferenceResponse, PhenixValue, PluginContext, PluginExecution, PluginHost, PluginId,
    PluginInstance, PluginManifest, ServiceContribution, ServiceId, ServiceRole,
};
use phenix_provider_sdk::{
    encode_model_inference_outcome, normalize_http_error, provider_auth_service,
    provider_http_client_builder, AuthDescriptor, AuthKind, Endpoint, HttpMethod, Protocol,
    ProtocolAdapter, ProviderAuthCommand, ProviderAuthInterface, ProviderAuthMethod,
    ProviderAuthResponse, ProviderAuthenticationResult, ProviderError, ProviderRequest,
    ProviderResponse, RateLimits, NETWORK_HTTP_CAPABILITY, SECRETS_MANAGE_CAPABILITY,
};
use reqwest::header::{HeaderName, HeaderValue, AUTHORIZATION};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};
use url::Url;

pub const OPENAI_CODEX_PROVIDER: &str = "openai-codex";
const AUTH_METHOD: &str = "oauth";
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const ISSUER: &str = "https://auth.openai.com";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const RESPONSES_ENDPOINT: &str = "https://chatgpt.com/backend-api/codex/";
// ChatGPT gates newer Codex models on this client compatibility header. Keep it
// distinct from the Phenix crate version and advance it only with verified wire parity.
const CODEX_COMPAT_VERSION: &str = "0.156.0";
const CREDENTIAL_FILE_ENV: &str = "PHENIX_CREDENTIAL_FILE";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MAX_CALLBACK_REQUEST_BYTES: usize = 16 * 1024;
const REFRESH_MARGIN_SECONDS: u64 = 5 * 60;

#[must_use]
pub fn openai_codex_manifest() -> PluginManifest {
    let network = network_authority();
    let secrets = secrets_authority();
    PluginManifest {
        id: provider_id(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![
            ServiceContribution {
                role: ServiceRole::Terminal,
                service: model_inference_service(),
                priority: 100,
                required_authority: network.clone(),
            },
            ServiceContribution {
                role: ServiceRole::Terminal,
                service: provider_auth_service(),
                priority: 100,
                required_authority: secrets.clone(),
            },
        ],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::new(
            network
                .capabilities()
                .cloned()
                .chain(secrets.capabilities().cloned()),
        ),
    }
}

#[must_use]
pub fn openai_codex_component_manifest() -> ComponentManifest {
    ComponentManifest {
        id: ComponentId::parse(OPENAI_CODEX_PROVIDER)
            .expect("static Codex provider component id is valid"),
        owner: provider_id(),
        imports: Vec::new(),
        exports: vec![
            ComponentExport {
                interface: ModelInferenceInterface::interface_id(),
                schema: ModelInferenceInterface::schema(),
                priority: 100,
                required_authority: network_authority(),
            },
            ComponentExport {
                interface: ProviderAuthInterface::interface_id(),
                schema: ProviderAuthInterface::schema(),
                priority: 100,
                required_authority: secrets_authority(),
            },
        ],
        listeners: Vec::new(),
        maximum_authority: openai_codex_manifest().maximum_authority,
    }
}

#[must_use]
pub fn openai_codex_factory() -> Box<dyn PluginInstance> {
    Box::new(OpenAiCodexPlugin::default())
}

fn provider_id() -> PluginId {
    PluginId::parse(OPENAI_CODEX_PROVIDER).expect("static Codex provider id is valid")
}

fn network_authority() -> Authority {
    Authority::new([capability(NETWORK_HTTP_CAPABILITY)])
}

fn secrets_authority() -> Authority {
    Authority::new([capability(SECRETS_MANAGE_CAPABILITY)])
}

fn capability(value: &str) -> CapabilityId {
    CapabilityId::parse(value).expect("static provider capability is valid")
}

#[derive(Default)]
struct OpenAiCodexPlugin {
    runtime: Option<tokio::runtime::Runtime>,
    client: OnceLock<Result<reqwest::Client, String>>,
    token_client: OnceLock<Result<reqwest::Client, String>>,
    store: Option<CredentialStore>,
    pending: Option<PendingAuthentication>,
}

struct PendingAuthentication {
    uri: String,
    instructions: String,
    result: Arc<parking_lot::Mutex<Option<Result<(), String>>>>,
    task: JoinHandle<()>,
}

fn build_codex_http_client() -> Result<reqwest::Client, String> {
    provider_http_client_builder()
        .map_err(|error| error.to_string())?
        .build()
        .map_err(|error| format!("cannot build Codex HTTP client: {error}"))
}

fn build_codex_token_client() -> Result<reqwest::Client, String> {
    provider_http_client_builder()
        .map_err(|error| error.to_string())?
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| format!("cannot build Codex OAuth token client: {error}"))
}

fn codex_request(
    endpoint: &Endpoint,
    request: &ModelInferenceRequest,
) -> Result<ProviderRequest, ProviderError> {
    let protocol = Protocol::OpenAiResponses;
    let mut outgoing = protocol.encode(endpoint, request)?;
    let mut body: Value =
        serde_json::from_slice(&outgoing.body).map_err(|error| ProviderError::Protocol {
            message: format!("cannot decode generated Codex request: {error}"),
        })?;
    let object = body
        .as_object_mut()
        .ok_or_else(|| ProviderError::Protocol {
            message: "generated Codex request body is not an object".to_owned(),
        })?;

    // backend described the Phenix execution path in legacy runtime targets.
    // It is never part of the provider wire contract.
    object.remove("backend");

    let input = object
        .remove("input")
        .ok_or_else(|| ProviderError::Protocol {
            message: "generated Codex request contains no input".to_owned(),
        })?;
    let input = match input {
        Value::String(text) => vec![codex_message("user", text)],
        Value::Array(items) => items
            .into_iter()
            .map(canonicalize_codex_input_item)
            .collect::<Result<Vec<_>, _>>()?,
        _ => {
            return Err(ProviderError::InvalidRequest {
                message: "Codex input must be text or response input items".to_owned(),
            });
        }
    };
    object.insert("input".to_owned(), Value::Array(input));
    let has_tools = !request.tools.is_empty();
    object.insert(
        "tool_choice".to_owned(),
        Value::String(if has_tools { "auto" } else { "none" }.to_owned()),
    );
    object.insert("parallel_tool_calls".to_owned(), Value::Bool(has_tools));
    object.insert("store".to_owned(), Value::Bool(false));
    object.insert("stream".to_owned(), Value::Bool(true));
    object
        .entry("include".to_owned())
        .or_insert_with(|| Value::Array(Vec::new()));

    outgoing.body = serde_json::to_vec(&body).map_err(|error| ProviderError::Protocol {
        message: format!("cannot encode Codex request: {error}"),
    })?;
    outgoing
        .headers
        .insert("accept".to_owned(), "text/event-stream".to_owned());
    outgoing
        .headers
        .insert("version".to_owned(), CODEX_COMPAT_VERSION.to_owned());
    Ok(outgoing)
}

fn codex_message(role: &str, text: String) -> Value {
    let content_type = if role == "assistant" {
        "output_text"
    } else {
        "input_text"
    };
    serde_json::json!({
        "type": "message",
        "role": role,
        "content": [{
            "type": content_type,
            "text": text,
        }],
    })
}

fn canonicalize_codex_input_item(item: Value) -> Result<Value, ProviderError> {
    let Some(object) = item.as_object() else {
        return Err(ProviderError::InvalidRequest {
            message: "Codex response input item must be an object".to_owned(),
        });
    };
    if object.contains_key("type") {
        return Ok(item);
    }
    let role = object.get("role").and_then(Value::as_str).ok_or_else(|| {
        ProviderError::InvalidRequest {
            message: "Codex message input contains no role".to_owned(),
        }
    })?;
    let content = object
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderError::InvalidRequest {
            message: "Codex message input contains no text content".to_owned(),
        })?;
    Ok(codex_message(role, content.to_owned()))
}

fn decode_codex_response(
    protocol: Protocol,
    response: &ProviderResponse,
) -> Result<ModelInferenceResponse, ProviderError> {
    let is_event_stream = response
        .headers
        .get("content-type")
        .is_some_and(|value| value.contains("text/event-stream"));
    if !is_event_stream && serde_json::from_slice::<Value>(&response.body).is_ok() {
        return protocol.decode(response);
    }

    let body = std::str::from_utf8(&response.body).map_err(|_| ProviderError::Protocol {
        message: "Codex streaming response was not UTF-8".to_owned(),
    })?;
    let mut completed = None;
    let mut failure = None;
    let mut output_items = Vec::new();
    let mut output_text = String::new();
    for raw_line in body.lines() {
        let line = raw_line.trim_end_matches('\r');
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let event: Value = serde_json::from_str(data).map_err(|error| ProviderError::Protocol {
            message: format!("cannot decode Codex SSE event: {error}"),
        })?;
        match event.get("type").and_then(Value::as_str) {
            Some("response.output_item.done") => {
                if let Some(item) = event.get("item") {
                    if matches!(
                        item.get("type").and_then(Value::as_str),
                        Some("message" | "function_call")
                    ) {
                        output_items.push(item.clone());
                    }
                }
            }
            Some("response.output_text.delta") => {
                if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                    output_text.push_str(delta);
                }
            }
            Some("response.output_text.done") if output_text.is_empty() => {
                if let Some(text) = event.get("text").and_then(Value::as_str) {
                    output_text.push_str(text);
                }
            }
            Some("response.completed") => {
                completed = event.get("response").cloned().or(Some(event));
            }
            Some("response.failed") | Some("response.incomplete") | Some("error") => {
                failure = event
                    .pointer("/response/error/message")
                    .or_else(|| event.pointer("/response/incomplete_details/reason"))
                    .or_else(|| event.pointer("/error/message"))
                    .or_else(|| event.get("message"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| Some(data.to_owned()));
            }
            _ => {}
        }
    }
    if let Some(message) = failure {
        return Err(ProviderError::Unavailable { message });
    }
    let mut completed = completed.ok_or_else(|| ProviderError::Protocol {
        message: "Codex SSE stream ended without response.completed".to_owned(),
    })?;
    let completed_object = completed
        .as_object_mut()
        .ok_or_else(|| ProviderError::Protocol {
            message: "Codex response.completed payload was not an object".to_owned(),
        })?;
    let completed_has_output = completed_object
        .get("output")
        .and_then(Value::as_array)
        .is_some_and(|output| !output.is_empty());
    let completed_has_output_text = completed_object
        .get("output_text")
        .and_then(Value::as_str)
        .is_some_and(|text| !text.is_empty());
    if !completed_has_output && !output_items.is_empty() {
        completed_object.insert("output".to_owned(), Value::Array(output_items));
    } else if !completed_has_output && !completed_has_output_text && !output_text.is_empty() {
        completed_object.insert("output_text".to_owned(), Value::String(output_text));
    }

    let completed_response = ProviderResponse {
        status: response.status,
        headers: response.headers.clone(),
        body: serde_json::to_vec(&completed).map_err(|error| ProviderError::Protocol {
            message: format!("cannot encode completed Codex response: {error}"),
        })?,
    };
    protocol.decode(&completed_response)
}

impl OpenAiCodexPlugin {
    fn runtime(&self) -> Result<&tokio::runtime::Runtime, ProviderError> {
        self.runtime
            .as_ref()
            .ok_or_else(|| ProviderError::Protocol {
                message: "Codex provider runtime is not initialized".to_owned(),
            })
    }

    fn client(&self) -> Result<&reqwest::Client, ProviderError> {
        self.client
            .get_or_init(build_codex_http_client)
            .as_ref()
            .map_err(|message| ProviderError::Transport {
                message: message.clone(),
            })
    }

    fn token_client(&self) -> Result<&reqwest::Client, ProviderError> {
        self.token_client
            .get_or_init(build_codex_token_client)
            .as_ref()
            .map_err(|message| ProviderError::Transport {
                message: message.clone(),
            })
    }

    fn store(&self) -> Result<&CredentialStore, ProviderError> {
        self.store.as_ref().ok_or_else(|| ProviderError::Protocol {
            message: "Codex credential store is not initialized".to_owned(),
        })
    }

    fn invoke_model(
        &self,
        request: ModelInferenceRequest,
    ) -> Result<ModelInferenceResponse, ProviderError> {
        let store = self.store()?.clone();
        let token_client = self.token_client()?.clone();
        let credential = self
            .runtime()?
            .block_on(credential_for_request(&store, &token_client))
            .map_err(authentication_error)?
            .ok_or_else(|| ProviderError::Authentication {
                message:
                    "OpenAI Codex requires ChatGPT OAuth; authenticate the openai-codex provider"
                        .to_owned(),
            })?;

        let endpoint =
            Endpoint::parse(RESPONSES_ENDPOINT).map_err(|error| ProviderError::Protocol {
                message: error.to_string(),
            })?;
        let protocol = Protocol::OpenAiResponses;
        let mut outgoing = codex_request(&endpoint, &request)?;
        outgoing.headers.insert(
            AUTHORIZATION.as_str().to_owned(),
            format!("Bearer {}", credential.access_token),
        );
        outgoing
            .headers
            .insert("chatgpt-account-id".to_owned(), credential.account_id);
        outgoing
            .headers
            .insert("originator".to_owned(), "phenix".to_owned());

        let client = self.client()?.clone();
        self.runtime()?.block_on(async move {
            let response = send_http(&client, outgoing).await?;
            if !(200..300).contains(&response.status) {
                return Err(normalize_http_error(&response));
            }
            let limits = RateLimits::from_headers(&response.headers);
            let mut decoded = decode_codex_response(protocol, &response)?;
            decoded.provider_metadata.insert(
                "provider".to_owned(),
                PhenixValue::String(OPENAI_CODEX_PROVIDER.to_owned()),
            );
            decoded.provider_metadata.insert(
                "protocol".to_owned(),
                PhenixValue::String(protocol.name().to_owned()),
            );
            if !limits.is_empty() {
                decoded.provider_metadata.insert(
                    "rate_limits".to_owned(),
                    serde_json::to_value(limits)
                        .expect("rate limits serialize")
                        .into(),
                );
            }
            Ok(decoded)
        })
    }

    fn auth_command(
        &mut self,
        command: ProviderAuthCommand,
    ) -> Result<ProviderAuthResponse, ProviderError> {
        match command {
            ProviderAuthCommand::Add { .. } => Err(ProviderError::Authentication {
                message:
                    "OpenAI Codex credentials are created by the interactive ChatGPT OAuth flow"
                        .to_owned(),
            }),
            ProviderAuthCommand::Methods => Ok(ProviderAuthResponse::Methods {
                methods: Vec::new(),
            }),
            ProviderAuthCommand::InteractiveMethods => {
                Ok(ProviderAuthResponse::InteractiveMethods {
                    methods: vec![ProviderAuthMethod {
                        id: AUTH_METHOD.to_owned(),
                        kind: AuthKind::OAuth,
                        name: "OpenAI Codex (ChatGPT OAuth)".to_owned(),
                        description: Some(
                            "Browser OAuth using your ChatGPT subscription".to_owned(),
                        ),
                    }],
                })
            }
            ProviderAuthCommand::Authenticate { method } => {
                let authentication = self.authenticate(&method)?;
                Ok(ProviderAuthResponse::Authentication { authentication })
            }
            ProviderAuthCommand::List => {
                let credentials = self
                    .store()?
                    .resolve()
                    .map_err(authentication_error)?
                    .map(|credential| {
                        vec![AuthDescriptor {
                            kind: AuthKind::OAuth,
                            expires_at: Some(credential.expires_at),
                        }]
                    })
                    .unwrap_or_default();
                Ok(ProviderAuthResponse::Credentials { credentials })
            }
            ProviderAuthCommand::Remove { kind } => {
                if kind != AuthKind::OAuth {
                    return Ok(ProviderAuthResponse::Removed { auth: None });
                }
                if let Some(pending) = self.pending.take() {
                    pending.task.abort();
                }
                let removed =
                    self.store()?
                        .remove()
                        .map_err(authentication_error)?
                        .map(|credential| AuthDescriptor {
                            kind: AuthKind::OAuth,
                            expires_at: Some(credential.expires_at),
                        });
                Ok(ProviderAuthResponse::Removed { auth: removed })
            }
        }
    }

    fn authenticate(
        &mut self,
        method: &str,
    ) -> Result<ProviderAuthenticationResult, ProviderError> {
        if method != AUTH_METHOD {
            return Err(ProviderError::Authentication {
                message: format!("unknown OpenAI Codex authentication method {method:?}"),
            });
        }

        let completed = self
            .pending
            .as_ref()
            .and_then(|pending| pending.result.lock().take());
        if let Some(result) = completed {
            self.pending.take();
            result.map_err(authentication_error)?;
            return Ok(ProviderAuthenticationResult::Authenticated);
        }

        if let Some(pending) = &self.pending {
            return Ok(ProviderAuthenticationResult::External {
                uri: pending.uri.clone(),
                instructions: Some(pending.instructions.clone()),
            });
        }

        if self
            .store()?
            .resolve()
            .map_err(authentication_error)?
            .is_some()
        {
            // Credential freshness is enforced by the model request path, which
            // can refresh without nesting a runtime inside the ACP application task.
            return Ok(ProviderAuthenticationResult::Authenticated);
        }

        let start = start_authorization().map_err(authentication_error)?;
        let uri = start.authorization_uri.clone();
        let instructions =
            "Complete the ChatGPT authorization in your browser, then return to Neovim.".to_owned();
        let store = self.store()?.clone();
        let result = Arc::new(parking_lot::Mutex::new(None));
        let task_result = Arc::clone(&result);
        let task = self.runtime()?.spawn(async move {
            let completed = finish_authorization(&store, start).await;
            *task_result.lock() = Some(completed);
        });
        self.pending = Some(PendingAuthentication {
            uri: uri.clone(),
            instructions: instructions.clone(),
            result,
            task,
        });
        Ok(ProviderAuthenticationResult::External {
            uri,
            instructions: Some(instructions),
        })
    }
}

impl Drop for OpenAiCodexPlugin {
    fn drop(&mut self) {
        if let Some(pending) = self.pending.take() {
            pending.task.abort();
        }
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

impl PluginInstance for OpenAiCodexPlugin {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        self.runtime = Some(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .map_err(|error| format!("cannot start Codex provider runtime: {error}"))?,
        );
        self.store = Some(CredentialStore::discover()?);
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service == &model_inference_service() {
            let context = PluginContext::new(host, (), (), ());
            let request = context
                .kernel
                .decode_projected::<ModelInferenceRequest>(
                    &ModelInferenceInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            return encode_model_inference_outcome(self.invoke_model(request));
        }

        if service == &provider_auth_service() {
            let command = serde_json::from_slice(input).map_err(|error| error.to_string())?;
            return self
                .auth_command(command)
                .and_then(|response| {
                    serde_json::to_vec(&response).map_err(|error| ProviderError::Protocol {
                        message: error.to_string(),
                    })
                })
                .map_err(|error| error.to_wire());
        }

        Err(format!("unsupported Codex provider service: {service}"))
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum StoredCredential {
    ApiKey {
        secret: String,
    },
    OAuth {
        access_token: String,
        refresh_token: String,
        id_token: String,
        account_id: String,
        expires_at: u64,
    },
}

#[derive(Clone)]
struct CodexCredential {
    access_token: String,
    refresh_token: String,
    id_token: String,
    account_id: String,
    expires_at: u64,
}

impl From<CodexCredential> for StoredCredential {
    fn from(value: CodexCredential) -> Self {
        Self::OAuth {
            access_token: value.access_token,
            refresh_token: value.refresh_token,
            id_token: value.id_token,
            account_id: value.account_id,
            expires_at: value.expires_at,
        }
    }
}

impl TryFrom<StoredCredential> for CodexCredential {
    type Error = String;

    fn try_from(value: StoredCredential) -> Result<Self, Self::Error> {
        match value {
            StoredCredential::OAuth {
                access_token,
                refresh_token,
                id_token,
                account_id,
                expires_at,
            } => Ok(Self {
                access_token,
                refresh_token,
                id_token,
                account_id,
                expires_at,
            }),
            StoredCredential::ApiKey { .. } => {
                Err("openai-codex requires ChatGPT OAuth, not an API key".to_owned())
            }
        }
    }
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredCredentials {
    providers: HashMap<String, Value>,
}

#[derive(Clone, Debug)]
struct CredentialStore {
    path: PathBuf,
}

impl CredentialStore {
    fn discover() -> Result<Self, String> {
        if let Some(path) = std::env::var_os(CREDENTIAL_FILE_ENV) {
            return Ok(Self { path: path.into() });
        }
        let state = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
            })
            .ok_or_else(|| {
                format!(
                    "set {CREDENTIAL_FILE_ENV}, XDG_STATE_HOME, or HOME for OpenAI Codex credentials"
                )
            })?;
        Ok(Self {
            path: state.join("phenix/credentials.json"),
        })
    }

    fn resolve(&self) -> Result<Option<CodexCredential>, String> {
        let credentials = self.read()?;
        credentials
            .providers
            .get(OPENAI_CODEX_PROVIDER)
            .cloned()
            .map(|value| {
                serde_json::from_value::<StoredCredential>(value)
                    .map_err(|error| format!("invalid openai-codex credential: {error}"))?
                    .try_into()
            })
            .transpose()
    }

    fn save(&self, credential: CodexCredential) -> Result<(), String> {
        let mut credentials = self.read()?;
        credentials.providers.insert(
            OPENAI_CODEX_PROVIDER.to_owned(),
            serde_json::to_value(StoredCredential::from(credential))
                .map_err(|error| format!("cannot encode openai-codex credential: {error}"))?,
        );
        self.write(&credentials)
    }

    fn remove(&self) -> Result<Option<CodexCredential>, String> {
        let mut credentials = self.read()?;
        let removed = credentials
            .providers
            .remove(OPENAI_CODEX_PROVIDER)
            .map(|value| {
                serde_json::from_value::<StoredCredential>(value)
                    .map_err(|error| format!("invalid openai-codex credential: {error}"))?
                    .try_into()
            })
            .transpose()?;
        if removed.is_some() {
            self.write(&credentials)?;
        }
        Ok(removed)
    }

    fn read(&self) -> Result<StoredCredentials, String> {
        match fs::read_to_string(&self.path) {
            Ok(source) => serde_json::from_str(&source)
                .map_err(|error| format!("cannot parse {}: {error}", self.path.display())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Ok(StoredCredentials::default())
            }
            Err(error) => Err(format!("cannot read {}: {error}", self.path.display())),
        }
    }

    fn write(&self, credentials: &StoredCredentials) -> Result<(), String> {
        let parent = self.path.parent().ok_or_else(|| {
            format!(
                "credential path {} has no parent directory",
                self.path.display()
            )
        })?;
        let parent_existed = parent.exists();
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        if !parent_existed {
            secure_directory(parent)?;
        }
        let temporary = self.path.with_extension("json.new");
        let source = serde_json::to_vec_pretty(credentials)
            .map_err(|error| format!("cannot encode credentials: {error}"))?;
        let mut options = OpenOptions::new();
        options.create(true).truncate(true).write(true);
        secure_file_options(&mut options);
        let mut file = options
            .open(&temporary)
            .map_err(|error| format!("cannot create {}: {error}", temporary.display()))?;
        secure_file(&temporary)?;
        file.write_all(&source)
            .and_then(|()| file.write_all(b"\n"))
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
        fs::rename(&temporary, &self.path)
            .map_err(|error| format!("cannot replace {}: {error}", self.path.display()))
    }
}

struct AuthorizationStart {
    listener: std::net::TcpListener,
    redirect_uri: String,
    verifier: String,
    state: String,
    authorization_uri: String,
}

fn start_authorization() -> Result<AuthorizationStart, String> {
    let listener = match std::net::TcpListener::bind(("127.0.0.1", 1455)) {
        Ok(listener) => listener,
        Err(_) => std::net::TcpListener::bind(("127.0.0.1", 1457)).map_err(|error| {
            format!("cannot bind OAuth callback on ports 1455 or 1457: {error}")
        })?,
    };
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("cannot configure OAuth callback listener: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("cannot inspect OAuth callback address: {error}"))?
        .port();
    let redirect_uri = format!("http://localhost:{port}/auth/callback");
    let verifier = random_urlsafe(64)?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = random_urlsafe(32)?;
    let authorization_uri = authorization_url(&redirect_uri, &challenge, &state)?;
    Ok(AuthorizationStart {
        listener,
        redirect_uri,
        verifier,
        state,
        authorization_uri,
    })
}

async fn finish_authorization(
    store: &CredentialStore,
    start: AuthorizationStart,
) -> Result<(), String> {
    let listener = TcpListener::from_std(start.listener)
        .map_err(|error| format!("cannot activate OAuth callback listener: {error}"))?;
    let result =
        tokio::time::timeout(LOGIN_TIMEOUT, receive_callback(listener, &start.state)).await;
    let code = match result {
        Ok(result) => result?,
        Err(_) => return Err("OAuth login timed out after 10 minutes".to_owned()),
    };
    let client = build_codex_token_client()?;
    let tokens = exchange_code(&client, &code, &start.redirect_uri, &start.verifier).await?;
    store.save(credential_from_tokens(tokens)?)
}

fn authorization_url(redirect_uri: &str, challenge: &str, state: &str) -> Result<String, String> {
    let mut url = Url::parse(&format!("{ISSUER}/oauth/authorize"))
        .map_err(|error| format!("invalid OAuth issuer: {error}"))?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair(
            "scope",
            "openid profile email offline_access api.connectors.read api.connectors.invoke",
        )
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("id_token_add_organizations", "true")
        .append_pair("codex_cli_simplified_flow", "true")
        .append_pair("state", state)
        .append_pair("originator", "phenix");
    Ok(url.into())
}

async fn receive_callback(listener: TcpListener, expected_state: &str) -> Result<String, String> {
    let (mut stream, _) = listener
        .accept()
        .await
        .map_err(|error| format!("OAuth callback failed: {error}"))?;
    let request = read_callback_request(&mut stream).await?;
    let target = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or_else(|| "OAuth callback did not contain a request target".to_owned())?;
    let url = Url::parse(&format!("http://localhost{target}"))
        .map_err(|error| format!("invalid OAuth callback URL: {error}"))?;
    if url.path() != "/auth/callback" {
        return Err("OAuth callback used an unexpected path".to_owned());
    }
    let query = url.query_pairs().collect::<BTreeMap<_, _>>();
    let result = if let Some(error) = query.get("error") {
        Err(format!("OAuth authorization was rejected: {error}"))
    } else if query
        .get("state")
        .map(|value| constant_time_equal(value.as_bytes(), expected_state.as_bytes()))
        != Some(true)
    {
        Err("OAuth callback state verification failed".to_owned())
    } else {
        query
            .get("code")
            .map(|value| value.to_string())
            .ok_or_else(|| "OAuth callback did not contain an authorization code".to_owned())
    };
    let (status, body) = if result.is_ok() {
        (
            "200 OK",
            "Phenix authentication completed. You may close this tab.",
        )
    } else {
        (
            "400 Bad Request",
            "Phenix authentication failed. Return to Neovim for details.",
        )
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    result
}

async fn read_callback_request(stream: &mut tokio::net::TcpStream) -> Result<String, String> {
    let mut request = Vec::with_capacity(2048);
    let mut chunk = [0_u8; 2048];
    loop {
        if request.len() >= MAX_CALLBACK_REQUEST_BYTES {
            return Err("OAuth callback request exceeded 16 KiB".to_owned());
        }
        let remaining = MAX_CALLBACK_REQUEST_BYTES - request.len();
        let read_len = remaining.min(chunk.len());
        let length = stream
            .read(&mut chunk[..read_len])
            .await
            .map_err(|error| format!("cannot read OAuth callback: {error}"))?;
        if length == 0 {
            return Err("OAuth callback ended before the HTTP headers completed".to_owned());
        }
        request.extend_from_slice(&chunk[..length]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8(request).map_err(|_| "OAuth callback was not valid HTTP".to_owned())
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    let length = left.len().max(right.len());
    let mut difference = left.len() ^ right.len();
    for index in 0..length {
        let left = left.get(index).copied().unwrap_or(0);
        let right = right.get(index).copied().unwrap_or(0);
        difference |= usize::from(left ^ right);
    }
    difference == 0
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    id_token: String,
}

#[derive(Deserialize)]
struct RefreshResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    id_token: Option<String>,
}

async fn exchange_code(
    client: &reqwest::Client,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> Result<TokenResponse, String> {
    post_token_form(
        client,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", CLIENT_ID),
            ("code_verifier", verifier),
        ],
    )
    .await
}

async fn refresh(
    store: &CredentialStore,
    client: &reqwest::Client,
    credential: CodexCredential,
) -> Result<CodexCredential, String> {
    let response: RefreshResponse = post_token_form(
        client,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", CLIENT_ID),
            ("refresh_token", &credential.refresh_token),
        ],
    )
    .await?;
    let access_token = response.access_token.unwrap_or(credential.access_token);
    let refresh_token = response.refresh_token.unwrap_or(credential.refresh_token);
    let id_token = response.id_token.unwrap_or(credential.id_token);
    let account_id = account_id_from_token(&id_token)
        .or_else(|| account_id_from_token(&access_token))
        .unwrap_or(credential.account_id);
    let expires_at = token_expiry(&access_token).unwrap_or(unix_time()?.saturating_add(3600));
    let refreshed = CodexCredential {
        access_token,
        refresh_token,
        id_token,
        account_id,
        expires_at,
    };
    store.save(refreshed.clone())?;
    Ok(refreshed)
}

async fn credential_for_request(
    store: &CredentialStore,
    client: &reqwest::Client,
) -> Result<Option<CodexCredential>, String> {
    let Some(credential) = store.resolve()? else {
        return Ok(None);
    };
    if credential.expires_at <= unix_time()?.saturating_add(REFRESH_MARGIN_SECONDS) {
        return refresh(store, client, credential).await.map(Some);
    }
    Ok(Some(credential))
}

async fn post_token_form<T: for<'de> Deserialize<'de>>(
    client: &reqwest::Client,
    form: &[(&str, &str)],
) -> Result<T, String> {
    let response = client
        .post(TOKEN_URL)
        .form(form)
        .send()
        .await
        .map_err(|error| format!("OAuth token request failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        let message = response
            .text()
            .await
            .unwrap_or_else(|_| "unreadable response".to_owned());
        return Err(format!("OAuth token endpoint returned {status}: {message}"));
    }
    response
        .json()
        .await
        .map_err(|error| format!("invalid OAuth token response: {error}"))
}

fn credential_from_tokens(tokens: TokenResponse) -> Result<CodexCredential, String> {
    let account_id = account_id_from_token(&tokens.id_token)
        .or_else(|| account_id_from_token(&tokens.access_token))
        .ok_or_else(|| "OAuth token does not identify a ChatGPT account".to_owned())?;
    let expires_at =
        token_expiry(&tokens.access_token).unwrap_or(unix_time()?.saturating_add(3600));
    Ok(CodexCredential {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        id_token: tokens.id_token,
        account_id,
        expires_at,
    })
}

fn jwt_payload(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn token_expiry(token: &str) -> Option<u64> {
    jwt_payload(token)?.get("exp")?.as_u64()
}

fn account_id_from_token(token: &str) -> Option<String> {
    let claims = jwt_payload(token)?;
    claims
        .get("chatgpt_account_id")
        .or_else(|| {
            claims
                .get("https://api.openai.com/auth")
                .and_then(|auth| auth.get("chatgpt_account_id"))
        })?
        .as_str()
        .map(ToOwned::to_owned)
}

fn random_urlsafe(bytes: usize) -> Result<String, String> {
    let mut value = vec![0_u8; bytes];
    getrandom::fill(&mut value)
        .map_err(|error| format!("cannot generate OAuth secret: {error}"))?;
    Ok(URL_SAFE_NO_PAD.encode(value))
}

fn unix_time() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| format!("system clock predates Unix epoch: {error}"))
}

fn authentication_error(message: impl ToString) -> ProviderError {
    ProviderError::Authentication {
        message: message.to_string(),
    }
}

async fn send_http(
    client: &reqwest::Client,
    request: ProviderRequest,
) -> Result<ProviderResponse, ProviderError> {
    let method = match request.method {
        HttpMethod::Get => reqwest::Method::GET,
        HttpMethod::Post => reqwest::Method::POST,
        HttpMethod::Put => reqwest::Method::PUT,
        HttpMethod::Patch => reqwest::Method::PATCH,
        HttpMethod::Delete => reqwest::Method::DELETE,
    };
    let mut outgoing = client.request(method, &request.url);
    for (name, value) in request.headers {
        let header_name =
            HeaderName::from_bytes(name.as_bytes()).map_err(|_| ProviderError::Protocol {
                message: format!("protocol produced invalid HTTP header name {name:?}"),
            })?;
        let header_value = HeaderValue::from_str(&value).map_err(|_| ProviderError::Protocol {
            message: format!("protocol produced invalid HTTP header value for {name:?}"),
        })?;
        outgoing = outgoing.header(header_name, header_value);
    }
    let response =
        outgoing
            .body(request.body)
            .send()
            .await
            .map_err(|error| ProviderError::Transport {
                message: error.to_string(),
            })?;
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_ascii_lowercase(), value.to_owned()))
        })
        .collect();
    let body = response
        .bytes()
        .await
        .map_err(|error| ProviderError::Transport {
            message: error.to_string(),
        })?
        .to_vec();
    Ok(ProviderResponse {
        status,
        headers,
        body,
    })
}

#[cfg(unix)]
fn secure_directory(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("cannot secure {}: {error}", path.display()))
}

#[cfg(not(unix))]
fn secure_directory(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn secure_file_options(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn secure_file_options(_options: &mut OpenOptions) {}

#[cfg(unix)]
fn secure_file(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("cannot secure {}: {error}", path.display()))
}

#[cfg(not(unix))]
fn secure_file(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model_request() -> ModelInferenceRequest {
        ModelInferenceRequest {
            model: phenix_core::ModelId::parse("gpt-5.6-terra").unwrap(),
            input: b"hello".to_vec().into(),
            options: BTreeMap::from([
                (
                    "backend".to_owned(),
                    PhenixValue::String("phenix".to_owned()),
                ),
                (
                    "inference".to_owned(),
                    serde_json::json!({"effort": "medium"}).into(),
                ),
            ]),
            tools: Vec::new(),
            continuation: Vec::new(),
        }
    }

    #[test]
    fn codex_request_matches_chatgpt_responses_contract() {
        let endpoint = Endpoint::parse(RESPONSES_ENDPOINT).unwrap();
        let request = codex_request(&endpoint, &model_request()).unwrap();
        let body: Value = serde_json::from_slice(&request.body).unwrap();

        assert_eq!(
            request.url,
            "https://chatgpt.com/backend-api/codex/responses"
        );
        assert_eq!(
            request.headers.get("accept").map(String::as_str),
            Some("text/event-stream")
        );
        assert_eq!(
            request.headers.get("version").map(String::as_str),
            Some(CODEX_COMPAT_VERSION)
        );
        assert!(body.get("backend").is_none());
        assert!(body.get("inference").is_none());
        assert_eq!(body["reasoning"]["effort"], "medium");
        assert_eq!(body["tool_choice"], "none");
        assert_eq!(body["parallel_tool_calls"], false);
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        assert_eq!(body["input"][0]["type"], "message");
        assert_eq!(body["input"][0]["role"], "user");
        assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(body["input"][0]["content"][0]["text"], "hello");
    }

    #[test]
    fn codex_response_decodes_completed_sse() {
        let response = ProviderResponse {
            status: 200,
            headers: BTreeMap::from([(
                "content-type".to_owned(),
                "text/event-stream".to_owned(),
            )]),
            body: concat!(
                "event: response.completed\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"response-1\",",
                "\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"world\"}]}],",
                "\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n\n"
            )
            .as_bytes()
            .to_vec(),
        };

        let decoded = decode_codex_response(Protocol::OpenAiResponses, &response).unwrap();
        assert_eq!(decoded.output.as_ref(), b"world");
        assert_eq!(
            decoded.provider_metadata["id"],
            PhenixValue::String("response-1".to_owned())
        );
    }

    #[test]
    fn codex_response_accumulates_streamed_output_before_metadata_only_completion() {
        let response = ProviderResponse {
            status: 200,
            headers: BTreeMap::from([(
                "content-type".to_owned(),
                "text/event-stream".to_owned(),
            )]),
            body: concat!(
                "event: response.output_item.added\n",
                "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg-1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[]}}\n\n",
                "event: response.output_text.delta\n",
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"wor\"}\n\n",
                "event: response.output_text.delta\n",
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"ld\"}\n\n",
                "event: response.output_item.done\n",
                "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg-1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"world\"}]}}\n\n",
                "event: response.completed\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"response-1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n\n"
            )
            .as_bytes()
            .to_vec(),
        };

        let decoded = decode_codex_response(Protocol::OpenAiResponses, &response).unwrap();
        assert_eq!(decoded.output.as_ref(), b"world");
        assert_eq!(
            decoded.provider_metadata["id"],
            PhenixValue::String("response-1".to_owned())
        );
    }

    #[test]
    fn codex_response_ignores_reasoning_output_items() {
        let response = ProviderResponse {
            status: 200,
            headers: BTreeMap::from([(
                "content-type".to_owned(),
                "text/event-stream".to_owned(),
            )]),
            body: concat!(
                "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"reasoning-1\",\"type\":\"reasoning\",\"content\":[{\"type\":\"reasoning_text\",\"text\":\"private reasoning\"}]}}\n\n",
                "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg-1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"public answer\"}]}}\n\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"response-1\",\"usage\":{}}}\n\n"
            )
            .as_bytes()
            .to_vec(),
        };

        let decoded = decode_codex_response(Protocol::OpenAiResponses, &response).unwrap();
        assert_eq!(decoded.output.as_ref(), b"public answer");
    }

    #[test]
    fn codex_response_falls_back_to_text_deltas_without_output_item_done() {
        let response = ProviderResponse {
            status: 200,
            headers: BTreeMap::from([(
                "content-type".to_owned(),
                "text/event-stream".to_owned(),
            )]),
            body: concat!(
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"wor\"}\n\n",
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"ld\"}\n\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"response-1\",\"usage\":{}}}\n\n"
            )
            .as_bytes()
            .to_vec(),
        };

        let decoded = decode_codex_response(Protocol::OpenAiResponses, &response).unwrap();
        assert_eq!(decoded.output.as_ref(), b"world");
    }

    #[test]
    fn codex_response_accumulates_streamed_function_calls() {
        let response = ProviderResponse {
            status: 200,
            headers: BTreeMap::from([(
                "content-type".to_owned(),
                "text/event-stream".to_owned(),
            )]),
            body: concat!(
                "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"item-1\",\"type\":\"function_call\",\"call_id\":\"call-1\",\"name\":\"fixture.echo\",\"arguments\":\"{\\\"value\\\":\\\"streamed\\\"}\"}}\n\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"response-1\",\"usage\":{}}}\n\n"
            )
            .as_bytes()
            .to_vec(),
        };

        let decoded = decode_codex_response(Protocol::OpenAiResponses, &response).unwrap();
        assert_eq!(decoded.output.as_ref(), b"");
        assert_eq!(decoded.tool_calls.len(), 1);
        assert_eq!(decoded.tool_calls[0].call_id, "call-1");
        assert_eq!(decoded.tool_calls[0].callable_id.as_str(), "fixture.echo");
        assert_eq!(
            decoded.tool_calls[0].input,
            PhenixValue::Map(BTreeMap::from([(
                "value".to_owned(),
                PhenixValue::String("streamed".to_owned())
            )]))
        );
    }

    #[test]
    fn provider_exposes_chatgpt_oauth_as_interactive_auth() {
        let mut plugin = OpenAiCodexPlugin::default();
        let response = plugin
            .auth_command(ProviderAuthCommand::InteractiveMethods)
            .unwrap();
        assert!(matches!(
            response,
            ProviderAuthResponse::InteractiveMethods { methods }
                if methods.len() == 1
                    && methods[0].id == AUTH_METHOD
                    && methods[0].kind == AuthKind::OAuth
        ));
    }

    #[test]
    fn authorization_url_keeps_codex_contract() {
        let url =
            authorization_url("http://localhost:1455/auth/callback", "challenge", "state").unwrap();
        let parsed = Url::parse(&url).unwrap();
        let query = parsed.query_pairs().collect::<BTreeMap<_, _>>();
        assert_eq!(
            query.get("response_type").map(|value| value.as_ref()),
            Some("code")
        );
        assert_eq!(
            query.get("client_id").map(|value| value.as_ref()),
            Some(CLIENT_ID)
        );
        assert_eq!(
            query
                .get("code_challenge_method")
                .map(|value| value.as_ref()),
            Some("S256")
        );
        assert_eq!(
            query.get("state").map(|value| value.as_ref()),
            Some("state")
        );
        assert_eq!(
            query.get("originator").map(|value| value.as_ref()),
            Some("phenix")
        );
    }

    #[test]
    fn callback_state_comparison_rejects_length_and_content_mismatch() {
        assert!(constant_time_equal(b"same", b"same"));
        assert!(!constant_time_equal(b"same", b"different"));
        assert!(!constant_time_equal(b"same", b"sam"));
    }

    #[test]
    fn credential_claims_support_both_account_shapes() {
        fn jwt(payload: Value) -> String {
            let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap());
            format!("header.{encoded}.signature")
        }

        assert_eq!(
            account_id_from_token(&jwt(serde_json::json!({
                "chatgpt_account_id": "account-direct"
            })))
            .as_deref(),
            Some("account-direct")
        );
        assert_eq!(
            account_id_from_token(&jwt(serde_json::json!({
                "https://api.openai.com/auth": {
                    "chatgpt_account_id": "account-nested"
                }
            })))
            .as_deref(),
            Some("account-nested")
        );
    }
}
