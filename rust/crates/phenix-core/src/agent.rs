use crate::{
    Bytes, CallableId, ComponentInterface, ContextResourceId, ContextRevisionId, InterfaceId,
    ModelId, PhenixSchema, PhenixValue, ServiceId, SkillId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MODEL_INFERENCE_SERVICE: &str = "phenix.models.inference@1";
pub const TOOL_SERVICE: &str = "phenix.tools@1";
pub const SKILL_SERVICE: &str = "phenix.skills@1";
pub const CONTEXT_SERVICE: &str = "phenix.context@1";

#[derive(phenix_sdk_macros::PhenixValue, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelInferenceRequest {
    pub model: ModelId,
    pub input: Bytes,
    #[serde(default)]
    pub options: BTreeMap<String, PhenixValue>,
    /// The complete model-visible tool surface for this inference turn.
    #[serde(default)]
    pub tools: Vec<ModelToolDescriptor>,
    /// Completed assistant/tool exchanges for this logical inference turn.
    #[serde(default)]
    pub continuation: Vec<ModelToolTurn>,
}

#[derive(phenix_sdk_macros::PhenixValue, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelInferenceResponse {
    pub output: Bytes,
    pub provider_metadata: BTreeMap<String, PhenixValue>,
    /// Structured tool calls emitted by the provider, in provider order.
    #[serde(default)]
    pub tool_calls: Vec<ModelToolCall>,
}

#[derive(
    phenix_sdk_macros::PhenixValue,
    Clone,
    Debug,
    Eq,
    PartialEq,
    Serialize,
    Deserialize,
)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelInferenceFailure {
    Authentication { message: String },
    Permission { message: String },
    NotFound { message: String },
    RateLimited {
        message: String,
        retry_after_ms: Option<u64>,
    },
    ContextLimit { message: String },
    InvalidRequest { message: String },
    Unavailable { message: String },
    Transport { message: String },
    Protocol { message: String },
}

impl ModelInferenceFailure {
    #[must_use]
    pub const fn retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimited { .. } | Self::Unavailable { .. } | Self::Transport { .. }
        )
    }

    #[must_use]
    pub const fn requires_context_recovery(&self) -> bool {
        matches!(self, Self::ContextLimit { .. })
    }

    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            Self::Authentication { message }
            | Self::Permission { message }
            | Self::NotFound { message }
            | Self::ContextLimit { message }
            | Self::InvalidRequest { message }
            | Self::Unavailable { message }
            | Self::Transport { message }
            | Self::Protocol { message }
            | Self::RateLimited { message, .. } => message,
        }
    }
}

/// Backend-neutral metadata presented to a model for one ordinary callable.
#[derive(phenix_sdk_macros::PhenixValue, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ModelToolDescriptor {
    pub id: CallableId,
    pub description: String,
    pub input_schema: PhenixSchema,
    pub output_schema: PhenixSchema,
}

/// A typed request to invoke an ordinary model-visible callable.
#[derive(phenix_sdk_macros::PhenixValue, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelToolCall {
    pub call_id: String,
    pub callable_id: CallableId,
    pub input: PhenixValue,
}

/// Structural result for one provider-issued model tool call.
///
/// Errors remain typed `PhenixValue` payloads instead of being flattened into
/// prompt prose. Provider adapters only serialize this value at the wire edge.
#[derive(phenix_sdk_macros::PhenixValue, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelToolResult {
    pub call_id: String,
    pub callable_id: CallableId,
    pub output: PhenixValue,
    pub is_error: bool,
}

/// One completed assistant/tool exchange used to continue provider inference.
#[derive(phenix_sdk_macros::PhenixValue, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelToolTurn {
    pub assistant_output: Bytes,
    pub tool_calls: Vec<ModelToolCall>,
    pub tool_results: Vec<ModelToolResult>,
}

pub struct ModelInferenceInterface;

impl ComponentInterface for ModelInferenceInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(MODEL_INFERENCE_SERVICE).expect("static model interface is valid")
    }

    fn schema() -> crate::InterfaceSchema {
        crate::InterfaceSchema::fallible_of::<
            ModelInferenceRequest,
            ModelInferenceResponse,
            ModelInferenceFailure,
        >()
    }
}

fn any_schema() -> PhenixSchema {
    PhenixSchema::Any
}

#[derive(phenix_sdk_macros::PhenixValue, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub id: CallableId,
    #[serde(default = "any_schema")]
    pub input_schema: PhenixSchema,
    #[serde(default = "any_schema")]
    pub output_schema: PhenixSchema,
    #[serde(default)]
    pub output_prefix: Bytes,
}

#[derive(phenix_sdk_macros::PhenixValue, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum ToolCommand {
    Register { tool: ToolDefinition },
    Get { id: CallableId },
    List,
    Invoke { id: CallableId, input: Bytes },
}

#[derive(phenix_sdk_macros::PhenixValue, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ToolResponse {
    Tool { tool: Option<ToolDefinition> },
    Tools { tools: Vec<ToolDefinition> },
    Output { output: Bytes },
}

#[derive(phenix_sdk_macros::PhenixValue, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SkillDefinition {
    pub id: SkillId,
    pub content: Bytes,
}

#[derive(phenix_sdk_macros::PhenixValue, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum SkillCommand {
    Register { skill: SkillDefinition },
    Get { id: SkillId },
    List,
}

#[derive(phenix_sdk_macros::PhenixValue, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum SkillResponse {
    Skill { skill: Option<SkillDefinition> },
    Skills { skills: Vec<SkillDefinition> },
}

#[derive(
    phenix_sdk_macros::PhenixValue,
    Clone,
    Debug,
    Eq,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ContextResourceKind {
    ProjectInstruction,
    ProjectDocument,
    Skill,
    External,
}

#[derive(
    phenix_sdk_macros::PhenixValue,
    Clone,
    Debug,
    Eq,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ContextScope {
    Workspace,
    PathPrefix(String),
}

#[derive(phenix_sdk_macros::PhenixValue, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContextDescriptor {
    pub resource_id: ContextResourceId,
    pub revision: ContextRevisionId,
    pub kind: ContextResourceKind,
    pub source: String,
    pub scope: ContextScope,
    pub content_identity: String,
    pub estimated_bytes: u64,
}

#[derive(phenix_sdk_macros::PhenixValue, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContextResourceRevision {
    pub descriptor: ContextDescriptor,
    pub content: Bytes,
}

#[derive(phenix_sdk_macros::PhenixValue, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum ContextCommand {
    Register {
        resource_id: ContextResourceId,
        kind: ContextResourceKind,
        source: String,
        scope: ContextScope,
        content: Bytes,
    },
    Get {
        resource_id: ContextResourceId,
        revision: ContextRevisionId,
    },
    List,
}

#[derive(phenix_sdk_macros::PhenixValue, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ContextResponse {
    Registered {
        resource: ContextResourceRevision,
    },
    Resource {
        resource: Option<ContextResourceRevision>,
    },
    Resources {
        descriptors: Vec<ContextDescriptor>,
    },
}

pub fn model_inference_service() -> ServiceId {
    ServiceId::parse(MODEL_INFERENCE_SERVICE).expect("static service id")
}

pub fn tool_service() -> ServiceId {
    ServiceId::parse(TOOL_SERVICE).expect("static service id")
}

pub fn skill_service() -> ServiceId {
    ServiceId::parse(SKILL_SERVICE).expect("static service id")
}

pub fn context_service() -> ServiceId {
    ServiceId::parse(CONTEXT_SERVICE).expect("static service id")
}
