#![forbid(unsafe_code)]

pub mod auth;
mod protocol;
mod types;

pub use protocol::{normalize_http_error, Protocol, ProtocolAdapter};
pub use types::*;

use phenix_core::{
    model_inference_service, Authority, CapabilityId, ComponentExport, ComponentId,
    ComponentInterface, ComponentManifest, InterfaceId, InvocationOutcome, ModelId,
    ModelInferenceInterface, ModelInferenceResponse, PhenixValue, PluginExecution, PluginId,
    PluginManifest, ServiceContribution, ServiceId, ServiceRole,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

pub const PROVIDER_AUTH_SERVICE: &str = "phenix.providers.auth@1";
pub const PROVIDER_MODELS_SERVICE: &str = "phenix.providers.models@1";
pub const NETWORK_HTTP_CAPABILITY: &str = "network.http";
pub const SECRETS_MANAGE_CAPABILITY: &str = "secrets.manage";

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
    pub provider_name: String,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderModelOrigin {
    Discovered,
    Declared,
    DiscoveredAndDeclared,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderModel {
    pub id: ModelId,
    pub origin: ProviderModelOrigin,
    pub thinking: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderModelsCommand {
    List,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderModelsResponse {
    Models { models: Vec<ProviderModel> },
}

pub struct ProviderModelsInterface;

impl ComponentInterface for ProviderModelsInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(PROVIDER_MODELS_SERVICE)
            .expect("static provider models interface is valid")
    }
}

#[must_use]
pub fn provider_models_service() -> ServiceId {
    ServiceId::parse(PROVIDER_MODELS_SERVICE).expect("static provider models service is valid")
}

pub struct ProviderRuntimeSpec {
    pub id: PluginId,
    pub display_name: String,
    pub endpoint: Endpoint,
    pub auth: auth::Definition,
    pub default_auth: Option<Auth>,
    pub declared_models: Vec<ModelId>,
    pub model_thinking: BTreeMap<ModelId, Vec<String>>,
    pub protocol: Arc<dyn ProtocolAdapter>,
}

impl ProviderRuntimeSpec {
    pub fn auth_kinds(&self) -> Vec<AuthKind> {
        self.auth.kinds()
    }

    pub fn supports_auth(&self) -> bool {
        !self.auth.is_empty()
    }

    pub fn supports_model_catalog(&self) -> bool {
        self.protocol.supports_model_catalog() || !self.declared_models.is_empty()
    }
}

#[derive(Clone)]
pub struct ProviderDefinition {
    spec: Arc<ProviderRuntimeSpec>,
}

impl ProviderDefinition {
    pub fn new(
        id: PluginId,
        endpoint: Endpoint,
        protocol: impl ProtocolAdapter + 'static,
        auth: impl Into<auth::Definition>,
    ) -> Self {
        let display_name = id.to_string();
        Self {
            spec: Arc::new(ProviderRuntimeSpec {
                id,
                display_name,
                endpoint,
                auth: auth.into(),
                default_auth: None,
                declared_models: Vec::new(),
                model_thinking: BTreeMap::new(),
                protocol: Arc::new(protocol),
            }),
        }
    }

    #[must_use]
    pub fn with_display_name(self, display_name: impl Into<String>) -> Self {
        let display_name = display_name.into();
        assert!(
            !display_name.trim().is_empty(),
            "provider display name must not be empty"
        );
        Self {
            spec: Arc::new(ProviderRuntimeSpec {
                id: self.spec.id.clone(),
                display_name,
                endpoint: self.spec.endpoint.clone(),
                auth: self.spec.auth.clone(),
                default_auth: self.spec.default_auth.clone(),
                declared_models: self.spec.declared_models.clone(),
                model_thinking: self.spec.model_thinking.clone(),
                protocol: Arc::clone(&self.spec.protocol),
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
            spec: Arc::new(ProviderRuntimeSpec {
                id: self.spec.id.clone(),
                display_name: self.spec.display_name.clone(),
                endpoint: self.spec.endpoint.clone(),
                auth: self.spec.auth.clone(),
                default_auth: Some(default_auth),
                declared_models: self.spec.declared_models.clone(),
                model_thinking: self.spec.model_thinking.clone(),
                protocol: Arc::clone(&self.spec.protocol),
            }),
        }
    }

    #[must_use]
    pub fn with_declared_models(self, models: impl IntoIterator<Item = ModelId>) -> Self {
        let mut declared_models = models.into_iter().collect::<Vec<_>>();
        declared_models.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        declared_models.dedup();
        Self {
            spec: Arc::new(ProviderRuntimeSpec {
                id: self.spec.id.clone(),
                display_name: self.spec.display_name.clone(),
                endpoint: self.spec.endpoint.clone(),
                auth: self.spec.auth.clone(),
                default_auth: self.spec.default_auth.clone(),
                declared_models,
                model_thinking: self.spec.model_thinking.clone(),
                protocol: Arc::clone(&self.spec.protocol),
            }),
        }
    }

    #[must_use]
    pub fn with_model_thinking<I, S>(self, model: ModelId, levels: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut levels = levels.into_iter().map(Into::into).collect::<Vec<_>>();
        assert!(
            levels.iter().all(|level| !level.trim().is_empty()),
            "provider thinking level must not be empty"
        );
        levels.sort();
        levels.dedup();
        let mut model_thinking = self.spec.model_thinking.clone();
        if levels.is_empty() {
            model_thinking.remove(&model);
        } else {
            model_thinking.insert(model, levels);
        }
        Self {
            spec: Arc::new(ProviderRuntimeSpec {
                id: self.spec.id.clone(),
                display_name: self.spec.display_name.clone(),
                endpoint: self.spec.endpoint.clone(),
                auth: self.spec.auth.clone(),
                default_auth: self.spec.default_auth.clone(),
                declared_models: self.spec.declared_models.clone(),
                model_thinking,
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
        if self.spec.supports_model_catalog() {
            services.push(ServiceContribution {
                role: ServiceRole::Terminal,
                service: provider_models_service(),
                priority: 100,
                required_authority: if self.spec.protocol.supports_model_catalog() {
                    network_authority()
                } else {
                    Authority::default()
                },
            });
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
        if self.spec.supports_model_catalog() {
            exports.push(ComponentExport {
                interface: ProviderModelsInterface::interface_id(),
                schema: ProviderModelsInterface::schema(),
                priority: 100,
                required_authority: if self.spec.protocol.supports_model_catalog() {
                    network_authority()
                } else {
                    Authority::default()
                },
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

    #[must_use]
    pub fn runtime_spec(&self) -> Arc<ProviderRuntimeSpec> {
        Arc::clone(&self.spec)
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
    use std::collections::BTreeMap;

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
        assert_eq!(manifest.services.len(), 3);
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
        assert_eq!(manifest.services.len(), 2);
        assert!(!manifest
            .maximum_authority
            .permits(&capability(SECRETS_MANAGE_CAPABILITY)));
        assert_eq!(definition.component_manifest().exports.len(), 2);
    }

    #[test]
    fn provider_catalog_merges_standard_discovery_with_declared_models() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let count = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..count]);
            assert!(request.starts_with("GET /v1/models HTTP/1.1"));

            let body = r#"{"data":[{"id":"model-live"},{"id":"model-declared"}]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        let definition = ProviderDefinition::new(
            PluginId::parse("provider.catalog").unwrap(),
            Endpoint::parse(format!("http://{address}/v1")).unwrap(),
            Protocol::OpenAiResponses,
            auth::Definition::none(),
        )
        .with_declared_models([ModelId::parse("model-declared").unwrap()])
        .with_model_thinking(ModelId::parse("model-live").unwrap(), ["low", "high"]);
        let manifest = definition.manifest();
        assert!(manifest
            .services
            .iter()
            .any(|service| service.service == provider_models_service()));
        let plugin = manifest.id.clone();
        let mut kernel = Kernel::new(KernelConfig::new([manifest]).unwrap());
        kernel
            .register_embedded_factory(plugin.clone(), definition.factory())
            .unwrap();
        kernel.activate_all().unwrap();

        let output = kernel
            .invoke(
                &provider_models_service(),
                &serde_json::to_vec(&ProviderModelsCommand::List).unwrap(),
                &network_authority(),
                Some(&plugin),
            )
            .unwrap();
        let response: ProviderModelsResponse = serde_json::from_slice(&output).unwrap();
        assert_eq!(
            response,
            ProviderModelsResponse::Models {
                models: vec![
                    ProviderModel {
                        id: ModelId::parse("model-declared").unwrap(),
                        origin: ProviderModelOrigin::DiscoveredAndDeclared,
                        thinking: Vec::new(),
                    },
                    ProviderModel {
                        id: ModelId::parse("model-live").unwrap(),
                        origin: ProviderModelOrigin::Discovered,
                        thinking: vec!["high".to_owned(), "low".to_owned()],
                    },
                ],
            }
        );
        server.join().unwrap();
    }

    #[test]
    fn declared_models_remain_available_when_remote_discovery_fails() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            let body = r#"{"error":{"message":"catalog unavailable"}}"#;
            write!(
                stream,
                "HTTP/1.1 503 Service Unavailable\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        let definition = ProviderDefinition::new(
            PluginId::parse("provider.catalog-fallback").unwrap(),
            Endpoint::parse(format!("http://{address}/v1")).unwrap(),
            Protocol::OpenAiResponses,
            auth::Definition::none(),
        )
        .with_declared_models([ModelId::parse("model-declared").unwrap()]);
        let manifest = definition.manifest();
        let plugin = manifest.id.clone();
        let mut kernel = Kernel::new(KernelConfig::new([manifest]).unwrap());
        kernel
            .register_embedded_factory(plugin.clone(), definition.factory())
            .unwrap();
        kernel.activate_all().unwrap();

        let output = kernel
            .invoke(
                &provider_models_service(),
                &serde_json::to_vec(&ProviderModelsCommand::List).unwrap(),
                &network_authority(),
                Some(&plugin),
            )
            .unwrap();
        let response: ProviderModelsResponse = serde_json::from_slice(&output).unwrap();
        assert_eq!(
            response,
            ProviderModelsResponse::Models {
                models: vec![ProviderModel {
                    id: ModelId::parse("model-declared").unwrap(),
                    origin: ProviderModelOrigin::Declared,
                    thinking: Vec::new(),
                }],
            }
        );
        server.join().unwrap();
    }

    #[test]
    fn nonstandard_protocol_can_publish_declared_models_without_discovery() {
        let definition = ProviderDefinition::new(
            PluginId::parse("provider.declared").unwrap(),
            Endpoint::parse("https://api.example.com/v1").unwrap(),
            Protocol::OpenCodeGo,
            auth::Definition::none(),
        )
        .with_declared_models([ModelId::parse("model-a").unwrap()]);
        assert!(definition
            .manifest()
            .services
            .iter()
            .any(|service| service.service == provider_models_service()));
    }

}
