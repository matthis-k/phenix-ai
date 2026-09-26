#![forbid(unsafe_code)]

pub mod auth;
mod protocol;
mod runtime;
mod store;
mod types;

pub use protocol::{normalize_http_error, Protocol, ProtocolAdapter};
pub use store::*;
pub use types::*;

use phenix_core::{
    model_inference_service, Authority, CapabilityId, ComponentExport, ComponentId,
    ComponentInterface, ComponentManifest, InterfaceId, InvocationOutcome, ModelInferenceInterface,
    ModelInferenceResponse, PhenixValue, PluginExecution, PluginId, PluginInstance, PluginManifest,
    ServiceContribution, ServiceId, ServiceRole,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const PROVIDER_AUTH_SERVICE: &str = "phenix.providers.auth@1";
pub const NETWORK_HTTP_CAPABILITY: &str = "network.http";
pub const SECRETS_MANAGE_CAPABILITY: &str = "secrets.manage";
pub const PHENIX_CA_BUNDLE_ENV: &str = "PHENIX_CA_BUNDLE";

pub fn encode_model_inference_outcome(
    result: Result<ModelInferenceResponse, ProviderError>,
) -> Result<Vec<u8>, String> {
    let outcome = match result {
        Ok(response) => InvocationOutcome::success(PhenixValue::from(&response)),
        Err(error) => {
            InvocationOutcome::domain_error(PhenixValue::from(&error.inference_failure()))
        }
    };
    serde_json::to_vec(&outcome.into_transport_value())
        .map_err(|error| format!("cannot encode model inference outcome: {error}"))
}

/// Configure provider HTTP clients with an explicit CA bundle when the product
/// supplies one. This avoids relying on a host certificate store in pure
/// packaging environments while preserving platform verification elsewhere.
pub fn provider_http_client_builder() -> Result<reqwest::ClientBuilder, ProviderError> {
    let mut builder = reqwest::Client::builder().tls_backend_rustls();
    let Some(path) =
        provider_ca_bundle_from(|name| std::env::var_os(name).map(std::path::PathBuf::from))
    else {
        return Ok(builder);
    };
    let source = PHENIX_CA_BUNDLE_ENV;
    let pem = std::fs::read(&path).map_err(|error| ProviderError::Transport {
        message: format!(
            "cannot read CA bundle from {source} ({}): {error}",
            path.display()
        ),
    })?;
    let certificates =
        reqwest::Certificate::from_pem_bundle(&pem).map_err(|error| ProviderError::Transport {
            message: format!(
                "cannot parse CA bundle from {source} ({}): {error}",
                path.display()
            ),
        })?;
    if certificates.is_empty() {
        return Err(ProviderError::Transport {
            message: format!(
                "CA bundle from {source} ({}) contains no certificates",
                path.display()
            ),
        });
    }
    builder = builder.tls_certs_only(certificates);
    Ok(builder)
}

fn provider_ca_bundle_from(
    mut value: impl FnMut(&str) -> Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    value(PHENIX_CA_BUNDLE_ENV)
}

pub mod provider {
    pub use super::{Endpoint, EndpointParseError, Protocol, ProtocolAdapter, ProviderDefinition};
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderAuthCommand {
    Add { auth: Auth },
    Methods,
    InteractiveMethods,
    Authenticate { method: String },
    List,
    Remove { kind: AuthKind },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderAuthMethod {
    pub id: String,
    pub kind: AuthKind,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderAuthenticationResult {
    Authenticated,
    External {
        uri: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instructions: Option<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderAuthResponse {
    Added {
        auth: AuthDescriptor,
    },
    Methods {
        methods: Vec<AuthKind>,
    },
    InteractiveMethods {
        methods: Vec<ProviderAuthMethod>,
    },
    Authentication {
        authentication: ProviderAuthenticationResult,
    },
    Credentials {
        credentials: Vec<AuthDescriptor>,
    },
    Removed {
        auth: Option<AuthDescriptor>,
    },
}

pub struct ProviderAuthInterface;

impl ComponentInterface for ProviderAuthInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(PROVIDER_AUTH_SERVICE).expect("static provider auth interface is valid")
    }
}

#[must_use]
pub fn provider_auth_service() -> ServiceId {
    ServiceId::parse(PROVIDER_AUTH_SERVICE).expect("static provider auth service is valid")
}

pub(crate) struct ProviderSpec {
    id: PluginId,
    endpoint: Endpoint,
    auth: auth::Definition,
    default_auth: Option<Auth>,
    protocol: Arc<dyn ProtocolAdapter>,
}

impl ProviderSpec {
    fn auth_kinds(&self) -> Vec<AuthKind> {
        self.auth.kinds()
    }

    fn supports_auth(&self) -> bool {
        !self.auth.is_empty()
    }
}

#[derive(Clone)]
pub struct ProviderDefinition {
    spec: Arc<ProviderSpec>,
}

impl ProviderDefinition {
    pub fn new(
        id: PluginId,
        endpoint: Endpoint,
        protocol: impl ProtocolAdapter + 'static,
        auth: impl Into<auth::Definition>,
    ) -> Self {
        Self {
            spec: Arc::new(ProviderSpec {
                id,
                endpoint,
                auth: auth.into(),
                default_auth: None,
                protocol: Arc::new(protocol),
            }),
        }
    }

    #[must_use]
    pub fn with_default_auth(self, default_auth: Auth) -> Self {
        assert!(
            self.spec.auth.kinds().contains(&default_auth.kind()),
            "provider default auth must use a supported authentication method"
        );
        Self {
            spec: Arc::new(ProviderSpec {
                id: self.spec.id.clone(),
                endpoint: self.spec.endpoint.clone(),
                auth: self.spec.auth.clone(),
                default_auth: Some(default_auth),
                protocol: Arc::clone(&self.spec.protocol),
            }),
        }
    }

    pub fn plugin_id(&self) -> &PluginId {
        &self.spec.id
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.spec.endpoint
    }

    pub fn protocol_name(&self) -> &'static str {
        self.spec.protocol.name()
    }

    pub fn auth(&self) -> &auth::Definition {
        &self.spec.auth
    }

    #[must_use]
    pub fn auth_kinds(&self) -> Vec<AuthKind> {
        self.spec.auth_kinds()
    }

    #[must_use]
    pub fn manifest(&self) -> PluginManifest {
        let network = network_authority();
        let mut services = vec![ServiceContribution {
            role: ServiceRole::Terminal,
            service: model_inference_service(),
            priority: 100,
            required_authority: network.clone(),
        }];
        let mut maximum_authority = network;
        if self.spec.supports_auth() {
            let secrets = secrets_authority();
            services.push(ServiceContribution {
                role: ServiceRole::Terminal,
                service: provider_auth_service(),
                priority: 100,
                required_authority: secrets.clone(),
            });
            maximum_authority = Authority::new(
                maximum_authority
                    .capabilities()
                    .cloned()
                    .chain(secrets.capabilities().cloned()),
            );
        }
        PluginManifest {
            id: self.spec.id.clone(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services,
            resource_namespaces: Vec::new(),
            maximum_authority,
        }
    }

    #[must_use]
    pub fn component_manifest(&self) -> ComponentManifest {
        let mut exports = vec![ComponentExport {
            interface: ModelInferenceInterface::interface_id(),
            schema: ModelInferenceInterface::schema(),
            priority: 100,
            required_authority: network_authority(),
        }];
        if self.spec.supports_auth() {
            exports.push(ComponentExport {
                interface: ProviderAuthInterface::interface_id(),
                schema: ProviderAuthInterface::schema(),
                priority: 100,
                required_authority: secrets_authority(),
            });
        }
        ComponentManifest {
            listeners: Vec::new(),
            id: provider_component_id(&self.spec.id),
            owner: self.spec.id.clone(),
            imports: Vec::new(),
            exports,
            maximum_authority: self.manifest().maximum_authority,
        }
    }

    pub fn factory(&self) -> impl Fn() -> Box<dyn PluginInstance> + Send + Sync + 'static {
        let spec = Arc::clone(&self.spec);
        move || Box::new(runtime::ProviderPlugin::new(Arc::clone(&spec)))
    }
}

fn provider_component_id(plugin: &PluginId) -> ComponentId {
    ComponentId::parse(plugin.as_str()).expect("plugin id is valid as provider component id")
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

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{Kernel, KernelConfig, ModelInferenceRequest, ModelInferenceResponse};
    use std::{
        collections::BTreeMap,
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    #[test]
    fn ca_bundle_selection_uses_only_the_explicit_phenix_override() {
        let explicit = provider_ca_bundle_from(|name| {
            (name == PHENIX_CA_BUNDLE_ENV)
                .then(|| std::path::PathBuf::from("/missing/explicit.pem"))
        });
        assert_eq!(
            explicit,
            Some(std::path::PathBuf::from("/missing/explicit.pem"))
        );

        let inherited = provider_ca_bundle_from(|name| match name {
            "SSL_CERT_FILE" => Some(std::path::PathBuf::from("/missing/ssl.pem")),
            "NIX_SSL_CERT_FILE" => Some(std::path::PathBuf::from("/nix/store/ca-bundle.crt")),
            _ => None,
        });
        assert_eq!(inherited, None);
    }

    #[test]
    fn provider_definition_derives_plugin_contracts_from_description() {
        let definition = ProviderDefinition::new(
            PluginId::parse("provider.example").unwrap(),
            Endpoint::parse("https://api.example.com/v1").unwrap(),
            Protocol::OpenAiResponses,
            auth::Definition::api_token(auth::ApiTokenMethod::bearer())
                .with_oauth(auth::OAuthMethod::bearer()),
        );

        assert_eq!(definition.plugin_id().as_str(), "provider.example");
        assert_eq!(
            definition.endpoint().as_str(),
            "https://api.example.com/v1/"
        );
        assert_eq!(definition.protocol_name(), "openai_responses");
        assert_eq!(
            definition.auth_kinds(),
            vec![AuthKind::ApiToken, AuthKind::OAuth]
        );
        let manifest = definition.manifest();
        assert_eq!(manifest.services.len(), 2);
        assert!(manifest
            .maximum_authority
            .permits(&capability(NETWORK_HTTP_CAPABILITY)));
        assert!(manifest
            .maximum_authority
            .permits(&capability(SECRETS_MANAGE_CAPABILITY)));
        let component = definition.component_manifest();
        assert!(component
            .exports
            .iter()
            .any(|export| export.interface == ModelInferenceInterface::interface_id()));
        assert!(component
            .exports
            .iter()
            .any(|export| export.interface == ProviderAuthInterface::interface_id()));
    }

    #[test]
    fn provider_definition_accepts_supported_default_auth() {
        let definition = ProviderDefinition::new(
            PluginId::parse("provider.environment").unwrap(),
            Endpoint::parse("https://api.example.com/v1").unwrap(),
            Protocol::OpenAiResponses,
            auth::Definition::api_token(auth::ApiTokenMethod::bearer()),
        )
        .with_default_auth(Auth::api_token(
            ApiTokenSource::env("EXAMPLE_API_KEY").unwrap(),
        ));

        assert_eq!(definition.auth_kinds(), vec![AuthKind::ApiToken]);
        assert!(matches!(
            definition.spec.default_auth,
            Some(Auth::ApiToken { .. })
        ));
    }

    #[test]
    fn unauthenticated_provider_does_not_gain_secret_authority() {
        let definition = ProviderDefinition::new(
            PluginId::parse("provider.public").unwrap(),
            Endpoint::parse("https://api.example.com/v1").unwrap(),
            Protocol::OpenAiResponses,
            auth::Definition::none(),
        );
        let manifest = definition.manifest();
        assert_eq!(manifest.services.len(), 1);
        assert!(!manifest
            .maximum_authority
            .permits(&capability(SECRETS_MANAGE_CAPABILITY)));
        assert_eq!(definition.component_manifest().exports.len(), 1);
    }

    #[test]
    fn generated_provider_executes_protocol_end_to_end() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let count = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..count]);
            assert!(request.starts_with("POST /v1/responses HTTP/1.1"));
            assert!(request.contains("\"model\":\"model-a\""));
            assert!(request.contains("\"input\":\"hello\""));

            let body = r#"{"id":"response-1","output":[{"content":[{"type":"output_text","text":"world"}]}]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nx-ratelimit-limit-requests: 100\r\nx-ratelimit-remaining-requests: 99\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        let definition = ProviderDefinition::new(
            PluginId::parse("provider.local").unwrap(),
            Endpoint::parse(format!("http://{address}/v1")).unwrap(),
            Protocol::OpenAiResponses,
            auth::Definition::none(),
        );
        let manifest = definition.manifest();
        let plugin = manifest.id.clone();
        let mut kernel = Kernel::new(KernelConfig::new([manifest]).unwrap());
        kernel
            .register_embedded_factory(plugin.clone(), definition.factory())
            .unwrap();
        kernel.activate_all().unwrap();

        let output = kernel
            .invoke(
                &model_inference_service(),
                &serde_json::to_vec(&phenix_core::PhenixValue::from(&ModelInferenceRequest {
                    model: phenix_core::ModelId::parse("model-a").unwrap(),
                    input: b"hello".to_vec().into(),
                    options: BTreeMap::new(),
                    cache: Default::default(),
                    tools: Vec::new(),
                    continuation: Vec::new(),
                }))
                .unwrap(),
                &network_authority(),
                Some(&plugin),
            )
            .unwrap();
        let output: phenix_core::PhenixValue = serde_json::from_slice(&output).unwrap();
        let response = ModelInferenceResponse::try_from(phenix_core::Project(&output)).unwrap();
        assert_eq!(response.output.as_ref(), b"world");
        assert_eq!(
            response.provider_metadata["id"],
            phenix_core::PhenixValue::String("response-1".into())
        );
        assert_eq!(
            response.provider_metadata["protocol"],
            phenix_core::PhenixValue::String("openai_responses".into())
        );
        assert_eq!(
            response.provider_metadata["rate_limits"],
            serde_json::json!({
                "requests": {
                    "limit": 100,
                    "remaining": 99
                }
            })
            .into()
        );
        server.join().unwrap();
    }
}
