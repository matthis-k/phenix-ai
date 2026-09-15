use crate::{PhenixValue, Type, TypeKind, ValueCodec, ValueError};
use serde::{Deserialize, Serialize};
use std::{
    fmt::{self, Display, Formatter},
    str::FromStr,
};

fn validate_identifier(value: &str) -> Result<(), &'static str> {
    if value.is_empty() {
        return Err("identifier must not be empty");
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/' | b':' | b'@')
    }) {
        return Err("identifier contains unsupported characters");
    }
    Ok(())
}

macro_rules! identifier {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
        #[serde(try_from = "String")]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> Result<Self, &'static str> {
                value.into().try_into()
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl Display for $name {
            fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = &'static str;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                value.to_owned().try_into()
            }
        }

        impl TryFrom<String> for $name {
            type Error = &'static str;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                validate_identifier(&value)?;
                Ok(Self(value))
            }
        }

        impl ValueCodec for $name {
            fn phenix_type() -> Type {
                Type::String
            }

            fn to_value(&self) -> PhenixValue {
                PhenixValue::String(self.0.clone())
            }

            fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
                match value {
                    PhenixValue::String(value) => Self::parse(value.clone())
                        .map_err(|error| ValueError::InvalidValue(error.into())),
                    _ => Err(ValueError::TypeMismatch {
                        expected: TypeKind::String,
                        actual: value.kind(),
                    }),
                }
            }
        }

        impl From<&$name> for PhenixValue {
            fn from(value: &$name) -> Self {
                <$name as ValueCodec>::to_value(value)
            }
        }

        impl<'value> TryFrom<crate::Exact<&'value PhenixValue>> for $name {
            type Error = ValueError;

            fn try_from(value: crate::Exact<&'value PhenixValue>) -> Result<Self, Self::Error> {
                <Self as ValueCodec>::from_value(value.0)
            }
        }

        impl<'value> TryFrom<crate::Project<&'value PhenixValue>> for $name {
            type Error = ValueError;

            fn try_from(value: crate::Project<&'value PhenixValue>) -> Result<Self, Self::Error> {
                <Self as ValueCodec>::project_from_value(value.0)
            }
        }
    };
}

identifier!(PluginId);
identifier!(ComponentId);
identifier!(ConfigurationFrontendId);
identifier!(ServiceId);
identifier!(CapabilityId);
identifier!(ResourceNamespace);
identifier!(EventTypeId);
identifier!(SubscriptionId);
identifier!(SdkNamespace);
identifier!(SdkResourceId);
identifier!(CallableId);
identifier!(ModelId);
identifier!(RoutingProfileId);
identifier!(SkillId);
identifier!(RuntimeId);
identifier!(SessionId);
identifier!(ContextResourceId);
identifier!(ContextRevisionId);
identifier!(ClientConnectionId);
identifier!(CapabilityGenerationId);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct InterfaceId(String);

impl InterfaceId {
    pub fn parse(value: impl Into<String>) -> Result<Self, &'static str> {
        value.into().try_into()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for InterfaceId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Display for InterfaceId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for InterfaceId {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.to_owned().try_into()
    }
}

impl TryFrom<String> for InterfaceId {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate_identifier(&value)?;
        let (identity, version) = value
            .rsplit_once('@')
            .ok_or("interface identifier must include an @version suffix")?;
        if identity.is_empty() {
            return Err("interface identifier identity must not be empty");
        }
        if identity.contains('@') {
            return Err("interface identifier must contain exactly one @version suffix");
        }
        let version = version
            .parse::<u64>()
            .map_err(|_| "interface identifier version must be a positive integer")?;
        if version == 0 {
            return Err("interface identifier version must be a positive integer");
        }
        Ok(Self(value))
    }
}

impl ValueCodec for InterfaceId {
    fn phenix_type() -> Type {
        Type::String
    }

    fn to_value(&self) -> PhenixValue {
        PhenixValue::String(self.0.clone())
    }

    fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        match value {
            PhenixValue::String(value) => {
                Self::parse(value.clone()).map_err(|error| ValueError::InvalidValue(error.into()))
            }
            _ => Err(ValueError::TypeMismatch {
                expected: TypeKind::String,
                actual: value.kind(),
            }),
        }
    }
}

impl From<&InterfaceId> for PhenixValue {
    fn from(value: &InterfaceId) -> Self {
        <InterfaceId as ValueCodec>::to_value(value)
    }
}

impl<'value> TryFrom<crate::Exact<&'value PhenixValue>> for InterfaceId {
    type Error = ValueError;

    fn try_from(value: crate::Exact<&'value PhenixValue>) -> Result<Self, Self::Error> {
        <Self as ValueCodec>::from_value(value.0)
    }
}

impl<'value> TryFrom<crate::Project<&'value PhenixValue>> for InterfaceId {
    type Error = ValueError;

    fn try_from(value: crate::Project<&'value PhenixValue>) -> Result<Self, Self::Error> {
        <Self as ValueCodec>::project_from_value(value.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_stable_and_reject_ambiguous_text() {
        let id = ServiceId::parse("artifact.read@1").unwrap();
        assert_eq!(id.as_str(), "artifact.read@1");
        assert!(PluginId::parse("").is_err());
        assert!(PluginId::parse("has space").is_err());
    }

    #[test]
    fn runtime_interface_ids_require_an_explicit_positive_version() {
        let id = InterfaceId::parse("phenix.models.inference@1").unwrap();
        assert_eq!(id.as_str(), "phenix.models.inference@1");
        assert!(InterfaceId::parse("phenix.models.inference").is_err());
        assert!(InterfaceId::parse("phenix.models.inference@0").is_err());
        assert!(InterfaceId::parse("phenix.models.inference@latest").is_err());
        assert!(InterfaceId::parse("phenix.models@inference@1").is_err());
        assert!(InterfaceId::parse("@1").is_err());
    }

    #[test]
    fn identifiers_round_trip_as_canonical_strings() {
        let id = ComponentId::parse("phenix.agent@1").unwrap();
        let encoded = serde_json::to_string(&id).unwrap();
        assert_eq!(encoded, "\"phenix.agent@1\"");
        assert_eq!(serde_json::from_str::<ComponentId>(&encoded).unwrap(), id);
        assert_eq!(ComponentId::from_value(&id.to_value()).unwrap(), id);
    }

    #[test]
    fn deserialization_preserves_identifier_validation() {
        assert!(serde_json::from_str::<PluginId>("\"has space\"").is_err());
        assert!(serde_json::from_str::<PluginId>("\"\"").is_err());
        assert!(serde_json::from_str::<CallableId>("\"has space\"").is_err());
        assert!(serde_json::from_str::<ModelId>("\"\"").is_err());
        assert!(serde_json::from_str::<SessionId>("\"\"").is_err());
        assert!(serde_json::from_str::<ContextResourceId>("\"has space\"").is_err());
        assert!(serde_json::from_str::<InterfaceId>("\"unversioned\"").is_err());
        assert!(serde_json::from_str::<InterfaceId>("\"phenix.models@inference@1\"").is_err());
        assert!(serde_json::from_str::<InterfaceId>("\"phenix.models.inference@0\"").is_err());
    }
}
