use phenix_sdk::{
    contract, live::Operation, Cardinality, ContractDefinition, ContractMetadata, ImportSlot,
    LivePortable, MetadataExecution, OperationSchema, ValueSchema,
};

#[derive(LivePortable)]
pub enum Error {
    Missing,
}

#[contract(id = "example.counter@1")]
pub trait Counter {
    fn write(&mut self, value: u64) -> Result<u64, Error>;
    fn read(&self) -> Result<u64, Error>;
}

#[contract(id = "example.counter@1")]
pub trait Reversed {
    fn read(&self) -> Result<u64, Error>;
    fn write(&mut self, value: u64) -> Result<u64, Error>;
}

#[derive(LivePortable)]
pub struct Request {
    pub cell: phenix_sdk::live::ValueRef<Vec<String>>,
    pub query: Option<String>,
}

#[contract(id = "example.search@2")]
pub trait Search {
    fn r#type(&self, request: Request) -> std::result::Result<Vec<String>, Error>;
}

fn metadata(definition: ContractDefinition) -> ContractMetadata {
    ContractMetadata {
        plugin: "example.contracts".into(),
        version: "1.0.0".into(),
        execution: MetadataExecution::None,
        definitions: vec![definition],
        requires: vec![],
        provides: vec![],
        properties: vec![],
    }
}

#[test]
fn declarations_match_independent_owned_metadata() {
    let expected = ContractDefinition {
        contract: "example.counter@1".into(),
        revision: 1,
        operations: vec![
            OperationSchema {
                name: "read".into(),
                request: ValueSchema::Struct(Default::default()),
                response: ValueSchema::U64,
                error: ValueSchema::Variants(vec!["Missing".into()]),
            },
            OperationSchema {
                name: "write".into(),
                request: ValueSchema::U64,
                response: ValueSchema::U64,
                error: ValueSchema::Variants(vec!["Missing".into()]),
            },
        ],
    };
    let generated = Counter::CONTRACT.owned().unwrap();
    assert_eq!(generated, expected);
    let bytes = metadata(generated).encode().unwrap();
    assert_eq!(bytes, metadata(expected).encode().unwrap());
    assert_eq!(
        bytes,
        metadata(Reversed::CONTRACT.owned().unwrap())
            .encode()
            .unwrap()
    );
    assert_eq!(
        ContractMetadata::decode(&bytes).unwrap().encode().unwrap(),
        bytes
    );
    assert_eq!(Counter::read::KEY, "example.counter@1/read");
    assert_eq!(Counter::read::DEFINITION.key, "example.counter@1/read");
    assert_eq!(Counter::write::DEFINITION.key, "example.counter@1/write");
}

#[test]
fn external_types_and_raw_identifiers_have_portable_identity() {
    let definition = Search::CONTRACT.owned().unwrap();
    assert_eq!(definition.revision, 2);
    assert_eq!(definition.operations[0].name, "type");
    assert_eq!(Search::r#type::DEFINITION.key, "example.search@2/type");
    assert_eq!(
        definition.operations[0].request,
        ValueSchema::Struct(
            [
                (
                    "cell".into(),
                    ValueSchema::Ref(
                        ValueSchema::List(Box::new(ValueSchema::String))
                            .id()
                            .unwrap()
                    ),
                ),
                (
                    "query".into(),
                    ValueSchema::Option(Box::new(ValueSchema::String)),
                ),
            ]
            .into()
        )
    );
    let mut native = metadata(definition);
    native.execution = MetadataExecution::StatePending;
    native.requires.push(ImportSlot {
        slot: "search".into(),
        contract: "example.search@2".into(),
        revision_min: 2,
        revision_max: 2,
        cardinality: Cardinality::Optional,
    });
    assert!(ContractMetadata::decode(&native.encode().unwrap()).is_ok());
}
