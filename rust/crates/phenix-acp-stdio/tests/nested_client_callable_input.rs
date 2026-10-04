use phenix_acp_stdio::{
    ClientCapabilityCallbacks, ClientPermissionIdentity, SdkApplicationService,
};
use phenix_application_interface::{
    types::{CapabilityInvokeInput, CapabilityInvokeResult},
    InvokeCapability, Operation,
};
use phenix_core::{
    CallableRef, CapabilityGenerationId, CapabilityOwnerId, ClientConnectionId, ContractId, Key,
    ObservableStore, PhenixValue, ReferenceId, ResolvedSdkContributions, PluginRuntimeId,
    SdkContribution, SharedCapabilityRegistry, Type, ValueCodec,
};
use std::collections::BTreeMap;

#[test]
fn capability_invocation_admits_client_callables_nested_in_its_input() {
    let capabilities = SharedCapabilityRegistry::default();
    let (callbacks, _receiver) = ClientCapabilityCallbacks::bounded(1);
    let owner = ClientConnectionId::parse("fixture-client").unwrap();
    let client_generation = CapabilityGenerationId::parse("fixture-client-generation").unwrap();
    let sdk = ResolvedSdkContributions::resolve(&[], &[], Vec::<SdkContribution>::new()).unwrap();
    let service = SdkApplicationService::new(
        &sdk,
        &ObservableStore::default(),
        capabilities.clone(),
        PluginRuntimeId::parse("fixture-sdk-runtime").unwrap(),
        CapabilityGenerationId::parse("fixture-sdk-generation").unwrap(),
        callbacks,
        ClientPermissionIdentity::new(owner.clone(), client_generation.clone()),
    )
    .unwrap();

    let callback = CallableRef::new(
        ContractId::parse("fixture.client-callback@1").unwrap(),
        CapabilityOwnerId::Client(owner),
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
        CapabilityOwnerId::Runtime(PluginRuntimeId::parse("fixture-runtime").unwrap()),
        CapabilityGenerationId::parse("fixture-runtime-generation").unwrap(),
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
            &ContractId::parse(InvokeCapability::ID).unwrap(),
            CapabilityInvokeInput {
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
        CapabilityInvokeResult::from_value(&result).unwrap().output,
        PhenixValue::Unit
    );
    assert_eq!(capabilities.schema(&callback).unwrap(), callback_schema);
}
