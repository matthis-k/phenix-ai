use crate::{
    normalize_http_error, provider_auth_service, ApiTokenScheme, ApiTokenSource, Auth, AuthKind,
    CredentialStore, ProviderAuthCommand, ProviderAuthResponse, ProviderError, ProviderRequest,
    ProviderResponse, ProviderSpec, RateLimits, Token,
};
use http::header::{HeaderMap, HeaderName as HttpHeaderName, HeaderValue, AUTHORIZATION};
use phenix_core::{
    model_inference_service, ComponentInterface, ModelInferenceInterface, ModelInferenceRequest,
    ModelInferenceResponse, PhenixValue, PluginContext, PluginHost, PluginInstance, ServiceId,
};
use std::sync::Arc;

pub(crate) struct ProviderPlugin {
    spec: Arc<ProviderSpec>,
    runtime: Option<tokio::runtime::Runtime>,
    client: reqwest::Client,
    credentials: Option<CredentialStore>,
}

impl ProviderPlugin {
    pub(crate) fn new(spec: Arc<ProviderSpec>) -> Self {
        Self {
            spec,
            runtime: None,
            client: reqwest::Client::new(),
            credentials: None,
        }
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
        if self.spec.auth.oauth.is_some() {
            if let Some(auth) = store.resolve(self.spec.id.as_str(), AuthKind::OAuth)? {
                if auth.is_expired() {
                    return Err(ProviderError::Authentication {
                        message: format!(
                            "OAuth credential for {} is expired; add a refreshed credential",
                            self.spec.id
                        ),
                    });
                }
                return Ok(Some(auth));
            }
        }
        if self.spec.auth.api_token.is_some() {
            if let Some(auth) = store.resolve(self.spec.id.as_str(), AuthKind::ApiToken)? {
                return Ok(Some(auth));
            }
        }
        Err(ProviderError::Authentication {
            message: format!("provider {} has no configured credentials", self.spec.id),
        })
    }

    fn invoke_model(
        &self,
        request: ModelInferenceRequest,
    ) -> Result<ModelInferenceResponse, ProviderError> {
        let mut outgoing = self.spec.protocol.encode(&self.spec.endpoint, &request)?;
        let auth = self.resolve_auth()?;
        apply_auth(&self.spec, &mut outgoing.headers, auth.as_ref())?;

        let client = self.client.clone();
        let protocol = Arc::clone(&self.spec.protocol);
        let endpoint = self.spec.endpoint.clone();
        self.runtime()?.block_on(async move {
            let response = send_http(&client, outgoing).await?;
            if !response.status.is_success() {
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
                credentials: store.list(self.spec.id.as_str())?,
            }),
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
    headers: &mut HeaderMap,
    auth: Option<&Auth>,
) -> Result<(), ProviderError> {
    match auth {
        None => Ok(()),
        Some(Auth::OAuth { access_token, .. }) if spec.auth.oauth.is_some() => {
            headers.insert(AUTHORIZATION, bearer_value(access_token));
            Ok(())
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
                    headers.insert(AUTHORIZATION, bearer_value(&token));
                }
                ApiTokenScheme::Header { name } => {
                    let name = HttpHeaderName::from_bytes(name.as_str().as_bytes())
                        .expect("configured provider header name remains valid");
                    headers.insert(name, token_value(&token));
                }
            }
            Ok(())
        }
        Some(_) => Err(ProviderError::Authentication {
            message: "credential type does not match provider auth configuration".to_owned(),
        }),
    }
}

fn bearer_value(token: &Token) -> HeaderValue {
    HeaderValue::from_str(&format!("Bearer {}", token.expose()))
        .expect("validated provider token remains a valid bearer header value")
}

fn token_value(token: &Token) -> HeaderValue {
    HeaderValue::from_str(token.expose())
        .expect("validated provider token remains a valid header value")
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
    let ProviderRequest {
        method,
        url,
        headers,
        body,
    } = request;
    let response = client
        .request(method, url)
        .headers(headers)
        .body(body)
        .send()
        .await
        .map_err(|error| ProviderError::Transport {
            message: error.to_string(),
        })?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = response
        .bytes()
        .await
        .map_err(|error| ProviderError::Transport {
            message: error.to_string(),
        })?;
    Ok(ProviderResponse {
        status,
        headers,
        body,
    })
}
