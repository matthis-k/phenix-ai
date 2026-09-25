use crate::{
    encode_model_inference_outcome, normalize_http_error, provider_auth_service,
    provider_http_client_builder, ApiTokenScheme, ApiTokenSource, Auth, AuthKind, CredentialStore,
    HttpMethod, ProviderAuthCommand, ProviderAuthResponse, ProviderError, ProviderRequest,
    ProviderResponse, ProviderSpec, RateLimits, Token,
};
use phenix_core::{
    model_inference_service, ArtifactRevision, ComponentInterface, ModelInferenceInterface,
    ModelInferenceRequest, ModelInferenceResponse, PhenixValue, PluginContext, PluginHost,
    PluginInstance, ServiceId,
};
use reqwest::header::{HeaderName, HeaderValue, AUTHORIZATION};
use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};

pub(crate) struct ProviderPlugin {
    spec: Arc<ProviderSpec>,
    runtime: Option<tokio::runtime::Runtime>,
    client: OnceLock<Result<reqwest::Client, String>>,
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
            .get_or_init(|| {
                provider_http_client_builder()
                    .and_then(|builder| {
                        builder.build().map_err(|error| ProviderError::Transport {
                            message: format!("cannot build provider HTTP client: {error}"),
                        })
                    })
                    .map_err(|error| error.to_string())
            })
            .as_ref()
            .map_err(|message| ProviderError::Transport {
                message: message.clone(),
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
        let cache_compatibility_identity = cache_compatibility_identity(&self.spec, &request)?;
        let mut outgoing = self.spec.protocol.encode(&self.spec.endpoint, &request)?;
        let cache_request = request.cache.clone();
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
            decoded.provider_metadata.insert(
                "cache_request".to_owned(),
                serde_json::to_value(cache_request)
                    .expect("model cache control serializes")
                    .into(),
            );
            if let Some(identity) = cache_compatibility_identity {
                decoded.provider_metadata.insert(
                    "cache_compatibility_identity".to_owned(),
                    PhenixValue::String(identity),
                );
            }
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
        match command {
            ProviderAuthCommand::Methods => Ok(ProviderAuthResponse::Methods {
                methods: self.spec.auth_kinds(),
            }),
            ProviderAuthCommand::InteractiveMethods => {
                Ok(ProviderAuthResponse::InteractiveMethods {
                    methods: Vec::new(),
                })
            }
            ProviderAuthCommand::Authenticate { method } => Err(ProviderError::Authentication {
                message: format!(
                    "provider {} does not expose interactive authentication method {method:?}",
                    self.spec.id
                ),
            }),
            ProviderAuthCommand::Add { auth } => {
                self.ensure_auth_supported(auth.kind())?;
                let auth = self.credentials()?.add(self.spec.id.as_str(), auth)?;
                Ok(ProviderAuthResponse::Added { auth })
            }
            ProviderAuthCommand::List => Ok(ProviderAuthResponse::Credentials {
                credentials: self.available_auth_descriptors()?,
            }),
            ProviderAuthCommand::Remove { kind } => {
                let auth = self.credentials()?.remove(self.spec.id.as_str(), kind)?;
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

impl Drop for ProviderPlugin {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
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
        Err(format!("unsupported provider service: {service}"))
    }
}

fn cache_compatibility_identity(
    spec: &ProviderSpec,
    request: &ModelInferenceRequest,
) -> Result<Option<String>, ProviderError> {
    let Some(prefix) = request.cache.local_prefix_identity.as_deref() else {
        return Ok(None);
    };
    cache_compatibility_identity_from_parts(CacheCompatibilityIdentityParts {
        prefix,
        provider: spec.id.as_str(),
        model: request.model.as_str(),
        protocol: spec.protocol.name(),
        endpoint: spec.endpoint.as_str(),
        capability_generation: request.cache.local_capability_generation.as_deref(),
        authority_identity: request.cache.local_authority_identity.as_deref(),
        options: &request.options,
    })
    .map(Some)
}

struct CacheCompatibilityIdentityParts<'a> {
    prefix: &'a str,
    provider: &'a str,
    model: &'a str,
    protocol: &'a str,
    endpoint: &'a str,
    capability_generation: Option<&'a str>,
    authority_identity: Option<&'a str>,
    options: &'a BTreeMap<String, PhenixValue>,
}

fn cache_compatibility_identity_from_parts(
    parts: CacheCompatibilityIdentityParts<'_>,
) -> Result<String, ProviderError> {
    let options = serde_json::to_vec(parts.options).map_err(|error| ProviderError::Protocol {
        message: format!("cannot encode cache compatibility options: {error}"),
    })?;
    let fields = [
        parts.prefix,
        parts.provider,
        parts.model,
        parts.protocol,
        parts.endpoint,
        parts.capability_generation.unwrap_or_default(),
        parts.authority_identity.unwrap_or_default(),
    ];
    let mut material = Vec::new();
    for field in fields {
        let field = field.as_bytes();
        material.extend_from_slice(&(field.len() as u64).to_be_bytes());
        material.extend_from_slice(field);
    }
    material.extend_from_slice(&(options.len() as u64).to_be_bytes());
    material.extend_from_slice(&options);
    Ok(ArtifactRevision::from_content(&material).to_string())
}

fn apply_auth(
    spec: &ProviderSpec,
    headers: &mut BTreeMap<String, String>,
    auth: Option<&Auth>,
) -> Result<(), ProviderError> {
    match auth {
        None => Ok(()),
        Some(Auth::OAuth { access_token, .. }) if spec.auth.oauth.is_some() => {
            headers.insert(
                AUTHORIZATION.as_str().to_owned(),
                format!("Bearer {}", access_token.expose()),
            );
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
        Some(_) => Err(ProviderError::Authentication {
            message: "credential type does not match provider auth configuration".to_owned(),
        }),
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

#[cfg(test)]
mod cache_identity_tests {
    use super::*;

    fn identity(
        provider: &str,
        model: &str,
        capability_generation: &str,
        authority_identity: &str,
        options: BTreeMap<String, PhenixValue>,
    ) -> String {
        cache_compatibility_identity_from_parts(CacheCompatibilityIdentityParts {
            prefix: "sha256:prefix",
            provider,
            model,
            protocol: "openai_responses",
            endpoint: "https://api.example.com/v1/",
            capability_generation: Some(capability_generation),
            authority_identity: Some(authority_identity),
            options: &options,
        })
        .unwrap()
    }

    #[test]
    fn compatibility_identity_is_stable_for_identical_inputs() {
        let options = BTreeMap::from([(
            "inference".to_owned(),
            serde_json::json!({"effort": "medium"}).into(),
        )]);
        assert_eq!(
            identity(
                "provider.openai",
                "gpt-5.6-sol",
                "generation-1",
                "authority-1",
                options.clone()
            ),
            identity(
                "provider.openai",
                "gpt-5.6-sol",
                "generation-1",
                "authority-1",
                options
            )
        );
    }

    #[test]
    fn model_provider_and_configuration_changes_invalidate_local_identity() {
        let base = identity(
            "provider.openai",
            "gpt-5.6-sol",
            "generation-1",
            "authority-1",
            BTreeMap::from([(
                "inference".to_owned(),
                serde_json::json!({"effort": "medium"}).into(),
            )]),
        );
        let model = identity(
            "provider.openai",
            "gpt-5.6-luna",
            "generation-1",
            "authority-1",
            BTreeMap::from([(
                "inference".to_owned(),
                serde_json::json!({"effort": "medium"}).into(),
            )]),
        );
        let provider = identity(
            "provider.other",
            "gpt-5.6-sol",
            "generation-1",
            "authority-1",
            BTreeMap::from([(
                "inference".to_owned(),
                serde_json::json!({"effort": "medium"}).into(),
            )]),
        );
        let options = identity(
            "provider.openai",
            "gpt-5.6-sol",
            "generation-1",
            "authority-1",
            BTreeMap::from([(
                "inference".to_owned(),
                serde_json::json!({"effort": "high"}).into(),
            )]),
        );

        let generation = identity(
            "provider.openai",
            "gpt-5.6-sol",
            "generation-2",
            "authority-1",
            BTreeMap::from([(
                "inference".to_owned(),
                serde_json::json!({"effort": "medium"}).into(),
            )]),
        );
        let authority = identity(
            "provider.openai",
            "gpt-5.6-sol",
            "generation-1",
            "authority-2",
            BTreeMap::from([(
                "inference".to_owned(),
                serde_json::json!({"effort": "medium"}).into(),
            )]),
        );

        assert_ne!(base, model);
        assert_ne!(base, provider);
        assert_ne!(base, options);
        assert_ne!(base, generation);
        assert_ne!(base, authority);
    }
}
