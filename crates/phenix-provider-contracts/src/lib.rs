//! Public provider discovery contracts. No provider logic or kernel dependency.
//!
//! Providers implement these operations independently. Existing Phenix Model,
//! Session, Journal, Storage and authority contracts remain the execution path.
//! Secret values are intentionally absent: credential import and OAuth callback
//! settlement require a separate authorized, non-journaled runtime operation.
use phenix_sdk::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum ProviderErrorCode {
    InvalidInput,
    Denied,
    NotFound,
    Unavailable,
    Conflict,
    AuthenticationRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ProviderError {
    pub code: ProviderErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ProviderQuery {
    /// Authorized runtime, project or session selector, not an authority grant.
    pub scope: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum AuthenticationKind {
    ApiKey,
    OAuthAuthorizationCode,
    OAuthDeviceCode,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct AuthenticationMethod {
    pub id: String,
    pub label: String,
    pub kind: AuthenticationKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum DiscoveryKind {
    Dynamic,
    Declared,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct DiscoverySource {
    pub id: String,
    pub kind: DiscoveryKind,
    /// Empty for a declared static catalog. Never infer a /models URL.
    pub endpoint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ConnectionDescriptor {
    pub id: String,
    pub label: String,
    /// Explicit model protocol identifiers, not an inferred URL format.
    pub protocols: Vec<String>,
    /// Transport endpoint or base URL when applicable.
    pub base_url: Option<String>,
    /// An empty list means no authentication is required.
    pub auth_methods: Vec<AuthenticationMethod>,
    pub model_sources: Vec<DiscoverySource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ProviderDescriptor {
    pub id: String,
    pub label: String,
    pub connections: Vec<ConnectionDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct AuthQuery {
    pub scope: String,
    pub provider_id: String,
    pub connection_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct CredentialProfile {
    /// Stable identity scoped to one provider + connection.
    pub id: String,
    pub label: String,
    /// References a declared AuthenticationMethod ID.
    pub method_id: String,
    /// Provider-authoritative validity, not a frontend credential guess.
    pub valid: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct AuthenticationStatus {
    pub provider_id: String,
    pub connection_id: String,
    /// Multiple accounts can use the same authentication method.
    /// There are no credential values in this response.
    pub profiles: Vec<CredentialProfile>,
}

impl AuthenticationStatus {
    /// UI-only connection readiness; execution still requires a selected
    /// authorized profile, Model binding and current provider validation.
    pub fn ready_for(&self, provider_id: &str, connection: &ConnectionDescriptor) -> bool {
        if self.provider_id != provider_id || self.connection_id != connection.id {
            return false;
        }
        connection.auth_methods.is_empty()
            || self
                .profiles
                .iter()
                .any(|profile| self.profile_is_valid_for(provider_id, connection, &profile.id))
    }

    /// Never infer selected-account authority from aggregate provider readiness.
    /// Duplicate profile IDs make that identity invalid, without denying an
    /// unrelated valid account on the same connection.
    pub fn profile_is_valid_for(
        &self,
        provider_id: &str,
        connection: &ConnectionDescriptor,
        profile_id: &str,
    ) -> bool {
        if self.provider_id != provider_id
            || self.connection_id != connection.id
            || profile_id.is_empty()
        {
            return false;
        }
        let mut matches = self
            .profiles
            .iter()
            .filter(|profile| profile.id == profile_id);
        let Some(profile) = matches.next() else {
            return false;
        };
        profile.valid
            && matches.next().is_none()
            && connection
                .auth_methods
                .iter()
                .any(|method| method.id == profile.method_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct AuthStart {
    pub scope: String,
    pub provider_id: String,
    pub connection_id: String,
    pub method_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum ChallengeKind {
    ApiKeyInput,
    OpenBrowser,
    DeviceAuthorization,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct AuthChallenge {
    pub attempt_id: String,
    pub kind: ChallengeKind,
    /// Only the provider-approved authorization/verification URL.
    pub url: Option<String>,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ModelQuery {
    pub scope: String,
    pub provider_id: String,
    pub connection_id: String,
    /// A specific named account, required where the connection needs auth.
    pub profile_id: Option<String>,
    pub cursor: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum ModelOrigin {
    Discovered,
    Declared,
    Merged,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ProviderModel {
    pub id: String,
    pub label: String,
    pub connection_id: String,
    pub protocol_id: String,
    /// References a provider-validated endpoint. No arbitrary model-supplied URL.
    pub endpoint_id: String,
    pub capabilities: Vec<String>,
    pub origin: ModelOrigin,
    pub selectable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ModelPage {
    /// The profile that was used to discover this page, never another account.
    pub profile_id: Option<String>,
    pub models: Vec<ProviderModel>,
    pub next_cursor: Option<String>,
    pub revision: u64,
}

#[contract(id = "phenix.provider.catalog@1")]
pub trait ProviderCatalog {
    fn describe(&self, request: ProviderQuery) -> Result<ProviderDescriptor, ProviderError>;
    fn models(&self, request: ModelQuery) -> Result<ModelPage, ProviderError>;
}

#[contract(id = "phenix.provider.auth@1")]
pub trait ProviderAuth {
    fn status(&self, request: AuthQuery) -> Result<AuthenticationStatus, ProviderError>;
    fn begin(&self, request: AuthStart) -> Result<AuthChallenge, ProviderError>;
}

#[plugin(
    id = "phenix.provider-contracts",
    version = "1.0.0",
    profile = "definitions",
    define(ProviderCatalog),
    define(ProviderAuth)
)]
pub mod definitions {
    use super::*;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(methods: Vec<AuthenticationMethod>) -> ConnectionDescriptor {
        ConnectionDescriptor {
            id: "api".into(),
            label: "API".into(),
            protocols: vec!["openai-chat".into(), "openai-responses".into()],
            base_url: Some("https://example.test/v1".into()),
            auth_methods: methods,
            model_sources: vec![DiscoverySource {
                id: "api-models".into(),
                kind: DiscoveryKind::Dynamic,
                endpoint: Some("https://example.test/v1/models".into()),
            }],
        }
    }

    fn method(id: &str, kind: AuthenticationKind) -> AuthenticationMethod {
        AuthenticationMethod {
            id: id.into(),
            label: id.into(),
            kind,
        }
    }

    fn profile(id: &str, method_id: &str, valid: bool) -> CredentialProfile {
        CredentialProfile {
            id: id.into(),
            label: id.into(),
            method_id: method_id.into(),
            valid,
            reason: None,
        }
    }

    #[test]
    fn readiness_covers_zero_one_two_methods_and_named_accounts() {
        let mut state = AuthenticationStatus {
            provider_id: "example".into(),
            connection_id: "api".into(),
            profiles: vec![],
        };
        assert!(state.ready_for("example", &descriptor(vec![])));
        let mut other = descriptor(vec![]);
        other.id = "unrelated-connection".into();
        assert!(!state.ready_for("example", &other));

        let key = method("key", AuthenticationKind::ApiKey);
        let oauth = method("oauth", AuthenticationKind::OAuthAuthorizationCode);
        let connection = descriptor(vec![key.clone(), oauth.clone()]);
        assert!(!state.ready_for("example", &connection));
        state.profiles.push(profile("unused", "unknown", true));
        assert!(!state.ready_for("example", &connection));
        state.profiles.push(profile("personal", "key", true));
        assert!(state.profile_is_valid_for("example", &connection, "personal"));
        assert!(state.ready_for("example", &connection));
        state.profiles.push(profile("work", "key", true));
        state.profiles[1].valid = false;
        assert!(!state.profile_is_valid_for("example", &connection, "personal"));
        assert!(state.profile_is_valid_for("example", &connection, "work"));
        assert!(state.ready_for("example", &connection));
        state.profiles[2].valid = false;
        assert!(!state.ready_for("example", &descriptor(vec![key])));
        state.profiles.push(profile("subscription", "oauth", true));
        assert!(state.ready_for("example", &descriptor(vec![oauth])));
    }

    #[test]
    fn readiness_never_accepts_foreign_provider_or_ambiguous_profile_identity() {
        let connection = descriptor(vec![
            method("key", AuthenticationKind::ApiKey),
            method("oauth", AuthenticationKind::OAuthAuthorizationCode),
        ]);
        let mut state = AuthenticationStatus {
            provider_id: "openai".into(),
            connection_id: "api".into(),
            profiles: vec![profile("personal", "key", true)],
        };
        assert!(state.ready_for("openai", &connection));
        assert!(!state.ready_for("anthropic", &connection));
        assert!(!state.profile_is_valid_for("openai", &connection, "unknown"));

        // A duplicate record cannot authorize this profile even if both say valid.
        state.profiles.push(profile("personal", "key", false));
        assert!(!state.profile_is_valid_for("openai", &connection, "personal"));
        assert!(!state.ready_for("openai", &connection));

        // Another valid OAuth profile remains an independent authorized choice.
        state.profiles.push(profile("subscription", "oauth", true));
        assert!(state.ready_for("openai", &connection));
        assert!(state.profile_is_valid_for("openai", &connection, "subscription"));
    }

    #[test]
    fn discovery_metadata_and_provider_identity_roundtrip() {
        let provider = ProviderDescriptor {
            id: "openai".into(),
            label: "OpenAI".into(),
            connections: vec![descriptor(vec![
                method("api-key", AuthenticationKind::ApiKey),
                method("oauth", AuthenticationKind::OAuthAuthorizationCode),
            ])],
        };
        let portable = provider.to_value();
        let decoded = ProviderDescriptor::from_value(portable).unwrap();
        assert_eq!(decoded, provider);
        let catalog = ModelPage {
            profile_id: Some("personal".into()),
            models: vec![ProviderModel {
                id: "sample".into(),
                label: "Sample".into(),
                connection_id: "api".into(),
                protocol_id: "openai-responses".into(),
                endpoint_id: "responses".into(),
                capabilities: vec!["tools".into()],
                origin: ModelOrigin::Discovered,
                selectable: true,
            }],
            next_cursor: None,
            revision: 4,
        };
        assert_eq!(ModelPage::from_value(catalog.to_value()).unwrap(), catalog);
    }

    #[test]
    fn definition_package_has_no_executable_handler() {
        let metadata = ContractMetadata::decode(definitions::METADATA.bytes()).unwrap();
        assert_eq!(metadata.execution, MetadataExecution::None);
        assert_eq!(metadata.definitions.len(), 2);
    }
}
