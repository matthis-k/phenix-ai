//! Pure admission validation for provider-owned model selections.
//! Runtime grants and model execution remain outside this policy.

use super::*;

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
