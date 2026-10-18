//! Runtime-selected OS credential storage. No plaintext fallback, Session,
//! Journal, configuration serialization or authentication/entitlement claims.
use keyring_core::{api::CredentialStoreApi, Entry};
use phenix_provider_contracts::{CredentialIdentity, ProviderError, ProviderErrorCode};

fn error(code: ProviderErrorCode, message: &str) -> ProviderError {
    ProviderError {
        code,
        message: message.into(),
    }
}

fn backend_error(problem: keyring_core::Error) -> ProviderError {
    // Never forward platform errors: they may contain secret bytes or paths.
    let code = match problem {
        keyring_core::Error::NoEntry => ProviderErrorCode::NotFound,
        _ => ProviderErrorCode::Unavailable,
    };
    error(code, "OS credential service operation failed")
}

fn authorize(identity: &CredentialIdentity, actual: Option<&str>) -> Result<String, ProviderError> {
    let resource = identity.resource()?;
    if actual != Some(resource.as_str()) {
        return Err(error(
            ProviderErrorCode::Denied,
            "credential resource not authorized",
        ));
    }
    Ok(resource)
}

fn entry(identity: &CredentialIdentity, actual: Option<&str>) -> Result<Entry, ProviderError> {
    // Authenticate before contacting D-Bus, including entry construction.
    let resource = authorize(identity, actual)?;
    // Build on this specific store, never mutate the process-global default.
    zbus_secret_service_keyring_store::Store::new()
        .map_err(backend_error)?
        .build("phenix.provider.credentials.v1", &resource, None)
        .map_err(backend_error)
}

#[phenix_sdk::plugin(
    id = "phenix.provider.secret-service",
    version = "1.0.0",
    provide(phenix_provider_contracts::ProviderCredentials, slot = "Credentials")
)]
mod plugin {
    use super::*;
    use phenix_provider_contracts::{CredentialImport, CredentialSecret, ProviderCredentials};
    use phenix_sdk::{CallError, CallResult, Host};

    #[derive(Default)]
    pub struct State;

    #[provide(ProviderCredentials::import)]
    fn import(host: &Host<'_>, request: CredentialImport) -> CallResult<(), ProviderError> {
        entry(&request.identity, host.resource()?.as_deref())
            .map_err(CallError::Domain)?
            .set_secret(request.secret.expose_for_authorized_provider())
            .map_err(backend_error)
            .map_err(CallError::Domain)
    }

    #[provide(ProviderCredentials::resolve)]
    fn resolve(
        host: &Host<'_>,
        request: CredentialIdentity,
    ) -> CallResult<CredentialSecret, ProviderError> {
        let bytes = entry(&request, host.resource()?.as_deref())
            .map_err(CallError::Domain)?
            .get_secret()
            .map_err(backend_error)
            .map_err(CallError::Domain)?;
        CredentialSecret::new(bytes).map_err(CallError::Domain)
    }

    #[provide(ProviderCredentials::revoke)]
    fn revoke(host: &Host<'_>, request: CredentialIdentity) -> CallResult<(), ProviderError> {
        entry(&request, host.resource()?.as_deref())
            .map_err(CallError::Domain)?
            .delete_credential()
            .map_err(backend_error)
            .map_err(CallError::Domain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_sdk::ContractMetadata;

    #[test]
    fn selectors_do_not_grant_service_access() {
        let identity = CredentialIdentity {
            provider_id: "openai".into(),
            connection_id: "api".into(),
            profile_id: "personal".into(),
        };
        let resource = identity.resource().unwrap();
        assert!(authorize(&identity, Some(&resource)).is_ok());
        for actual in [None, Some("phenix.session.s1"), Some("phenix.providers.s1")] {
            assert_eq!(
                entry(&identity, actual).unwrap_err().code,
                ProviderErrorCode::Denied
            );
        }
        let other = CredentialIdentity {
            profile_id: "work".into(),
            ..identity
        };
        assert_eq!(
            entry(&other, Some(&resource)).unwrap_err().code,
            ProviderErrorCode::Denied
        );
    }

    #[test]
    fn no_journal_or_file_fallback_and_errors_are_redacted() {
        let metadata = ContractMetadata::decode(plugin::METADATA.bytes()).unwrap();
        assert!(metadata.requires.is_empty());
        assert_eq!(metadata.provides.len(), 1);
        let problem = keyring_core::Error::BadEncoding(b"fixture-secret".to_vec());
        let mapped = backend_error(problem);
        assert_eq!(mapped.code, ProviderErrorCode::Unavailable);
        assert!(!format!("{mapped:?}").contains("fixture-secret"));
        assert_eq!(
            backend_error(keyring_core::Error::NoEntry).code,
            ProviderErrorCode::NotFound
        );
    }
}
