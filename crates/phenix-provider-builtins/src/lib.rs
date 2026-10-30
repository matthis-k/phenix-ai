//! Common provider registrations without secrets, runtime policy or network I/O.
//!
//! A provider plugin may reuse these descriptions, override an installed
//! connection, or supply a different provider entirely. The runtime must
//! revalidate and authorize every selected account and model.

pub mod catalog;
pub mod declaration;
pub mod defaults;
pub use catalog::discovered_model_page;

use phenix_provider_contracts::{
    AdapterBinding, ApiKeyOptions, ApiKeyPresentation, AuthenticationKind, AuthenticationMethod,
    ConnectionDescriptor, DiscoveryKind, DiscoverySource, ProtocolBinding, ProviderDescriptor,
    ProviderPage,
};
use phenix_provider_protocols::{same_credential_origin, validate_endpoint, EndpointMode};
use std::collections::{BTreeMap, BTreeSet};

pub const OPENAI_CHAT: &str = "openai-chat";
pub const OPENAI_RESPONSES: &str = "openai-responses";
pub const ANTHROPIC_MESSAGES: &str = "anthropic-messages";

#[cfg(test)]
fn method(id: &str, label: &str, kind: AuthenticationKind) -> AuthenticationMethod {
    AuthenticationMethod {
        id: id.into(),
        label: label.into(),
        kind,
        adapter: AdapterBinding::from_typed("fixture.auth", &()).unwrap(),
    }
}

#[cfg(test)]
fn api_key() -> Vec<AuthenticationMethod> {
    vec![method("api-key", "API key", AuthenticationKind::ApiKey)]
}

#[cfg(test)]
fn source(endpoint: Option<&str>, id: &str) -> DiscoverySource {
    DiscoverySource {
        id: id.into(),
        kind: if endpoint.is_some() {
            DiscoveryKind::Dynamic
        } else {
            DiscoveryKind::Declared
        },
        endpoint: endpoint.map(str::to_string),
        adapter: AdapterBinding::from_typed("fixture.models", &()).unwrap(),
    }
}

#[cfg(test)]
fn connection(
    id: &str,
    label: &str,
    base: &str,
    protocols: &[&str],
    methods: Vec<AuthenticationMethod>,
    discovery: Option<&str>,
) -> ConnectionDescriptor {
    ConnectionDescriptor {
        id: id.into(),
        label: label.into(),
        protocols: protocols
            .iter()
            .map(|id| ProtocolBinding {
                id: (*id).into(),
                endpoint: None,
            })
            .collect(),
        base_url: Some(base.into()),
        auth_methods: methods,
        model_sources: vec![source(discovery, "models")],
    }
}

#[cfg(test)]
fn provider(id: &str, label: &str, connections: Vec<ConnectionDescriptor>) -> ProviderDescriptor {
    ProviderDescriptor {
        id: id.into(),
        label: label.into(),
        connections,
    }
}

/// The smallest author-facing provider connection: endpoint, protocol, auth.
///
/// The provider/connection ID belongs to the registry, and named accounts
/// belong to the credential store, NOT this declaration. Extra protocol
/// endpoints, discovery codecs, and environment hints are explicit optional
/// extensions. A declaration is metadata, never network or kernel authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderDeclaration<'a> {
    pub endpoint: &'a str,
    pub protocol: &'a str,
    pub auth: &'a str,
}

/// Lookup of auth implementations installed by the provider composition.
/// External plugins can register new methods without changing Phenix core.
pub trait AuthenticationAdapters {
    fn methods(&self, adapter: &str) -> Result<Vec<AuthenticationMethod>, &'static str>;
}

/// Convenience registry for the providers shipped in the default profile.
/// It does not grant access to any of the methods it describes.
pub struct BuiltinAuthenticationAdapters;

impl AuthenticationAdapters for BuiltinAuthenticationAdapters {
    fn methods(&self, adapter: &str) -> Result<Vec<AuthenticationMethod>, &'static str> {
        match adapter {
            "api-key" => Ok(vec![AuthenticationMethod {
                id: "api-key".into(),
                label: "API key".into(),
                kind: AuthenticationKind::ApiKey,
                adapter: AdapterBinding::from_typed(
                    "api-key",
                    &ApiKeyOptions {
                        environment_variable: None,
                        interactive: true,
                        presentation: ApiKeyPresentation::Bearer,
                    },
                )
                .map_err(|_| "invalid API key adapter")?,
            }]),
            "chatgpt-oauth" => Ok(vec![AuthenticationMethod {
                id: "chatgpt-sign-in".into(),
                label: "Sign in with ChatGPT".into(),
                kind: AuthenticationKind::OAuthAuthorizationCode,
                adapter: AdapterBinding::from_typed("chatgpt.oauth", &())
                    .map_err(|_| "invalid OAuth adapter")?,
            }]),
            "none" => Ok(vec![]),
            _ => Err("authentication adapter is not registered"),
        }
    }
}

impl ProviderDeclaration<'_> {
    pub fn connection(self, id: &str, label: &str) -> Result<ConnectionDescriptor, &'static str> {
        self.connection_with(id, label, &BuiltinAuthenticationAdapters)
    }

    pub fn connection_with(
        self,
        id: &str,
        label: &str,
        registry: &impl AuthenticationAdapters,
    ) -> Result<ConnectionDescriptor, &'static str> {
        if self.endpoint.is_empty() || self.protocol.is_empty() {
            return Err("provider endpoint and protocol are required");
        }
        validate_endpoint(self.endpoint, EndpointMode::LocalLoopback)
            .map_err(|_| "unsafe provider endpoint")?;
        let auth_methods = registry.methods(self.auth)?;
        Ok(ConnectionDescriptor {
            id: id.into(),
            label: label.into(),
            protocols: vec![ProtocolBinding {
                id: self.protocol.into(),
                endpoint: None,
            }],
            base_url: Some(self.endpoint.into()),
            auth_methods,
            // No implicit /models guess. A provider or registered discovery
            // adapter contributes this only where the provider supports it.
            model_sources: vec![],
        })
    }
}

/// Shared endpoint and discovery conventions for provider declarations.
///
/// This references installed standard protocol implementations; it does not
/// introduce provider-specific Rust, perform discovery, or grant credentials.
/// The declaration chooses supported wire protocols and the actual catalog
/// response adapter. Non-standard providers can still supply exact endpoints.
pub trait StandardConventions {
    /// Populate endpoints for selected established protocol wire formats.
    /// An omitted protocol is never inferred to be supported.
    fn with_standard_protocols(self, additional: &[&str]) -> Result<Self, &'static str>
    where
        Self: Sized;

    /// Select a documented /models endpoint and existing catalog adapter.
    /// The caller opts in explicitly; not all OpenAI-compatible APIs provide
    /// discovery, and some use a different response schema.
    fn with_standard_models(self, adapter: &str) -> Result<Self, &'static str>
    where
        Self: Sized;
}

fn standard_endpoint(base: &str, path: &str) -> Result<String, &'static str> {
    if base.contains('?') || base.ends_with('/') {
        return Err("standard convention requires a plain endpoint base");
    }
    let authorized = validate_endpoint(base, EndpointMode::LocalLoopback)
        .map_err(|_| "unsafe standard provider base")?;
    let target = format!("{base}/{path}");
    let candidate = validate_endpoint(&target, EndpointMode::LocalLoopback)
        .map_err(|_| "unsafe standard protocol endpoint")?;
    if !same_credential_origin(&authorized, &candidate) {
        return Err("cross-origin standard protocol endpoint");
    }
    Ok(target)
}

impl StandardConventions for ConnectionDescriptor {
    fn with_standard_protocols(mut self, additional: &[&str]) -> Result<Self, &'static str> {
        let base = self.base_url.as_deref().ok_or("missing provider URL")?;
        let mut ids = self
            .protocols
            .iter()
            .map(|binding| binding.id.as_str())
            .collect::<Vec<_>>();
        ids.extend_from_slice(additional);
        let mut seen = BTreeSet::new();
        let mut protocols = Vec::new();
        for id in ids {
            if !seen.insert(id) {
                continue;
            }
            let path = match id {
                OPENAI_CHAT => "chat/completions",
                OPENAI_RESPONSES => "responses",
                ANTHROPIC_MESSAGES => "messages",
                _ => return Err("protocol has no registered standard endpoint"),
            };
            protocols.push(ProtocolBinding {
                id: id.to_owned(),
                endpoint: Some(standard_endpoint(base, path)?),
            });
        }
        self.protocols = protocols;
        Ok(self)
    }

    fn with_standard_models(self, adapter: &str) -> Result<Self, &'static str> {
        let base = self.base_url.as_deref().ok_or("missing provider URL")?;
        let endpoint = standard_endpoint(base, "models")?;
        self.with_catalog(Some(&endpoint), adapter, DiscoveryKind::Dynamic)
    }
}

/// Named adapters are registry-level implementations. The builder merely
/// references them; it does not implement OAuth or model streaming.
pub trait ConnectionExtensions {
    fn with_protocol(self, id: &str, endpoint: Option<&str>) -> Self;
    fn with_catalog(
        self,
        endpoint: Option<&str>,
        adapter: &str,
        kind: DiscoveryKind,
    ) -> Result<Self, &'static str>
    where
        Self: Sized;
    fn with_key_environment(self, name: &str) -> Result<Self, &'static str>
    where
        Self: Sized;
    fn with_key_presentation(self, presentation: ApiKeyPresentation) -> Result<Self, &'static str>
    where
        Self: Sized;
}

impl ConnectionExtensions for ConnectionDescriptor {
    fn with_protocol(mut self, id: &str, endpoint: Option<&str>) -> Self {
        // Replace the primary protocol's optional endpoint without creating
        // a duplicate, otherwise append an additional supported protocol.
        if let Some(existing) = self.protocols.iter_mut().find(|item| item.id == id) {
            existing.endpoint = endpoint.map(str::to_owned);
        } else {
            self.protocols.push(ProtocolBinding {
                id: id.into(),
                endpoint: endpoint.map(str::to_owned),
            });
        }
        self
    }

    fn with_catalog(
        mut self,
        endpoint: Option<&str>,
        adapter: &str,
        kind: DiscoveryKind,
    ) -> Result<Self, &'static str> {
        self.model_sources.push(DiscoverySource {
            id: "models".into(),
            kind,
            endpoint: endpoint.map(str::to_owned),
            adapter: AdapterBinding::from_typed(adapter, &())
                .map_err(|_| "unregistered model catalog adapter")?,
        });
        Ok(self)
    }

    fn with_key_environment(mut self, name: &str) -> Result<Self, &'static str> {
        let Some(method) = self.auth_methods.iter_mut().find(|m| m.id == "api-key") else {
            return Err("key environment variable requires API key auth");
        };
        let mut options = method
            .adapter
            .config
            .decode::<ApiKeyOptions>()
            .map_err(|_| "API-key source adapter has incompatible configuration")?;
        options.environment_variable = Some(name.into());
        options
            .validate()
            .map_err(|_| "invalid API key environment")?;
        method.adapter = AdapterBinding::from_typed("api-key", &options)
            .map_err(|_| "invalid API key environment")?;
        Ok(self)
    }

    fn with_key_presentation(
        mut self,
        presentation: ApiKeyPresentation,
    ) -> Result<Self, &'static str> {
        presentation
            .validate()
            .map_err(|_| "invalid API-key wire presentation")?;
        let Some(method) = self.auth_methods.iter_mut().find(|m| m.id == "api-key") else {
            return Err("wire presentation requires the API-key strategy");
        };
        let mut options = method
            .adapter
            .config
            .decode::<ApiKeyOptions>()
            .map_err(|_| "API-key source adapter has incompatible configuration")?;
        options.presentation = presentation;
        options
            .validate()
            .map_err(|_| "invalid API-key wire presentation")?;
        method.adapter = AdapterBinding::from_typed("api-key", &options)
            .map_err(|_| "invalid API-key wire presentation")?;
        Ok(self)
    }
}

/// Compile the immutable built-in provider data through the same SDK path
/// available to third-party provider registration contributors. This does
/// not install auth methods or grant model execution authority.
pub fn registrations() -> Vec<ProviderDescriptor> {
    declaration::compile(defaults::PROVIDERS, &BuiltinAuthenticationAdapters)
        .expect("valid built-in provider data")
}

#[cfg(test)]
mod declaration_tests {
    use super::*;

    #[test]
    fn minimal_provider_is_three_fields_and_never_implicitly_discovers_models() {
        let declaration = ProviderDeclaration {
            endpoint: "https://api.example.test/v1",
            protocol: OPENAI_RESPONSES,
            auth: "api-key",
        };
        let connection = declaration.connection("api", "Example").unwrap();
        assert_eq!(connection.base_url.as_deref(), Some(declaration.endpoint));
        assert_eq!(connection.protocols[0].id, declaration.protocol);
        assert_eq!(connection.auth_methods[0].id, declaration.auth);
        assert!(connection.model_sources.is_empty());
        assert_eq!(connection.id, "api");
    }

    #[test]
    fn standard_conventions_are_provider_neutral_and_opt_in() {
        let connection = ProviderDeclaration {
            endpoint: "https://inference.example.test/api/v1",
            protocol: OPENAI_CHAT,
            auth: "api-key",
        }
        .connection("standard", "Standard provider")
        .unwrap()
        .with_standard_protocols(&[OPENAI_RESPONSES])
        .unwrap()
        .with_standard_models("openai.models")
        .unwrap();
        assert_eq!(connection.protocols[0].id, OPENAI_CHAT);
        assert_eq!(
            connection.protocols[0].endpoint.as_deref(),
            Some("https://inference.example.test/api/v1/chat/completions")
        );
        assert_eq!(
            connection.protocols[1].endpoint.as_deref(),
            Some("https://inference.example.test/api/v1/responses")
        );
        assert_eq!(
            connection.model_sources[0].endpoint.as_deref(),
            Some("https://inference.example.test/api/v1/models")
        );
        assert_eq!(connection.model_sources[0].adapter.id, "openai.models");
        validate_registrations(&[ProviderDescriptor {
            id: "vendor-neutral".into(),
            label: "Generic HTTP models".into(),
            connections: vec![connection],
        }])
        .unwrap();

        let unsupported = ProviderDeclaration {
            endpoint: "https://inference.example.test/api/v1",
            protocol: "custom-binary",
            auth: "api-key",
        }
        .connection("custom", "Unconventional")
        .unwrap();
        assert_eq!(
            unsupported.with_standard_protocols(&[]).unwrap_err(),
            "protocol has no registered standard endpoint"
        );
        assert_eq!(
            standard_endpoint("https://example.test/v1?token=unsafe", "models"),
            Err("standard convention requires a plain endpoint base")
        );
    }

    #[test]
    fn standard_conventions_do_not_install_catalogs_implicitly() {
        let connection = ProviderDeclaration {
            endpoint: "https://example.test/v1",
            protocol: ANTHROPIC_MESSAGES,
            auth: "api-key",
        }
        .connection("api", "Example")
        .unwrap()
        .with_standard_protocols(&[])
        .unwrap();
        assert_eq!(
            connection.protocols[0].endpoint.as_deref(),
            Some("https://example.test/v1/messages")
        );
        assert!(connection.model_sources.is_empty());
    }

    #[test]
    fn unknown_adapter_or_unsafe_endpoint_fails_closed() {
        let declaration = ProviderDeclaration {
            endpoint: "https://api.example.test/v1",
            protocol: OPENAI_CHAT,
            auth: "unregistered-oauth",
        };
        assert_eq!(
            declaration.connection("api", "Unregistered").unwrap_err(),
            "authentication adapter is not registered"
        );
        assert_eq!(
            ProviderDeclaration {
                endpoint: "http://api.example.test/v1",
                ..declaration
            }
            .connection("api", "Unsafe")
            .unwrap_err(),
            "unsafe provider endpoint"
        );
    }

    #[test]
    fn externally_registered_authentication_adapter_is_supported() {
        struct ThirdParty;
        impl AuthenticationAdapters for ThirdParty {
            fn methods(&self, adapter: &str) -> Result<Vec<AuthenticationMethod>, &'static str> {
                if adapter != "vendor.sso" {
                    return Err("unknown third-party adapter");
                }
                Ok(vec![AuthenticationMethod {
                    id: "sso".into(),
                    label: "Vendor SSO".into(),
                    kind: AuthenticationKind::Plugin,
                    adapter: AdapterBinding::from_typed("vendor.sso", &()).unwrap(),
                }])
            }
        }
        let connection = ProviderDeclaration {
            endpoint: "https://api.vendor.example/v1",
            protocol: "vendor-protocol",
            auth: "vendor.sso",
        }
        .connection_with("custom", "Vendor", &ThirdParty)
        .unwrap();
        assert_eq!(connection.auth_methods[0].adapter.id, "vendor.sso");
        assert_eq!(connection.protocols[0].id, "vendor-protocol");
        validate_registrations(&[ProviderDescriptor {
            id: "vendor".into(),
            label: "Vendor".into(),
            connections: vec![connection],
        }])
        .unwrap();
    }

    #[test]
    fn extra_protocols_and_discovery_are_optional_but_origin_is_checked() {
        let connection = ProviderDeclaration {
            endpoint: "https://api.example.test/v1",
            protocol: OPENAI_RESPONSES,
            auth: "api-key",
        }
        .connection("api", "Example")
        .unwrap()
        .with_protocol(
            OPENAI_RESPONSES,
            Some("https://api.example.test/v1/responses"),
        )
        .with_catalog(
            Some("https://api.example.test/v1/models"),
            "test.models",
            DiscoveryKind::Dynamic,
        )
        .unwrap();
        validate_registrations(&[ProviderDescriptor {
            id: "example".into(),
            label: "Example".into(),
            connections: vec![connection.clone()],
        }])
        .unwrap();
        let wrong_origin = connection.with_protocol(
            OPENAI_CHAT,
            Some("https://different.example.test/v1/chat/completions"),
        );
        assert_eq!(
            validate_registrations(&[ProviderDescriptor {
                id: "example".into(),
                label: "Example".into(),
                connections: vec![wrong_origin],
            }]),
            Err("cross-origin protocol endpoint")
        );
    }
}

/// Build one immutable catalog snapshot from the explicitly selected
/// registration contributors. Provider IDs must be globally unique, including
/// across two independently valid contributors. No registration grants model,
/// authentication, credential, or invocation authority.
pub fn merge_registration_pages(
    pages: impl IntoIterator<Item = ProviderPage>,
) -> Result<Vec<ProviderDescriptor>, &'static str> {
    const MAX_PROVIDERS: usize = 256;
    const MAX_CONNECTIONS: usize = 1024;
    let mut merged: BTreeMap<String, ProviderDescriptor> = BTreeMap::new();
    let mut connections = 0usize;
    for page in pages {
        if page.revision == 0 {
            return Err("invalid provider registration revision");
        }
        for mut provider in page.providers {
            if provider.id.is_empty() {
                return Err("duplicate or empty provider identity");
            }
            connections = connections
                .checked_add(provider.connections.len())
                .ok_or("provider registration limit exceeded")?;
            if connections > MAX_CONNECTIONS {
                return Err("provider registration limit exceeded");
            }
            if let Some(existing) = merged.get_mut(&provider.id) {
                if existing.label != provider.label {
                    return Err("conflicting provider display identity");
                }
                existing.connections.append(&mut provider.connections);
            } else {
                if merged.len() >= MAX_PROVIDERS {
                    return Err("provider registration limit exceeded");
                }
                merged.insert(provider.id.clone(), provider);
            }
        }
    }
    let mut result = merged.into_values().collect::<Vec<_>>();
    // Connection ordering is independent of startup slot order; metadata
    // conflicts still fail closed in the existing validation boundary.
    for provider in &mut result {
        provider.connections.sort_by(|a, b| a.id.cmp(&b.id));
    }
    validate_registrations(&result)?;
    Ok(result)
}

/// Defensive startup check for static registrations. Dynamic provider plugins
/// must receive the same runtime validation before installation.
pub fn validate_registrations(providers: &[ProviderDescriptor]) -> Result<(), &'static str> {
    let mut seen_providers = BTreeSet::new();
    for provider in providers {
        if !seen_providers.insert(provider.id.as_str()) || provider.id.is_empty() {
            return Err("duplicate or empty provider identity");
        }
        let mut seen_connections = BTreeSet::new();
        for connection in &provider.connections {
            if connection.id.is_empty()
                || !seen_connections.insert(connection.id.as_str())
                || connection.protocols.is_empty()
            {
                return Err("invalid provider connection");
            }
            // A connection may be in-process (no URL), local HTTP on a literal
            // loopback IP, or remote HTTPS. All explicitly declared network
            // endpoints on one connection must share one credential origin.
            let mut credential_origin = connection
                .base_url
                .as_deref()
                .map(|base| {
                    validate_endpoint(base, EndpointMode::LocalLoopback)
                        .map_err(|_| "unsafe provider endpoint")
                })
                .transpose()?;
            let mut protocol_ids = BTreeSet::new();
            for protocol in &connection.protocols {
                if protocol.id.is_empty() || !protocol_ids.insert(protocol.id.as_str()) {
                    return Err("duplicate or empty provider protocol");
                }
                if let Some(endpoint) = protocol.endpoint.as_deref() {
                    let checked = validate_endpoint(endpoint, EndpointMode::LocalLoopback)
                        .map_err(|_| "unsafe protocol endpoint")?;
                    if let Some(origin) = credential_origin.as_ref() {
                        if !same_credential_origin(origin, &checked) {
                            return Err("cross-origin protocol endpoint");
                        }
                    } else {
                        credential_origin = Some(checked);
                    }
                }
            }
            let mut method_ids = BTreeSet::new();
            for method in &connection.auth_methods {
                if !method_ids.insert(method.id.as_str()) || method.id.is_empty() {
                    return Err("duplicate authentication method");
                }
                if method.adapter.validate().is_err() {
                    return Err("invalid authentication adapter");
                }
            }
            let mut source_ids = BTreeSet::new();
            for source in &connection.model_sources {
                if source.id.is_empty() || !source_ids.insert(source.id.as_str()) {
                    return Err("duplicate or empty model discovery source");
                }
                if source.adapter.validate().is_err() {
                    return Err("invalid model discovery adapter");
                }
                match (&source.kind, source.endpoint.as_deref()) {
                    (DiscoveryKind::Declared, None) => {}
                    // Plugins may discover models in-process without an HTTP
                    // endpoint. The selected implementation owns that lookup.
                    (DiscoveryKind::Dynamic, None) => {}
                    (DiscoveryKind::Dynamic, Some(endpoint)) => {
                        let checked = validate_endpoint(endpoint, EndpointMode::LocalLoopback)
                            .map_err(|_| "unsafe catalog endpoint")?;
                        if let Some(origin) = credential_origin.as_ref() {
                            if !same_credential_origin(origin, &checked) {
                                return Err("cross-origin model discovery");
                            }
                        } else {
                            credential_origin = Some(checked);
                        }
                    }
                    _ => return Err("inconsistent model discovery source"),
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contributor_pages_merge_deterministically_without_granting_anything() {
        let providers = registrations();
        let first = ProviderPage {
            providers: vec![providers[1].clone()],
            revision: 7,
        };
        let second = ProviderPage {
            providers: vec![providers[0].clone()],
            revision: 3,
        };
        let merged = merge_registration_pages([first, second]).unwrap();
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].id, "openai");
        assert_eq!(merged[1].id, "openrouter");
        assert!(merge_registration_pages(Vec::<ProviderPage>::new())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn contributor_pages_reject_collisions_invalid_revision_and_origins() {
        let providers = registrations();
        let page = ProviderPage {
            providers: vec![providers[0].clone()],
            revision: 1,
        };
        assert_eq!(
            merge_registration_pages([page.clone(), page]).unwrap_err(),
            "invalid provider connection"
        );
        assert_eq!(
            merge_registration_pages([ProviderPage {
                providers: vec![],
                revision: 0,
            }])
            .unwrap_err(),
            "invalid provider registration revision"
        );
        let mut invalid = providers[0].clone();
        invalid.connections[0].protocols[0].endpoint =
            Some("https://untrusted.example/v1/responses".into());
        assert_eq!(
            merge_registration_pages([ProviderPage {
                providers: vec![invalid],
                revision: 1,
            }])
            .unwrap_err(),
            "cross-origin protocol endpoint"
        );
    }

    #[test]
    fn separate_api_key_and_oauth_contributors_join_one_provider_without_authority() {
        let catalog = registrations();
        let openai = &catalog[0];
        let api = ProviderPage {
            providers: vec![provider(
                &openai.id,
                &openai.label,
                vec![openai.connections[0].clone()],
            )],
            revision: 1,
        };
        let oauth = ProviderPage {
            providers: vec![provider(
                &openai.id,
                &openai.label,
                vec![openai.connections[1].clone()],
            )],
            revision: 2,
        };
        let forward = merge_registration_pages([api.clone(), oauth.clone()]).unwrap();
        let reverse = merge_registration_pages([oauth.clone(), api.clone()]).unwrap();
        assert_eq!(forward, reverse);
        assert_eq!(forward.len(), 1);
        assert_eq!(forward[0].id, "openai");
        assert_eq!(
            forward[0]
                .connections
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>(),
            vec!["api", "chatgpt"]
        );
        // Registration pages cannot take over a sibling connection or
        // silently rename an already selected provider identity.
        let mut wrong_label = oauth.clone();
        wrong_label.providers[0].label = "Different provider".into();
        assert_eq!(
            merge_registration_pages([api.clone(), wrong_label]).unwrap_err(),
            "conflicting provider display identity"
        );
        let mut duplicate = oauth;
        duplicate.providers[0].connections[0].id = "api".into();
        assert_eq!(
            merge_registration_pages([api, duplicate]).unwrap_err(),
            "invalid provider connection"
        );
    }

    #[test]
    fn six_groups_and_distinct_api_and_chatgpt_accounts() {
        let catalog = registrations();
        validate_registrations(&catalog).unwrap();
        assert_eq!(catalog.len(), 6);
        let openai = &catalog[0];
        assert_eq!(openai.id, "openai");
        assert_eq!(openai.connections.len(), 2);
        assert_ne!(openai.connections[0].id, openai.connections[1].id);
        assert_eq!(
            openai.connections[0].auth_methods[0].kind,
            AuthenticationKind::ApiKey
        );
        assert_eq!(
            openai.connections[1].auth_methods[0].kind,
            AuthenticationKind::OAuthAuthorizationCode
        );
        assert_eq!(
            openai.connections[1].protocols,
            vec![ProtocolBinding {
                id: OPENAI_RESPONSES.into(),
                endpoint: Some("https://api.openai.com/v1/responses".into()),
            }]
        );
    }

    #[test]
    fn registration_accepts_no_auth_key_only_and_two_independent_methods() {
        // These are metadata-only fixtures, not claims that a provider
        // backend, OAuth issuer, or usable model has been installed.
        let no_auth = connection(
            "no-auth",
            "Credential-free connection",
            "https://models.example.test/v1",
            &[OPENAI_CHAT],
            vec![],
            None,
        );
        let key_only = connection(
            "key",
            "Named API-key connection",
            "https://models.example.test/v1",
            &[OPENAI_CHAT],
            api_key(),
            Some("https://models.example.test/v1/models"),
        );
        let both = connection(
            "both",
            "Two explicit methods",
            "https://models.example.test/v1",
            &[OPENAI_RESPONSES],
            vec![
                method("api-key", "API key", AuthenticationKind::ApiKey),
                method(
                    "oauth",
                    "Approved OAuth",
                    AuthenticationKind::OAuthAuthorizationCode,
                ),
            ],
            Some("https://models.example.test/v1/models"),
        );
        let fixture = provider(
            "fixture",
            "Fixture",
            vec![no_auth.clone(), key_only.clone(), both.clone()],
        );
        assert_eq!(validate_registrations(&[fixture]), Ok(()));
        assert_eq!(
            [
                no_auth.auth_methods.len(),
                key_only.auth_methods.len(),
                both.auth_methods.len()
            ],
            [0, 1, 2]
        );
        assert_eq!(both.auth_methods[0].kind, AuthenticationKind::ApiKey);
        assert_eq!(
            both.auth_methods[1].kind,
            AuthenticationKind::OAuthAuthorizationCode
        );
        // The choice is scoped to one connection. Duplicating method IDs
        // within that connection would make explicit authentication ambiguous.
        let mut duplicate = both;
        duplicate.auth_methods[1].id = "api-key".into();
        let bad = provider("fixture", "Fixture", vec![duplicate]);
        assert_eq!(
            validate_registrations(&[bad]),
            Err("duplicate authentication method")
        );
    }

    #[test]
    fn each_provider_uses_an_explicit_protocol_and_safe_endpoint() {
        let all = registrations();
        assert!(all.iter().all(|provider| provider
            .connections
            .iter()
            .all(|connection| !connection.protocols.is_empty() && connection.base_url.is_some())));
        assert_eq!(
            all.iter()
                .find(|provider| provider.id == "anthropic")
                .unwrap()
                .connections[0]
                .protocols,
            vec![ProtocolBinding {
                id: ANTHROPIC_MESSAGES.into(),
                endpoint: Some("https://api.anthropic.com/v1/messages".into()),
            }]
        );
        validate_registrations(&all).unwrap();
    }

    #[test]
    fn reject_duplicate_provider_and_unsafe_registered_endpoint() {
        let mut providers = registrations();
        providers.push(providers[0].clone());
        assert_eq!(
            validate_registrations(&providers),
            Err("duplicate or empty provider identity")
        );
        let mut providers = registrations();
        providers[0].connections[0].base_url = Some("http://external.test/v1".into());
        assert_eq!(
            validate_registrations(&providers),
            Err("unsafe provider endpoint")
        );
    }

    #[test]
    fn reject_ambiguous_or_inconsistent_model_discovery_sources() {
        let mut providers = registrations();
        let duplicate = providers[0].connections[0].model_sources[0].clone();
        providers[0].connections[0].model_sources.push(duplicate);
        assert_eq!(
            validate_registrations(&providers),
            Err("duplicate or empty model discovery source")
        );

        let mut providers = registrations();
        providers[0].connections[0].model_sources[0].id.clear();
        assert_eq!(
            validate_registrations(&providers),
            Err("duplicate or empty model discovery source")
        );

        // Dynamic discovery without an endpoint is valid when a plugin
        // supplies the catalog in-process instead of making an HTTP call.
        let mut providers = registrations();
        providers[0].connections[0].model_sources[0].endpoint = None;
        assert_eq!(validate_registrations(&providers), Ok(()));

        let mut providers = registrations();
        providers[0].connections[0].model_sources[0].kind = DiscoveryKind::Declared;
        assert_eq!(
            validate_registrations(&providers),
            Err("inconsistent model discovery source")
        );

        let mut providers = registrations();
        providers[0].connections[0].model_sources[0].endpoint =
            Some("http://external.test/models".into());
        assert_eq!(
            validate_registrations(&providers),
            Err("unsafe catalog endpoint")
        );
    }

    #[test]
    fn registration_rejects_ambiguous_connection_and_protocol_identities() {
        let mut providers = registrations();
        providers[0].connections[0].id.clear();
        assert_eq!(
            validate_registrations(&providers),
            Err("invalid provider connection")
        );

        let mut providers = registrations();
        providers[0].connections[0].protocols.push(ProtocolBinding {
            id: OPENAI_CHAT.into(),
            endpoint: None,
        });
        assert_eq!(
            validate_registrations(&providers),
            Err("duplicate or empty provider protocol")
        );

        let mut providers = registrations();
        providers[0].connections[0].protocols[0].id.clear();
        assert_eq!(
            validate_registrations(&providers),
            Err("duplicate or empty provider protocol")
        );
    }

    #[test]
    fn registration_rejects_cross_origin_model_discovery() {
        let mut providers = registrations();
        providers[0].connections[0].model_sources[0].endpoint =
            Some("https://unrelated.test/v1/models".into());
        assert_eq!(
            validate_registrations(&providers),
            Err("cross-origin model discovery")
        );

        let mut providers = registrations();
        providers[0].connections[0].model_sources[0].endpoint =
            Some("https://api.openai.com:8443/v1/models".into());
        assert_eq!(
            validate_registrations(&providers),
            Err("cross-origin model discovery")
        );

        // Same origin with a different path is allowed for model discovery.
        let mut providers = registrations();
        providers[0].connections[0].model_sources[0].endpoint =
            Some("https://api.openai.com/available-models".into());
        validate_registrations(&providers).unwrap();
    }

    #[test]
    fn protocol_overrides_must_keep_the_registered_credential_origin() {
        let mut providers = registrations();
        providers[0].connections[0].protocols[0].endpoint =
            Some("https://api.openai.com/v1/responses".into());
        validate_registrations(&providers).unwrap();

        providers[0].connections[0].protocols[0].endpoint =
            Some("https://outside.test/responses".into());
        assert_eq!(
            validate_registrations(&providers),
            Err("cross-origin protocol endpoint")
        );

        providers[0].connections[0].protocols[0].endpoint =
            Some("http://outside.test/responses".into());
        assert_eq!(
            validate_registrations(&providers),
            Err("unsafe protocol endpoint")
        );
    }

    #[test]
    fn plugin_defined_adapter_metadata_is_typed_and_validated() {
        let catalog = registrations();
        let openai = &catalog[0].connections[0];
        let key = &openai.auth_methods[0].adapter;
        assert_eq!(key.id, "api-key");
        let settings = key.config.decode::<ApiKeyOptions>().unwrap();
        assert_eq!(
            settings.environment_variable.as_deref(),
            Some("OPENAI_API_KEY")
        );
        assert!(settings.interactive);
        assert_eq!(settings.presentation, ApiKeyPresentation::Bearer);

        // Both connections reuse the exact same API-key strategy, even
        // though one executes Responses and another executes Messages.
        let anthropic = catalog
            .iter()
            .find(|provider| provider.id == "anthropic")
            .unwrap()
            .connections
            .iter()
            .find(|connection| connection.id == "api")
            .unwrap();
        let anthropic_key = anthropic.auth_methods[0]
            .adapter
            .config
            .decode::<ApiKeyOptions>()
            .unwrap();
        assert_eq!(anthropic.auth_methods[0].adapter.id, "api-key");
        assert_eq!(
            anthropic_key.presentation,
            ApiKeyPresentation::Header {
                name: "x-api-key".into(),
                prefix: String::new(),
            }
        );

        // Acquisition source and HTTP presentation are independent options;
        // configuring either first must produce the same method declaration.
        let header = ApiKeyPresentation::Header {
            name: "x-api-key".into(),
            prefix: String::new(),
        };
        for configured in [
            openai
                .clone()
                .with_key_presentation(header.clone())
                .unwrap()
                .with_key_environment("EXAMPLE_API_KEY")
                .unwrap(),
            openai
                .clone()
                .with_key_environment("EXAMPLE_API_KEY")
                .unwrap()
                .with_key_presentation(header.clone())
                .unwrap(),
        ] {
            let configured_key = configured.auth_methods[0]
                .adapter
                .config
                .decode::<ApiKeyOptions>()
                .unwrap();
            assert_eq!(configured_key.presentation, header);
            assert_eq!(
                configured_key.environment_variable.as_deref(),
                Some("EXAMPLE_API_KEY")
            );
        }

        let chatgpt = &catalog[0].connections[1];
        assert_eq!(chatgpt.auth_methods[0].adapter.id, "chatgpt.oauth");
        assert!(chatgpt.auth_methods[0]
            .adapter
            .config
            .decode::<()>()
            .is_ok());
        assert_ne!(
            openai.model_sources[0].adapter.id,
            chatgpt.model_sources[0].adapter.id
        );

        let mut invalid = catalog;
        invalid[0].connections[0].auth_methods[0]
            .adapter
            .config
            .schema_id = "not-a-schema".into();
        assert_eq!(
            validate_registrations(&invalid),
            Err("invalid authentication adapter")
        );

        let mut invalid = registrations();
        invalid[0].connections[0].model_sources[0].adapter.id = "../file".into();
        assert_eq!(
            validate_registrations(&invalid),
            Err("invalid model discovery adapter")
        );
    }

    #[test]
    fn plugin_contributions_can_describe_offline_and_local_models() {
        // Local or in-process providers do not need a fabricated remote URL.
        let mut in_process = connection(
            "native",
            "In-process model",
            "https://unused.example.test",
            &["native-model"],
            vec![],
            None,
        );
        in_process.base_url = None;
        let native = provider("native", "Native", vec![in_process.clone()]);
        assert_eq!(validate_registrations(&[native]), Ok(()));

        // Dynamic discovery can be implemented by the plugin without HTTP.
        in_process.model_sources[0].kind = DiscoveryKind::Dynamic;
        let native = provider("native", "Native", vec![in_process]);
        assert_eq!(validate_registrations(&[native]), Ok(()));

        let mut local = connection(
            "ollama",
            "Local HTTP model",
            "http://127.0.0.1:11434/v1",
            &[OPENAI_CHAT],
            vec![],
            Some("http://127.0.0.1:11434/api/tags"),
        );
        let installed = provider("local", "Local", vec![local.clone()]);
        assert_eq!(validate_registrations(&[installed]), Ok(()));

        // A local provider cannot use a DNS hostname as a shortcut to
        // credential-bearing loopback HTTP.
        local.base_url = Some("http://localhost:11434/v1".into());
        let installed = provider("local", "Local", vec![local.clone()]);
        assert_eq!(
            validate_registrations(&[installed]),
            Err("unsafe provider endpoint")
        );

        local.base_url = Some("http://127.0.0.1:11434/v1".into());
        local.model_sources[0].endpoint = Some("http://127.0.0.1:11435/api/tags".into());
        let installed = provider("local", "Local", vec![local]);
        assert_eq!(
            validate_registrations(&[installed]),
            Err("cross-origin model discovery")
        );
    }

    #[test]
    fn protocol_specific_urls_do_not_require_a_fake_base_url() {
        let mut local = connection(
            "direct",
            "Direct protocol endpoint",
            "https://unused.example.test",
            &[OPENAI_CHAT],
            vec![],
            None,
        );
        local.base_url = None;
        local.protocols[0].endpoint = Some("https://models.example.test/v1/chat".into());
        let installed = provider("direct", "Direct", vec![local.clone()]);
        validate_registrations(&[installed]).unwrap();

        local.model_sources[0].kind = DiscoveryKind::Dynamic;
        local.model_sources[0].endpoint = Some("https://foreign.example.test/models".into());
        let installed = provider("direct", "Direct", vec![local]);
        assert_eq!(
            validate_registrations(&[installed]),
            Err("cross-origin model discovery")
        );
    }

    #[test]
    fn registration_never_serializes_credentials() {
        let providers = registrations();
        let json = format!("{providers:?}");
        assert!(!json.contains("secret="));
        assert!(providers
            .iter()
            .all(
                |provider| provider.connections.iter().all(|connection| connection
                    .auth_methods
                    .iter()
                    .all(|method| !method.id.is_empty()))
            ));
    }

    #[test]
    fn documented_protocol_endpoints_are_explicit_and_origin_scoped() {
        let providers = registrations();
        validate_registrations(&providers).unwrap();

        let expected = [
            (
                "openai",
                "api",
                OPENAI_RESPONSES,
                "https://api.openai.com/v1/responses",
            ),
            (
                "openai",
                "api",
                OPENAI_CHAT,
                "https://api.openai.com/v1/chat/completions",
            ),
            (
                "openrouter",
                "api",
                OPENAI_CHAT,
                "https://openrouter.ai/api/v1/chat/completions",
            ),
            (
                "openrouter",
                "api",
                OPENAI_RESPONSES,
                "https://openrouter.ai/api/v1/responses",
            ),
            (
                "anthropic",
                "api",
                ANTHROPIC_MESSAGES,
                "https://api.anthropic.com/v1/messages",
            ),
            (
                "opencode-zen",
                "api",
                OPENAI_CHAT,
                "https://opencode.ai/zen/v1/chat/completions",
            ),
            (
                "opencode-zen",
                "api",
                OPENAI_RESPONSES,
                "https://opencode.ai/zen/v1/responses",
            ),
            (
                "opencode-go",
                "api",
                OPENAI_CHAT,
                "https://opencode.ai/zen/go/v1/chat/completions",
            ),
            (
                "opencode-go",
                "api",
                OPENAI_RESPONSES,
                "https://opencode.ai/zen/go/v1/responses",
            ),
            (
                "zai",
                "standard",
                OPENAI_CHAT,
                "https://api.z.ai/api/paas/v4/chat/completions",
            ),
        ];

        for (provider_id, connection_id, protocol_id, endpoint) in expected {
            let connection = providers
                .iter()
                .find(|provider| provider.id == provider_id)
                .unwrap()
                .connections
                .iter()
                .find(|connection| connection.id == connection_id)
                .unwrap();
            let configured = connection
                .protocols
                .iter()
                .find(|protocol| protocol.id == protocol_id)
                .unwrap();
            assert_eq!(configured.endpoint.as_deref(), Some(endpoint));
        }

        // A standard route is public metadata, not proof of account
        // authorization or an installed executable transport.
        let chatgpt = &providers[0].connections[1];
        assert_eq!(chatgpt.id, "chatgpt");
        assert_eq!(
            chatgpt.protocols[0].endpoint.as_deref(),
            Some("https://api.openai.com/v1/responses")
        );

        // Anthropic wire shape does not imply this gateway accepts x-api-key
        // instead of the gateway's account-bound bearer authentication.
        for provider_id in ["opencode-zen", "opencode-go"] {
            let connection = &providers
                .iter()
                .find(|provider| provider.id == provider_id)
                .unwrap()
                .connections[0];
            assert!(connection.protocols.iter().any(|protocol| {
                protocol.id == ANTHROPIC_MESSAGES && protocol.endpoint.is_none()
            }));
        }
    }
}
