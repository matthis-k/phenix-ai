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

const ENV_BIND_PREFIX: &[u8] = b"phenix-credential-env-v1:";

/// Tagged values are source metadata, not API-key bytes. Direct key import
/// rejects this reserved marker, so a raw key can never accidentally resolve
/// a different environment variable.
fn encoded_env_binding(name: &str) -> Result<Vec<u8>, ProviderError> {
    if !valid_key_environment_name(name) {
        return Err(error(
            ProviderErrorCode::InvalidInput,
            "invalid API-key variable name",
        ));
    }
    let mut bytes = ENV_BIND_PREFIX.to_vec();
    bytes.extend_from_slice(name.as_bytes());
    Ok(bytes)
}

fn decoded_env_binding(bytes: &[u8]) -> Result<Option<&str>, ProviderError> {
    let Some(raw) = bytes.strip_prefix(ENV_BIND_PREFIX) else {
        return Ok(None);
    };
    let name = std::str::from_utf8(raw).map_err(|_| {
        error(
            ProviderErrorCode::InvalidInput,
            "invalid bound variable name",
        )
    })?;
    if !valid_key_environment_name(name) {
        return Err(error(
            ProviderErrorCode::InvalidInput,
            "invalid bound variable name",
        ));
    }
    Ok(Some(name))
}

fn read_named_environment(
    name: &str,
) -> Result<phenix_provider_contracts::CredentialSecret, ProviderError> {
    let bytes = std::env::var_os(name)
        .ok_or_else(|| {
            error(
                ProviderErrorCode::NotFound,
                "configured API-key variable is unset",
            )
        })?
        .into_encoded_bytes();
    if bytes.is_empty() || bytes.len() > 4096 || !bytes.iter().all(u8::is_ascii_graphic) {
        return Err(error(
            ProviderErrorCode::InvalidInput,
            "invalid API-key environment value",
        ));
    }
    phenix_provider_contracts::CredentialSecret::new(bytes)
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
    use phenix_provider_contracts::{
        CredentialEnvironmentBinding, CredentialImport, CredentialRotation, CredentialSecret,
        ProviderCredentials,
    };
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

    #[provide(ProviderCredentials::bind_environment)]
    fn bind_environment(
        state: &mut State,
        host: &Host<'_>,
        request: CredentialEnvironmentBinding,
    ) -> CallResult<(), ProviderError> {
        let id =
            authorize(&request.identity, host.resource()?.as_deref()).map_err(CallError::Domain)?;
        let bytes =
            encoded_env_binding(&request.environment_variable).map_err(CallError::Domain)?;
        let record = Secret::new(bytes)
            .map_err(backend_error)
            .map_err(CallError::Domain)?;
        // Persist ONLY the name. The environment value is resolved lazily on
        // every authorized provider operation and is never copied to the vault.
        state
            .backend()
            .map_err(CallError::Domain)?
            .store(&id, &record)
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
        // Preserve the opt-in launcher environment-only path without opening
        // a credential-file vault or requiring file-storage consent. An
        // explicit account binding is stored in that vault and therefore
        // requires its independent, explicit consent.
        if std::env::var("PHENIX_CREDENTIAL_FILE_CONSENT").as_deref() != Ok("1") {
            return environment_key()
                .map_err(CallError::Domain)?
                .ok_or_else(|| CallError::Domain(backend_error(CredentialError::ConsentRequired)));
        }
        // An explicitly bound exact-account source wins over a process-wide
        // legacy launcher hint. Never persist the resolved environment value.
        let stored = state.backend().map_err(CallError::Domain)?.read(&id);
        match stored {
            Ok(secret) => {
                let bytes = secret.expose_for_authorized_provider();
                if let Some(name) = decoded_env_binding(bytes).map_err(CallError::Domain)? {
                    read_named_environment(name).map_err(CallError::Domain)
                } else {
                    CredentialSecret::new(bytes.to_vec()).map_err(CallError::Domain)
                }
            }
            Err(CredentialError::NotFound) => environment_key()
                .map_err(CallError::Domain)?
                .ok_or_else(|| CallError::Domain(backend_error(CredentialError::NotFound))),
            Err(problem) => Err(CallError::Domain(backend_error(problem))),
        }
    }


    #[provide(ProviderCredentials::compare_exchange)]
    fn compare_exchange(
        state: &mut State,
        host: &Host<'_>,
        request: CredentialRotation,
    ) -> CallResult<bool, ProviderError> {
        let id =
            authorize(&request.identity, host.resource()?.as_deref()).map_err(CallError::Domain)?;
        // Account-local OAuth rotation is never a variable-name binding and
        // must not resolve through inherited process environment fallbacks.
        for credential in [&request.expected, &request.replacement] {
            if credential
                .expose_for_authorized_provider()
                .starts_with(ENV_BIND_PREFIX)
            {
                return Err(CallError::Domain(error(
                    ProviderErrorCode::InvalidInput,
                    "rotation does not accept environment name markers",
                )));
            }
        }
        if std::env::var("PHENIX_CREDENTIAL_FILE_CONSENT").as_deref() != Ok("1") {
            return Err(CallError::Domain(backend_error(
                CredentialError::ConsentRequired,
            )));
        }
        let old = Secret::new(request.expected.expose_for_authorized_provider().to_vec())
            .map_err(backend_error)
            .map_err(CallError::Domain)?;
        let replacement =
            Secret::new(request.replacement.expose_for_authorized_provider().to_vec())
                .map_err(backend_error)
                .map_err(CallError::Domain)?;
        state
            .backend()
            .map_err(CallError::Domain)?
            .compare_and_store(&id, &old, &replacement)
            .map_err(backend_error)
            .map_err(CallError::Domain)
    }

    #[provide(ProviderCredentials::revoke)]
    fn revoke(
        state: &mut State,
        host: &Host<'_>,
        request: CredentialIdentity,
    ) -> CallResult<(), ProviderError> {
        let id = authorize(&request, host.resource()?.as_deref()).map_err(CallError::Domain)?;
        let bound = state
            .backend()
            .map_err(CallError::Domain)?
            .read(&id)
            .ok()
            .and_then(|secret| {
                decoded_env_binding(secret.expose_for_authorized_provider())
                    .ok()
                    .flatten()
                    .map(str::to_owned)
            })
            .is_some();
        // Deleting an explicit variable-name binding revokes the reference.
        // A process-global inherited variable cannot be revoked by a file write.
        if !bound && environment_key().map_err(CallError::Domain)?.is_some() {
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
    fn exact_account_environment_source_is_name_only_and_disjoint_from_raw_keys() {
        let bound = encoded_env_binding("OPENAI_API_KEY").unwrap();
        assert_eq!(decoded_env_binding(&bound).unwrap(), Some("OPENAI_API_KEY"));
        assert_eq!(decoded_env_binding(b"sk-direct-key").unwrap(), None);
        assert!(encoded_env_binding("unsafe-name").is_err());
        assert!(!bound
            .windows(b"secret-api-key".len())
            .any(|part| part == b"secret-api-key"));
    }

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
