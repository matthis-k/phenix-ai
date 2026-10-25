//! Shared, provider-neutral authentication admission and strategy selection.
//! Never interpret a provider/account selector as a credential permission.
use phenix_provider_contracts::{
    ApiKeyOptions, ApiKeyPresentation, AuthenticationKind, ConnectionDescriptor,
    CredentialIdentity, ProviderError, ProviderErrorCode,
};

fn denied() -> ProviderError {
    ProviderError {
        code: ProviderErrorCode::Denied,
        message: "provider account resource not authorized".into(),
    }
}

/// A catalog-management scope is read-only metadata; it cannot authenticate.
/// The exact credential root is the only entry to a named account flow.
pub fn account(
    resource: Option<&str>,
    scope: &str,
    provider: &str,
    connection: &str,
) -> Result<Option<CredentialIdentity>, ProviderError> {
    if scope.is_empty()
        || scope.len() > 128
        || !scope.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-' || byte == b'_'
        })
    {
        return Err(ProviderError {
            code: ProviderErrorCode::InvalidInput,
            message: "invalid provider management scope".into(),
        });
    }
    let Some(resource) = resource else {
        return Err(denied());
    };
    if let Some(management) = resource.strip_prefix("phenix.providers.") {
        return if management == scope {
            Ok(None)
        } else {
            Err(denied())
        };
    }
    if scope != "user" {
        return Err(denied());
    }
    let identity = CredentialIdentity::from_resource(resource).map_err(|_| denied())?;
    if identity.provider_id() != provider || identity.connection_id() != connection {
        return Err(denied());
    }
    Ok(Some(identity))
}

/// Only an explicitly declared *Bearer* key adapter may use the shared bearer
/// verification strategy. A custom header, OAuth or unknown adapter is not
/// silently coerced into a bearer token.
pub fn standard_bearer_key(connection: &ConnectionDescriptor, method_id: &str) -> bool {
    connection.auth_methods.iter().any(|method| {
        method.id == method_id
            && method.kind == AuthenticationKind::ApiKey
            && method.adapter.id == "api-key"
            && method
                .adapter
                .config
                .decode::<ApiKeyOptions>()
                .is_ok_and(|options| {
                    options.validate().is_ok() && options.presentation == ApiKeyPresentation::Bearer
                })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_provider_builtins::registrations;

    #[test]
    fn account_selection_cannot_cross_provider_connection_or_profile_grant() {
        let identity = CredentialIdentity::new("openai", "api", "personal").unwrap();
        let resource = identity.resource().unwrap();
        assert_eq!(
            account(Some(&resource), "user", "openai", "api")
                .unwrap()
                .unwrap(),
            identity
        );
        assert_eq!(
            account(Some("phenix.providers.user"), "user", "openai", "api").unwrap(),
            None
        );
        for (scope, provider, connection) in [
            ("user", "openai", "chatgpt"),
            ("user", "anthropic", "api"),
            ("other", "openai", "api"),
        ] {
            assert_eq!(
                account(Some(&resource), scope, provider, connection)
                    .unwrap_err()
                    .code,
                ProviderErrorCode::Denied
            );
        }
        assert_eq!(
            account(Some("phenix.providers.admin"), "user", "openai", "api")
                .unwrap_err()
                .code,
            ProviderErrorCode::Denied
        );
        assert_eq!(
            account(None, "user", "openai", "api").unwrap_err().code,
            ProviderErrorCode::Denied
        );
        for scope in ["", "../user", "too much / scope"] {
            assert_eq!(
                account(Some(&resource), scope, "openai", "api")
                    .unwrap_err()
                    .code,
                ProviderErrorCode::InvalidInput
            );
        }
    }

    #[test]
    fn only_advertised_bearer_api_key_is_selected() {
        let all = registrations();
        assert!(standard_bearer_key(&all[0].connections[0], "api-key"));
        assert!(!standard_bearer_key(
            &all[0].connections[0],
            "chatgpt-sign-in"
        ));
        assert!(!standard_bearer_key(
            &all[0].connections[1],
            "chatgpt-sign-in"
        ));
        assert!(!standard_bearer_key(&all[2].connections[0], "api-key"));
        assert!(!standard_bearer_key(&all[0].connections[0], "other"));
    }
}
