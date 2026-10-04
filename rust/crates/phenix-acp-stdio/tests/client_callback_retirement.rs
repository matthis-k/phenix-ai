use phenix_acp_stdio::{
    ClientCapabilityCallbacks, ClientPermissionIdentity, SdkApplicationService,
};
use phenix_application_interface::{
    types::{CapabilityInvokeInput, Empty, SdkValue},
    GetSdk, InvokeCapability, Operation,
};
use phenix_core::{
    CallableRef, CapabilityError, CapabilityGenerationId, CapabilityOwnerId, ClientConnectionId,
    ContractId, Key, ObservableRegistration, ObservableStore, PhenixValue, PluginId,
    PluginManifest, ReferenceId, ResolvedSdkContributions, PluginRuntimeId, SdkContribution,
    SdkNamespace, SdkObservableResource, SdkResourceId, SharedCapabilityRegistry, SnapshotPolicy,
    Type, ValueCodec, ValueId, ValuePath,
};
use std::collections::BTreeMap;

fn key(value: &str) -> Key {
    Key::parse(value).expect("fixture key is valid")
}

fn variant(tag: &str) -> PhenixValue {
    PhenixValue::Variant {
        tag: key(tag),
        value: Box::new(PhenixValue::Unit),
    }
}

#[test]
fn disconnected_client_listener_retires_its_owner_generation() {
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

    let capabilities = SharedCapabilityRegistry::default();
    let (callbacks, callback_receiver) = ClientCapabilityCallbacks::bounded(1);
    drop(callback_receiver);
    let client_owner = ClientConnectionId::parse("fixture-client").expect("client owner is valid");
    let client_generation = CapabilityGenerationId::parse("fixture-client-generation")
        .expect("client generation is valid");
    let service = SdkApplicationService::new(
        &sdk,
        &store,
        capabilities,
        PluginRuntimeId::parse("fixture.runtime").expect("fixture runtime id is valid"),
        CapabilityGenerationId::parse("fixture-runtime-generation")
            .expect("fixture runtime generation is valid"),
        callbacks,
        ClientPermissionIdentity::new(client_owner.clone(), client_generation.clone()),
    )
    .expect("fixture SDK service builds");

    let sdk = service
        .invoke(
            &ContractId::parse(GetSdk::ID).expect("SDK get operation id is valid"),
            Empty {}.to_value(),
        )
        .expect("SDK get succeeds");
    let sdk = SdkValue::from_value(&sdk).expect("SDK value decodes");
    let PhenixValue::Table(namespaces) = sdk.value else {
        panic!("SDK root is a table");
    };
    let PhenixValue::Table(resources) = namespaces.get(&key("fixture")).expect("fixture namespace")
    else {
        panic!("fixture namespace is a table");
    };
    let PhenixValue::Table(state) = resources.get(&key("state")).expect("state resource") else {
        panic!("state resource is a table");
    };
    let PhenixValue::Callable(listen) = state.get(&key("listen")).expect("listen capability")
    else {
        panic!("listen is callable");
    };

    let listener = CallableRef::new(
        ContractId::parse("phenix.observable-delivery@1").expect("listener contract is valid"),
        CapabilityOwnerId::Client(client_owner),
        client_generation,
        ReferenceId::parse("listener").expect("listener reference is valid"),
    );

    let listen_input = PhenixValue::Table(BTreeMap::from([
        (key("initial"), variant("full")),
        (key("listener"), PhenixValue::Callable(listener.clone())),
        (key("mode"), variant("full")),
        (
            key("path"),
            PhenixValue::Table(BTreeMap::from([(
                key("segments"),
                PhenixValue::List(Vec::new()),
            )])),
        ),
        (key("scope"), variant("recursive")),
    ]));

    service
        .invoke(
            &ContractId::parse(InvokeCapability::ID)
                .expect("capability invoke operation id is valid"),
            CapabilityInvokeInput {
                callable: PhenixValue::Callable(listen.clone()),
                input: listen_input,
            }
            .to_value(),
        )
        .expect("listen still returns its stop handle after callback disconnect");

    assert_eq!(
        service.capabilities().schema(&listener),
        Err(CapabilityError::StaleReference(listener))
    );
}
