//! S1b offline typed authoring fixture. Property helper macros are S3.
use phenix_sdk::{
    ContractDefinition, ContractMetadata, MetadataExecution, OperationSchema, ValueSchema,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let metadata = ContractMetadata {
        plugin: "example.reactive-contracts".into(),
        version: "1.0.0".into(),
        execution: MetadataExecution::None,
        // Reverse authored order proves canonical declaration sorting.
        definitions: vec![
            ContractDefinition {
                contract: "example.echo@1".into(),
                revision: 1,
                operations: vec![OperationSchema {
                    name: "echo".into(),
                    request: ValueSchema::String,
                    response: ValueSchema::String,
                    error: ValueSchema::Variants(vec!["Missing".into()]),
                }],
            },
            ContractDefinition {
                contract: "example.counter@1".into(),
                revision: 1,
                operations: vec![
                    OperationSchema {
                        name: "write".into(),
                        request: ValueSchema::U64,
                        response: ValueSchema::U64,
                        error: ValueSchema::Variants(vec!["Missing".into()]),
                    },
                    OperationSchema {
                        name: "read".into(),
                        request: ValueSchema::U64,
                        response: ValueSchema::U64,
                        error: ValueSchema::Variants(vec!["Missing".into()]),
                    },
                ],
            },
        ],
        requires: vec![],
        provides: vec![],
        properties: vec![],
    };
    let output = std::env::args()
        .nth(1)
        .ok_or("usage: example-reactive-contracts DESCRIPTOR-OUTPUT")?;
    std::fs::write(output, metadata.encode()?)?;
    Ok(())
}
