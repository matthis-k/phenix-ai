pub mod abi;
pub mod descriptor;
pub mod graph;
pub mod graph_v2;
pub use graph::*;
pub mod reference;
pub mod scope;
pub mod state;
pub mod static_schema;
pub use reference::*;
pub use state::*;
pub use static_schema::{StaticField, StaticSchema, StaticVariant};
mod portable;
pub use portable::Blob;
pub mod value;
pub use descriptor::*;
pub use scope::*;
use serde::{Deserialize, Serialize};
pub use value::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Code {
    DescriptorInvalid,
    ArtifactInvalid,
    AbiIncompatible,
    MissingBinding,
    AmbiguousBinding,
    SchemaRejected,
    Denied,
    PluginFailed,
    InfrastructureFailure,
    ReentrancyRejected,
    ScopeRejected,
    Cancelled,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Failure {
    pub code: Code,
    pub message: String,
}
impl Failure {
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
    pub fn to_value(&self) -> Value {
        Value::map([
            ("code", Value::String(format!("{:?}", self.code))),
            ("message", Value::String(self.message.clone())),
        ])
    }
    pub fn from_value(value: Value) -> Result<Self, Failure> {
        serde_json::from_value(value.to_json())
            .map_err(|e| Self::new(Code::PluginFailed, e.to_string()))
    }
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}
impl std::error::Error for Failure {}
pub trait Portable: Sized {
    const SCHEMA: StaticSchema;
    fn to_value(&self) -> Value;
    fn from_value(value: Value) -> Result<Self, Failure>;
}
pub trait Operation {
    type Request: Portable;
    type Response: Portable;
    type Error: Portable;
    const KEY: &'static str;
}
#[derive(Debug)]
pub enum CallError<E> {
    Domain(E),
    Kernel(Failure),
}
pub type CallResult<T, E> = Result<T, CallError<E>>;
impl<E> From<Failure> for CallError<E> {
    fn from(value: Failure) -> Self {
        Self::Kernel(value)
    }
}
