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

    /// Recover the exact non-secret identity from an already authorized
    /// resource. This is display/capability metadata, never a credential
    /// lookup or an authority grant.
    pub fn from_resource(resource: &str) -> Result<Self, ProviderError> {
        fn decode(item: &str) -> Option<String> {
            if item.is_empty() || !item.len().is_multiple_of(2) || item.len() > 128 {
                return None;
            }
            let mut result = Vec::with_capacity(item.len() / 2);
            for pair in item.as_bytes().chunks_exact(2) {
                let text = std::str::from_utf8(pair).ok()?;
                result.push(u8::from_str_radix(text, 16).ok()?);
            }
            String::from_utf8(result).ok()
        }
        let invalid = || ProviderError {
            code: ProviderErrorCode::InvalidInput,
            message: "invalid credential identity".into(),
        };
        let encoded = resource
            .strip_prefix("phenix.credentials.")
            .ok_or_else(invalid)?;
        let components = encoded.split('.').collect::<Vec<_>>();
        if components.len() != 3 {
            return Err(invalid());
        }
        let identity = Self {
            provider_id: decode(components[0]).ok_or_else(invalid)?,
            connection_id: decode(components[1]).ok_or_else(invalid)?,
            profile_id: decode(components[2]).ok_or_else(invalid)?,
        };
        if identity.resource()?.as_str() != resource {
            return Err(invalid());
        }
        Ok(identity)
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
    fn launcher_resource_can_be_decoded_without_widening_identity() {
        let selected = identity("openai", "api", "personal");
        let resource = selected.resource().unwrap();
        assert_eq!(
            CredentialIdentity::from_resource(&resource).unwrap(),
            selected
        );
        for invalid in [
            "",
            "phenix.credentials.openai.api.personal",
            "phenix.credentials.6f70.617069",
            "phenix.credentials.6f70.617069.706572736f6e616c.extra",
            "phenix.credentials.4F70.617069.706572736f6e616c",
            "phenix.credentials.2e.617069.706572736f6e616c",
        ] {
            assert_eq!(
                CredentialIdentity::from_resource(invalid).unwrap_err().code,
                ProviderErrorCode::InvalidInput,
            );
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
