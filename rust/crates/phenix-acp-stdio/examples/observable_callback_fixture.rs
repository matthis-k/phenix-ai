use phenix_acp_stdio::{
    execute_admitted_client_tool_call, model_tool_surface, serve_sdk_application,
    serve_stdio_with_events_and_callbacks, ChannelTransport, ClientCallableCallbacks,
    ClientReferenceIdentity, SdkApplicationService,
};
use phenix_application_interface::types::ExecutionChange;
use phenix_core::{
    CallableId, ClientConnectionId, ContractId, ModelToolCall, ModelToolDescriptor,
    ObservableRegistration, ObservableStore, PhenixValue, PluginId, PluginManifest,
    PluginRuntimeId, ReferenceGenerationId, ResolvedSdkContributions, SdkContribution,
    SdkNamespace, SdkObservableResource, SdkResourceId, SessionId, SharedCallableRegistry,
    SnapshotPolicy, Type, ValueId, ValuePath,
};
use std::time::Duration;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    let provider = PluginId::parse("fixture").expect("fixture provider id is valid");
    let value_id = ValueId::parse("fixture.state@1").expect("fixture value id is valid");
    let store = ObservableStore::default();
    store
        .register(ObservableRegistration {
            id: value_id.clone(),
            owner: provider.clone(),
            schema: Type::U64,
            snapshot_policy: SnapshotPolicy::CopyOnChange,
            initial: PhenixValue::U64(41),
        })
        .expect("fixture observable registers");

    let mut contribution = SdkContribution::new(
        provider.clone(),
        SdkNamespace::parse("fixture").expect("fixture namespace is valid"),
    );
    contribution.insert_observable(SdkObservableResource::new(
        SdkResourceId::parse("sdk/fixture/state").expect("fixture resource id is valid"),
        ["state"],
        value_id,
        ValuePath::root(),
        Type::U64,
    ));
    let sdk = ResolvedSdkContributions::resolve(
        &[PluginManifest::resource_only(provider)],
        &[],
        [contribution],
    )
    .expect("fixture SDK resolves");

    let capabilities = SharedCallableRegistry::default();
    let (client_callbacks, callback_receiver) = ClientCallableCallbacks::bounded(8);
    let service = SdkApplicationService::new(
        &sdk,
        &store,
        capabilities,
        PluginRuntimeId::parse("fixture.runtime").expect("fixture runtime id is valid"),
        ReferenceGenerationId::parse("fixture-generation").expect("fixture generation id is valid"),
        client_callbacks,
        ClientReferenceIdentity::new(
            ClientConnectionId::parse("lua-client-1").expect("fixture client id is valid"),
            ReferenceGenerationId::parse("connection-1")
                .expect("fixture client generation is valid"),
        ),
    )
    .expect("fixture SDK service builds");

    let tool_service = service.clone();
    let tool_roundtrip = tokio::spawn(async move {
        let session_id = SessionId::parse("fixture-session").expect("fixture session id is valid");
        let runtime_tool = ModelToolDescriptor {
            id: CallableId::parse("fixture.runtime.inspect")
                .expect("fixture runtime tool id is valid"),
            description: "Inspect the runtime fixture".to_owned(),
            input_schema: Type::Unit,
            output_schema: Type::Unit,
        };

        let mut tools = None;
        for _ in 0..1000 {
            let surface = model_tool_surface(&tool_service, &session_id, [runtime_tool.clone()])
                .expect("model tool surface builds");
            if surface
                .iter()
                .any(|tool| tool.id.as_str() == "fixture.client.echo")
            {
                tools = Some(surface);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let tools = tools.expect("client tool becomes model-visible");
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].id.as_str(), "fixture.client.echo");
        assert_eq!(tools[1].id.as_str(), "fixture.runtime.inspect");

        let service = tool_service.clone();
        let change = tokio::task::spawn_blocking(move || {
            execute_admitted_client_tool_call(
                &service,
                &session_id,
                "fixture-execution",
                ModelToolCall {
                    call_id: "fixture-model-call".to_owned(),
                    callable_id: CallableId::parse("fixture.client.echo")
                        .expect("fixture client tool id is valid"),
                    input: PhenixValue::String("hello".to_owned()),
                },
                |_| panic!("permission callback must not run for a permission-free fixture tool"),
            )
        })
        .await
        .expect("model tool worker joins");
        assert_eq!(
            change,
            ExecutionChange::ToolResult {
                call_id: "fixture-model-call".to_owned(),
                output: PhenixValue::String("hello from Lua".to_owned()),
            }
        );
    });

    let (transport, application_receiver) = ChannelTransport::new(8);
    let application = tokio::spawn(serve_sdk_application(service, application_receiver));
    let (_event_sender, event_receiver) = mpsc::channel(1);

    serve_stdio_with_events_and_callbacks(
        transport,
        [
            "discovery",
            "sessions",
            "prompt",
            "sdk",
            "observables",
            "capabilities",
            "callables",
            "client-tools",
        ]
        .map(|name| {
            ContractId::parse(format!("phenix.application.capability.{name}@1"))
                .expect("fixture capability id is valid")
        }),
        event_receiver,
        callback_receiver,
    )
    .await
    .expect("fixture ACP server exits cleanly");

    tool_roundtrip
        .await
        .expect("model-to-client tool roundtrip completes");
    application.abort();
}
