//! Non-journaled credential-provider operations. These are management/transport
//! imports, never Model tools, ordinary configuration or frontend event data.
use super::{ProviderError, ProviderErrorCode};
use phenix_sdk::{contract, Blob, Code, Failure, Portable, StaticField, StaticSchema, Value};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

/// Validated, immutable scope identity: only checked constructors and strict
/// wire/serde decoding can create it. Values are selectors, never grants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "UntrustedCredentialIdentity")]
pub struct CredentialIdentity {
    provider_id: String,
    connection_id: String,
    profile_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UntrustedCredentialIdentity {
    provider_id: String,
    connection_id: String,
    profile_id: String,
}

impl TryFrom<UntrustedCredentialIdentity> for CredentialIdentity {
    type Error = String;
    fn try_from(raw: UntrustedCredentialIdentity) -> Result<Self, Self::Error> {
        Self::new(raw.provider_id, raw.connection_id, raw.profile_id).map_err(|error| error.message)
    }
}

impl Portable for CredentialIdentity {
    const SCHEMA: StaticSchema = StaticSchema::Struct(&[
        StaticField {
            name: "connection_id",
            schema: StaticSchema::String,
        },
        StaticField {
            name: "profile_id",
            schema: StaticSchema::String,
        },
        StaticField {
            name: "provider_id",
            schema: StaticSchema::String,
        },
    ]);

    fn to_value(&self) -> Value {
        Value::map([
            ("connection_id", self.connection_id.to_value()),
            ("profile_id", self.profile_id.to_value()),
            ("provider_id", self.provider_id.to_value()),
        ])
    }

    fn from_value(value: Value) -> Result<Self, Failure> {
        let Value::Map(mut fields) = value else {
            return Err(Failure::new(
                Code::SchemaRejected,
                "credential identity must be a struct",
            ));
        };
        let mut take = |key| {
            fields
                .remove(key)
                .ok_or_else(|| {
                    Failure::new(Code::SchemaRejected, "missing credential identity field")
                })
                .and_then(String::from_value)
        };
        let connection = take("connection_id")?;
        let profile = take("profile_id")?;
        let provider = take("provider_id")?;
        if !fields.is_empty() {
            return Err(Failure::new(
                Code::SchemaRejected,
                "unknown credential identity field",
            ));
        }
        Self::new(provider, connection, profile)
            .map_err(|_| Failure::new(Code::SchemaRejected, "invalid credential identity"))
    }
}

impl CredentialIdentity {
    pub fn new(
        provider_id: impl Into<String>,
        connection_id: impl Into<String>,
        profile_id: impl Into<String>,
    ) -> Result<Self, ProviderError> {
        let identity = Self {
            provider_id: provider_id.into(),
            connection_id: connection_id.into(),
            profile_id: profile_id.into(),
        };
        identity.resource()?;
        Ok(identity)
    }

    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }
    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }
    pub fn profile_id(&self) -> &str {
        &self.profile_id
    }

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

    /// Request one exact, explicitly selected credential root for a native
    /// `ProviderCredentials::resolve` import. This is authority attenuation,
    /// not authority creation: the kernel still rejects absent grants and
    /// caller scopes that do not already authorize this resource and operation.
    ///
    /// Never include this scope in ordinary ModelRequest, journal, ACP wire
    /// payloads or a Basic graph-wide session resource set.
    pub fn resolve_scope(&self) -> Result<phenix_sdk::ScopedRequest, ProviderError> {
        use phenix_sdk::{Authority, Operation, ScopedRequest};
        let resource = self.resource()?;
        Ok(ScopedRequest {
            authority: Authority {
                operations: [<ProviderCredentials::resolve as Operation>::KEY.into()].into(),
                resources: [resource.clone()].into(),
            },
            resource: Some(resource),
        })
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
        let identity = Self::new(
            decode(components[0]).ok_or_else(invalid)?,
            decode(components[1]).ok_or_else(invalid)?,
            decode(components[2]).ok_or_else(invalid)?,
        )?;
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
            bytes.zeroize();
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
        self.0.zeroize();
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
        CredentialIdentity::new(provider, connection, profile).unwrap()
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
            assert!(CredentialIdentity::new("a", "api", invalid).is_err());
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
                CredentialIdentity::new(
                    "a".repeat(sizes.0),
                    "b".repeat(sizes.1),
                    "c".repeat(sizes.2)
                )
                .unwrap_err()
                .code,
                ProviderErrorCode::InvalidInput
            );
        }
    }

    #[test]
    fn no_invalid_credential_identity_can_be_deserialized() {
        let original = identity("openai", "api", "personal");
        assert_eq!(
            CredentialIdentity::from_value(original.to_value()).unwrap(),
            original
        );
        assert_eq!(
            serde_json::from_value::<CredentialIdentity>(serde_json::to_value(&original).unwrap())
                .unwrap(),
            original
        );

        let bad_wire = Value::map([
            ("provider_id", Value::String("../other".into())),
            ("connection_id", Value::String("api".into())),
            ("profile_id", Value::String("personal".into())),
        ]);
        assert!(CredentialIdentity::from_value(bad_wire).is_err());
        assert!(
            serde_json::from_value::<CredentialIdentity>(serde_json::json!({
                "provider_id": "../other",
                "connection_id": "api",
                "profile_id": "personal"
            }))
            .is_err()
        );
    }

    #[test]
    fn extra_credential_identity_fields_are_never_accepted() {
        assert!(
            serde_json::from_value::<CredentialIdentity>(serde_json::json!({
                "provider_id": "openai",
                "connection_id": "api",
                "profile_id": "personal",
                "scope_override": "phenix.providers.admin"
            }))
            .is_err()
        );
    }

    #[test]
    fn resolution_scope_is_one_exact_operation_and_credential_identity() {
        use phenix_sdk::{Operation, ScopedRequest};
        let work = identity("openai", "api", "work");
        let personal = identity("openai", "api", "personal");
        let scope: ScopedRequest = work.resolve_scope().unwrap();
        let expected_resource = work.resource().unwrap();
        assert_eq!(scope.resource.as_deref(), Some(expected_resource.as_str()));
        assert_eq!(
            scope.authority.operations,
            [<ProviderCredentials::resolve as Operation>::KEY.into()].into()
        );
        assert_eq!(
            scope.authority.resources,
            [expected_resource.clone()].into()
        );
        assert!(!scope
            .authority
            .resources
            .contains(&personal.resource().unwrap()));
        assert!(!scope
            .authority
            .operations
            .contains(<ProviderCredentials::import as Operation>::KEY));
        assert!(!scope
            .authority
            .operations
            .contains(<ProviderCredentials::revoke as Operation>::KEY));
        // Constructing a scope never grants it; the running kernel still
        // requires matching trusted composition grants and caller authority.
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
