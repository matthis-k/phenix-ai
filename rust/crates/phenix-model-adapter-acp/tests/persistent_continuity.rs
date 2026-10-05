use phenix_domain::{
    ExecutionId, InferenceOptions, ModelAdapterId, ModelId, ModelProviderId, ModelTarget, SessionId,
};
use phenix_model_adapter::{
    ModelAdapter, ModelAdapterError, ModelAdapterHost, ModelEvent, ModelExecutionRequest,
    ModelSessionRequest, ToolInvocation, ToolProvision, ToolResult,
};
use phenix_model_adapter_acp::{AcpModelAdapter, AcpModelAdapterConfig};

#[derive(Default)]
struct CollectingHost {
    content: String,
}

impl ModelAdapterHost for CollectingHost {
    fn emit(&mut self, event: ModelEvent) -> Result<(), ModelAdapterError> {
        if let ModelEvent::ContentDelta(text) = event {
            self.content.push_str(&text);
        }
        Ok(())
    }

    fn invoke_tool(
        &mut self,
        _invocation: ToolInvocation,
    ) -> Result<ToolResult, ModelAdapterError> {
        Err(ModelAdapterError::Unsupported(
            "continuity fixture does not expose tools".to_owned(),
        ))
    }
}

fn target() -> ModelTarget {
    ModelTarget {
        adapter: ModelAdapterId::parse("fixture-acp").unwrap(),
        provider: ModelProviderId::parse("fixture-provider").unwrap(),
        model: ModelId::parse("fixture-model").unwrap(),
        inference: InferenceOptions::default(),
    }
}

fn session_request(model_adapter: &AcpModelAdapter) -> ModelSessionRequest {
    ModelSessionRequest {
        model: target(),
        tools: ToolProvision::default()
            .prepare(&model_adapter.features())
            .unwrap(),
    }
}

fn execute(
    session: &std::sync::Arc<dyn phenix_model_adapter::ModelSession>,
    execution_id: &str,
    prompt: &str,
) -> String {
    let mut host = CollectingHost::default();
    session
        .execute(
            ModelExecutionRequest {
                execution_id: ExecutionId::parse(execution_id).unwrap(),
                prompt: prompt.to_owned(),
            },
            &mut host,
        )
        .unwrap();
    host.content
}

#[test]
fn stable_phenix_session_reuses_one_native_acp_conversation() {
    let fixture = env!("CARGO_BIN_EXE_acp-continuity-fixture");
    let cwd = std::env::current_dir().unwrap();
    let mut model_adapter = AcpModelAdapter::new(AcpModelAdapterConfig::new(
        ModelAdapterId::parse("fixture-acp").unwrap(),
        ModelProviderId::parse("fixture-provider").unwrap(),
        fixture,
        cwd,
    ));

    let first_id = SessionId::parse("phenix-session-1").unwrap();
    let first = model_adapter
        .open_persistent_session(&first_id, session_request(&model_adapter))
        .unwrap();
    assert_eq!(execute(&first, "execution-1", "first"), "turn:1");

    let first_again = model_adapter
        .open_persistent_session(&first_id, session_request(&model_adapter))
        .unwrap();
    assert!(std::sync::Arc::ptr_eq(&first, &first_again));
    assert_eq!(execute(&first_again, "execution-2", "second"), "turn:2");

    let second_id = SessionId::parse("phenix-session-2").unwrap();
    let second = model_adapter
        .open_persistent_session(&second_id, session_request(&model_adapter))
        .unwrap();
    assert!(!std::sync::Arc::ptr_eq(&first, &second));
    assert_eq!(execute(&second, "execution-3", "isolated"), "turn:1");
}
