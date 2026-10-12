use phenix_contract::{
    descriptor::static_metadata::metadata, Cardinality, ContractDefinition, ContractMetadata,
    ExportSlot, ImportSlot, MetadataBytes, MetadataExecution, OperationSchema, PropertyDefinition,
    StaticContract, StaticExport, StaticImport, StaticOperation, StaticProperty, StaticSchema,
    ValueSchema, NATIVE_SECTION_BYTES,
};

const CONTRACT: StaticContract = StaticContract {
    contract: "example.counter@1",
    revision: 1,
    operations: &[StaticOperation {
        name: "read",
        key: "example.counter@1/read",
        request: StaticSchema::Struct(&[]),
        response: StaticSchema::U64,
        error: StaticSchema::Variants(&["Missing"]),
    }],
};
const PRIMARY: StaticImport = StaticImport {
    provider: None,
    slot: "primary",
    contract: "example.counter@1",
    revision_min: 1,
    revision_max: 3,
    cardinality: Cardinality::One,
};
const IMPORTS: [StaticImport; 3] = [
    PRIMARY,
    StaticImport {
        provider: None,
        slot: "fallback",
        cardinality: Cardinality::Optional,
        ..PRIMARY
    },
    StaticImport {
        provider: None,
        slot: "all",
        cardinality: Cardinality::Many { minimum: 0 },
        ..PRIMARY
    },
];
const EXPORT: StaticExport = StaticExport {
    slot: "service",
    contract: "example.counter@1",
    revision: 1,
};
const PROPERTIES: [StaticProperty; 2] = [
    StaticProperty {
        id: "history",
        schema: StaticSchema::List(&StaticSchema::Ref(&StaticSchema::String)),
        get: true,
        set: false,
        watch: false,
    },
    StaticProperty {
        id: "counter",
        schema: StaticSchema::U64,
        get: true,
        set: true,
        watch: true,
    },
];
// These constants prove metadata and section generation during constant evaluation.
#[allow(clippy::large_const_arrays)]
const NATIVE: MetadataBytes = metadata(
    "example.native",
    "1.0.0",
    MetadataExecution::StatePending,
    [CONTRACT],
    IMPORTS,
    [EXPORT],
    PROPERTIES,
);
#[allow(clippy::large_const_arrays)]
const SECTION: [u8; NATIVE_SECTION_BYTES] = NATIVE.native_section();

#[test]
fn const_native_bytes_match_independent_owned_metadata() {
    let expected = ContractMetadata {
        plugin: "example.native".into(),
        version: "1.0.0".into(),
        execution: MetadataExecution::StatePending,
        definitions: vec![ContractDefinition {
            contract: "example.counter@1".into(),
            revision: 1,
            operations: vec![OperationSchema {
                name: "read".into(),
                request: ValueSchema::Struct(Default::default()),
                response: ValueSchema::U64,
                error: ValueSchema::Variants(vec!["Missing".into()]),
            }],
        }],
        requires: vec![
            ImportSlot {
                provider: None,
                slot: "all".into(),
                contract: "example.counter@1".into(),
                revision_min: 1,
                revision_max: 3,
                cardinality: Cardinality::Many { minimum: 0 },
            },
            ImportSlot {
                provider: None,
                slot: "fallback".into(),
                contract: "example.counter@1".into(),
                revision_min: 1,
                revision_max: 3,
                cardinality: Cardinality::Optional,
            },
            ImportSlot {
                provider: None,
                slot: "primary".into(),
                contract: "example.counter@1".into(),
                revision_min: 1,
                revision_max: 3,
                cardinality: Cardinality::One,
            },
        ],
        provides: vec![ExportSlot {
            slot: "service".into(),
            contract: "example.counter@1".into(),
            revision: 1,
        }],
        properties: vec![
            PropertyDefinition {
                id: "counter".into(),
                schema: ValueSchema::U64,
                get: true,
                set: true,
                watch: true,
            },
            PropertyDefinition {
                id: "history".into(),
                schema: ValueSchema::List(Box::new(ValueSchema::Ref(
                    ValueSchema::String.id().unwrap(),
                ))),
                get: true,
                set: false,
                watch: false,
            },
        ],
    };
    assert_eq!(NATIVE.bytes(), expected.encode().unwrap());
    assert_eq!(ContractMetadata::decode(NATIVE.bytes()).unwrap(), expected);
    let length = u32::from_be_bytes(SECTION[..4].try_into().unwrap()) as usize;
    assert_eq!(length, NATIVE.bytes().len());
    assert_eq!(&SECTION[4..4 + length], NATIVE.bytes());
    assert!(SECTION[4 + length..].iter().all(|byte| *byte == 0));
}

#[test]
fn native_bytes_ignore_contribution_discovery_order() {
    let forward = metadata(
        "example.native",
        "1.0.0",
        MetadataExecution::StatePending,
        [CONTRACT],
        [IMPORTS[2], IMPORTS[1], IMPORTS[0]],
        [EXPORT],
        [PROPERTIES[1], PROPERTIES[0]],
    );
    assert_eq!(forward.bytes(), NATIVE.bytes());
}

#[test]
fn invalid_native_declarations_reject_before_encoding() {
    let rejected = |imports, exports, properties| {
        assert!(std::panic::catch_unwind(|| metadata(
            "example.native",
            "1.0.0",
            MetadataExecution::StatePending,
            [CONTRACT],
            imports,
            exports,
            properties,
        ))
        .is_err());
    };
    rejected([IMPORTS[0], IMPORTS[0], IMPORTS[2]], [EXPORT], PROPERTIES);
    rejected(
        [
            StaticImport {
                provider: None,
                revision_min: 0,
                ..IMPORTS[0]
            },
            IMPORTS[1],
            IMPORTS[2],
        ],
        [EXPORT],
        PROPERTIES,
    );
    rejected(
        [
            StaticImport {
                provider: None,
                revision_max: 0,
                ..IMPORTS[0]
            },
            IMPORTS[1],
            IMPORTS[2],
        ],
        [EXPORT],
        PROPERTIES,
    );
    rejected(
        [
            StaticImport {
                provider: None,
                cardinality: Cardinality::Many { minimum: 65537 },
                ..IMPORTS[0]
            },
            IMPORTS[1],
            IMPORTS[2],
        ],
        [EXPORT],
        PROPERTIES,
    );
    rejected(
        IMPORTS,
        [StaticExport {
            revision: 0,
            ..EXPORT
        }],
        PROPERTIES,
    );
    rejected(
        IMPORTS,
        [StaticExport {
            contract: "example.counter@01",
            ..EXPORT
        }],
        PROPERTIES,
    );
    rejected(IMPORTS, [EXPORT], [PROPERTIES[0], PROPERTIES[0]]);
    rejected(
        IMPORTS,
        [EXPORT],
        [
            StaticProperty {
                id: "bad/path",
                ..PROPERTIES[0]
            },
            PROPERTIES[1],
        ],
    );
    rejected(
        IMPORTS,
        [EXPORT],
        [
            StaticProperty {
                get: false,
                ..PROPERTIES[0]
            },
            PROPERTIES[1],
        ],
    );
    assert!(std::panic::catch_unwind(|| metadata(
        "example.native",
        "1.0.0",
        MetadataExecution::None,
        [CONTRACT],
        IMPORTS,
        [EXPORT],
        PROPERTIES,
    ))
    .is_err());
    assert!(std::panic::catch_unwind(|| metadata(
        "example.native",
        "1.0.0",
        MetadataExecution::StatePending,
        [CONTRACT],
        [],
        [EXPORT, EXPORT],
        [],
    ))
    .is_err());
    for id in [
        "counter",
        "counter@0",
        "counter@01",
        "counter@4294967296",
        "bad/path@1",
    ] {
        assert!(
            std::panic::catch_unwind(|| metadata(
                "example.native",
                "1.0.0",
                MetadataExecution::StatePending,
                [StaticContract {
                    contract: id,
                    ..CONTRACT
                }],
                [],
                [],
                [],
            ))
            .is_err(),
            "{id}"
        );
    }
}

#[test]
fn metadata_capacity_is_bounded() {
    // Distinct valid properties exceed the metadata buffer capacity.
    let mut properties = [PROPERTIES[1]; 1024];
    for (index, property) in properties.iter_mut().enumerate() {
        property.id =
            Box::leak(format!("property.{index:04}.{}", "x".repeat(160)).into_boxed_str());
    }
    assert!(std::panic::catch_unwind(|| metadata(
        "example.native",
        "1.0.0",
        MetadataExecution::StatePending,
        [],
        [],
        [],
        properties,
    ))
    .is_err());
}
