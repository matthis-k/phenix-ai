use crate::{
    normalize_http_error, oauth, provider_auth_service, ApiTokenScheme, ApiTokenSource, Auth,
    AuthKind, CredentialStore, HttpMethod, ProviderAuthCommand, ProviderAuthResponse, ProviderError,
    ProviderRequest, ProviderResponse, ProviderSpec, RateLimits, Token,
};
use phenix_core::{
    model_inference_service, ComponentInterface, ModelInferenceInterface, ModelInferenceRequest,
    ModelInferenceResponse, PhenixValue, PluginContext, PluginHost, PluginInstance, ServiceId,
};
use reqwest::header::{HeaderName, HeaderValue, AUTHORIZATION};
use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};

pub(crate) struct ProviderPlugin {
    spec: Arc<ProviderSpec>,
    runtime: Option<tokio::runtime::Runtime>,
    client: OnceLock<Result<reqwest::Client, reqwest::Error>>,
    credentials: Option<CredentialStore>,
}

impl ProviderPlugin {
    pub(crate) fn new(spec: Arc<ProviderSpec>) -> Self {
        Self {
            spec,
            runtime: None,
            client: OnceLock::new(),
            credentials: None,
        }
    }

    fn client(&self) -> Result<&reqwest::Client, ProviderError> {
        self.client
            .get_or_init(|| reqwest::Client::builder().build())
            .as_ref()
            .map_err(|error| ProviderError::Transport {
                message: format!("cannot build provider HTTP client: {error}"),
            })
    }

    fn runtime(&self) -> Result<&tokio::runtime::Runtime, ProviderError> {
        self.runtime
            .as_ref()
            .ok_or_else(|| ProviderError::Protocol {
                message: "provider runtime is not initialized".to_owned(),
            })
    }

    fn credentials(&self) -> Result<&CredentialStore, ProviderError> {
        self.credentials
            .as_ref()
            .ok_or_else(|| ProviderError::Protocol {
                message: "provider credential store is not initialized".to_owned(),
            })
    }

    fn resolve_auth(&self) -> Result<Option<Auth>, ProviderError> {
        if !self.spec.supports_auth() {
            return Ok(None);
        }
        let store = self.credentials()?;
        if let Some(method) = &self.spec.auth.oauth {
            if let Some(auth) = store.resolve(self.spec.id.as_str(), AuthKind::OAuth)? {
                return oauth::refresh_if_needed(method, self.spec.id.as_str(), store, auth)
                    .map(Some);
            }
        }
        if self.spec.auth.api_token.is_some() {
            if let Some(auth) = store.resolve(self.spec.id.as_str(), AuthKind::ApiToken)? {
                return Ok(Some(auth));
            }
        }
        if let Some(auth) = self.spec.default_auth.clone() {
            if auth.is_expired() {
                return Err(ProviderError::Authentication {
                    message: format!("default OAuth credential for {} is expired", self.spec.id),
                });
            }
            return Ok(Some(auth));
        }
        Err(ProviderError::Authentication {
            message: format!("provider {} has no configured credentials", self.spec.id),
        })
    }

    fn available_auth_descriptors(&self) -> Result<Vec<crate::AuthDescriptor>, ProviderError> {
        let mut credentials = self.credentials()?.list(self.spec.id.as_str())?;
        if let Some(default_auth) = &self.spec.default_auth {
            let available = match default_auth {
                Auth::ApiToken {
                    source: ApiTokenSource::Environment { variable },
                } => std::env::var(variable.as_str())
                    .ok()
                    .is_some_and(|value| !value.trim().is_empty()),
                Auth::ApiToken {
                    source: ApiTokenSource::Literal { .. },
                } => true,
                Auth::OAuth { .. } => !default_auth.is_expired(),
            };
            let descriptor = default_auth.descriptor();
            if available && !credentials.iter().any(|item| item.kind == descriptor.kind) {
                credentials.push(descriptor);
                credentials.sort_by_key(|item| item.kind);
            }
        }
        Ok(credentials)
    }

    fn invoke_model(
        &self,
        request: ModelInferenceRequest,
    ) -> Result<ModelInferenceResponse, ProviderError> {
        let mut outgoing = self.spec.protocol.encode(&self.spec.endpoint, &request)?;
        let auth = self.resolve_auth()?;
        apply_auth(&self.spec, &mut outgoing.headers, auth.as_ref())?;

        let client = self.client()?.clone();
        let protocol = Arc::clone(&self.spec.protocol);
        let endpoint = self.spec.endpoint.clone();
        self.runtime()?.block_on(async move {
            let response = send_http(&client, outgoing).await?;
            if !(200..300).contains(&response.status) {
                return Err(normalize_http_error(&response));
            }
            let limits = RateLimits::from_headers(&response.headers);
            let mut decoded = protocol.decode(&response)?;
            decoded.provider_metadata.insert(
                "protocol".to_owned(),
                PhenixValue::String(protocol.name().to_owned()),
            );
            decoded.provider_metadata.insert(
                "endpoint".to_owned(),
                PhenixValue::String(endpoint.as_str().to_owned()),
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
        &self,
        command: ProviderAuthCommand,
    ) -> Result<ProviderAuthResponse, ProviderError> {
        let store = self.credentials()?;
        match command {
            ProviderAuthCommand::Add { auth } => {
                self.ensure_auth_supported(auth.kind())?;
                let auth = store.add(self.spec.id.as_str(), auth)?;
                Ok(ProviderAuthResponse::Added { auth })
            }
            ProviderAuthCommand::Methods => Ok(ProviderAuthResponse::Methods {
                methods: self.spec.auth_kinds(),
            }),
            ProviderAuthCommand::List => Ok(ProviderAuthResponse::Credentials {
                credentials: self.available_auth_descriptors()?,
            }),
            ProviderAuthCommand::BeginOAuth => {
                let method = self.spec.auth.oauth.as_ref().ok_or_else(|| {
                    ProviderError::Authentication {
                        message: format!("provider {} does not support OAuth", self.spec.id),
                    }
                })?;
                let external = oauth::begin(method, self.spec.id.as_str(), store.clone())?;
                Ok(ProviderAuthResponse::External {
                    uri: external.uri,
                    instructions: external.instructions,
                })
            }
            ProviderAuthCommand::Remove { kind } => {
                let auth = store.remove(self.spec.id.as_str(), kind)?;
                Ok(ProviderAuthResponse::Removed { auth })
            }
        }
    }

    fn ensure_auth_supported(&self, kind: AuthKind) -> Result<(), ProviderError> {
        let supported = match kind {
            AuthKind::ApiToken => self.spec.auth.api_token.is_some(),
            AuthKind::OAuth => self.spec.auth.oauth.is_some(),
        };
        if !supported {
            return Err(ProviderError::Authentication {
                message: format!(
                    "provider {} does not accept {kind:?} credentials",
                    self.spec.id
                ),
            });
        }
        Ok(())
    }
}

impl PluginInstance for ProviderPlugin {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        self.runtime = Some(
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| format!("cannot start provider runtime: {error}"))?,
        );
        self.credentials = self
            .spec
            .supports_auth()
            .then(CredentialStore::discover)
            .transpose()
            .map_err(|error| error.to_string())?;
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
            return self
                .invoke_model(request)
                .and_then(|response| {
                    context.kernel.encode_value(&response).map_err(|error| {
                        ProviderError::Protocol {
                            message: error.to_string(),
                        }
                    })
                })
                .map_err(|error| error.to_wire());
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
        Err(format!("unsupported provider service: {service}"))
    }
}

fn apply_auth(
    spec: &ProviderSpec,
    headers: &mut BTreeMap<String, String>,
    auth: Option<&Auth>,
) -> Result<(), ProviderError> {
    match auth {
        None => Ok(()),
        Some(Auth::OAuth { access_token, .. }) => {
            let method = spec.auth.oauth.as_ref().ok_or_else(|| {
                ProviderError::Authentication {
                    message: "OAuth credential is not accepted by this provider".to_owned(),
                }
            })?;
            oauth::apply_headers(method, access_token, headers)
        }
        Some(Auth::ApiToken { source }) => {
            let token = resolve_api_token(source)?;
            let scheme =
                spec.auth
                    .api_token
                    .as_ref()
                    .ok_or_else(|| ProviderError::Authentication {
                        message: "API-token credential is not accepted by this provider".to_owned(),
                    })?;
            match scheme {
                ApiTokenScheme::Bearer => {
                    headers.insert(
                        AUTHORIZATION.as_str().to_owned(),
                        format!("Bearer {}", token.expose()),
                    );
                }
                ApiTokenScheme::Header { name } => {
                    headers.insert(name.as_str().to_owned(), token.expose().to_owned());
                }
            }
            Ok(())
        }
    }
}

fn resolve_api_token(source: &ApiTokenSource) -> Result<Token, ProviderError> {
    match source {
        ApiTokenSource::Literal { token } => Ok(token.clone()),
        ApiTokenSource::Environment { variable } => {
            let value = std::env::var(variable.as_str()).map_err(|error| {
                ProviderError::Authentication {
                    message: format!(
                        "API-token environment variable {} is unavailable: {error}",
                        variable.as_str()
                    ),
                }
            })?;
            Token::parse(value).map_err(|error| ProviderError::Authentication {
                message: format!(
                    "API-token environment variable {} is invalid: {error}",
                    variable.as_str()
                ),
            })
        }
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
