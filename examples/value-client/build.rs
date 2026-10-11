use phenix_sdk::*;
use std::{env, fs, path::PathBuf};
fn refs() -> ContractDefinition {
    ContractDefinition {
        contract: "test.refs@1".into(),
        revision: 1,
        operations: vec![
            OperationSchema {
                name: "get".into(),
                request: ValueSchema::String,
                response: ValueSchema::Ref(ValueSchema::U64.id().unwrap()),
                error: ValueSchema::String,
            },
            OperationSchema {
                name: "notes".into(),
                request: ValueSchema::String,
                response: ValueSchema::Ref(
                    ValueSchema::List(Box::new(ValueSchema::String))
                        .id()
                        .unwrap(),
                ),
                error: ValueSchema::String,
            },
        ],
    }
}
fn main() {
    let property = |slot: &str, name: &str| ImportSlot {
        slot: slot.into(),
        contract: format!("test.state-provider.property.{name}@1"),
        revision_min: 1,
        revision_max: 1,
        cardinality: Cardinality::One,
    };
    let metadata = ContractMetadata {
        plugin: "test.state-client".into(),
        version: "1.0.0".into(),
        execution: MetadataExecution::StatePending,
        definitions: vec![
            refs(),
            ContractDefinition {
                contract: "test.state@1".into(),
                revision: 1,
                operations: vec![
                    OperationSchema {
                        name: "read".into(),
                        request: ValueSchema::Ref(ValueSchema::U64.id().unwrap()),
                        response: ValueSchema::U64,
                        error: ValueSchema::String,
                    },
                    OperationSchema {
                        name: "run".into(),
                        request: ValueSchema::String,
                        response: ValueSchema::List(Box::new(ValueSchema::U64)),
                        error: ValueSchema::String,
                    },
                ],
            },
        ],
        requires: vec![
            property("counter", "counter"),
            property("notes", "notes"),
            ImportSlot {
                slot: "refs".into(),
                contract: "test.refs@1".into(),
                revision_min: 1,
                revision_max: 1,
                cardinality: Cardinality::One,
            },
            ImportSlot {
                slot: "many".into(),
                contract: "test.refs@1".into(),
                revision_min: 1,
                revision_max: 1,
                cardinality: Cardinality::Many { minimum: 1 },
            },
        ],
        provides: vec![ExportSlot {
            slot: "test".into(),
            contract: "test.state@1".into(),
            revision: 1,
        }],
        properties: vec![],
    };
    let bytes = metadata.encode().unwrap();
    let mut section = vec![0_u8; SECTION_BYTES];
    section[..4].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
    section[4..4 + bytes.len()].copy_from_slice(&bytes);
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("metadata.bin"),
        section,
    )
    .unwrap();
}
