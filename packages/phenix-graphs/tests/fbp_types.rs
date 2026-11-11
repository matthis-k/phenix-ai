use phenix_graph::network::{InputRef, NetworkBuilder, NetworkError, OutputRef};
use phenix_graph::process::{Process, ProcessContext, ProcessFactory, Step};
use phenix_graph::{ContractValue, Interface, PortDef, Schema, TypeIdentity};
use std::any::TypeId;
use std::num::NonZeroUsize;
use std::sync::Arc;

struct Noop;
impl Process for Noop {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        Ok(Step::Finished)
    }
}
fn factory() -> Arc<dyn ProcessFactory> {
    Arc::new(|| Noop)
}
fn capacity() -> NonZeroUsize {
    NonZeroUsize::new(2).unwrap()
}
fn schema() -> Schema {
    Schema {
        name: "fbp.record".into(),
        revision: 7,
        structure: "record{value:i64}".into(),
        codec: "record-le-v1".into(),
    }
}

#[test]
fn canonical_maps_are_unique_sorted_symmetric_and_exact() {
    let interface = Interface::new(
        [PortDef::local::<i64>("z"), PortDef::local::<String>("a")],
        [PortDef::local::<bool>("a"), PortDef::local::<()>("z")],
    )
    .unwrap();
    assert_eq!(
        interface
            .inputs()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["a", "z"]
    );
    assert_eq!(
        interface
            .outputs()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["a", "z"]
    );
    assert_eq!(
        interface.input("a").unwrap().value,
        TypeIdentity::Local(TypeId::of::<String>())
    );
    assert_eq!(
        interface.output("a").unwrap().value,
        TypeIdentity::Local(TypeId::of::<bool>())
    );
    assert!(interface.input("missing").is_none());
    let reordered = Interface::new(
        [PortDef::local::<String>("a"), PortDef::local::<i64>("z")],
        [PortDef::local::<()>("z"), PortDef::local::<bool>("a")],
    )
    .unwrap();
    assert_eq!(interface, reordered);
    assert_ne!(interface, Interface::default());
}

#[test]
fn duplicate_names_and_invalid_names_reject_in_each_direction() {
    for output in [false, true] {
        let ports = [
            PortDef::local::<i64>("same"),
            PortDef::local::<bool>("same"),
        ];
        assert!(
            if output {
                Interface::new([], ports)
            } else {
                Interface::new(ports, [])
            }
            .is_err()
        );
        for name in ["", "line\nbreak", "null\0"] {
            let ports = [PortDef::local::<i64>(name)];
            assert!(
                if output {
                    Interface::new([], ports)
                } else {
                    Interface::new(ports, [])
                }
                .is_err()
            );
        }
    }
}

#[test]
fn port_names_are_not_a_second_path_parser() {
    let interface = Arc::new(
        Interface::new(
            [PortDef::local::<i64>("request/with.dots")],
            [PortDef::local::<i64>("result with spaces")],
        )
        .unwrap(),
    );
    let mut builder = NetworkBuilder::new();
    let node = builder.node("worker", interface, factory()).unwrap();
    assert_eq!(
        node.input::<i64>("request/with.dots").unwrap().path().port,
        "request/with.dots"
    );
    assert_eq!(
        node.output::<i64>("result with spaces")
            .unwrap()
            .path()
            .port,
        "result with spaces"
    );
    assert!(matches!(
        node.input::<i64>("result with spaces"),
        Err(NetworkError::UnknownInput(_))
    ));
}

#[test]
fn portable_identity_includes_every_full_descriptor_field() {
    let original = schema();
    let port = PortDef::portable("value", &original).unwrap();
    assert_eq!(port.value, TypeIdentity::Wire(original.identity()));
    for changed in [
        Schema {
            name: "other".into(),
            ..original.clone()
        },
        Schema {
            revision: 0,
            ..original.clone()
        },
        Schema {
            structure: "record{value:u64}".into(),
            ..original.clone()
        },
        Schema {
            codec: "other-codec".into(),
            ..original.clone()
        },
    ] {
        assert_ne!(
            port.value,
            PortDef::portable("value", &changed).unwrap().value
        );
    }
    assert_eq!(port, PortDef::portable("value", &original.clone()).unwrap());
}

#[test]
fn portable_constructor_rejects_incomplete_descriptors_without_inventing_revision_rules() {
    for bad in [
        Schema {
            name: "".into(),
            ..schema()
        },
        Schema {
            structure: "  ".into(),
            ..schema()
        },
        Schema {
            codec: "".into(),
            ..schema()
        },
        Schema {
            name: "bad\nname".into(),
            ..schema()
        },
    ] {
        assert!(PortDef::portable("value", &bad).is_err());
    }
    assert!(
        PortDef::portable(
            "value",
            &Schema {
                revision: 0,
                ..schema()
            }
        )
        .is_ok()
    );
}

#[test]
fn local_identity_cannot_be_forged_with_a_diagnostic_wire_label() {
    let forged = Arc::new(
        Interface::new(
            [],
            [PortDef {
                name: "value".into(),
                value: TypeIdentity::Wire(std::any::type_name::<i64>().into()),
            }],
        )
        .unwrap(),
    );
    let mut builder = NetworkBuilder::new();
    let source = builder.node("source", forged, factory()).unwrap();
    let sink = builder
        .node(
            "sink",
            Arc::new(Interface::new([PortDef::local::<i64>("value")], []).unwrap()),
            factory(),
        )
        .unwrap();
    assert!(matches!(
        source.output::<i64>("value"),
        Err(NetworkError::WrongType { .. })
    ));
    assert!(matches!(
        builder.connect_dynamic(
            &source.output_endpoint("value").unwrap(),
            &sink.input_endpoint("value").unwrap(),
            capacity()
        ),
        Err(NetworkError::WrongType { .. })
    ));
    assert!(builder.build().unwrap().connections().is_empty());
}

#[test]
fn local_types_with_identical_shape_are_not_interchangeable() {
    struct A(i64);
    struct B(i64);
    assert_eq!(A(1).0, B(1).0);
    let mut builder = NetworkBuilder::new();
    let source = builder
        .node(
            "source",
            Arc::new(Interface::new([], [PortDef::local::<A>("out")]).unwrap()),
            factory(),
        )
        .unwrap();
    let sink = builder
        .node(
            "sink",
            Arc::new(Interface::new([PortDef::local::<B>("in")], []).unwrap()),
            factory(),
        )
        .unwrap();
    assert!(source.output::<A>("out").is_ok());
    assert!(source.output::<B>("out").is_err());
    assert!(
        builder
            .connect_dynamic(
                &source.output_endpoint("out").unwrap(),
                &sink.input_endpoint("in").unwrap(),
                capacity()
            )
            .is_err()
    );
}

struct Provider;
struct Consumer;
macro_rules! portable_value {
    ($ty:ty) => {
        impl ContractValue for $ty {
            fn schema() -> Schema {
                schema()
            }
            fn encode(&self) -> Vec<u8> {
                vec![]
            }
            fn decode(_: &[u8]) -> Result<Self, String> {
                Ok(Self)
            }
        }
    };
}
portable_value!(Provider);
portable_value!(Consumer);

#[test]
fn independently_declared_portable_rust_types_connect_by_complete_schema() {
    let mut builder = NetworkBuilder::new();
    let source = builder
        .node(
            "source",
            Arc::new(Interface::new([], [PortDef::wire::<Provider>("out").unwrap()]).unwrap()),
            factory(),
        )
        .unwrap();
    let sink = builder
        .node(
            "sink",
            Arc::new(Interface::new([PortDef::wire::<Consumer>("in").unwrap()], []).unwrap()),
            factory(),
        )
        .unwrap();
    assert!(source.output::<Provider>("out").is_err());
    assert!(sink.input::<Consumer>("in").is_err());
    assert!(source.wire_output::<Provider>("out").is_ok());
    assert!(sink.wire_input::<Consumer>("in").is_ok());
    builder
        .connect_dynamic(
            &source.output_endpoint("out").unwrap(),
            &sink.input_endpoint("in").unwrap(),
            capacity(),
        )
        .unwrap();
    assert_eq!(
        builder
            .build()
            .unwrap()
            .compile()
            .unwrap()
            .connections()
            .len(),
        1
    );
}

#[test]
fn portable_mismatches_in_each_schema_field_reject_dynamic_wiring() {
    let original = schema();
    for changed in [
        Schema {
            name: "other".into(),
            ..original.clone()
        },
        Schema {
            revision: 8,
            ..original.clone()
        },
        Schema {
            structure: "different".into(),
            ..original.clone()
        },
        Schema {
            codec: "different".into(),
            ..original.clone()
        },
    ] {
        let mut builder = NetworkBuilder::new();
        let source = builder
            .node(
                "source",
                Arc::new(
                    Interface::new([], [PortDef::portable("out", &original).unwrap()]).unwrap(),
                ),
                factory(),
            )
            .unwrap();
        let sink = builder
            .node(
                "sink",
                Arc::new(Interface::new([PortDef::portable("in", &changed).unwrap()], []).unwrap()),
                factory(),
            )
            .unwrap();
        assert!(matches!(
            builder.connect_dynamic(
                &source.output_endpoint("out").unwrap(),
                &sink.input_endpoint("in").unwrap(),
                capacity()
            ),
            Err(NetworkError::WrongType { .. })
        ));
    }
}

struct Mislabelled;
impl ContractValue for Mislabelled {
    fn schema() -> Schema {
        schema()
    }
    fn wire_schema() -> Arc<str> {
        "forged-short-label".into()
    }
    fn encode(&self) -> Vec<u8> {
        vec![]
    }
    fn decode(_: &[u8]) -> Result<Self, String> {
        Ok(Self)
    }
}

#[test]
fn portable_declarations_and_typed_lookup_do_not_trust_overridden_wire_labels() {
    let value = phenix_graph::Value::of(Mislabelled);
    assert_eq!(value.identity(), TypeIdentity::Wire(schema().identity()));
    value.get::<Mislabelled>().unwrap();
    value.get::<Consumer>().unwrap();
    phenix_graph::Value::of(Consumer)
        .get::<Mislabelled>()
        .unwrap();
    assert!(
        phenix_graph::Value::from_wire_parts(Mislabelled::wire_schema(), vec![])
            .get::<Mislabelled>()
            .is_err()
    );
    let port = PortDef::wire::<Mislabelled>("out").unwrap();
    assert_eq!(port.value, TypeIdentity::Wire(schema().identity()));
    let mut builder = NetworkBuilder::new();
    let source = builder
        .node(
            "source",
            Arc::new(Interface::new([], [port]).unwrap()),
            factory(),
        )
        .unwrap();
    assert!(source.wire_output::<Mislabelled>("out").is_ok());
    let forged = builder
        .node(
            "forged",
            Arc::new(
                Interface::new(
                    [],
                    [PortDef {
                        name: "out".into(),
                        value: TypeIdentity::Wire(Mislabelled::wire_schema()),
                    }],
                )
                .unwrap(),
            ),
            factory(),
        )
        .unwrap();
    assert!(forged.wire_output::<Mislabelled>("out").is_err());
}

#[test]
fn typed_handles_clone_and_debug_without_payload_trait_bounds() {
    struct MoveOnly;
    fn clone_input<T>(input: &InputRef<T>) -> InputRef<T> {
        input.clone()
    }
    fn clone_output<T>(output: &OutputRef<T>) -> OutputRef<T> {
        output.clone()
    }
    let mut builder = NetworkBuilder::new();
    let node = builder
        .node(
            "node",
            Arc::new(
                Interface::new(
                    [PortDef::local::<MoveOnly>("in")],
                    [PortDef::local::<MoveOnly>("out")],
                )
                .unwrap(),
            ),
            factory(),
        )
        .unwrap();
    let input = clone_input(&node.input::<MoveOnly>("in").unwrap());
    let output = clone_output(&node.output::<MoveOnly>("out").unwrap());
    assert!(!format!("{input:?} {output:?}").is_empty());
    builder.connect(output, input, capacity()).unwrap();
    assert_eq!(
        builder
            .build()
            .unwrap()
            .compile()
            .unwrap()
            .connections()
            .len(),
        1
    );
}

#[test]
fn exact_interface_equality_includes_extra_inputs_outputs_and_types() {
    let base = Interface::new(
        [PortDef::local::<i64>("in")],
        [PortDef::local::<i64>("out")],
    )
    .unwrap();
    assert_ne!(
        base,
        Interface::new(
            [PortDef::local::<i64>("in"), PortDef::local::<i64>("extra")],
            [PortDef::local::<i64>("out")]
        )
        .unwrap()
    );
    assert_ne!(
        base,
        Interface::new(
            [PortDef::local::<i64>("in")],
            [PortDef::local::<i64>("out"), PortDef::local::<i64>("extra")]
        )
        .unwrap()
    );
    assert_ne!(
        base,
        Interface::new(
            [PortDef::local::<String>("in")],
            [PortDef::local::<i64>("out")]
        )
        .unwrap()
    );
}
