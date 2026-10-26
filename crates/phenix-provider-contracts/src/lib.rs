//! Public provider discovery contracts. No provider logic or kernel dependency.
//!
//! Providers implement these operations independently. Existing Phenix Model,
//! Session, Journal, Storage and authority contracts remain the execution path.
//! Discovery values contain no secrets. The separate ProviderCredentials
//! contract is reserved for authorized, non-journaled credential-provider calls;
//! it is never a Model tool or ordinary configuration serialization.
use phenix_sdk::*;
use serde::{Deserialize, Serialize};

pub mod adapter;
pub use adapter::{AdapterBinding, AdapterConfig, ApiKeyOptions, ApiKeyPresentation};

pub mod credentials;
pub use credentials::{
    CredentialEnvironmentBinding, CredentialIdentity, CredentialImport, CredentialRotation,
    CredentialSecret, ProviderCredentials,
};

pub mod model_session;
pub use model_session::{HttpModelAuthorization, HttpModelInstall, HttpModelSession};

pub mod selection;
pub use selection::{
    bind_http_model, revalidate_http_model, validate_model_selection, AdmittedModelChoice,
    HttpModelBinding, ModelSelection,
};

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum ProviderErrorCode {
    InvalidInput,
    Denied,
    NotFound,
    Unavailable,
    Conflict,
    AuthenticationRequired,
    // Discovery is optional for a provider, but a failed authenticated
    // catalog must carry a specific safe reason through the control plane.
    DiscoveryNetwork,
    DiscoveryTimeout,
    DiscoveryUnauthorized,
    DiscoveryRateLimited,
    DiscoveryServerError,
    DiscoveryInvalidResponse,
    DiscoveryIssuerUnavailable,
    DiscoveryRejected,
    DiscoveryTooLarge,
    // Local credential generation was revoked, but the remote refresh
    // session could not be confirmed revoked (network/issuer unavailable).
    RemoteRevocationUnconfirmed,
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
pub struct ProviderDetailsQuery {
    pub scope: String,
    pub provider_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ProviderPage {
    /// Multiple providers may be contributed by one external plugin.
    pub providers: Vec<ProviderDescriptor>,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum AuthenticationKind {
    ApiKey,
    OAuthAuthorizationCode,
    OAuthDeviceCode,
    /// A plugin-owned authentication method with a standard UI challenge.
    Plugin,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct AuthenticationMethod {
    pub id: String,
    pub label: String,
    pub kind: AuthenticationKind,
    /// Describes plugin-owned behavior, never an authority grant.
    pub adapter: AdapterBinding,
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
    /// Optional network URL. Declared catalogs and plugin-owned dynamic
    /// discovery need no URL; never infer a /models endpoint.
    pub endpoint: Option<String>,
    /// The discovery plugin's decoder or static-catalog implementation.
    pub adapter: AdapterBinding,
}

/// An inference protocol contributed by a provider plugin.
///
/// `id` is a provider-domain protocol identifier, not a kernel operation.
/// The endpoint overrides the connection's default for this protocol only.
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ProtocolBinding {
    pub id: String,
    pub endpoint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ConnectionDescriptor {
    pub id: String,
    pub label: String,
    /// Protocol-specific configuration, resolved by provider plugins.
    pub protocols: Vec<ProtocolBinding>,
    /// Optional network base URL. In-process models need none; individual
    /// protocol bindings may specify their own explicit endpoints.
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

/// A profile is either authenticated or carries the provider's reason
/// why it cannot be used. Successful profiles cannot contain an error reason.
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum CredentialProfileStatus {
    Authenticated,
    Unavailable { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct CredentialProfile {
    /// Stable identity scoped to one provider + connection.
    pub id: String,
    pub label: String,
    /// References a declared AuthenticationMethod ID.
    pub method_id: String,
    /// Provider-authoritative status, not credential-presence evidence.
    pub status: CredentialProfileStatus,
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
        matches!(profile.status, CredentialProfileStatus::Authenticated)
            && matches.next().is_none()
            && {
                let mut methods = connection
                    .auth_methods
                    .iter()
                    .filter(|method| method.id == profile.method_id);
                methods.next().is_some() && methods.next().is_none()
            }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct AuthStart {
    pub scope: String,
    pub provider_id: String,
    pub connection_id: String,
    pub method_id: String,
}

/// An authentication challenge with its required fields in the variant.
/// There is no representable combination of a browser challenge without a
/// browser URL, a device challenge without a user code, or an API-key
/// challenge with an unrelated URL.
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum AuthChallenge {
    ApiKeyInput {
        attempt_id: String,
        prompt: String,
    },
    OpenBrowser {
        attempt_id: String,
        url: String,
    },
    DeviceAuthorization {
        attempt_id: String,
        verification_url: String,
        user_code: String,
    },
}

/// An attempt is scoped to one provider, connection and account flow.
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct AuthAttemptQuery {
    pub scope: String,
    pub provider_id: String,
    pub connection_id: String,
    pub attempt_id: String,
}

/// An authenticated account selected by a completed login flow. The
/// success payload has no validity flag or error reason, unlike a catalog
/// profile whose current validity can subsequently change.
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct AuthenticatedProfile {
    pub id: String,
    pub label: String,
    pub method_id: String,
}

impl AuthenticatedProfile {
    pub fn into_credential_profile(self) -> CredentialProfile {
        CredentialProfile {
            id: self.id,
            label: self.label,
            method_id: self.method_id,
            status: CredentialProfileStatus::Authenticated,
        }
    }
}

/// Polling produces an exhaustive lifecycle state; registration and
/// credential presence are not substitutes for successful authentication.
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum AuthAttemptState {
    AwaitingInput { challenge: AuthChallenge },
    Pending,
    Completed { profile: AuthenticatedProfile },
    Failed { reason: ProviderError },
    Expired,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ModelQuery {
    pub scope: String,
    pub provider_id: String,
    pub connection_id: String,
    /// A specific named account, required where the connection needs auth.
    pub profile_id: Option<String>,
    pub cursor: Option<String>,
    pub limit: u64,
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

/// Immutable registration data contributed by a selected plugin at startup.
/// This contract does not authenticate an account, make models available,
/// select an adapter, or grant authority to the registration's provider IDs.
/// A manager must validate and merge all selected contributions before use.
#[contract(id = "phenix.provider.registrations@1")]
pub trait ProviderRegistrations {
    fn list(&self) -> Result<ProviderPage, ProviderError>;
}

/// A provider-facing authentication operation. Interactive flows return a
/// challenge/attempt state; a successful profile is never inferred from a
/// bool, a stored token, or the existence of a registration.
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum AuthenticationCommand {
    Status { query: AuthQuery },
    Begin { request: AuthStart },
    Poll { request: AuthAttemptQuery },
    Cancel { request: AuthAttemptQuery },
}

/// The only outcomes exposed by the unified authentication port.
/// An awaiting challenge is not an authenticated profile.
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum AuthenticationEvent {
    Status { value: AuthenticationStatus },
    Challenge { value: AuthChallenge },
    Attempt { value: AuthAttemptState },
}

/// Canonical, account-scoped inference request, not a transport configuration.
/// The provider resolves the permitted protocol, endpoint, credentials,
/// session-local conversation state and token limits from its private state
/// and verified bindings, never from untrusted model output.
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ProviderCompletion {
    pub scope: String,
    pub provider_id: String,
    pub connection_id: String,
    pub profile_id: Option<String>,
    pub model_id: String,
    pub request: phenix_basic_contracts::ModelRequest,
}

/// The single provider execution-facing interface. Protocols, authorization
/// schemes, streaming, conversational IDs and transport retries are internal
/// strategies; they do not alter this client-facing operation set.
///
/// Providers and managers keep authoritative private Rust state. Published
/// provider metadata grants neither a credential nor permission to invoke a
/// model. Existing ProviderAuth/ProviderCatalog stay as transition adapters.
#[contract(id = "phenix.provider.runtime@1")]
pub trait ProviderRuntime {
    fn authenticate(
        &self,
        command: AuthenticationCommand,
    ) -> Result<AuthenticationEvent, ProviderError>;
    fn models(&self, query: ModelQuery) -> Result<ModelPage, ProviderError>;
    fn complete(
        &self,
        request: ProviderCompletion,
    ) -> Result<phenix_basic_contracts::ModelReply, ProviderError>;
}

#[contract(id = "phenix.provider.catalog@1")]
pub trait ProviderCatalog {
    fn list(&self, request: ProviderQuery) -> Result<ProviderPage, ProviderError>;
    fn describe(&self, request: ProviderDetailsQuery) -> Result<ProviderDescriptor, ProviderError>;
    fn models(&self, request: ModelQuery) -> Result<ModelPage, ProviderError>;
}

#[contract(id = "phenix.provider.auth@1")]
pub trait ProviderAuth {
    fn status(&self, request: AuthQuery) -> Result<AuthenticationStatus, ProviderError>;
    fn begin(&self, request: AuthStart) -> Result<AuthChallenge, ProviderError>;
    fn poll(&self, request: AuthAttemptQuery) -> Result<AuthAttemptState, ProviderError>;
    fn cancel(&self, request: AuthAttemptQuery) -> Result<AuthAttemptState, ProviderError>;
}

#[plugin(
    id = "phenix.provider-contracts",
    version = "1.0.0",
    profile = "definitions",
    define(ProviderCatalog),
    define(ProviderRegistrations),
    define(ProviderAuth),
    define(ProviderRuntime),
    define(ProviderCredentials),
    define(HttpModelSession)
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
            protocols: vec![
                ProtocolBinding {
                    id: "openai-chat".into(),
                    endpoint: None,
                },
                ProtocolBinding {
                    id: "openai-responses".into(),
                    endpoint: Some("https://example.test/v1/responses".into()),
                },
            ],
            base_url: Some("https://example.test/v1".into()),
            auth_methods: methods,
            model_sources: vec![DiscoverySource {
                id: "api-models".into(),
                kind: DiscoveryKind::Dynamic,
                endpoint: Some("https://example.test/v1/models".into()),
                adapter: AdapterBinding::from_typed("openai.models", &()).unwrap(),
            }],
        }
    }

    fn method(id: &str, kind: AuthenticationKind) -> AuthenticationMethod {
        AuthenticationMethod {
            id: id.into(),
            label: id.into(),
            kind,
            adapter: AdapterBinding::from_typed("test.auth", &()).unwrap(),
        }
    }

    fn profile(id: &str, method_id: &str, valid: bool) -> CredentialProfile {
        CredentialProfile {
            id: id.into(),
            label: id.into(),
            method_id: method_id.into(),
            status: if valid {
                CredentialProfileStatus::Authenticated
            } else {
                CredentialProfileStatus::Unavailable {
                    reason: "invalid".into(),
                }
            },
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
        state.profiles[1].status = CredentialProfileStatus::Unavailable {
            reason: "revoked".into(),
        };
        assert!(!state.profile_is_valid_for("example", &connection, "personal"));
        assert!(state.profile_is_valid_for("example", &connection, "work"));
        assert!(state.ready_for("example", &connection));
        state.profiles[2].status = CredentialProfileStatus::Unavailable {
            reason: "revoked".into(),
        };
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
        let multi = ProviderPage {
            providers: vec![provider.clone()],
            revision: 2,
        };
        assert_eq!(ProviderPage::from_value(multi.to_value()).unwrap(), multi);
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
    fn unified_provider_port_is_typed_and_has_no_implicit_authentication() {
        let command = AuthenticationCommand::Begin {
            request: AuthStart {
                scope: "user".into(),
                provider_id: "openai".into(),
                connection_id: "chatgpt".into(),
                method_id: "chatgpt-sign-in".into(),
            },
        };
        let encoded = command.to_value();
        assert!(AuthenticationCommand::SCHEMA
            .owned()
            .unwrap()
            .accepts(&encoded));
        assert_eq!(AuthenticationCommand::from_value(encoded).unwrap(), command);

        let waiting = AuthenticationEvent::Challenge {
            value: AuthChallenge::OpenBrowser {
                attempt_id: "pending-1".into(),
                url: "https://example.test/login".into(),
            },
        };
        assert_eq!(
            AuthenticationEvent::from_value(waiting.to_value()).unwrap(),
            waiting
        );
        let req = ProviderCompletion {
            scope: "user".into(),
            provider_id: "openai".into(),
            connection_id: "api".into(),
            profile_id: Some("personal".into()),
            model_id: "example-model".into(),
            request: phenix_basic_contracts::ModelRequest {
                session: "session-1".into(),
                run: "run-1".into(),
                history: vec![],
                tools: vec![],
                skills: vec![],
                context: vec![],
                limits: phenix_basic_contracts::Limits::default(),
            },
        };
        assert_eq!(ProviderCompletion::from_value(req.to_value()).unwrap(), req);
    }

    #[test]
    fn definition_package_has_no_executable_handler() {
        let metadata = ContractMetadata::decode(definitions::METADATA.bytes()).unwrap();
        assert_eq!(metadata.execution, MetadataExecution::None);
        assert_eq!(metadata.definitions.len(), 6);
    }

    #[test]
    fn authentication_completion_cannot_contain_an_unavailable_profile() {
        let successful = AuthAttemptState::Completed {
            profile: AuthenticatedProfile {
                id: "personal".into(),
                label: "Personal".into(),
                method_id: "oauth".into(),
            },
        };
        let wire = successful.to_value();
        assert!(AuthAttemptState::SCHEMA.owned().unwrap().accepts(&wire));
        assert_eq!(AuthAttemptState::from_value(wire).unwrap(), successful);
        let AuthAttemptState::Completed { profile } = successful else {
            unreachable!("fixture is completed");
        };
        assert_eq!(
            profile.into_credential_profile().status,
            CredentialProfileStatus::Authenticated,
        );
    }

    #[test]
    fn credential_profile_status_is_exclusive_and_roundtrips() {
        for status in [
            CredentialProfileStatus::Authenticated,
            CredentialProfileStatus::Unavailable {
                reason: "expired".into(),
            },
        ] {
            let encoded = status.to_value();
            assert!(CredentialProfileStatus::SCHEMA
                .owned()
                .unwrap()
                .accepts(&encoded));
            assert_eq!(
                CredentialProfileStatus::from_value(encoded).unwrap(),
                status
            );
        }
        let conflicting = Value::map([
            ("Authenticated", Value::Null),
            (
                "Unavailable",
                Value::map([("reason", Value::String("expired".into()))]),
            ),
        ]);
        assert!(CredentialProfileStatus::from_value(conflicting).is_err());
    }

    #[test]
    fn challenge_variants_require_their_own_fields() {
        for challenge in [
            AuthChallenge::ApiKeyInput {
                attempt_id: "attempt-1".into(),
                prompt: "Enter an API key".into(),
            },
            AuthChallenge::OpenBrowser {
                attempt_id: "attempt-2".into(),
                url: "https://example.test/authorize".into(),
            },
            AuthChallenge::DeviceAuthorization {
                attempt_id: "attempt-3".into(),
                verification_url: "https://example.test/device".into(),
                user_code: "ABCD".into(),
            },
        ] {
            let value = challenge.to_value();
            assert!(AuthChallenge::SCHEMA.owned().unwrap().accepts(&value));
            assert_eq!(AuthChallenge::from_value(value).unwrap(), challenge);
        }
        let missing_url = Value::map([(
            "OpenBrowser",
            Value::map([("attempt_id", Value::String("attempt-2".into()))]),
        )]);
        assert!(!AuthChallenge::SCHEMA.owned().unwrap().accepts(&missing_url));
        assert!(AuthChallenge::from_value(missing_url).is_err());
    }
}
