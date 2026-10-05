use crate::SessionRecord;
use phenix_core::{Bytes, ComponentInterface, InterfaceId};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Component, Path},
};

pub const SDK_SESSION_SERVICE: &str = "phenix.api.sessions@1";
pub const SDK_TOOLS_SERVICE: &str = "phenix.api.tools@1";
pub const SDK_SKILLS_SERVICE: &str = "phenix.api.skills@1";
pub const SDK_CONFIG_SERVICE: &str = "phenix.api.config@1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum SdkSessionCommand {
    Open {
        id: String,
        #[serde(default)]
        agent: Option<String>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum SdkSessionResponse {
    Opened {
        session: SessionRecord,
        created: bool,
    },
}

pub struct SdkSessionInterface;

impl ComponentInterface for SdkSessionInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(SDK_SESSION_SERVICE).expect("static SDK session interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<SdkSessionCommand, SdkSessionResponse>()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct SdkTool {
    pub id: String,
    pub service: String,
    pub required_permissions: BTreeSet<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum SdkToolCommand {
    Register {
        id: String,
        service: String,
        #[serde(default)]
        required_permissions: BTreeSet<String>,
    },
    Invoke {
        execution_id: String,
        id: String,
        input: Vec<u8>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum SdkToolResponse {
    Tool { tool: SdkTool },
    Output { output: Vec<u8> },
}

pub struct SdkToolsInterface;

impl ComponentInterface for SdkToolsInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(SDK_TOOLS_SERVICE).expect("static SDK tools interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<SdkToolCommand, SdkToolResponse>()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct SdkSkill {
    pub id: String,
    pub content: Bytes,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, phenix_sdk_macros::PhenixValue)]
#[serde(deny_unknown_fields)]
pub struct SdkSkillSummary {
    pub id: String,
    pub revision: String,
    pub source: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum SdkSkillCommand {
    Register { id: String, content: Bytes },
    Get { id: String },
    List,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum SdkSkillResponse {
    Skill { skill: Option<SdkSkill> },
    Skills { skills: Vec<SdkSkillSummary> },
}

pub struct SdkSkillsInterface;

impl ComponentInterface for SdkSkillsInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(SDK_SKILLS_SERVICE).expect("static SDK skills interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<SdkSkillCommand, SdkSkillResponse>()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum SdkConfigCommand {
    Read { path: SdkConfigPath },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "String")]
pub struct SdkConfigPath(String);

impl SdkConfigPath {
    pub fn parse(value: impl Into<String>) -> Result<Self, &'static str> {
        value.into().try_into()
    }

    #[must_use]
    pub fn as_path(&self) -> &Path {
        Path::new(&self.0)
    }
}

impl TryFrom<String> for SdkConfigPath {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err("config path must not be empty");
        }
        if value
            .split(std::path::MAIN_SEPARATOR)
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
            || !Path::new(&value)
                .components()
                .all(|component| matches!(component, Component::Normal(_)))
        {
            return Err("config path must be relative and contain no . or .. components");
        }
        Ok(Self(value))
    }
}

impl phenix_core::ValueCodec for SdkConfigPath {
    fn phenix_type() -> phenix_core::Type {
        phenix_core::Type::String
    }

    fn to_value(&self) -> phenix_core::PhenixValue {
        phenix_core::PhenixValue::String(self.0.clone())
    }

    fn from_value(value: &phenix_core::PhenixValue) -> Result<Self, phenix_core::ValueError> {
        let value = String::try_from(phenix_core::Exact(value))?;
        Self::try_from(value).map_err(|error| phenix_core::ValueError::InvalidValue(error.into()))
    }

    fn project_from_value(
        value: &phenix_core::PhenixValue,
    ) -> Result<Self, phenix_core::ValueError> {
        let value = String::try_from(phenix_core::Project(value))?;
        Self::try_from(value).map_err(|error| phenix_core::ValueError::InvalidValue(error.into()))
    }
}

impl From<&SdkConfigPath> for phenix_core::PhenixValue {
    fn from(value: &SdkConfigPath) -> Self {
        <SdkConfigPath as phenix_core::ValueCodec>::to_value(value)
    }
}

impl<'value> TryFrom<phenix_core::Exact<&'value phenix_core::PhenixValue>> for SdkConfigPath {
    type Error = phenix_core::ValueError;

    fn try_from(
        value: phenix_core::Exact<&'value phenix_core::PhenixValue>,
    ) -> Result<Self, Self::Error> {
        <Self as phenix_core::ValueCodec>::from_value(value.0)
    }
}

impl<'value> TryFrom<phenix_core::Project<&'value phenix_core::PhenixValue>> for SdkConfigPath {
    type Error = phenix_core::ValueError;

    fn try_from(
        value: phenix_core::Project<&'value phenix_core::PhenixValue>,
    ) -> Result<Self, Self::Error> {
        <Self as phenix_core::ValueCodec>::project_from_value(value.0)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum SdkConfigResponse {
    File { content: Bytes },
}

pub struct SdkConfigInterface;

impl ComponentInterface for SdkConfigInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(SDK_CONFIG_SERVICE).expect("static SDK config interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<SdkConfigCommand, SdkConfigResponse>()
    }
}
