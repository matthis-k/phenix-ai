use phenix_domain::{
    CallableDescriptor, CallableFeatureSet, CallableId, CallableKind, CallablePolicy, ExecutionId,
    InferenceOptions, ModelAdapterId, ModelId, ModelProviderId, ModelTarget, PhenixSchema,
};
use phenix_model_adapter::{
    ModelAdapter, ModelAdapterError, ModelAdapterHost, ModelEvent, ModelExecutionRequest,
    ModelSessionRequest, ToolInvocation, ToolProvision, ToolResult,
};
use phenix_model_adapter_acp::{AcpModelAdapter, AcpModelAdapterConfig};
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Default)]
struct ToolHost {
    content: String,
    invocations: Vec<ToolInvocation>,
}

impl ModelAdapterHost for ToolHost {
    fn emit(&mut self, event: ModelEvent) -> Result<(), ModelAdapterError> {
        if let ModelEvent::ContentDelta(text) = event {
            self.content.push_str(&text);
        }
        Ok(())
    }

    fn invoke_tool(&mut self, invocation: ToolInvocation) -> Result<ToolResult, ModelAdapterError> {
        let arguments: serde_json::Value = serde_json::from_str(&invocation.arguments_json)
            .map_err(|error| ModelAdapterError::Protocol(error.to_string()))?;
        let value = arguments
            .get("value")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ModelAdapterError::Protocol("echo value is missing".to_owned()))?;
        let output = format!("echo:{value}");
        self.invocations.push(invocation);
        Ok(ToolResult {
            output,
            success: true,
        })
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

fn callable() -> CallableDescriptor {
    CallableDescriptor {
        id: CallableId::parse("phenix.echo").unwrap(),
        kind: CallableKind::Tool,
        description: "Echo the supplied value".to_owned(),
        input_schema: PhenixSchema::Table(BTreeMap::from([(
            "value".parse().unwrap(),
            PhenixSchema::String,
        )])),
        output_schema: PhenixSchema::String,
        features: CallableFeatureSet::default(),
        policy: CallablePolicy::default(),
    }
}

#[test]
fn real_acp_agent_calls_conductor_tool_and_continues_model_turn() {
    let fixture = env!("CARGO_BIN_EXE_acp-tool-bridge-fixture");
    let cwd = std::env::current_dir().unwrap();
    let mut model_adapter = AcpModelAdapter::new(AcpModelAdapterConfig::new(
        ModelAdapterId::parse("fixture-acp").unwrap(),
        ModelProviderId::parse("fixture-provider").unwrap(),
        fixture,
        cwd,
    ));
    let tools = ToolProvision {
        callables: vec![callable()],
    }
    .prepare(&model_adapter.features())
    .unwrap();
    let session = model_adapter
        .open_session(ModelSessionRequest {
            model: target(),
            tools,
        })
        .unwrap();
    let mut host = ToolHost::default();

    session
        .execute(
            ModelExecutionRequest {
                execution_id: ExecutionId::parse("tool-execution").unwrap(),
                prompt: "use the echo tool".to_owned(),
            },
            &mut host,
        )
        .unwrap();

    assert_eq!(host.invocations.len(), 1);
    assert_eq!(host.invocations[0].callable.as_str(), "phenix.echo");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&host.invocations[0].arguments_json).unwrap(),
        json!({"value": "from-acp"})
    );
    assert_eq!(host.content, "continued:echo:from-acp");
}
