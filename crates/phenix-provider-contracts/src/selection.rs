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
) -> Result<ProviderModel, ProviderError> {
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
    let connection = provider
        .connections
        .iter()
        .find(|connection| connection.id == requested.connection_id)
        .ok_or_else(|| {
            selection_error(
                ProviderErrorCode::NotFound,
                "connection not registered for selected provider",
            )
        })?;
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
    if !selected.selectable
        || selected.endpoint_id.is_empty()
        || !connection.protocols.contains(&selected.protocol_id)
    {
        return Err(selection_error(
            ProviderErrorCode::Denied,
            "model is not selectable through the installed protocol",
        ));
    }
    Ok(selected.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (ProviderDescriptor, AuthenticationStatus, ModelPage, ModelSelection) {
        let provider = ProviderDescriptor {
            id: "openai".into(),
            label: "OpenAI".into(),
            connections: vec![ConnectionDescriptor {
                id: "api".into(),
                label: "API".into(),
                protocols: vec!["openai-responses".into()],
                base_url: Some("https://api.openai.com/v1".into()),
                auth_methods: vec![AuthenticationMethod {
                    id: "api-key".into(),
                    label: "API key".into(),
                    kind: AuthenticationKind::ApiKey,
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
                    valid: true,
                    reason: None,
                },
                CredentialProfile {
                    id: "work".into(),
                    label: "Work".into(),
                    method_id: "api-key".into(),
                    valid: true,
                    reason: None,
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
        assert_eq!(admitted, page.models[0]);
        assert_eq!(ModelSelection::from_value(request.to_value()).unwrap(), request);
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
        auth.profiles[0].valid = false;
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
}
