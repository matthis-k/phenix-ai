//! Provider-domain adapter identities and schema-checked, non-secret settings.
//!
//! An adapter name is descriptive metadata. The existing composition must
//! independently resolve the selected implementation and its authority.
//! Never put credentials, OAuth tokens or other secrets in these settings.

use super::{ProviderError, ProviderErrorCode};
use phenix_sdk::{Blob, Portable, Value};
use serde::{Deserialize, Serialize};

const MAX_CONFIG_BYTES: usize = 16 * 1024;

fn rejected(message: &str) -> ProviderError {
    ProviderError {
        code: ProviderErrorCode::InvalidInput,
        message: message.into(),
    }
}

/// Canonical M1 value bytes with the exact declared Portable schema identity.
///
/// The implementing plugin decodes its own typed configuration. This is not
/// executable code, an ABI negotiation, or authority to invoke the adapter.
#[derive(Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct AdapterConfig {
    pub schema_id: String,
    pub data: Blob,
}

impl std::fmt::Debug for AdapterConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdapterConfig")
            .field("schema_id", &self.schema_id)
            .field("bytes", &self.data.0.len())
            .finish()
    }
}

impl AdapterConfig {
    fn schema_id<T: Portable>() -> String {
        T::SCHEMA
            .id_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    pub fn from_typed<T: Portable>(config: &T) -> Result<Self, ProviderError> {
        let bytes = config
            .to_value()
            .encode()
            .map_err(|_| rejected("invalid adapter configuration"))?;
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err(rejected("adapter configuration exceeds size limit"));
        }
        Ok(Self {
            schema_id: Self::schema_id::<T>(),
            data: Blob(bytes),
        })
    }

    /// Validate a catalog entry even before its specific plugin is loaded.
    /// Only the selected adapter can check its declared structural schema.
    pub fn validate(&self) -> Result<(), ProviderError> {
        if self.schema_id.len() != 64
            || !self
                .schema_id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || self.data.0.len() > MAX_CONFIG_BYTES
        {
            return Err(rejected("invalid adapter configuration"));
        }
        Value::decode(&self.data.0).map_err(|_| rejected("invalid adapter configuration"))?;
        Ok(())
    }

    pub fn decode<T: Portable>(&self) -> Result<T, ProviderError> {
        if self.schema_id != Self::schema_id::<T>() || self.data.0.len() > MAX_CONFIG_BYTES {
            return Err(rejected("adapter configuration schema mismatch"));
        }
        let value =
            Value::decode(&self.data.0).map_err(|_| rejected("invalid adapter configuration"))?;
        T::from_value(value).map_err(|_| rejected("invalid adapter configuration"))
    }
}

/// A provider-defined adapter selector with configuration supplied by its
/// implementing plugin. This does not create an executable plugin binding.
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct AdapterBinding {
    pub id: String,
    pub config: AdapterConfig,
}

impl AdapterBinding {
    pub fn validate(&self) -> Result<(), ProviderError> {
        if self.id.is_empty()
            || self.id.len() > 128
            || !self
                .id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            return Err(rejected("invalid adapter identity"));
        }
        self.config.validate()
    }

    pub fn from_typed<T: Portable>(id: &str, config: &T) -> Result<Self, ProviderError> {
        let binding = Self {
            id: id.into(),
            config: AdapterConfig::from_typed(config)?,
        };
        binding.validate()?;
        Ok(binding)
    }
}

/// Standard API-key source options; provider-specific auth plugins may define
/// their own Portable configuration instead. This contains no credential.
/// Wire presentation is independent of the model protocol and provider ID.
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub enum ApiKeyPresentation {
    /// Standard HTTP bearer authorization.
    Bearer,
    /// A separately configured API-key HTTP header; `prefix` is public text
    /// prepended to the secret and may be empty (e.g. x-api-key).
    Header { name: String, prefix: String },
}

impl ApiKeyPresentation {
    pub fn validate(&self) -> Result<(), ProviderError> {
        if let Self::Header { name, prefix } = self {
            let safe_name = !name.is_empty()
                && name.len() <= 64
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
            let restricted = [
                "host",
                "cookie",
                "set-cookie",
                "content-length",
                "transfer-encoding",
                "connection",
                "content-type",
                "accept",
                "expect",
                "proxy-authorization",
            ];
            if !safe_name
                || restricted
                    .iter()
                    .any(|reserved| name.eq_ignore_ascii_case(reserved))
                || prefix.len() > 64
                || prefix.bytes().any(|byte| byte.is_ascii_control())
            {
                return Err(rejected("invalid API-key header presentation"));
            }
        }
        Ok(())
    }

    /// Public wire header **name**. Never expose the credential itself.
    pub fn header_name(&self) -> &str {
        match self {
            Self::Bearer => "Authorization",
            Self::Header { name, .. } => name,
        }
    }

    pub fn header_value<'a>(&'a self, credential: &'a str) -> String {
        match self {
            Self::Bearer => format!("Bearer {credential}"),
            Self::Header { prefix, .. } => format!("{prefix}{credential}"),
        }
    }
}

/// One reusable acquisition strategy with independent source and wire options.
#[derive(Debug, Clone, PartialEq, Eq, Portable, Serialize, Deserialize)]
pub struct ApiKeyOptions {
    pub environment_variable: Option<String>,
    pub interactive: bool,
    pub presentation: ApiKeyPresentation,
}

impl ApiKeyOptions {
    /// An API key can come from masked direct input or a provider-configured
    /// environment variable name. Both are ordinary sources for the *same*
    /// API-key method. This configuration must never contain the key itself.
    pub fn validate(&self) -> Result<(), ProviderError> {
        self.presentation.validate()?;
        let valid_name = |name: &str| {
            !name.is_empty()
                && name.len() <= 128
                && name
                    .bytes()
                    .next()
                    .is_some_and(|first| first.is_ascii_uppercase() || first == b'_')
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        };
        if self
            .environment_variable
            .as_deref()
            .is_some_and(|name| !valid_name(name))
            || (!self.interactive && self.environment_variable.is_none())
        {
            return Err(rejected("invalid API-key source configuration"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq, Portable)]
    struct CustomOptions {
        endpoint: String,
        enabled: bool,
    }

    #[test]
    fn different_plugins_can_use_distinct_typed_settings() {
        let key = AdapterBinding::from_typed(
            "api-key",
            &ApiKeyOptions {
                environment_variable: Some("OPENAI_API_KEY".into()),
                interactive: true,
                presentation: ApiKeyPresentation::Bearer,
            },
        )
        .unwrap();
        assert_eq!(
            key.config
                .decode::<ApiKeyOptions>()
                .unwrap()
                .environment_variable,
            Some("OPENAI_API_KEY".into())
        );
        assert!(key.config.decode::<CustomOptions>().is_err());
        assert_eq!(AdapterBinding::from_value(key.to_value()).unwrap(), key);

        let other = AdapterBinding::from_typed(
            "example.catalog",
            &CustomOptions {
                endpoint: "https://example.test/models".into(),
                enabled: false,
            },
        )
        .unwrap();
        assert!(!other.config.decode::<CustomOptions>().unwrap().enabled);
        assert_ne!(other.config.schema_id, key.config.schema_id);
    }

    #[test]
    fn api_key_source_options_are_names_not_credential_values() {
        for valid in ["OPENAI_API_KEY", "OPENROUTER_API_KEY", "_CUSTOM_KEY"] {
            assert!(ApiKeyOptions {
                environment_variable: Some(valid.into()),
                interactive: true,
                presentation: ApiKeyPresentation::Bearer,
            }
            .validate()
            .is_ok());
        }
        for invalid in [
            "",
            "lowercase",
            "API-KEY",
            "A B",
            "9API_KEY",
            "K=value",
            "../KEY",
        ] {
            assert!(ApiKeyOptions {
                environment_variable: Some(invalid.into()),
                interactive: true,
                presentation: ApiKeyPresentation::Bearer,
            }
            .validate()
            .is_err());
        }
        assert!(ApiKeyOptions {
            environment_variable: None,
            interactive: true,
            presentation: ApiKeyPresentation::Bearer,
        }
        .validate()
        .is_ok());
        assert!(ApiKeyOptions {
            environment_variable: None,
            interactive: false,
            presentation: ApiKeyPresentation::Bearer,
        }
        .validate()
        .is_err());
    }

    #[test]
    fn forged_schema_or_noncanonical_data_cannot_be_decoded() {
        let mut entry = AdapterBinding::from_typed("example", &()).unwrap();
        assert!(entry.config.decode::<()>().is_ok());
        entry.config.schema_id = "0".repeat(64);
        assert!(entry.config.decode::<()>().is_err());
        entry.config = AdapterConfig::from_typed(&()).unwrap();
        entry.config.data.0 = vec![0xff; 8];
        assert!(entry.config.decode::<()>().is_err());
        assert!(!format!("{:?}", entry.config).contains("255"));
    }

    #[test]
    fn adapter_identity_is_bounded_and_not_an_artifact_path() {
        for bad in ["", "../plugin.so", "a/b", "a:b"] {
            assert!(AdapterBinding::from_typed(bad, &()).is_err());
        }
        assert!(AdapterBinding::from_typed(&"a".repeat(129), &()).is_err());
    }
}
