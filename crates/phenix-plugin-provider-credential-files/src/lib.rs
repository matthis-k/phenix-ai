//! Explicitly selected, opt-in headless credential fallback. This is not the
//! default credential backend or provider authentication/entitlement evidence.
//! The backend has no Session, Journal, Model or frontend dependencies.
//! Callers must use these operations only on the non-journaled management or
//! authorized transport path, never as model tools.
use phenix_provider_contracts::{CredentialIdentity, ProviderError, ProviderErrorCode};
use phenix_provider_credential_files::{CredentialError, CredentialFiles, CredentialId, Secret};

fn error(code: ProviderErrorCode, message: &str) -> ProviderError {
    ProviderError {
        code,
        message: message.into(),
    }
}

fn backend_error(problem: CredentialError) -> ProviderError {
    match problem {
        CredentialError::InvalidIdentity => {
            error(ProviderErrorCode::InvalidInput, "invalid credential input")
        }
        CredentialError::ConsentRequired => error(
            ProviderErrorCode::Denied,
            "credential file fallback requires explicit consent",
        ),
        CredentialError::InsecureDirectory | CredentialError::InsecureFile => error(
            ProviderErrorCode::Denied,
            "credential file permissions or ownership are unsafe",
        ),
        CredentialError::NotFound => {
            error(ProviderErrorCode::NotFound, "credential profile not found")
        }
        CredentialError::Io => error(
            ProviderErrorCode::Unavailable,
            "credential file operation failed",
        ),
    }
}

/// Name-only configuration: the value stays in the runtime environment,
/// never in the provider descriptor, Nix config, ACP or session journal.
/// This backend supports any provider/account admitted by the exact root.
/// Source order is explicit: a populated environment key wins over the
/// interactive file store, which is consulted only when the variable is unset.
fn environment_key_name() -> Result<Option<String>, ProviderError> {
    let Some(raw) = std::env::var_os("PHENIX_CREDENTIAL_API_KEY_ENV_NAME") else {
        return Ok(None);
    };
    let name = raw.into_string().map_err(|_| {
        error(
            ProviderErrorCode::InvalidInput,
            "invalid API-key environment name",
        )
    })?;
    if !valid_key_environment_name(&name) {
        return Err(error(
            ProviderErrorCode::InvalidInput,
            "invalid API-key environment name",
        ));
    }
    Ok(Some(name))
}

fn valid_key_environment_name(name: &str) -> bool {
    phenix_provider_contracts::ApiKeyOptions {
        environment_variable: Some(name.to_owned()),
        interactive: true,
        presentation: phenix_provider_contracts::ApiKeyPresentation::Bearer,
    }
    .validate()
    .is_ok()
}

/// ProviderCredentials::resolve only. A key read from an environment
/// variable must never be copied into persistent storage or journals.
/// CredentialSecret owns the resulting byte buffer and zeroes it on drop.
fn environment_key() -> Result<Option<phenix_provider_contracts::CredentialSecret>, ProviderError> {
    let Some(name) = environment_key_name()? else {
        return Ok(None);
    };
    let Some(raw) = std::env::var_os(&name) else {
        return Ok(None);
    };
    let bytes = raw.into_encoded_bytes();
    if bytes.is_empty() || bytes.len() > 4096 || !bytes.iter().all(u8::is_ascii_graphic) {
        return Err(error(
            ProviderErrorCode::InvalidInput,
            "invalid API-key environment value",
        ));
    }
    phenix_provider_contracts::CredentialSecret::new(bytes).map(Some)
}

fn authorize(
    identity: &CredentialIdentity,
    actual: Option<&str>,
) -> Result<CredentialId, ProviderError> {
    let resource = identity.resource()?;
    if actual != Some(resource.as_str()) {
        return Err(error(
            ProviderErrorCode::Denied,
            "credential resource not authorized",
        ));
    }
    Ok(CredentialId {
        provider: identity.provider_id().to_owned(),
        connection: identity.connection_id().to_owned(),
        profile: identity.profile_id().to_owned(),
    })
}

#[phenix_sdk::plugin(
    id = "phenix.provider.credential-files",
    version = "1.0.0",
    provide(phenix_provider_contracts::ProviderCredentials, slot = "Credentials")
)]
mod plugin {
    use super::*;
    use phenix_provider_contracts::{CredentialImport, CredentialSecret, ProviderCredentials};
    use phenix_sdk::{CallError, CallResult, Host};

    #[derive(Default)]
    pub struct State {
        backend: Option<CredentialFiles>,
    }

    impl State {
        fn backend(&mut self) -> Result<&CredentialFiles, ProviderError> {
            if self.backend.is_none() {
                // No implicit plaintext fallback, guessed path or directory creation.
                if std::env::var("PHENIX_CREDENTIAL_FILE_CONSENT").as_deref() != Ok("1") {
                    return Err(backend_error(CredentialError::ConsentRequired));
                }
                let directory = std::env::var_os("PHENIX_CREDENTIAL_FILE_DIRECTORY")
                    .map(std::path::PathBuf::from)
                    .filter(|path| path.is_absolute())
                    .ok_or_else(|| {
                        error(
                            ProviderErrorCode::Unavailable,
                            "absolute credential file directory is not configured",
                        )
                    })?;
                self.backend = Some(CredentialFiles::open(directory, true).map_err(backend_error)?);
            }
            // The backend pins its validated directory descriptor for this generation.
            Ok(self.backend.as_ref().unwrap())
        }
    }

    #[provide(ProviderCredentials::import)]
    fn import(
        state: &mut State,
        host: &Host<'_>,
        request: CredentialImport,
    ) -> CallResult<(), ProviderError> {
        // Check the authenticated call resource before opening/touching the backend.
        let id =
            authorize(&request.identity, host.resource()?.as_deref()).map_err(CallError::Domain)?;
        let secret = Secret::new(request.secret.expose_for_authorized_provider().to_vec())
            .map_err(backend_error)
            .map_err(CallError::Domain)?;
        state
            .backend()
            .map_err(CallError::Domain)?
            .store(&id, &secret)
            .map_err(backend_error)
            .map_err(CallError::Domain)
    }

    #[provide(ProviderCredentials::resolve)]
    fn resolve(
        state: &mut State,
        host: &Host<'_>,
        request: CredentialIdentity,
    ) -> CallResult<CredentialSecret, ProviderError> {
        let id = authorize(&request, host.resource()?.as_deref()).map_err(CallError::Domain)?;
        // Read the environment only after verifying the kernel's exact
        // credential root; an arbitrary ACP selector cannot obtain it.
        if let Some(secret) = environment_key().map_err(CallError::Domain)? {
            return Ok(secret);
        }
        let secret = state
            .backend()
            .map_err(CallError::Domain)?
            .read(&id)
            .map_err(backend_error)
            .map_err(CallError::Domain)?;
        CredentialSecret::new(secret.expose_for_authorized_provider().to_vec())
            .map_err(CallError::Domain)
    }

    #[provide(ProviderCredentials::revoke)]
    fn revoke(
        state: &mut State,
        host: &Host<'_>,
        request: CredentialIdentity,
    ) -> CallResult<(), ProviderError> {
        let id = authorize(&request, host.resource()?.as_deref()).map_err(CallError::Domain)?;
        // Revoking a file cannot invalidate a process environment variable.
        // Fail explicitly rather than falsely reporting an account revoked.
        if environment_key().map_err(CallError::Domain)?.is_some() {
            return Err(CallError::Domain(error(
                ProviderErrorCode::Denied,
                "API key supplied by environment; unset it and restart to revoke",
            )));
        }
        state
            .backend()
            .map_err(CallError::Domain)?
            .revoke(&id)
            .map_err(backend_error)
            .map_err(CallError::Domain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_sdk::ContractMetadata;

    #[test]
    fn environment_source_accepts_only_bounded_variable_names() {
        for allowed in ["OPENAI_API_KEY", "MY_PROVIDER_KEY", "_KEY", "A9"] {
            assert!(valid_key_environment_name(allowed), "{allowed}");
        }
        for invalid in [
            "",
            "api_key",
            "9KEY",
            "OPENAI-KEY",
            "../SECRET",
            "OPENAI_KEY=secret",
            "A B",
            "A\\nB",
        ] {
            assert!(!valid_key_environment_name(invalid), "{invalid:?}");
        }
        assert!(!valid_key_environment_name(&"A".repeat(129)));
    }

    #[test]
    fn profile_selectors_never_grant_credential_access() {
        let id = CredentialIdentity::new("openai", "api", "personal").unwrap();
        let authorized = id.resource().unwrap();
        assert!(authorize(&id, Some(&authorized)).is_ok());
        for resource in [None, Some("phenix.session.s1"), Some("phenix.providers.s1")] {
            assert_eq!(
                authorize(&id, resource).unwrap_err().code,
                ProviderErrorCode::Denied
            );
        }
        let other = CredentialIdentity::new("openai", "api", "work").unwrap();
        assert_eq!(
            authorize(&other, Some(&authorized)).unwrap_err().code,
            ProviderErrorCode::Denied
        );
    }

    #[test]
    fn errors_and_metadata_do_not_carry_secrets_or_journal_imports() {
        let metadata = ContractMetadata::decode(plugin::METADATA.bytes()).unwrap();
        assert!(metadata.requires.is_empty());
        assert_eq!(metadata.provides.len(), 1);
        for problem in [
            CredentialError::Io,
            CredentialError::InsecureFile,
            CredentialError::ConsentRequired,
        ] {
            let mapped = backend_error(problem);
            assert!(!mapped.message.contains("/"));
        }
    }
}
