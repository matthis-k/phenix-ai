//! Non-journaled credential-provider operations. These are management/transport
//! imports, never Model tools, ordinary configuration or frontend event data.
use super::{ProviderError, ProviderErrorCode};
use phenix_sdk::{contract, Blob, Code, Failure, Portable, StaticSchema, Value};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct CredentialIdentity {
    pub provider_id: String,
    pub connection_id: String,
    pub profile_id: String,
}

impl CredentialIdentity {
    /// Exact credential resource; a provider-list/session grant is insufficient.
    /// Encoding each component avoids collisions between dotted/underscored IDs.
    pub fn resource(&self) -> Result<String, ProviderError> {
        // The file backend's hex leaf (two separators + ".credential") is
        // limited to 240 bytes. This also fits canonical 256-byte resources.
        if self.provider_id.len() + self.connection_id.len() + self.profile_id.len() > 113 {
            return Err(ProviderError {
                code: ProviderErrorCode::InvalidInput,
                message: "invalid credential identity".into(),
            });
        }
        let mut encoded = Vec::new();
        for item in [&self.provider_id, &self.connection_id, &self.profile_id] {
            if item.is_empty()
                || item.len() > 64
                || !item
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
            {
                return Err(ProviderError {
                    code: ProviderErrorCode::InvalidInput,
                    message: "invalid credential identity".into(),
                });
            }
            encoded.push(
                item.bytes()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
            );
        }
        Ok(format!("phenix.credentials.{}", encoded.join(".")))
    }
}

/// Secret bytes deliberately have no serde/config serialization or Clone.
/// Portable encoding exists only for the separately granted native call.
/// Debug redacts both submitted and resolved values.
pub struct CredentialSecret(Vec<u8>);

impl CredentialSecret {
    pub fn new(mut bytes: Vec<u8>) -> Result<Self, ProviderError> {
        if bytes.is_empty() || bytes.len() > 64 * 1024 || bytes.contains(&0) {
            bytes.fill(0);
            return Err(ProviderError {
                code: ProviderErrorCode::InvalidInput,
                message: "invalid credential secret".into(),
            });
        }
        Ok(Self(bytes))
    }

    pub fn expose_for_authorized_provider(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for CredentialSecret {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        output.write_str("CredentialSecret([REDACTED])")
    }
}

impl Drop for CredentialSecret {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

impl Portable for CredentialSecret {
    const SCHEMA: StaticSchema = <Blob as Portable>::SCHEMA;

    fn to_value(&self) -> Value {
        Value::Bytes(self.0.clone())
    }

    fn from_value(value: Value) -> Result<Self, Failure> {
        let Blob(bytes) = Blob::from_value(value)?;
        Self::new(bytes)
            .map_err(|_| Failure::new(Code::SchemaRejected, "invalid credential secret"))
    }
}

#[derive(Debug, Portable)]
pub struct CredentialImport {
    pub identity: CredentialIdentity,
    pub secret: CredentialSecret,
}

/// Storage presence is not authentication/entitlement evidence. ProviderAuth
/// still owns method validity; selection still requires fresh provider proof.
#[contract(id = "phenix.provider.credentials@1")]
pub trait ProviderCredentials {
    fn import(&self, request: CredentialImport) -> Result<(), ProviderError>;
    fn resolve(&self, request: CredentialIdentity) -> Result<CredentialSecret, ProviderError>;
    fn revoke(&self, request: CredentialIdentity) -> Result<(), ProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(provider: &str, connection: &str, profile: &str) -> CredentialIdentity {
        CredentialIdentity {
            provider_id: provider.into(),
            connection_id: connection.into(),
            profile_id: profile.into(),
        }
    }

    #[test]
    fn exact_resource_cannot_alias_another_account_or_accept_paths() {
        assert_ne!(
            identity("a__b", "c", "d").resource().unwrap(),
            identity("a", "b__c", "d").resource().unwrap()
        );
        assert_ne!(
            identity("a", "api", "work").resource().unwrap(),
            identity("a", "api", "personal").resource().unwrap()
        );
        for invalid in ["", "../secret", "a.b", "a/b"] {
            assert!(identity("a", "api", invalid).resource().is_err());
        }
    }

    #[test]
    fn identity_boundaries_fit_authority_and_file_backend() {
        // 113 input bytes produce a 239-byte backend leaf and 247-byte resource.
        assert_eq!(
            identity(&"a".repeat(64), &"b".repeat(48), "c")
                .resource()
                .unwrap()
                .len(),
            247
        );
        for sizes in [(64, 49, 1), (38, 38, 38), (64, 64, 64), (65, 1, 1)] {
            assert_eq!(
                identity(
                    &"a".repeat(sizes.0),
                    &"b".repeat(sizes.1),
                    &"c".repeat(sizes.2)
                )
                .resource()
                .unwrap_err()
                .code,
                ProviderErrorCode::InvalidInput
            );
        }
    }

    #[test]
    fn submitted_and_resolved_secrets_roundtrip_without_debug_leakage() {
        let request = CredentialImport {
            identity: identity("openai", "api", "personal"),
            secret: CredentialSecret::new(b"fixture-secret".to_vec()).unwrap(),
        };
        assert!(!format!("{request:?}").contains("fixture-secret"));
        let decoded = CredentialImport::from_value(request.to_value()).unwrap();
        assert_eq!(
            decoded.secret.expose_for_authorized_provider(),
            b"fixture-secret"
        );
        assert!(!format!("{:?}", decoded.secret).contains("fixture-secret"));
        for invalid in [vec![], vec![0], vec![1; 64 * 1024 + 1]] {
            let error = CredentialSecret::new(invalid).unwrap_err();
            assert_eq!(error.code, ProviderErrorCode::InvalidInput);
        }
    }
}
