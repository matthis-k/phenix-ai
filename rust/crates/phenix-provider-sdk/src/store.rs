use crate::{ApiTokenSource, Auth, AuthDescriptor, AuthKind, ProviderError, Secret, Token};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

const CREDENTIAL_FILE_ENV: &str = "PHENIX_PROVIDER_CREDENTIAL_FILE";

#[derive(Clone, Debug)]
pub struct CredentialStore {
    path: PathBuf,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredCredentials {
    providers: BTreeMap<String, ProviderCredentials>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProviderCredentials {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    api_token: Option<ApiTokenSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    oauth: Option<OAuthCredential>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OAuthCredential {
    access_token: Token,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    refresh_token: Option<Secret>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expires_at: Option<u64>,
}

impl ProviderCredentials {
    fn add(&mut self, provider: &str, auth: Auth) -> Result<AuthDescriptor, ProviderError> {
        let descriptor = auth.descriptor();
        match auth {
            Auth::ApiToken { source } => {
                if self.api_token.is_some() {
                    return Err(duplicate_credential(provider, AuthKind::ApiToken));
                }
                self.api_token = Some(source);
            }
            Auth::OAuth {
                access_token,
                refresh_token,
                expires_at,
            } => {
                if self.oauth.is_some() {
                    return Err(duplicate_credential(provider, AuthKind::OAuth));
                }
                self.oauth = Some(OAuthCredential {
                    access_token,
                    refresh_token,
                    expires_at,
                });
            }
        }
        Ok(descriptor)
    }

    fn list(&self) -> Vec<AuthDescriptor> {
        let mut credentials = Vec::with_capacity(2);
        if self.api_token.is_some() {
            credentials.push(AuthDescriptor {
                kind: AuthKind::ApiToken,
                expires_at: None,
            });
        }
        if let Some(oauth) = &self.oauth {
            credentials.push(AuthDescriptor {
                kind: AuthKind::OAuth,
                expires_at: oauth.expires_at,
            });
        }
        credentials
    }

    fn remove(&mut self, kind: AuthKind) -> Option<AuthDescriptor> {
        match kind {
            AuthKind::ApiToken => self.api_token.take().map(|_| AuthDescriptor {
                kind,
                expires_at: None,
            }),
            AuthKind::OAuth => self.oauth.take().map(|oauth| AuthDescriptor {
                kind,
                expires_at: oauth.expires_at,
            }),
        }
    }

    fn resolve(&self, kind: AuthKind) -> Option<Auth> {
        match kind {
            AuthKind::ApiToken => self
                .api_token
                .clone()
                .map(|source| Auth::ApiToken { source }),
            AuthKind::OAuth => self.oauth.as_ref().map(|oauth| Auth::OAuth {
                access_token: oauth.access_token.clone(),
                refresh_token: oauth.refresh_token.clone(),
                expires_at: oauth.expires_at,
            }),
        }
    }

    fn is_empty(&self) -> bool {
        self.api_token.is_none() && self.oauth.is_none()
    }
}

impl CredentialStore {
    pub fn discover() -> Result<Self, CredentialStoreError> {
        if let Some(path) = std::env::var_os(CREDENTIAL_FILE_ENV) {
            return Ok(Self { path: path.into() });
        }
        let state = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
            })
            .ok_or(CredentialStoreError::MissingStateDirectory)?;
        Ok(Self {
            path: state.join("phenix/provider-credentials.json"),
        })
    }

    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn add(&self, provider: &str, auth: Auth) -> Result<AuthDescriptor, ProviderError> {
        let mut stored = self.read().map_err(store_error)?;
        let descriptor = stored
            .providers
            .entry(provider.to_owned())
            .or_default()
            .add(provider, auth)?;
        self.write(&stored).map_err(store_error)?;
        Ok(descriptor)
    }

    pub fn list(&self, provider: &str) -> Result<Vec<AuthDescriptor>, ProviderError> {
        Ok(self
            .read()
            .map_err(store_error)?
            .providers
            .get(provider)
            .map(ProviderCredentials::list)
            .unwrap_or_default())
    }

    pub fn remove(
        &self,
        provider: &str,
        kind: AuthKind,
    ) -> Result<Option<AuthDescriptor>, ProviderError> {
        let mut stored = self.read().map_err(store_error)?;
        let Some(credentials) = stored.providers.get_mut(provider) else {
            return Ok(None);
        };
        let removed = credentials.remove(kind);
        if removed.is_none() {
            return Ok(None);
        }
        if credentials.is_empty() {
            stored.providers.remove(provider);
        }
        self.write(&stored).map_err(store_error)?;
        Ok(removed)
    }

    pub(crate) fn resolve(
        &self,
        provider: &str,
        kind: AuthKind,
    ) -> Result<Option<Auth>, ProviderError> {
        Ok(self
            .read()
            .map_err(store_error)?
            .providers
            .get(provider)
            .and_then(|credentials| credentials.resolve(kind)))
    }

    fn read(&self) -> Result<StoredCredentials, CredentialStoreError> {
        match fs::read_to_string(&self.path) {
            Ok(source) => {
                serde_json::from_str(&source).map_err(|error| CredentialStoreError::Parse {
                    path: self.path.clone(),
                    message: error.to_string(),
                })
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Ok(StoredCredentials::default())
            }
            Err(error) => Err(CredentialStoreError::Io {
                path: self.path.clone(),
                message: error.to_string(),
            }),
        }
    }

    fn write(&self, credentials: &StoredCredentials) -> Result<(), CredentialStoreError> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| CredentialStoreError::NoParent(self.path.clone()))?;
        let parent_existed = parent.exists();
        fs::create_dir_all(parent).map_err(|error| CredentialStoreError::Io {
            path: parent.to_owned(),
            message: error.to_string(),
        })?;
        if !parent_existed {
            secure_directory(parent)?;
        }
        let temporary = self.path.with_extension("json.new");
        let source = serde_json::to_vec_pretty(credentials).map_err(|error| {
            CredentialStoreError::Parse {
                path: self.path.clone(),
                message: error.to_string(),
            }
        })?;
        let mut options = OpenOptions::new();
        options.create(true).truncate(true).write(true);
        secure_file_options(&mut options);
        let mut file = options
            .open(&temporary)
            .map_err(|error| CredentialStoreError::Io {
                path: temporary.clone(),
                message: error.to_string(),
            })?;
        secure_file(&temporary)?;
        file.write_all(&source)
            .and_then(|()| file.write_all(b"\n"))
            .and_then(|()| file.sync_all())
            .map_err(|error| CredentialStoreError::Io {
                path: temporary.clone(),
                message: error.to_string(),
            })?;
        fs::rename(&temporary, &self.path).map_err(|error| CredentialStoreError::Io {
            path: self.path.clone(),
            message: error.to_string(),
        })
    }
}

fn duplicate_credential(provider: &str, kind: AuthKind) -> ProviderError {
    ProviderError::Authentication {
        message: format!("provider {provider} already has a {kind:?} credential"),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CredentialStoreError {
    MissingStateDirectory,
    NoParent(PathBuf),
    Io { path: PathBuf, message: String },
    Parse { path: PathBuf, message: String },
}

impl Display for CredentialStoreError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingStateDirectory => write!(
                f,
                "set {CREDENTIAL_FILE_ENV}, XDG_STATE_HOME, or HOME for provider credentials"
            ),
            Self::NoParent(path) => {
                write!(
                    f,
                    "provider credential path {} has no parent",
                    path.display()
                )
            }
            Self::Io { path, message } => {
                write!(
                    f,
                    "provider credential I/O at {}: {message}",
                    path.display()
                )
            }
            Self::Parse { path, message } => {
                write!(
                    f,
                    "provider credential parse at {}: {message}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for CredentialStoreError {}

fn store_error(error: CredentialStoreError) -> ProviderError {
    ProviderError::Protocol {
        message: error.to_string(),
    }
}

#[cfg(unix)]
fn secure_directory(path: &Path) -> Result<(), CredentialStoreError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
        CredentialStoreError::Io {
            path: path.to_owned(),
            message: error.to_string(),
        }
    })
}

#[cfg(not(unix))]
fn secure_directory(_path: &Path) -> Result<(), CredentialStoreError> {
    Ok(())
}

#[cfg(unix)]
fn secure_file_options(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn secure_file_options(_options: &mut OpenOptions) {}

#[cfg(unix)]
fn secure_file(path: &Path) -> Result<(), CredentialStoreError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
        CredentialStoreError::Io {
            path: path.to_owned(),
            message: error.to_string(),
        }
    })
}

#[cfg(not(unix))]
fn secure_file(_path: &Path) -> Result<(), CredentialStoreError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TokenParseError;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_store(name: &str) -> CredentialStore {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        CredentialStore::at(std::env::temp_dir().join(format!(
            "phenix-provider-{name}-{}-{nonce}/credentials.json",
            std::process::id()
        )))
    }

    #[test]
    fn api_token_and_oauth_have_one_typed_slot_each() {
        let store = temp_store("credentials");
        store
            .add(
                "provider.test",
                Auth::ApiToken {
                    source: ApiTokenSource::Literal {
                        token: Token::parse("secret-token").unwrap(),
                    },
                },
            )
            .unwrap();
        store
            .add(
                "provider.test",
                Auth::OAuth {
                    access_token: Token::parse("oauth-access").unwrap(),
                    refresh_token: Some(Secret::parse("oauth-refresh").unwrap()),
                    expires_at: Some(42),
                },
            )
            .unwrap();
        assert!(store
            .add(
                "provider.test",
                Auth::ApiToken {
                    source: ApiTokenSource::Literal {
                        token: Token::parse("other-token").unwrap()
                    }
                }
            )
            .is_err());
        assert_eq!(
            store.list("provider.test").unwrap(),
            vec![
                AuthDescriptor {
                    kind: AuthKind::ApiToken,
                    expires_at: None,
                },
                AuthDescriptor {
                    kind: AuthKind::OAuth,
                    expires_at: Some(42),
                },
            ]
        );
        assert!(format!(
            "{:?}",
            store.resolve("provider.test", AuthKind::OAuth).unwrap()
        )
        .contains("<redacted>"));
        assert_eq!(
            store.remove("provider.test", AuthKind::ApiToken).unwrap(),
            Some(AuthDescriptor {
                kind: AuthKind::ApiToken,
                expires_at: None
            })
        );
        assert_eq!(
            store.remove("provider.test", AuthKind::OAuth).unwrap(),
            Some(AuthDescriptor {
                kind: AuthKind::OAuth,
                expires_at: Some(42)
            })
        );
        assert!(store.list("provider.test").unwrap().is_empty());
    }

    #[test]
    fn persisted_shape_cannot_express_duplicate_auth_kinds() {
        let store = temp_store("parsed-shape");
        let parent = store.path.parent().unwrap();
        fs::create_dir_all(parent).unwrap();
        fs::write(
            &store.path,
            r#"{"providers":{"provider.test":{"api_token":{"type":"literal","token":"token"},"oauth":{"access_token":"oauth"}}}}"#,
        )
        .unwrap();
        assert_eq!(store.list("provider.test").unwrap().len(), 2);
    }

    #[test]
    fn environment_api_token_persists_only_the_variable_name() {
        let store = temp_store("environment");
        store
            .add(
                "provider.test",
                Auth::ApiToken {
                    source: ApiTokenSource::Environment {
                        variable: crate::EnvironmentVariable::parse("PHENIX_TEST_API_KEY").unwrap(),
                    },
                },
            )
            .unwrap();
        let source = fs::read_to_string(&store.path).unwrap();
        assert!(source.contains("PHENIX_TEST_API_KEY"));
        assert!(!source.contains("secret-token"));
    }

    #[test]
    fn token_type_rejects_newline_before_store() {
        assert_eq!(
            Token::parse("bad\nvalue").unwrap_err(),
            TokenParseError::InvalidHeaderValue
        );
    }
}
