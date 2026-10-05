use phenix_acp_stdio::{ClientCallableCallbacks, ClientReferenceIdentity, SdkApplicationService};
use phenix_application_interface::{
    AddClientTool, InvokeCallable, Operation,
    types::{
        CallableInvokeInput, CallableResult, ClientToolAddInput, ClientToolAdmission,
        ClientToolDefinition,
    },
};
use phenix_core::{
    CallableError, CallableRef, ClientConnectionId, ContractId, ObservableStore, PhenixValue,
    PluginRuntimeId, ReferenceGenerationId, ReferenceId, ReferenceOwnerId,
    ResolvedSdkContributions, SdkContribution, SharedCallableRegistry, Type, ValueCodec,
};

fn client_callable(
    owner: &ClientConnectionId,
    generation: &ReferenceGenerationId,
    reference: &str,
) -> CallableRef {
    CallableRef::new(
        ContractId::parse("fixture.client-tool@1").unwrap(),
        ReferenceOwnerId::Client(owner.clone()),
        generation.clone(),
        ReferenceId::parse(reference).unwrap(),
    )
}

fn service(
    capabilities: SharedCallableRegistry,
    callbacks: ClientCallableCallbacks,
    owner: ClientConnectionId,
    generation: ReferenceGenerationId,
) -> SdkApplicationService {
    let sdk = ResolvedSdkContributions::resolve(&[], &[], Vec::<SdkContribution>::new()).unwrap();
    SdkApplicationService::new(
        &sdk,
        &ObservableStore::default(),
        capabilities,
        PluginRuntimeId::parse("fixture.runtime").unwrap(),
        ReferenceGenerationId::parse("fixture.runtime-generation").unwrap(),
        callbacks,
        ClientReferenceIdentity::new(owner, generation),
    )
    .unwrap()
}

fn definition(invoke: CallableRef) -> ClientToolDefinition {
    ClientToolDefinition {
        id: phenix_core::CallableId::parse("fixture.client.echo").unwrap(),
        description: "Echo a client value".to_owned(),
        input: Type::U64,
        output: Type::String,
        capabilities: Vec::new(),
        requires_permission: false,
        invoke: PhenixValue::Callable(invoke),
    }
}

#[tokio::test]
async fn reconnect_requires_a_new_callable_generation_and_allows_readmission() {
    let capabilities = SharedCallableRegistry::default();
    let owner = ClientConnectionId::parse("fixture-client").unwrap();
    let first_generation = ReferenceGenerationId::parse("fixture-generation-1").unwrap();
    let first_callable = client_callable(&owner, &first_generation, "echo-1");
    let session = phenix_core::SessionId::parse("session-a").unwrap();

    let (first_callbacks, first_receiver) = ClientCallableCallbacks::bounded(1);
    let first = service(
        capabilities.clone(),
        first_callbacks,
        owner.clone(),
        first_generation,
    );
    let first_admission = first
        .invoke(
            &ContractId::parse(AddClientTool::ID).unwrap(),
            ClientToolAddInput {
                session_id: session.clone(),
                tool: definition(first_callable.clone()),
            }
            .to_value(),
        )
        .unwrap();
    let first_admission = ClientToolAdmission::from_value(&first_admission).unwrap();
    assert_eq!(first_admission.callable_id.as_str(), "fixture.client.echo");
    drop(first_receiver);

    first.retire_client();
    assert_eq!(
        capabilities.schema(&first_callable),
        Err(CallableError::StaleReference(first_callable.clone()))
    );

    let second_generation = ReferenceGenerationId::parse("fixture-generation-2").unwrap();
    let second_callable = client_callable(&owner, &second_generation, "echo-2");
    let (second_callbacks, mut second_receiver) = ClientCallableCallbacks::bounded(1);
    let second = service(capabilities, second_callbacks, owner, second_generation);

    assert!(second.client_tool_descriptors(&session).is_empty());
    let second_admission = second
        .invoke(
            &ContractId::parse(AddClientTool::ID).unwrap(),
            ClientToolAddInput {
                session_id: session.clone(),
                tool: definition(second_callable.clone()),
            }
            .to_value(),
        )
        .unwrap();
    let second_admission = ClientToolAdmission::from_value(&second_admission).unwrap();
    assert_eq!(second_admission.callable_id.as_str(), "fixture.client.echo");
    assert_ne!(second_admission.admission_id, first_admission.admission_id);

    let second_for_call = second.clone();
    let session_for_call = session.clone();
    let worker = tokio::task::spawn_blocking(move || {
        second_for_call.invoke(
            &ContractId::parse(InvokeCallable::ID).unwrap(),
            CallableInvokeInput {
                session_id: session_for_call,
                callable_id: phenix_core::CallableId::parse("fixture.client.echo").unwrap(),
                input: PhenixValue::U64(7),
            }
            .to_value(),
        )
    });

    let invocation = second_receiver.recv().await.unwrap();
    assert_eq!(
        invocation.request().callable,
        PhenixValue::Callable(second_callable)
    );
    assert_eq!(invocation.request().input, PhenixValue::U64(7));
    invocation.respond(Ok(
        phenix_application_interface::types::CallableInvocationResult {
            output: PhenixValue::String("ok".to_owned()),
        },
    ));

    let result = worker.await.unwrap().unwrap();
    assert_eq!(
        CallableResult::from_value(&result).unwrap().output,
        PhenixValue::String("ok".to_owned())
    );
}
