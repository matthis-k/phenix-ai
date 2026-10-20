use phenix_sdk::{
    contract, live::Operation, Cardinality, ContractDefinition, ContractMetadata, ImportSlot,
    MetadataExecution, OperationSchema, Portable, ValueSchema,
};

#[derive(Portable)]
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

#[derive(Portable)]
pub struct Request {
    pub cell: phenix_sdk::live::ValueRef<Vec<String>>,
    pub query: Option<String>,
}

#[contract(id = "example.search@2")]
pub trait Search {
    fn r#type(&self, request: Request) -> std::result::Result<Vec<String>, Error>;
}

#[phenix_sdk::plugin(
    id = "example.contracts",
    version = "1.0.0",
    profile = "definitions",
    define(Search),
    define(Counter)
)]
pub mod definitions {
    use super::*;
}

#[phenix_sdk::plugin(
    id = "example.contracts",
    version = "1.0.0",
    profile = "definitions",
    define(Counter),
    define(Search)
)]
pub mod reversed_definitions {
    use super::*;
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
        provider: None,
        slot: "search".into(),
        contract: "example.search@2".into(),
        revision_min: 2,
        revision_max: 2,
        cardinality: Cardinality::Optional,
    });
    assert!(ContractMetadata::decode(&native.encode().unwrap()).is_ok());
}

#[test]
fn definition_only_plugin_emits_independent_canonical_bytes() {
    let mut expected = metadata(Counter::CONTRACT.owned().unwrap());
    expected.definitions.push(Search::CONTRACT.owned().unwrap());
    let bytes = definitions::METADATA.bytes();
    assert_eq!(bytes, expected.encode().unwrap());
    assert_eq!(bytes, reversed_definitions::METADATA.bytes());
    let decoded = ContractMetadata::decode(bytes).unwrap();
    assert_eq!(decoded.execution, MetadataExecution::None);
    assert!(decoded.requires.is_empty());
    assert!(decoded.provides.is_empty());
    assert!(decoded.properties.is_empty());
    assert_eq!(decoded.encode().unwrap(), bytes);
}

#[derive(Debug, Clone, PartialEq, Eq, Portable)]
enum TaggedResult {
    Finished { output: String, tokens: u64 },
    Reprepare,
}

#[test]
fn tagged_variants_are_schema_checked_and_canonical() {
    use phenix_sdk::{StaticSchema, Value};
    let schema = TaggedResult::SCHEMA.owned().unwrap();
    let expected = ValueSchema::Tagged(std::collections::BTreeMap::from([
        (
            "Finished".into(),
            Some(ValueSchema::Struct(std::collections::BTreeMap::from([
                ("output".into(), ValueSchema::String),
                ("tokens".into(), ValueSchema::U64),
            ]))),
        ),
        ("Reprepare".into(), None),
    ]));
    assert_eq!(schema, expected);
    assert_eq!(TaggedResult::SCHEMA.id().unwrap(), expected.id().unwrap());
    assert_eq!(
        TaggedResult::SCHEMA.encode().bytes(),
        expected.encode().unwrap()
    );

    for value in [
        TaggedResult::Reprepare,
        TaggedResult::Finished {
            output: "hello".into(),
            tokens: 9,
        },
    ] {
        let raw = value.to_value();
        assert!(schema.accepts(&raw));
        assert_eq!(TaggedResult::from_value(raw).unwrap(), value);
    }
    for malformed in [
        Value::Map(Default::default()),
        Value::map([
            ("Reprepare", Value::Null),
            ("Finished", Value::Map(Default::default())),
        ]),
        Value::map([("Reprepare", Value::String("non-null".into()))]),
        Value::map([("Unknown", Value::Null)]),
        Value::map([(
            "Finished",
            Value::map([("output", Value::String("missing tokens".into()))]),
        )]),
        Value::map([(
            "Finished",
            Value::map([
                ("output", Value::String("wrong".into())),
                ("tokens", Value::String("nine".into())),
            ]),
        )]),
    ] {
        assert!(!schema.accepts(&malformed));
        assert!(TaggedResult::from_value(malformed).is_err());
    }

    // The static projection remains the sole canonical schema.
    assert!(matches!(TaggedResult::SCHEMA, StaticSchema::Tagged(_)));
}
