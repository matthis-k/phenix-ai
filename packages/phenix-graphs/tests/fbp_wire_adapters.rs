use phenix_graph::{
    ContractValue, Failure, Interface, PortDef, Schema, Value,
    component::{Body, ComponentDef},
    connection::ConnectionError,
    contract::Contract,
    interface::{InterfaceReason, PortableSchemaField},
    network::NetworkBuilder,
    process::{Process, ProcessContext, Step},
};
use std::{
    collections::BTreeMap,
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Revision1(i64);
#[derive(Clone, Debug, PartialEq, Eq)]
struct Revision2(i64);

impl ContractValue for Revision1 {
    fn schema() -> Schema {
        Schema {
            name: "test.counter".into(),
            revision: 1,
            structure: "counter-v1:i64".into(),
            codec: "i64-le-v1".into(),
        }
    }
    fn encode(&self) -> Vec<u8> {
        self.0.encode()
    }
    fn decode(bytes: &[u8]) -> Result<Self, Failure> {
        i64::decode(bytes).map(Self)
    }
}
impl ContractValue for Revision2 {
    fn schema() -> Schema {
        Schema {
            name: "test.counter".into(),
            revision: 2,
            structure: "counter-v2:i64".into(),
            codec: "i64-le-v2".into(),
        }
    }
    fn encode(&self) -> Vec<u8> {
        self.0.encode()
    }
    fn decode(bytes: &[u8]) -> Result<Self, Failure> {
        i64::decode(bytes).map(Self)
    }
}
fn cap() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}
fn adapter() -> ComponentDef {
    ComponentDef::adapt_wire::<Revision1, Revision2, _>(|old| Ok(Revision2(old.0 + 1))).unwrap()
}
fn spawn(definition: &ComponentDef) -> Box<dyn Process> {
    let Body::Primitive(factory) = definition.body() else {
        panic!("expected primitive")
    };
    factory.spawn()
}

#[test]
fn schema_revisions_remain_strict_but_explicit_adapter_connects_them() {
    let v1 = PortDef::wire::<Revision1>("input").unwrap();
    let v2 = PortDef::wire::<Revision2>("input").unwrap();
    assert_ne!(v1.value, v2.value);
    let expected = Interface::new([v1.clone()], []).unwrap();
    let newer = Interface::new([v2], []).unwrap();
    assert_eq!(
        *expected.check_shape(&newer).unwrap_err().reason,
        InterfaceReason::IncompatibleType
    );

    let definition = adapter();
    let contract = Contract::new("test/v1-to-v2", definition.interface().clone()).unwrap();
    let claimed = definition.claim(&contract).unwrap();
    contract.check_implementation(&claimed).unwrap();

    let mut builder = NetworkBuilder::new();
    let bridge = builder.add("migration", Arc::new(claimed)).unwrap();
    assert!(bridge.wire_input::<Revision2>("input").is_err());
    assert!(bridge.wire_output::<Revision1>("output").is_err());
    builder
        .expose_input(
            "old",
            bridge.wire_input::<Revision1>("input").unwrap(),
            cap(),
        )
        .unwrap();
    builder
        .expose_output("new", bridge.wire_output::<Revision2>("output").unwrap())
        .unwrap();
    let compiled = builder.build().unwrap().compile().unwrap();
    compiled
        .check_implementation("migration", &contract)
        .unwrap();
    let mut session = compiled.start();

    assert_eq!(
        session
            .inject("old", Value::of(Revision2(9)))
            .unwrap_err()
            .reason,
        ConnectionError::WrongType
    );
    session.inject("old", Value::of(Revision1(9))).unwrap();
    session.run_until_idle(32);
    let output = session.take_output("new").unwrap().unwrap();
    assert_eq!(
        *output.value().decoded::<Revision2>().unwrap(),
        Revision2(10)
    );
    assert_eq!(
        output.value().identity(),
        PortDef::wire::<Revision2>("output").unwrap().value
    );
    assert!(session.take_output("new").unwrap().is_none());
}

#[test]
fn output_retains_input_lease_while_conversion_borrows_decoded_wire_value() {
    struct DropLease(Arc<AtomicUsize>);
    impl Drop for DropLease {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let counter = Arc::new(AtomicUsize::new(0));
    let lease: Arc<dyn Send + Sync> = Arc::new(DropLease(counter.clone()));
    let mut process = spawn(&adapter());
    let mut context = ProcessContext::new(
        BTreeMap::from([(
            "input".into(),
            Value::of(Revision1(42)).with_lease(lease.clone()),
        )]),
        vec![],
    );
    let Step::Emit(output) = process.step(&mut context).unwrap() else {
        panic!("expected migrated output");
    };
    assert_eq!(*output.value.decoded::<Revision2>().unwrap(), Revision2(43));
    drop(context);
    drop(lease);
    assert_eq!(counter.load(Ordering::SeqCst), 0);
    drop(output);
    assert_eq!(counter.load(Ordering::SeqCst), 1);
    assert!(matches!(
        process.step(&mut ProcessContext::default()).unwrap(),
        Step::Complete
    ));
    assert!(matches!(
        process
            .step(&mut ProcessContext::new(
                BTreeMap::new(),
                vec!["input".into()]
            ))
            .unwrap(),
        Step::Finished
    ));
}

#[test]
fn conversion_error_and_panic_are_failures_not_forged_migration_output() {
    let failing = ComponentDef::adapt_wire::<Revision1, Revision2, _>(|_| {
        Err(Failure::external(
            "unsupported schema migration",
            std::io::Error::other("explicit conversion declined"),
        ))
    })
    .unwrap();
    let mut process = spawn(&failing);
    let mut context = ProcessContext::new(
        BTreeMap::from([("input".into(), Value::of(Revision1(10)))]),
        vec![],
    );
    assert!(process.step(&mut context).is_err());

    let panicking = ComponentDef::adapt_wire::<Revision1, Revision2, _>(|_| {
        panic!("migration callback panicked");
    })
    .unwrap();
    let mut process = spawn(&panicking);
    let mut context = ProcessContext::new(
        BTreeMap::from([("input".into(), Value::of(Revision1(10)))]),
        vec![],
    );
    assert!(process.step(&mut context).is_err());
}

#[test]
fn invalid_wire_schema_rejects_adapter_construction() {
    struct Invalid;
    impl ContractValue for Invalid {
        fn schema() -> Schema {
            Schema {
                name: "".into(),
                revision: 1,
                structure: "invalid".into(),
                codec: "invalid".into(),
            }
        }
        fn encode(&self) -> Vec<u8> {
            Vec::new()
        }
        fn decode(_: &[u8]) -> Result<Self, Failure> {
            Ok(Self)
        }
    }
    let bad = ComponentDef::adapt_wire::<Invalid, Revision2, _>(|_| Ok(Revision2(0)));
    assert!(matches!(
        *bad.unwrap_err().reason,
        InterfaceReason::InvalidPortableSchema {
            field: PortableSchemaField::Name,
            ..
        }
    ));
}
