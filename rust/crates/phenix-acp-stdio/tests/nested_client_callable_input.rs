use phenix_acp_stdio::{ClientCallableCallbacks, ClientReferenceIdentity, SdkApplicationService};
use phenix_application_interface::{
    InvokeCallableReference, Operation,
    types::{CallableInvocation, CallableInvocationResult},
};
use phenix_core::{
    CallableRef, ClientConnectionId, ContractId, Key, ObservableStore, PhenixValue,
    PluginRuntimeId, ReferenceGenerationId, ReferenceId, ReferenceOwnerId,
    ResolvedSdkContributions, SdkContribution, SharedCallableRegistry, Type, ValueCodec,
};
use std::collections::BTreeMap;

#[test]
fn capability_invocation_admits_client_callables_nested_in_its_input() {
    let capabilities = SharedCallableRegistry::default();
    let (callbacks, _receiver) = ClientCallableCallbacks::bounded(1);
    let owner = ClientConnectionId::parse("fixture-client").unwrap();
    let client_generation = ReferenceGenerationId::parse("fixture-client-generation").unwrap();
    let sdk = ResolvedSdkContributions::resolve(&[], &[], Vec::<SdkContribution>::new()).unwrap();
    let service = SdkApplicationService::new(
        &sdk,
        &ObservableStore::default(),
        capabilities.clone(),
        PluginRuntimeId::parse("fixture-sdk-runtime").unwrap(),
        ReferenceGenerationId::parse("fixture-sdk-generation").unwrap(),
        callbacks,
        ClientReferenceIdentity::new(owner.clone(), client_generation.clone()),
    )
    .unwrap();

    let callback = CallableRef::new(
        ContractId::parse("fixture.client-callback@1").unwrap(),
        ReferenceOwnerId::Client(owner),
        client_generation,
        ReferenceId::parse("callback").unwrap(),
    );
    let callback_schema = Type::Callable {
        contract: callback.contract().clone(),
        input: Box::new(Type::U64),
        output: Box::new(Type::String),
    };
    let outer = CallableRef::new(
        ContractId::parse("fixture.outer-capability@1").unwrap(),
        ReferenceOwnerId::Runtime(PluginRuntimeId::parse("fixture-runtime").unwrap()),
        ReferenceGenerationId::parse("fixture-runtime-generation").unwrap(),
        ReferenceId::parse("outer").unwrap(),
    );
    let callback_key = Key::parse("callback").unwrap();
    capabilities
        .register(
            outer.clone(),
            Type::Callable {
                contract: outer.contract().clone(),
                input: Box::new(Type::Table(BTreeMap::from([(
                    callback_key.clone(),
                    callback_schema.clone(),
                )]))),
                output: Box::new(Type::Unit),
            },
            |_| Ok(PhenixValue::Unit),
        )
        .unwrap();

    let result = service
        .invoke(
            &ContractId::parse(InvokeCallableReference::ID).unwrap(),
            CallableInvocation {
                callable: PhenixValue::Callable(outer),
                input: PhenixValue::Table(BTreeMap::from([(
                    callback_key,
                    PhenixValue::Callable(callback.clone()),
                )])),
            }
            .to_value(),
        )
        .unwrap();
    assert_eq!(
        CallableInvocationResult::from_value(&result)
            .unwrap()
            .output,
        PhenixValue::Unit
    );
    assert_eq!(capabilities.schema(&callback).unwrap(), callback_schema);
}
