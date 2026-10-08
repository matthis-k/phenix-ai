use phenix_core::{ComponentInterface, InterfaceId, ServiceId};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use std::collections::BTreeSet;
use std::fmt::{self, Display, Formatter};

pub const OPTIONS_SERVICE: &str = "phenix.options@1";

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct OptionKey(String);

impl OptionKey {
    pub fn parse(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        if value.is_empty() {
            return Err("option key must not be empty");
        }
        if !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err("option key contains unsupported characters");
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for OptionKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for OptionKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for OptionKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct OptionSubjectId(String);

impl OptionSubjectId {
    pub fn parse(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err("option scope subject must not be empty");
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for OptionSubjectId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for OptionSubjectId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

macro_rules! validated_string_value {
    ($ty:ty, $parse:path) => {
        impl phenix_core::ValueCodec for $ty {
            fn phenix_type() -> phenix_core::Type {
                phenix_core::Type::String
            }

            fn to_value(&self) -> phenix_core::PhenixValue {
                phenix_core::PhenixValue::String(self.as_str().to_owned())
            }

            fn from_value(
                value: &phenix_core::PhenixValue,
            ) -> Result<Self, phenix_core::ValueError> {
                let value = String::try_from(phenix_core::Exact(value))?;
                $parse(value).map_err(|error| phenix_core::ValueError::InvalidValue(error.into()))
            }

            fn project_from_value(
                value: &phenix_core::PhenixValue,
            ) -> Result<Self, phenix_core::ValueError> {
                let value = String::try_from(phenix_core::Project(value))?;
                $parse(value).map_err(|error| phenix_core::ValueError::InvalidValue(error.into()))
            }
        }

        impl From<&$ty> for phenix_core::PhenixValue {
            fn from(value: &$ty) -> Self {
                <$ty as phenix_core::ValueCodec>::to_value(value)
            }
        }

        impl<'value> TryFrom<phenix_core::Exact<&'value phenix_core::PhenixValue>> for $ty {
            type Error = phenix_core::ValueError;

            fn try_from(
                value: phenix_core::Exact<&'value phenix_core::PhenixValue>,
            ) -> Result<Self, Self::Error> {
                <Self as phenix_core::ValueCodec>::from_value(value.0)
            }
        }

        impl<'value> TryFrom<phenix_core::Project<&'value phenix_core::PhenixValue>> for $ty {
            type Error = phenix_core::ValueError;

            fn try_from(
                value: phenix_core::Project<&'value phenix_core::PhenixValue>,
            ) -> Result<Self, Self::Error> {
                <Self as phenix_core::ValueCodec>::project_from_value(value.0)
            }
        }
    };
}

validated_string_value!(OptionKey, OptionKey::parse);
validated_string_value!(OptionSubjectId, OptionSubjectId::parse);

#[derive(
    Clone,
    Copy,
    Debug,
    Deserialize,
    Eq,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum OptionScopeKind {
    Global,
    Session,
    Agent,
}

#[derive(
    Clone,
    Debug,
    Deserialize,
    Eq,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum OptionScope {
    Global,
    Session(OptionSubjectId),
    Agent(OptionSubjectId),
}

#[derive(
    Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(deny_unknown_fields)]
pub struct OptionContext {
    pub session: Option<OptionSubjectId>,
    pub agent: Option<OptionSubjectId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum OptionValue {
    Bool(bool),
    Integer(i64),
    String(String),
    StringList(Vec<String>),
}

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Deserialize,
    Eq,
    PartialEq,
    Serialize,
    phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum OptionStartupPrecedence {
    #[default]
    Nix,
    File,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum OptionValueLayer {
    Runtime,
    Nix,
    File,
    Default,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct OptionDefinition {
    key: OptionKey,
    default: OptionValue,
    scopes: BTreeSet<OptionScopeKind>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OptionDefinitionWire {
    key: OptionKey,
    default: OptionValue,
    scopes: BTreeSet<OptionScopeKind>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct OptionAssignment {
    pub key: OptionKey,
    pub scope: OptionScope,
    pub value: OptionValue,
}

impl OptionDefinition {
    pub fn new(
        key: OptionKey,
        default: OptionValue,
        scopes: impl IntoIterator<Item = OptionScopeKind>,
    ) -> Result<Self, String> {
        let scopes = scopes.into_iter().collect::<BTreeSet<_>>();
        if scopes.is_empty() {
            return Err(format!("option {key} has no writable scope"));
        }
        Ok(Self {
            key,
            default,
            scopes,
        })
    }

    #[must_use]
    pub fn key(&self) -> &OptionKey {
        &self.key
    }

    #[must_use]
    pub fn default_value(&self) -> &OptionValue {
        &self.default
    }

    pub fn scopes(&self) -> impl Iterator<Item = OptionScopeKind> + '_ {
        self.scopes.iter().copied()
    }
}

impl TryFrom<OptionDefinitionWire> for OptionDefinition {
    type Error = String;

    fn try_from(wire: OptionDefinitionWire) -> Result<Self, Self::Error> {
        Self::new(wire.key, wire.default, wire.scopes)
    }
}

impl<'de> Deserialize<'de> for OptionDefinition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OptionDefinitionWire::deserialize(deserializer)?
            .try_into()
            .map_err(D::Error::custom)
    }
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum OptionValueSource {
    Default,
    Global,
    Session,
    Agent,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct ResolvedOption {
    pub key: OptionKey,
    pub value: OptionValue,
    pub source: OptionValueSource,
    pub layer: OptionValueLayer,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum OptionCommand {
    Define {
        definition: OptionDefinition,
    },
    GetDefinition {
        key: OptionKey,
    },
    Configure {
        file_values: Vec<OptionAssignment>,
        nix_values: Vec<OptionAssignment>,
        precedence: OptionStartupPrecedence,
    },
    Set {
        key: OptionKey,
        scope: OptionScope,
        value: OptionValue,
    },
    Unset {
        key: OptionKey,
        scope: OptionScope,
    },
    Resolve {
        key: OptionKey,
        context: OptionContext,
    },
    List {
        context: OptionContext,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum OptionResponse {
    Defined {
        definition: OptionDefinition,
    },
    Definition {
        definition: Option<OptionDefinition>,
    },
    Configured {
        count: usize,
    },
    Updated {
        key: OptionKey,
        scope: OptionScope,
    },
    Value {
        option: ResolvedOption,
    },
    Options {
        options: Vec<ResolvedOption>,
    },
}

pub struct OptionsInterface;

impl ComponentInterface for OptionsInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(OPTIONS_SERVICE).expect("static options interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::fallible_of::<OptionCommand, OptionResponse, String>()
    }
}

#[must_use]
pub fn options_service() -> ServiceId {
    ServiceId::parse(OPTIONS_SERVICE).expect("static options service id is valid")
}
