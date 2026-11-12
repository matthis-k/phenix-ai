//! Plugin lifecycle errors. Not graph execution errors.
use std::{fmt, sync::Arc};

/// An owned callback cause, retaining its concrete type for source inspection.
pub type CallbackError = Box<dyn std::error::Error + Send + Sync + 'static>;

// Field order keeps callback methods and destructors executable until the cause drops.
struct LeasedCallback {
    source: CallbackError,
    _lease: Arc<dyn Send + Sync>,
}
impl fmt::Debug for LeasedCallback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LeasedCallback")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}
impl fmt::Display for LeasedCallback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(f)
    }
}
impl std::error::Error for LeasedCallback {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

/// Retain native code/resources through every method and destructor of an escaping
/// callback cause. The original concrete error remains in the source chain.
pub fn lease_callback_error(source: CallbackError, lease: Arc<dyn Send + Sync>) -> CallbackError {
    Box::new(LeasedCallback {
        source,
        _lease: lease,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallbackPhase {
    Construct,
    Contribute,
    Bind,
}

#[derive(Debug)]
pub struct CallbackContext {
    pub plugin: String,
    pub phase: CallbackPhase,
    pub contribution: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    DescriptorInvalid,
    SchemaRejected,
    Denied,
    MissingBinding,
    InfrastructureFailure,
    PluginFailed,
}
#[derive(Debug)]
pub struct Failure {
    pub code: Code,
    pub message: String,
    pub context: Option<CallbackContext>,
    source: Option<CallbackError>,
}
impl Failure {
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            context: None,
            source: None,
        }
    }

    pub fn with_source(code: Code, message: impl Into<String>, source: CallbackError) -> Self {
        Self {
            source: Some(source),
            ..Self::new(code, message)
        }
    }

    pub(crate) fn at(
        self,
        plugin: &str,
        phase: CallbackPhase,
        lease: Arc<dyn Send + Sync>,
    ) -> Self {
        let mut error = Self::with_source(
            self.code,
            self.message.clone(),
            lease_callback_error(Box::new(self), lease),
        );
        error.context = Some(CallbackContext {
            plugin: plugin.into(),
            phase,
            contribution: None,
        });
        error
    }
}
impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(context) = &self.context {
            write!(f, "plugin {:?} {:?}: ", context.plugin, context.phase)?;
        }
        write!(f, "{:?}: {}", self.code, self.message)
    }
}
impl std::error::Error for Failure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_deref().map(|source| source as _)
    }
}
pub(crate) fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}
