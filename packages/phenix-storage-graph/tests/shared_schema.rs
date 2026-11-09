use phenix_graph::{ContractValue, InputSlot, Value};
use phenix_storage::{Address, Codec, Error, Grant, Operation, Registry, SchemaCodec, Scope};

#[derive(ContractValue)]
#[contract(schema = "shared.item.v1")]
struct Item {
    value: i64,
}

#[derive(ContractValue)]
#[contract(schema = "shared.item.v1")]
struct Independent {
    value: i64,
}

#[derive(ContractValue)]
#[contract(schema = "shared.item.v1")]
struct Incompatible {
    value: String,
}

#[derive(ContractValue)]
#[contract(schema = "shared.outer.v1")]
struct Outer {
    item: Option<Item>,
}

#[derive(ContractValue)]
#[contract(schema = "shared.outer.v1")]
struct OtherOuter {
    item: Option<Incompatible>,
}

struct Adapter<T>(std::marker::PhantomData<T>);
impl<T: ContractValue> Codec for Adapter<T> {
    type Value = T;
    fn schema(&self) -> phenix_graph::Schema {
        T::schema()
    }
    fn encode(&self, value: &T) -> phenix_storage::Result<Vec<u8>> {
        Ok(value.encode())
    }
    fn decode(&self, bytes: &[u8]) -> phenix_storage::Result<T> {
        T::decode(bytes).map_err(Error::Codec)
    }
}

#[test]
fn structural_identity_is_used_by_graph_slots_and_values() {
    assert_eq!(Item::schema(), Independent::schema());
    assert_eq!(Item::wire_schema(), Independent::wire_schema());
    assert_ne!(Item::wire_schema(), Incompatible::wire_schema());
    assert_ne!(Outer::wire_schema(), OtherOuter::wire_schema());
    assert_eq!(InputSlot::immediate::<Item>(0).schema, Item::wire_schema());
    let value = Value::of(Item { value: 42 });
    assert_eq!(value.get::<Independent>().unwrap().value, 42);
    assert!(value.get::<Incompatible>().is_err());
    // Primitive envelopes use the same complete descriptor as their typed slots.
    assert_eq!(*Value::of(42i64).schema(), i64::wire_schema());
}

#[test]
fn storage_admits_the_same_descriptor_before_decoding() {
    let registry = Registry::new();
    registry.mount_memory("main").unwrap();
    let address = Address::parse("store://main/items").unwrap();
    registry.register(&address, Item::schema()).unwrap();
    let session = registry.session([Grant::new(
        Scope::Dataset(address.clone()),
        [Operation::Read, Operation::Write],
    )]);
    let writer = SchemaCodec {
        codec: Adapter::<Item>(std::marker::PhantomData),
        schema: Item::schema(),
    };
    let reader = Adapter::<Independent>(std::marker::PhantomData);
    session
        .put(&address, "one", &Item { value: 7 }, &writer)
        .unwrap();
    assert_eq!(
        session
            .get(&address, "one", &reader)
            .unwrap()
            .unwrap()
            .value,
        7
    );

    struct NeverDecode;
    impl Codec for NeverDecode {
        type Value = ();
        fn schema(&self) -> phenix_graph::Schema {
            Incompatible::schema()
        }
        fn encode(&self, _: &()) -> phenix_storage::Result<Vec<u8>> {
            panic!("must reject before encode")
        }
        fn decode(&self, _: &[u8]) -> phenix_storage::Result<()> {
            panic!("must reject before decode")
        }
    }
    assert!(matches!(
        session.get(&address, "one", &NeverDecode),
        Err(Error::SchemaMismatch)
    ));
    assert!(matches!(
        session.put(&address, "one", &(), &NeverDecode),
        Err(Error::SchemaMismatch)
    ));
}
