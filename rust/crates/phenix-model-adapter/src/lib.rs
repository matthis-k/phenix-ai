#![forbid(unsafe_code)]

use phenix_domain::{
    AuthenticationInput, AuthenticationMethodId, CallableDescriptor, CallableId, ExecutionId,
    ModelAdapterCatalog, ModelTarget, SessionId,
};
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::sync::Arc;

/// Concrete representation used to materialize runtime-owned callables for a
/// adapter session. This is intentionally distinct from callable semantics:
/// the same `ToolProvision` may be represented natively, through MCP, or by an
/// ACP extension without changing the callable contract itself.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum ToolPresentation {
    Native,
    McpStdio,
    AcpExtension,
}

const TOOL_PRESENTATION_PREFERENCE: [ToolPresentation; 3] = [
    ToolPresentation::Native,
    ToolPresentation::AcpExtension,
    ToolPresentation::McpStdio,
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelAdapterFeatures {
    /// Every representation this adapter can use for runtime-owned callables.
    /// The runtime selects one deterministic presentation per session.
    pub tool_presentations: BTreeSet<ToolPresentation>,
    pub images: bool,
    pub persistent_sessions: bool,
}

impl ModelAdapterFeatures {
    #[must_use]
    pub fn preferred_tool_presentation(&self) -> Option<ToolPresentation> {
        TOOL_PRESENTATION_PREFERENCE
            .into_iter()
            .find(|presentation| self.tool_presentations.contains(presentation))
    }
}

/// Semantic runtime-owned callable provision before adapter presentation is
/// selected.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolProvision {
    pub callables: Vec<CallableDescriptor>,
}

/// A `ToolProvision` after adapter capability negotiation. Construction is
/// private so an empty surface cannot claim a presentation and a populated
/// surface cannot bypass runtime-owned negotiation.
#[derive(Clone, Debug, PartialEq)]
pub enum PreparedToolSurface {
    Empty,
    Hosted {
        presentation: ToolPresentation,
        callables: Vec<CallableDescriptor>,
    },
}

impl PreparedToolSurface {
    #[must_use]
    pub fn presentation(&self) -> Option<ToolPresentation> {
        match self {
            Self::Empty => None,
            Self::Hosted { presentation, .. } => Some(*presentation),
        }
    }

    #[must_use]
    pub fn callables(&self) -> &[CallableDescriptor] {
        match self {
            Self::Empty => &[],
            Self::Hosted { callables, .. } => callables,
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }
}

impl ToolProvision {
    pub fn prepare(
        self,
        features: &ModelAdapterFeatures,
    ) -> Result<PreparedToolSurface, ModelAdapterError> {
        if self.callables.is_empty() {
            return Ok(PreparedToolSurface::Empty);
        }
        let presentation = features.preferred_tool_presentation().ok_or_else(|| {
            ModelAdapterError::Unsupported(
                "adapter cannot host runtime-provisioned tools".to_owned(),
            )
        })?;
        Ok(PreparedToolSurface::Hosted {
            presentation,
            callables: self.callables,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelSessionRequest {
    pub model: ModelTarget,
    pub tools: PreparedToolSurface,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelExecutionRequest {
    pub execution_id: ExecutionId,
    pub prompt: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelEvent {
    ContentDelta(String),
    ReasoningDelta(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolInvocation {
    pub callable: CallableId,
    pub arguments_json: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolResult {
    pub output: String,
    pub success: bool,
}

pub trait ModelAdapterHost {
    fn emit(&mut self, event: ModelEvent) -> Result<(), ModelAdapterError>;
    fn invoke_tool(&mut self, invocation: ToolInvocation) -> Result<ToolResult, ModelAdapterError>;
}

/// A materialized adapter session may be executing on the runtime execution
/// worker while a frontend request concurrently asks it to cancel. Implementors
/// therefore expose thread-safe shared methods rather than requiring exclusive
/// ownership for the lifetime of a model turn.
pub trait ModelSession: Send + Sync {
    fn execute(
        &self,
        request: ModelExecutionRequest,
        host: &mut dyn ModelAdapterHost,
    ) -> Result<(), ModelAdapterError>;
    fn cancel(&self, execution_id: &ExecutionId) -> Result<(), ModelAdapterError>;
}

pub trait ModelAdapter: Send {
    fn features(&self) -> ModelAdapterFeatures;

    fn catalog(&mut self) -> Result<ModelAdapterCatalog, ModelAdapterError> {
        Err(ModelAdapterError::Unsupported(
            "adapter does not provide model/auth discovery".to_owned(),
        ))
    }

    fn authenticate(&mut self, _method: &AuthenticationMethodId) -> Result<(), ModelAdapterError> {
        Err(ModelAdapterError::Unsupported(
            "adapter does not provide authentication actions".to_owned(),
        ))
    }

    fn authenticate_with_input(
        &mut self,
        method: &AuthenticationMethodId,
        input: Option<&AuthenticationInput>,
    ) -> Result<(), ModelAdapterError> {
        if input.is_some() {
            return Err(ModelAdapterError::Unsupported(
                "adapter authentication method does not accept structured input".to_owned(),
            ));
        }
        self.authenticate(method)
    }

    /// Materialize an execution-local adapter session. Adapters without native
    /// conversation persistence may create a fresh session for every call.
    fn open_session(
        &mut self,
        request: ModelSessionRequest,
    ) -> Result<Arc<dyn ModelSession>, ModelAdapterError>;

    /// Open or reuse the native conversation associated with one stable Phenix
    /// session. The runtime calls this only for a fixed target when the
    /// adapter advertises `persistent_sessions`.
    ///
    /// An adapter must not advertise that feature without implementing this
    /// method: silently falling back to `open_session` would turn a multi-turn
    /// conversation into unrelated adapter turns while claiming continuity.
    fn open_persistent_session(
        &mut self,
        _session_id: &SessionId,
        _request: ModelSessionRequest,
    ) -> Result<Arc<dyn ModelSession>, ModelAdapterError> {
        Err(ModelAdapterError::Unsupported(
            "adapter advertises persistent sessions but does not implement stable session opening"
                .to_owned(),
        ))
    }

    /// Dispose any persistent native conversation associated with one stable
    /// Phenix session. This operation is deliberately idempotent so the
    /// runtime can fan a terminal session close out to every registered
    /// adapter without tracking which fixed targets the session previously
    /// touched.
    fn close_persistent_session(
        &mut self,
        _session_id: &SessionId,
    ) -> Result<(), ModelAdapterError> {
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelAdapterError {
    Unsupported(String),
    Transport(String),
    Protocol(String),
    ContextOverflow(String),
}

impl Display for ModelAdapterError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(v) => write!(f, "unsupported adapter capability: {v}"),
            Self::Transport(v) => write!(f, "adapter transport error: {v}"),
            Self::Protocol(v) => write!(f, "adapter protocol error: {v}"),
            Self::ContextOverflow(v) => write!(f, "adapter context overflow: {v}"),
        }
    }
}
impl Error for ModelAdapterError {}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_domain::{InferenceOptions, ModelAdapterId, ModelId, ModelProviderId};

    fn features(presentations: impl IntoIterator<Item = ToolPresentation>) -> ModelAdapterFeatures {
        ModelAdapterFeatures {
            tool_presentations: presentations.into_iter().collect(),
            images: false,
            persistent_sessions: false,
        }
    }

    fn model() -> ModelTarget {
        ModelTarget {
            adapter: ModelAdapterId::parse("mock").unwrap(),
            provider: ModelProviderId::parse("mock-provider").unwrap(),
            model: ModelId::parse("mock-model").unwrap(),
            inference: InferenceOptions::default(),
        }
    }

    struct FeatureOnlyPersistentModelAdapter;

    impl ModelAdapter for FeatureOnlyPersistentModelAdapter {
        fn features(&self) -> ModelAdapterFeatures {
            ModelAdapterFeatures {
                tool_presentations: BTreeSet::new(),
                images: false,
                persistent_sessions: true,
            }
        }

        fn open_session(
            &mut self,
            _request: ModelSessionRequest,
        ) -> Result<Arc<dyn ModelSession>, ModelAdapterError> {
            Err(ModelAdapterError::Protocol(
                "ephemeral opening should not satisfy persistent contract".to_owned(),
            ))
        }
    }

    #[test]
    fn empty_tool_provision_needs_no_presentation() {
        let surface = ToolProvision::default().prepare(&features([])).unwrap();
        assert_eq!(surface.presentation(), None);
        assert!(surface.is_empty());
        assert!(surface.callables().is_empty());
    }

    #[test]
    fn model_adapter_can_advertise_multiple_presentations_with_deterministic_preference() {
        let supported = features([
            ToolPresentation::McpStdio,
            ToolPresentation::AcpExtension,
            ToolPresentation::Native,
        ]);
        assert_eq!(
            supported.preferred_tool_presentation(),
            Some(ToolPresentation::Native)
        );
        assert_eq!(features([]).preferred_tool_presentation(), None);
    }

    #[test]
    fn persistent_capability_does_not_silently_fall_back_to_ephemeral_opening() {
        let mut adapter = FeatureOnlyPersistentModelAdapter;
        let request = ModelSessionRequest {
            model: model(),
            tools: ToolProvision::default()
                .prepare(&adapter.features())
                .unwrap(),
        };
        let error = match adapter
            .open_persistent_session(&SessionId::parse("session-1").unwrap(), request)
        {
            Ok(_) => panic!("persistent opening must require an implementation"),
            Err(error) => error,
        };
        assert!(matches!(error, ModelAdapterError::Unsupported(_)));
    }

    #[test]
    fn persistent_close_is_idempotent_by_default() {
        let mut adapter = FeatureOnlyPersistentModelAdapter;
        let session = SessionId::parse("session-1").unwrap();
        adapter.close_persistent_session(&session).unwrap();
        adapter.close_persistent_session(&session).unwrap();
    }
}
