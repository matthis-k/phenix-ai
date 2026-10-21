//! Pure admission validation for provider-owned model selections.
//! Runtime grants and model execution remain outside this policy.

use super::*;

/// Non-secret identity that a caller asks the authoritative runtime to commit.
/// The runtime must validate its own scope/authority and fresh entitlement;
/// this value by itself never grants Model execution or credential access.
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ModelSelection {
    pub provider_id: String,
    pub connection_id: String,
    pub profile_id: Option<String>,
    pub model_id: String,
    pub protocol_id: String,
    pub endpoint_id: String,
    pub catalog_revision: u64,
}

/// A checked model choice, tied to the exact provider/connection,
/// account and catalog revision that passed provider-domain admission.
/// This is NOT an authority token. A trusted composition still selects
/// the executing native plugin and authorizes the requested operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedModelChoice {
    requested: ModelSelection,
    connection: ConnectionDescriptor,
    model: ProviderModel,
    protocol: ProtocolBinding,
    credential_identity: Option<CredentialIdentity>,
    credential_profile: Option<CredentialProfile>,
}

impl AdmittedModelChoice {
    pub fn selection(&self) -> &ModelSelection {
        &self.requested
    }

    pub fn model(&self) -> &ProviderModel {
        &self.model
    }

    pub fn protocol(&self) -> &ProtocolBinding {
        &self.protocol
    }

    /// The exact non-secret profile identity bound during admission.
    /// None means the selected connection explicitly requires no authentication.
    /// Possession of this value is not a grant to resolve a secret.
    pub fn credential_identity(&self) -> Option<&CredentialIdentity> {
        self.credential_identity.as_ref()
    }
}

/// A complete, explicitly configured HTTP destination for one admitted model.
///
/// This is non-secret execution metadata, **not** a credential or kernel grant.
/// The transport must independently check current profile entitlement, native
/// provider binding, selected credential root, and request authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpModelBinding {
    choice: AdmittedModelChoice,
    endpoint: phenix_provider_protocols::ValidatedEndpoint,
}

impl HttpModelBinding {
    pub fn endpoint(&self) -> &str {
        &self.endpoint.url
    }

    pub fn model_id(&self) -> &str {
        &self.choice.selection().model_id
    }

    pub fn protocol_id(&self) -> &str {
        &self.choice.protocol().id
    }

    pub fn credential_identity(&self) -> Option<&CredentialIdentity> {
        self.choice.credential_identity()
    }

    /// Selected, validated authentication method category. The method's
    /// presence is not a credential-resolution grant, and a generic HTTP
    /// adapter must reject methods it does not explicitly implement.
    pub fn authentication_kind(&self) -> Option<&AuthenticationKind> {
        let profile = self.choice.credential_profile.as_ref()?;
        self.choice
            .connection
            .auth_methods
            .iter()
            .find(|method| method.id == profile.method_id)
            .map(|method| &method.kind)
    }

    pub fn selection(&self) -> &ModelSelection {
        self.choice.selection()
    }
}

/// Resolve only the explicitly registered endpoint for the selected protocol.
/// An inference path must not guess `/chat/completions`, `/responses` or a
/// connection's protocol from an endpoint URL or a model name.
///
/// Recheck the *current* registration even if the admission choice was checked
/// against an earlier descriptor; that choice cannot outlive registry changes
/// silently. Runtime authorization and credential freshness remain mandatory.
pub fn bind_http_model(
    provider: &ProviderDescriptor,
    admitted: AdmittedModelChoice,
) -> Result<HttpModelBinding, ProviderError> {
    if provider.id != admitted.selection().provider_id {
        return Err(selection_error(
            ProviderErrorCode::Denied,
            "selected provider changed",
        ));
    }
    let mut matches = provider
        .connections
        .iter()
        .filter(|connection| connection.id == admitted.selection().connection_id);
    let connection = matches.next().ok_or_else(|| {
        selection_error(
            ProviderErrorCode::NotFound,
            "selected connection is no longer installed",
        )
    })?;
    if matches.next().is_some() {
        return Err(selection_error(
            ProviderErrorCode::Conflict,
            "ambiguous selected connection",
        ));
    }
    // Admission is pinned to the full installed connection registration.
    // Matching IDs and the selected protocol alone cannot authorize a stale
    // account method or a changed endpoint registration.
    if connection != &admitted.connection {
        return Err(selection_error(
            ProviderErrorCode::Conflict,
            "selected connection registration changed",
        ));
    }
    let mut protocols = connection
        .protocols
        .iter()
        .filter(|protocol| protocol.id == admitted.protocol().id);
    let protocol = protocols.next().ok_or_else(|| {
        selection_error(
            ProviderErrorCode::NotFound,
            "selected protocol is no longer installed",
        )
    })?;
    if protocols.next().is_some() || protocol != admitted.protocol() {
        return Err(selection_error(
            ProviderErrorCode::Conflict,
            "selected protocol registration changed",
        ));
    }
    let url = protocol.endpoint.as_deref().ok_or_else(|| {
        selection_error(
            ProviderErrorCode::Unavailable,
            "selected protocol has no explicit inference endpoint",
        )
    })?;
    let checked = phenix_provider_protocols::validate_endpoint(
        url,
        phenix_provider_protocols::EndpointMode::LocalLoopback,
    )
    .map_err(|_| selection_error(ProviderErrorCode::InvalidInput, "unsafe inference endpoint"))?;
    if let Some(base) = connection.base_url.as_deref() {
        let authorized_origin = phenix_provider_protocols::validate_endpoint(
            base,
            phenix_provider_protocols::EndpointMode::LocalLoopback,
        )
        .map_err(|_| selection_error(ProviderErrorCode::InvalidInput, "unsafe provider origin"))?;
        if !phenix_provider_protocols::same_credential_origin(&authorized_origin, &checked) {
            return Err(selection_error(
                ProviderErrorCode::Denied,
                "inference endpoint changed credential origin",
            ));
        }
    }
    Ok(HttpModelBinding {
        choice: admitted,
        endpoint: checked,
    })
}

/// Revalidate an admitted HTTP choice against the *current* provider, account
/// status and model catalog before configuring a transport. This checks the
/// selected profile's authentication method as well as the catalog's full
/// model metadata, even if a buggy discovery source reuses a revision.
///
/// The caller must obtain this evidence from authorized runtime providers
/// and independently authorize the exact credential root and Model execution.
/// This remains non-secret metadata, not a grant or proof of live entitlement.
pub fn revalidate_http_model(
    provider: &ProviderDescriptor,
    authentication: &AuthenticationStatus,
    page: &ModelPage,
    admitted: AdmittedModelChoice,
    runtime_catalog_is_fresh: bool,
) -> Result<HttpModelBinding, ProviderError> {
    let current = validate_model_selection(
        provider,
        authentication,
        page,
        admitted.selection(),
        runtime_catalog_is_fresh,
    )?;
    if current != admitted {
        return Err(selection_error(
            ProviderErrorCode::Conflict,
            "selected model, connection or authentication registration changed",
        ));
    }
    bind_http_model(provider, current)
}

fn selection_error(code: ProviderErrorCode, message: &str) -> ProviderError {
    ProviderError {
        code,
        message: message.into(),
    }
}

/// Verify a concrete model choice against the *runtime-provided* provider,
/// authentication status and catalog page. Never accept a frontend-supplied
/// provider descriptor, profile validity or freshness flag as evidence.
///
/// Callers must separately check scope, grant, endpoint origin and the current
/// provider entitlement before admission, and persist the resulting selection
/// with revision CAS. This helper is only the pure identity/admission policy.
pub fn validate_model_selection(
    provider: &ProviderDescriptor,
    authentication: &AuthenticationStatus,
    page: &ModelPage,
    requested: &ModelSelection,
    runtime_catalog_is_fresh: bool,
) -> Result<AdmittedModelChoice, ProviderError> {
    if !runtime_catalog_is_fresh || page.revision == 0 {
        return Err(selection_error(
            ProviderErrorCode::Unavailable,
            "current model catalog evidence is required",
        ));
    }
    if requested.catalog_revision != page.revision {
        return Err(selection_error(
            ProviderErrorCode::Conflict,
            "model catalog revision changed",
        ));
    }
    if provider.id != requested.provider_id || provider.id.is_empty() {
        return Err(selection_error(
            ProviderErrorCode::Denied,
            "provider identity mismatch",
        ));
    }
    let mut connections = provider
        .connections
        .iter()
        .filter(|connection| connection.id == requested.connection_id);
    let connection = connections.next().ok_or_else(|| {
        selection_error(
            ProviderErrorCode::NotFound,
            "connection not registered for selected provider",
        )
    })?;
    if connections.next().is_some() {
        return Err(selection_error(
            ProviderErrorCode::Conflict,
            "ambiguous duplicate provider connection",
        ));
    }
    if authentication.provider_id != requested.provider_id
        || authentication.connection_id != requested.connection_id
        || page.profile_id != requested.profile_id
    {
        return Err(selection_error(
            ProviderErrorCode::Denied,
            "catalog or authentication belongs to another connection or account",
        ));
    }
    if connection.auth_methods.is_empty() {
        if requested.profile_id.is_some() {
            return Err(selection_error(
                ProviderErrorCode::Denied,
                "no-auth connection must not adopt an account profile",
            ));
        }
    } else if !requested.profile_id.as_deref().is_some_and(|profile| {
        authentication.profile_is_valid_for(&provider.id, connection, profile)
    }) {
        return Err(selection_error(
            ProviderErrorCode::AuthenticationRequired,
            "selected credential profile is not authenticated",
        ));
    }

    // The native credential contract has a stricter bounded identity than
    // the catalog's display IDs. Refuse choices that could not later resolve
    // their exact authorized profile without substituting another account.
    let credential_identity = requested
        .profile_id
        .as_deref()
        .map(|profile| {
            CredentialIdentity::new(&requested.provider_id, &requested.connection_id, profile)
        })
        .transpose()?;

    // A model ID may have several explicitly distinct protocol bindings, but
    // an ambiguous duplicate of the same complete binding is never admitted.
    let mut matches = page.models.iter().filter(|model| {
        model.id == requested.model_id
            && model.connection_id == requested.connection_id
            && model.protocol_id == requested.protocol_id
            && model.endpoint_id == requested.endpoint_id
    });
    let selected = matches.next().ok_or_else(|| {
        selection_error(
            ProviderErrorCode::NotFound,
            "requested model binding is not in current catalog",
        )
    })?;
    if matches.next().is_some() {
        return Err(selection_error(
            ProviderErrorCode::Conflict,
            "duplicate model binding in current catalog",
        ));
    }
    let mut protocols = connection
        .protocols
        .iter()
        .filter(|protocol| protocol.id == selected.protocol_id);
    let protocol = protocols.next().ok_or_else(|| {
        selection_error(
            ProviderErrorCode::Denied,
            "selected protocol is not installed for this connection",
        )
    })?;
    if protocols.next().is_some() {
        return Err(selection_error(
            ProviderErrorCode::Conflict,
            "ambiguous duplicate protocol binding",
        ));
    }
    if !selected.selectable || selected.endpoint_id.is_empty() {
        return Err(selection_error(
            ProviderErrorCode::Denied,
            "model is not selectable through the installed protocol",
        ));
    }
    let credential_profile = requested.profile_id.as_deref().and_then(|profile_id| {
        authentication
            .profiles
            .iter()
            .find(|profile| profile.id == profile_id)
            .cloned()
    });
    Ok(AdmittedModelChoice {
        requested: requested.clone(),
        connection: connection.clone(),
        model: selected.clone(),
        protocol: protocol.clone(),
        credential_identity,
        credential_profile,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (
        ProviderDescriptor,
        AuthenticationStatus,
        ModelPage,
        ModelSelection,
    ) {
        let provider = ProviderDescriptor {
            id: "openai".into(),
            label: "OpenAI".into(),
            connections: vec![ConnectionDescriptor {
                id: "api".into(),
                label: "API".into(),
                protocols: vec![ProtocolBinding {
                    id: "openai-responses".into(),
                    endpoint: None,
                }],
                base_url: Some("https://api.openai.com/v1".into()),
                auth_methods: vec![AuthenticationMethod {
                    id: "api-key".into(),
                    label: "API key".into(),
                    kind: AuthenticationKind::ApiKey,
                    adapter: AdapterBinding::from_typed(
                        "api-key",
                        &ApiKeyOptions {
                            environment_variable: None,
                            interactive: true,
                        },
                    )
                    .unwrap(),
                }],
                model_sources: vec![],
            }],
        };
        let auth = AuthenticationStatus {
            provider_id: "openai".into(),
            connection_id: "api".into(),
            profiles: vec![
                CredentialProfile {
                    id: "personal".into(),
                    label: "Personal".into(),
                    method_id: "api-key".into(),
                    status: CredentialProfileStatus::Authenticated,
                },
                CredentialProfile {
                    id: "work".into(),
                    label: "Work".into(),
                    method_id: "api-key".into(),
                    status: CredentialProfileStatus::Authenticated,
                },
            ],
        };
        let model = ProviderModel {
            id: "o-series".into(),
            label: "O series".into(),
            connection_id: "api".into(),
            protocol_id: "openai-responses".into(),
            endpoint_id: "responses".into(),
            capabilities: vec!["tools".into()],
            origin: ModelOrigin::Discovered,
            selectable: true,
        };
        let page = ModelPage {
            profile_id: Some("personal".into()),
            models: vec![model],
            next_cursor: None,
            revision: 7,
        };
        let request = ModelSelection {
            provider_id: "openai".into(),
            connection_id: "api".into(),
            profile_id: Some("personal".into()),
            model_id: "o-series".into(),
            protocol_id: "openai-responses".into(),
            endpoint_id: "responses".into(),
            catalog_revision: 7,
        };
        (provider, auth, page, request)
    }

    #[test]
    fn accepts_only_exact_selected_profile_and_admitted_binding() {
        let (provider, auth, page, request) = fixture();
        let admitted = validate_model_selection(&provider, &auth, &page, &request, true).unwrap();
        assert_eq!(admitted.model(), &page.models[0]);
        assert_eq!(admitted.selection(), &request);
        assert_eq!(admitted.protocol(), &provider.connections[0].protocols[0]);
        assert_eq!(
            ModelSelection::from_value(request.to_value()).unwrap(),
            request
        );
    }

    #[test]
    fn refuses_other_account_or_foreign_provider_even_with_same_model_id() {
        let (provider, auth, page, mut request) = fixture();
        request.profile_id = Some("work".into());
        assert!(validate_model_selection(&provider, &auth, &page, &request, true).is_err());
        request.profile_id = Some("personal".into());
        request.provider_id = "anthropic".into();
        assert!(validate_model_selection(&provider, &auth, &page, &request, true).is_err());
    }

    #[test]
    fn refuses_stale_or_changed_catalog_and_revoked_profile() {
        let (provider, mut auth, page, mut request) = fixture();
        assert!(validate_model_selection(&provider, &auth, &page, &request, false).is_err());
        request.catalog_revision = 6;
        assert!(validate_model_selection(&provider, &auth, &page, &request, true).is_err());
        request.catalog_revision = 7;
        auth.profiles[0].status = CredentialProfileStatus::Unavailable {
            reason: "revoked".into(),
        };
        assert!(validate_model_selection(&provider, &auth, &page, &request, true).is_err());
    }

    #[test]
    fn refuses_forged_protocol_endpoint_and_unselectable_or_ambiguous_catalog() {
        let (provider, auth, mut page, mut request) = fixture();
        request.protocol_id = "anthropic-messages".into();
        assert!(validate_model_selection(&provider, &auth, &page, &request, true).is_err());
        request.protocol_id = "openai-responses".into();
        request.endpoint_id = "unverified-endpoint".into();
        assert!(validate_model_selection(&provider, &auth, &page, &request, true).is_err());
        request.endpoint_id = "responses".into();
        page.models[0].selectable = false;
        assert!(validate_model_selection(&provider, &auth, &page, &request, true).is_err());
        page.models[0].selectable = true;
        page.models.push(page.models[0].clone());
        assert!(validate_model_selection(&provider, &auth, &page, &request, true).is_err());
    }

    #[test]
    fn ambiguous_connection_or_protocol_metadata_fails_closed() {
        let (mut provider, auth, page, request) = fixture();
        let duplicate = provider.connections[0].clone();
        provider.connections.push(duplicate);
        assert_eq!(
            validate_model_selection(&provider, &auth, &page, &request, true)
                .unwrap_err()
                .code,
            ProviderErrorCode::Conflict
        );
        provider.connections.pop();
        let duplicate_protocol = provider.connections[0].protocols[0].clone();
        provider.connections[0].protocols.push(duplicate_protocol);
        assert_eq!(
            validate_model_selection(&provider, &auth, &page, &request, true)
                .unwrap_err()
                .code,
            ProviderErrorCode::Conflict
        );
    }

    #[test]
    fn no_auth_connection_cannot_be_assigned_an_arbitrary_profile() {
        let (mut provider, mut auth, mut page, mut request) = fixture();
        provider.connections[0].auth_methods.clear();
        auth.profiles.clear();
        page.profile_id = None;
        request.profile_id = None;
        assert!(validate_model_selection(&provider, &auth, &page, &request, true).is_ok());
        request.profile_id = Some("personal".into());
        assert!(validate_model_selection(&provider, &auth, &page, &request, true).is_err());
    }
    #[test]
    fn admission_returns_coupled_protocol_and_account_instead_of_bare_metadata() {
        let (provider, auth, page, request) = fixture();
        let selected = validate_model_selection(&provider, &auth, &page, &request, true).unwrap();
        assert_eq!(selected.selection().profile_id.as_deref(), Some("personal"));
        assert_eq!(selected.model().protocol_id, selected.protocol().id);
        assert_eq!(selected.selection().catalog_revision, page.revision);
        // A caller can alter its own untrusted request, but not the checked
        // binding's privately held snapshot.
        let mut changed = request.clone();
        changed.profile_id = Some("work".into());
        assert_eq!(selected.selection().profile_id.as_deref(), Some("personal"));
        assert_ne!(selected.selection(), &changed);
    }

    #[test]
    fn admitted_choice_carries_exact_validated_credential_identity() {
        let (provider, auth, page, request) = fixture();
        let choice = validate_model_selection(&provider, &auth, &page, &request, true).unwrap();
        let identity = choice
            .credential_identity()
            .expect("authenticated connection");
        assert_eq!(identity.provider_id(), "openai");
        assert_eq!(identity.connection_id(), "api");
        assert_eq!(identity.profile_id(), "personal");
        assert_eq!(
            identity,
            &CredentialIdentity::new("openai", "api", "personal").unwrap()
        );

        let (mut provider, mut auth, mut page, mut request) = fixture();
        provider.connections[0].auth_methods.clear();
        auth.profiles.clear();
        page.profile_id = None;
        request.profile_id = None;
        let choice = validate_model_selection(&provider, &auth, &page, &request, true).unwrap();
        assert!(choice.credential_identity().is_none());
    }

    #[test]
    fn authenticated_selection_rejects_identity_unrepresentable_by_secret_store() {
        let (provider, mut auth, mut page, mut request) = fixture();
        auth.profiles[0].id = "work.team".into();
        page.profile_id = Some("work.team".into());
        request.profile_id = Some("work.team".into());
        assert_eq!(
            validate_model_selection(&provider, &auth, &page, &request, true)
                .unwrap_err()
                .code,
            ProviderErrorCode::InvalidInput
        );

        let (mut provider, mut auth, page, mut request) = fixture();
        provider.id = "org.provider".into();
        auth.provider_id = provider.id.clone();
        request.provider_id = provider.id.clone();
        assert_eq!(
            validate_model_selection(&provider, &auth, &page, &request, true)
                .unwrap_err()
                .code,
            ProviderErrorCode::InvalidInput
        );
    }

    #[test]
    fn http_binding_requires_explicit_selected_protocol_endpoint() {
        let (provider, auth, page, request) = fixture();
        let admitted = validate_model_selection(&provider, &auth, &page, &request, true).unwrap();
        assert_eq!(
            bind_http_model(&provider, admitted).unwrap_err().code,
            ProviderErrorCode::Unavailable
        );

        let (mut provider, auth, page, request) = fixture();
        provider.connections[0].protocols[0].endpoint =
            Some("https://api.openai.com/v1/responses".into());
        let admitted = validate_model_selection(&provider, &auth, &page, &request, true).unwrap();
        let bound = bind_http_model(&provider, admitted).unwrap();
        assert_eq!(bound.endpoint(), "https://api.openai.com/v1/responses");
        assert_eq!(bound.model_id(), "o-series");
        assert_eq!(bound.protocol_id(), "openai-responses");
        assert_eq!(
            bound.credential_identity().unwrap().profile_id(),
            "personal"
        );
    }

    #[test]
    fn http_binding_invalidates_admission_when_connection_auth_or_origin_changes() {
        let (mut provider, auth, page, request) = fixture();
        provider.connections[0].protocols[0].endpoint =
            Some("https://api.openai.com/v1/responses".into());
        let admitted = validate_model_selection(&provider, &auth, &page, &request, true).unwrap();

        // A former authenticated choice cannot outlive a connection that
        // stopped requiring authentication, even when its protocol is unchanged.
        provider.connections[0].auth_methods.clear();
        assert_eq!(
            bind_http_model(&provider, admitted).unwrap_err().code,
            ProviderErrorCode::Conflict
        );

        let (mut provider, auth, page, request) = fixture();
        provider.connections[0].protocols[0].endpoint =
            Some("https://api.openai.com/v1/responses".into());
        let admitted = validate_model_selection(&provider, &auth, &page, &request, true).unwrap();

        // An origin-equivalent base URL is still a different registration.
        // The consumer must revalidate against the current descriptor.
        provider.connections[0].base_url = Some("https://api.openai.com/v2".into());
        assert_eq!(
            bind_http_model(&provider, admitted).unwrap_err().code,
            ProviderErrorCode::Conflict
        );
    }

    #[test]
    fn http_binding_rejects_changed_registration_and_cross_origin_targets() {
        let (mut provider, auth, page, request) = fixture();
        provider.connections[0].protocols[0].endpoint =
            Some("https://api.openai.com/v1/responses".into());
        let admitted = validate_model_selection(&provider, &auth, &page, &request, true).unwrap();
        provider.connections[0].protocols[0].endpoint =
            Some("https://api.openai.com/v1/other".into());
        assert_eq!(
            bind_http_model(&provider, admitted).unwrap_err().code,
            ProviderErrorCode::Conflict
        );

        for endpoint in [
            "https://other-origin.example/v1/responses",
            "http://other-origin.example/v1/responses",
            "https://alice:secret@api.openai.com/v1/responses",
            "https://api.openai.com/v1/responses#fragment",
        ] {
            let (mut provider, auth, page, request) = fixture();
            provider.connections[0].protocols[0].endpoint = Some(endpoint.into());
            let admitted =
                validate_model_selection(&provider, &auth, &page, &request, true).unwrap();
            assert!(bind_http_model(&provider, admitted).is_err(), "{endpoint}");
        }
    }

    #[test]
    fn live_http_revalidation_rejects_revoked_profile_and_stale_catalog() {
        let (mut provider, mut auth, mut page, request) = fixture();
        provider.connections[0].protocols[0].endpoint =
            Some("https://api.openai.com/v1/responses".into());
        let admitted = validate_model_selection(&provider, &auth, &page, &request, true).unwrap();
        let binding =
            revalidate_http_model(&provider, &auth, &page, admitted.clone(), true).unwrap();
        assert_eq!(binding.model_id(), "o-series");
        assert_eq!(
            binding.credential_identity().unwrap().profile_id(),
            "personal"
        );

        auth.profiles[0].status = CredentialProfileStatus::Unavailable {
            reason: "revoked".into(),
        };
        assert_eq!(
            revalidate_http_model(&provider, &auth, &page, admitted.clone(), true)
                .unwrap_err()
                .code,
            ProviderErrorCode::AuthenticationRequired
        );
        auth.profiles[0].status = CredentialProfileStatus::Authenticated;
        assert_eq!(
            revalidate_http_model(&provider, &auth, &page, admitted.clone(), false)
                .unwrap_err()
                .code,
            ProviderErrorCode::Unavailable
        );
        page.revision += 1;
        assert_eq!(
            revalidate_http_model(&provider, &auth, &page, admitted, true)
                .unwrap_err()
                .code,
            ProviderErrorCode::Conflict
        );
    }

    #[test]
    fn live_http_revalidation_rejects_same_revision_catalog_and_auth_method_changes() {
        let (mut provider, mut auth, mut page, request) = fixture();
        provider.connections[0].protocols[0].endpoint =
            Some("https://api.openai.com/v1/responses".into());
        // A new method can be equally valid for the connection, but it must
        // not silently replace the selected account's admitted key method.
        provider.connections[0]
            .auth_methods
            .push(AuthenticationMethod {
                id: "other-key".into(),
                label: "Other API key".into(),
                kind: AuthenticationKind::ApiKey,
                adapter: AdapterBinding::from_typed(
                    "api-key",
                    &ApiKeyOptions {
                        environment_variable: None,
                        interactive: true,
                    },
                )
                .unwrap(),
            });
        let admitted = validate_model_selection(&provider, &auth, &page, &request, true).unwrap();
        auth.profiles[0].method_id = "other-key".into();
        assert_eq!(
            revalidate_http_model(&provider, &auth, &page, admitted.clone(), true)
                .unwrap_err()
                .code,
            ProviderErrorCode::Conflict
        );
        auth.profiles[0].method_id = "api-key".into();

        // A plugin that serves changed model metadata without bumping its
        // revision also cannot silently reuse an older admitted choice.
        page.models[0].capabilities.push("new-capability".into());
        assert_eq!(
            revalidate_http_model(&provider, &auth, &page, admitted, true)
                .unwrap_err()
                .code,
            ProviderErrorCode::Conflict
        );
    }

    #[test]
    fn selected_model_scopes_exact_secret_resolution_before_reading_bytes() {
        use phenix_sdk::Operation;
        let (mut provider, auth, page, request) = fixture();
        provider.connections[0].protocols[0].endpoint =
            Some("https://api.openai.com/v1/responses".into());
        let choice = validate_model_selection(&provider, &auth, &page, &request, true).unwrap();
        let bound = bind_http_model(&provider, choice).unwrap();
        let personal = CredentialIdentity::new("openai", "api", "personal").unwrap();
        let work = CredentialIdentity::new("openai", "api", "work").unwrap();
        let request_scope = bound.resolution_scope_for(&personal).unwrap();
        assert_eq!(
            request_scope.resource.as_deref(),
            Some(personal.resource().unwrap().as_str())
        );
        assert_eq!(
            request_scope.authority.operations,
            [<super::super::credentials::ProviderCredentials::resolve as Operation>::KEY
                .into()].into()
        );
        assert_eq!(
            request_scope.authority.resources,
            [personal.resource().unwrap()].into()
        );
        assert_eq!(
            bound.resolution_scope_for(&work).unwrap_err().code,
            ProviderErrorCode::Denied
        );

        let (mut local, mut no_auth, mut models, mut request) = fixture();
        local.connections[0].auth_methods.clear();
        local.connections[0].base_url = Some("http://127.0.0.1:11434/v1".into());
        local.connections[0].protocols[0].endpoint =
            Some("http://127.0.0.1:11434/v1/responses".into());
        no_auth.profiles.clear();
        models.profile_id = None;
        request.profile_id = None;
        let choice = validate_model_selection(&local, &no_auth, &models, &request, true).unwrap();
        let bound = bind_http_model(&local, choice).unwrap();
        assert_eq!(
            bound.resolution_scope_for(&personal).unwrap_err().code,
            ProviderErrorCode::Denied
        );
    }

    #[test]
    fn explicit_credential_free_local_protocol_still_has_no_profile() {
        let (mut provider, mut auth, mut page, mut request) = fixture();
        provider.connections[0].auth_methods.clear();
        provider.connections[0].base_url = Some("http://127.0.0.1:11434/v1".into());
        provider.connections[0].protocols[0].endpoint =
            Some("http://127.0.0.1:11434/v1/responses".into());
        auth.profiles.clear();
        page.profile_id = None;
        request.profile_id = None;
        let admitted = validate_model_selection(&provider, &auth, &page, &request, true).unwrap();
        let bound = bind_http_model(&provider, admitted).unwrap();
        assert!(bound.credential_identity().is_none());
        assert_eq!(bound.endpoint(), "http://127.0.0.1:11434/v1/responses");
    }
}
