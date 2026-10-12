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
    let metadata = ContractMetadata {
        plugin: if cfg!(feature = "alternative") {
            "test.state-alternative"
        } else {
            "test.state-provider"
        }
        .into(),
        version: "1.0.0".into(),
        execution: MetadataExecution::StatePending,
        definitions: vec![
            refs(),
            ContractDefinition {
                contract: "test.provider@1".into(),
                revision: 1,
                operations: vec![OperationSchema {
                    name: "run".into(),
                    request: ValueSchema::String,
                    response: ValueSchema::List(Box::new(ValueSchema::U64)),
                    error: ValueSchema::String,
                }],
            },
        ],
        requires: vec![],
        provides: vec![
            ExportSlot {
                slot: "refs".into(),
                contract: "test.refs@1".into(),
                revision: 1,
            },
            ExportSlot {
                slot: "test".into(),
                contract: "test.provider@1".into(),
                revision: 1,
            },
        ],
        properties: vec![
            PropertyDefinition {
                id: "counter".into(),
                schema: ValueSchema::U64,
                get: true,
                set: true,
                watch: true,
            },
            PropertyDefinition {
                id: "notes".into(),
                schema: ValueSchema::List(Box::new(ValueSchema::String)),
                get: true,
                set: true,
                watch: true,
            },
        ],
    };
    let bytes = metadata.encode().unwrap();
    let mut section = vec![0_u8; NATIVE_SECTION_BYTES];
    section[..4].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
    section[4..4 + bytes.len()].copy_from_slice(&bytes);
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("metadata.bin"),
        section,
    )
    .unwrap();
}
