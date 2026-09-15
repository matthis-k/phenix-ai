use crate::providers;
use genai::resolver::AuthData;
use genai::ModelIden;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const CREDENTIAL_FILE_ENV: &str = "PHENIX_CREDENTIAL_FILE";

#[derive(Clone, Debug)]
pub(crate) struct CredentialStore {
    pub(crate) path: PathBuf,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredCredentials {
    providers: BTreeMap<String, StoredCredential>,
}

#[derive(Clone, Debug)]
pub(crate) enum StoredCredential {
    ApiKey {
        secret: SecretString,
    },
    OAuth {
        access_token: SecretString,
        refresh_token: SecretString,
        id_token: SecretString,
        account_id: String,
        expires_at: u64,
    },
}

// The persisted credential file and the client auth wire both carry the secret
// as a plain string, so `Serialize`/`Deserialize` are implemented manually to
// expose the wrapped secrets at this single persistence/transport boundary.
// `secrecy` withholds automatic serialization from `SecretBox` to prevent
// accidental exfiltration; the credential store is the explicit, intentional
// implementation of that, so no `Serialize` in the runtime derives.
impl Serialize for StoredCredential {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        match self {
            Self::ApiKey { secret } => {
                let mut state = serializer.serialize_struct("ApiKey", 2)?;
                state.serialize_field("type", "api_key")?;
                state.serialize_field("secret", secret.expose_secret())?;
                state.end()
            }
            Self::OAuth {
                access_token,
                refresh_token,
                id_token,
                account_id,
                expires_at,
            } => {
                let mut state = serializer.serialize_struct("OAuth", 6)?;
                state.serialize_field("type", "o_auth")?;
                state.serialize_field("access_token", access_token.expose_secret())?;
                state.serialize_field("refresh_token", refresh_token.expose_secret())?;
                state.serialize_field("id_token", id_token.expose_secret())?;
                state.serialize_field("account_id", account_id)?;
                state.serialize_field("expires_at", expires_at)?;
                state.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for StoredCredential {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
        enum Untagged {
            ApiKey {
                secret: String,
            },
            OAuth {
                access_token: String,
                refresh_token: String,
                id_token: String,
                account_id: String,
                expires_at: u64,
            },
        }
        match Untagged::deserialize(deserializer)? {
            Untagged::ApiKey { secret } => Ok(Self::ApiKey {
                secret: SecretString::from(secret),
            }),
            Untagged::OAuth {
                access_token,
                refresh_token,
                id_token,
                account_id,
                expires_at,
            } => Ok(Self::OAuth {
                access_token: SecretString::from(access_token),
                refresh_token: SecretString::from(refresh_token),
                id_token: SecretString::from(id_token),
                account_id,
                expires_at,
            }),
        }
    }
}

impl CredentialStore {
    pub(crate) fn discover() -> Result<Self, String> {
        if let Some(path) = std::env::var_os(CREDENTIAL_FILE_ENV) {
            return Ok(Self { path: path.into() });
        }
        let state = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
            })
            .ok_or_else(|| {
                format!("set {CREDENTIAL_FILE_ENV}, XDG_STATE_HOME, or HOME for Phenix credentials")
            })?;
        Ok(Self {
            path: state.join("phenix/credentials.json"),
        })
    }

    pub(crate) fn resolve(&self, provider: &str) -> Result<Option<StoredCredential>, String> {
        let credentials = self.read()?;
        Ok(credentials.providers.get(provider).cloned())
    }

    pub(crate) fn save_oauth(
        &self,
        provider: &str,
        credential: StoredCredential,
    ) -> Result<(), String> {
        if !matches!(credential, StoredCredential::OAuth { .. }) {
            return Err("OAuth credential store received an API key".to_owned());
        }
        let mut credentials = self.read()?;
        credentials
            .providers
            .insert(provider.to_owned(), credential);
        self.write(&credentials)
    }

    pub(crate) fn save_api_key(&self, provider: &str, secret: &str) -> Result<(), String> {
        if secret.trim().is_empty() {
            return Err("API key must not be empty".to_owned());
        }
        let mut credentials = self.read()?;
        credentials.providers.insert(
            provider.to_owned(),
            StoredCredential::ApiKey {
                secret: SecretString::from(secret.to_owned()),
            },
        );
        self.write(&credentials)
    }

    pub(crate) fn api_key(&self, provider: &str) -> Result<Option<String>, String> {
        match self.resolve(provider)? {
            Some(StoredCredential::ApiKey { secret }) => {
                Ok(Some(secret.expose_secret().to_owned()))
            }
            Some(StoredCredential::OAuth { .. }) => Err(format!(
                "provider {provider:?} has an OAuth credential, not an API key"
            )),
            None => Ok(None),
        }
    }

    pub(crate) fn auth_for_model(
        &self,
        model: ModelIden,
    ) -> Result<Option<AuthData>, genai::resolver::Error> {
        let adapter = model.adapter_kind.as_lower_str();
        let provider = providers::auth_provider_for_adapter(adapter).unwrap_or(adapter);
        if let Some(secret) = self
            .api_key(provider)
            .map_err(genai::resolver::Error::Custom)?
        {
            return Ok(Some(AuthData::from_single(secret)));
        }
        if let Some(secret) = providers::environment_api_key(provider) {
            return Ok(Some(AuthData::from_single(secret)));
        }
        Ok(None)
    }

    fn read(&self) -> Result<StoredCredentials, String> {
        match fs::read_to_string(&self.path) {
            Ok(source) => serde_json::from_str(&source)
                .map_err(|error| format!("cannot parse {}: {error}", self.path.display())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Ok(StoredCredentials::default())
            }
            Err(error) => Err(format!("cannot read {}: {error}", self.path.display())),
        }
    }

    fn write(&self, credentials: &StoredCredentials) -> Result<(), String> {
        let parent = self.path.parent().ok_or_else(|| {
            format!(
                "credential path {} has no parent directory",
                self.path.display()
            )
        })?;
        let parent_existed = parent.exists();
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        if !parent_existed {
            secure_directory(parent)?;
        }
        let temporary = self.path.with_extension("json.new");
        let source = serde_json::to_vec_pretty(credentials)
            .map_err(|error| format!("cannot encode credentials: {error}"))?;
        let mut options = OpenOptions::new();
        options.create(true).truncate(true).write(true);
        secure_file_options(&mut options);
        let mut file = options
            .open(&temporary)
            .map_err(|error| format!("cannot create {}: {error}", temporary.display()))?;
        secure_file(&temporary)?;
        file.write_all(&source)
            .and_then(|()| file.write_all(b"\n"))
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
        fs::rename(&temporary, &self.path)
            .map_err(|error| format!("cannot replace {}: {error}", self.path.display()))
    }
}

#[cfg(unix)]
fn secure_directory(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("cannot secure {}: {error}", path.display()))
}

#[cfg(not(unix))]
fn secure_directory(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn secure_file_options(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(unix)]
fn secure_file(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("cannot secure {}: {error}", path.display()))
}

#[cfg(not(unix))]
fn secure_file(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(not(unix))]
fn secure_file_options(_options: &mut OpenOptions) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "phenix-{label}-test-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn api_key_round_trips_through_secure_store() {
        let root = temp_root("credential");
        let store = CredentialStore {
            path: root.join("credentials.json"),
        };
        store.save_api_key("openai-api", "test-secret").unwrap();
        assert_eq!(
            store.api_key("openai-api").unwrap().as_deref(),
            Some("test-secret")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn oauth_credential_persists_with_unchanged_wire_shape() {
        let root = temp_root("credential-oauth");
        let store = CredentialStore {
            path: root.join("credentials.json"),
        };
        let credential = StoredCredential::OAuth {
            access_token: SecretString::from("access".to_owned()),
            refresh_token: SecretString::from("refresh".to_owned()),
            id_token: SecretString::from("id".to_owned()),
            account_id: "account".to_owned(),
            expires_at: 99,
        };
        store.save_oauth("openai-codex", credential).unwrap();

        let source = fs::read_to_string(&store.path).unwrap();
        assert!(source.contains(r#""type": "o_auth""#));
        assert!(source.contains(r#""access_token": "access""#));
        assert!(!source.contains("<redacted>"));

        let resolved = store.resolve("openai-codex").unwrap().unwrap();
        let StoredCredential::OAuth {
            access_token,
            refresh_token,
            id_token,
            account_id,
            expires_at,
        } = resolved
        else {
            panic!("expected OAuth credential");
        };
        assert_eq!(access_token.expose_secret(), "access");
        assert_eq!(refresh_token.expose_secret(), "refresh");
        assert_eq!(id_token.expose_secret(), "id");
        assert_eq!(account_id, "account");
        assert_eq!(expires_at, 99);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn credential_wire_rejects_unknown_fields() {
        let error = serde_json::from_str::<StoredCredential>(
            r#"{"type":"api_key","secret":"test-secret","unexpected":true}"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }
}
