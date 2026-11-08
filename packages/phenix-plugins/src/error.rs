//! Plugin lifecycle errors. Not graph execution errors.
use std::fmt;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    DescriptorInvalid,
    SchemaRejected,
    Denied,
    MissingBinding,
    InfrastructureFailure,
    PluginFailed,
}
#[derive(Clone, Debug, PartialEq, Eq)]
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
}
impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}
impl std::error::Error for Failure {}
pub(crate) fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}
