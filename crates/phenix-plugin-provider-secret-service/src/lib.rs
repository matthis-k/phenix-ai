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

const ENV_BIND_PREFIX: &[u8] = b"phenix-credential-env-v1:";

fn valid_name(name: &str) -> bool {
    phenix_provider_contracts::ApiKeyOptions {
        environment_variable: Some(name.to_owned()),
        interactive: true,
        presentation: phenix_provider_contracts::ApiKeyPresentation::Bearer,
    }
    .validate()
    .is_ok()
}

fn encode_name(name: &str) -> Result<Vec<u8>, ProviderError> {
    if !valid_name(name) {
        return Err(error(
            ProviderErrorCode::InvalidInput,
            "invalid environment variable name",
        ));
    }
    let mut bytes = ENV_BIND_PREFIX.to_vec();
    bytes.extend_from_slice(name.as_bytes());
    Ok(bytes)
}

fn resolve_stored(
    bytes: Vec<u8>,
) -> Result<phenix_provider_contracts::CredentialSecret, ProviderError> {
    if let Some(encoded) = bytes.strip_prefix(ENV_BIND_PREFIX) {
        let name = std::str::from_utf8(encoded).map_err(|_| {
            error(
                ProviderErrorCode::InvalidInput,
                "invalid bound environment name",
            )
        })?;
        if !valid_name(name) {
            return Err(error(
                ProviderErrorCode::InvalidInput,
                "invalid bound environment name",
            ));
        }
        let bytes = std::env::var_os(name)
            .ok_or_else(|| {
                error(
                    ProviderErrorCode::NotFound,
                    "bound environment variable is unset",
                )
            })?
            .into_encoded_bytes();
        if bytes.is_empty() || bytes.len() > 4096 || !bytes.iter().all(u8::is_ascii_graphic) {
            return Err(error(
                ProviderErrorCode::InvalidInput,
                "invalid API key environment value",
            ));
        }
        phenix_provider_contracts::CredentialSecret::new(bytes)
    } else {
        phenix_provider_contracts::CredentialSecret::new(bytes)
    }
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
    use phenix_provider_contracts::{
        CredentialEnvironmentBinding, CredentialImport, CredentialRotation, CredentialSecret,
        ProviderCredentials,
    };
    use phenix_sdk::{CallError, CallResult, Host};

    #[derive(Default)]
    pub struct State;

    #[provide(ProviderCredentials::import)]
    fn import(host: &Host<'_>, request: CredentialImport) -> CallResult<(), ProviderError> {
        if request
            .secret
            .expose_for_authorized_provider()
            .starts_with(ENV_BIND_PREFIX)
        {
            return Err(CallError::Domain(error(
                ProviderErrorCode::InvalidInput,
                "reserved credential source marker",
            )));
        }
        entry(&request.identity, host.resource()?.as_deref())
            .map_err(CallError::Domain)?
            .set_secret(request.secret.expose_for_authorized_provider())
            .map_err(backend_error)
            .map_err(CallError::Domain)
    }

    #[provide(ProviderCredentials::bind_environment)]
    fn bind_environment(
        host: &Host<'_>,
        request: CredentialEnvironmentBinding,
    ) -> CallResult<(), ProviderError> {
        let target =
            entry(&request.identity, host.resource()?.as_deref()).map_err(CallError::Domain)?;
        let tagged = encode_name(&request.environment_variable).map_err(CallError::Domain)?;
        target
            .set_secret(&tagged)
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
        resolve_stored(bytes).map_err(CallError::Domain)
    }


    #[provide(ProviderCredentials::compare_exchange)]
    fn compare_exchange(
        host: &Host<'_>,
        request: CredentialRotation,
    ) -> CallResult<bool, ProviderError> {
        // The Secret Service backend does not provide an atomic CAS contract.
        // Never emulate token rotation with non-atomic read+set_secret.
        entry(&request.identity, host.resource()?.as_deref()).map_err(CallError::Domain)?;
        Err(CallError::Domain(error(
            ProviderErrorCode::Unavailable,
            "atomic credential rotation unsupported by this backend",
        )))
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
    fn secret_service_environment_binding_is_only_the_variable_name() {
        let encoded = encode_name("OPENAI_API_KEY").unwrap();
        assert!(encoded.starts_with(ENV_BIND_PREFIX));
        assert_eq!(&encoded[ENV_BIND_PREFIX.len()..], b"OPENAI_API_KEY");
        assert!(encode_name("BAD/NAME").is_err());
        assert_eq!(
            resolve_stored(b"plain-direct-test".to_vec())
                .unwrap()
                .expose_for_authorized_provider(),
            b"plain-direct-test"
        );
    }

    #[test]
    fn selectors_do_not_grant_service_access() {
        let identity = CredentialIdentity::new("openai", "api", "personal").unwrap();
        let resource = identity.resource().unwrap();
        assert!(authorize(&identity, Some(&resource)).is_ok());
        for actual in [None, Some("phenix.session.s1"), Some("phenix.providers.s1")] {
            assert_eq!(
                entry(&identity, actual).unwrap_err().code,
                ProviderErrorCode::Denied
            );
        }
        let other = CredentialIdentity::new("openai", "api", "work").unwrap();
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
