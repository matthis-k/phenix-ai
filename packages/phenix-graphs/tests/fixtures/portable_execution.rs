#[cfg(feature = "derive")]
use phenix_graph::testing::portable::{GoldenVector, check};
use phenix_graph::{
    ContractValue, Interface, NetworkBuilder, PortDef, Process, ProcessContext, ProcessEmission,
    Step, Value,
};
use portable_consumer::Number as ConsumerNumber;
use portable_provider::Number as ProviderNumber;
use std::{marker::PhantomData, num::NonZeroUsize, sync::Arc};

struct Relay<T>(PhantomData<T>);
impl<T: ContractValue> Process for Relay<T> {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
        if let Some(value) = context.take("in") {
            Ok(Step::Emit(ProcessEmission::new(
                "out",
                Value::of(value.get::<T>()?),
            )))
        } else {
            Ok(Step::Complete)
        }
    }
}
fn interface<T: ContractValue>() -> Arc<Interface> {
    Arc::new(
        Interface::new(
            [PortDef::wire::<T>("in").unwrap()],
            [PortDef::wire::<T>("out").unwrap()],
        )
        .unwrap(),
    )
}

fn main() {
    #[cfg(feature = "derive")]
    record_conformance();
    assert_ne!(
        std::any::TypeId::of::<ProviderNumber>(),
        std::any::TypeId::of::<ConsumerNumber>()
    );
    assert_eq!(ProviderNumber::schema(), ConsumerNumber::schema());
    let mut graph = NetworkBuilder::new();
    let provider = graph
        .node(
            "provider",
            interface::<ProviderNumber>(),
            Arc::new(|| Relay::<ProviderNumber>(PhantomData)),
        )
        .unwrap();
    let consumer = graph
        .node(
            "consumer",
            interface::<ConsumerNumber>(),
            Arc::new(|| Relay::<ConsumerNumber>(PhantomData)),
        )
        .unwrap();
    let capacity = NonZeroUsize::new(1).unwrap();
    graph
        .connect_dynamic(
            provider
                .wire_output::<ProviderNumber>("out")
                .unwrap()
                .endpoint(),
            consumer
                .wire_input::<ConsumerNumber>("in")
                .unwrap()
                .endpoint(),
            capacity,
        )
        .unwrap();
    graph
        .expose_input(
            "request",
            provider.wire_input::<ProviderNumber>("in").unwrap(),
            capacity,
        )
        .unwrap();
    graph
        .expose_output(
            "result",
            consumer.wire_output::<ConsumerNumber>("out").unwrap(),
        )
        .unwrap();
    let mut session = graph.build().unwrap().compile().unwrap().start();
    for value in [i64::MIN, -1, 0, 1, 42, i64::MAX] {
        session
            .inject("request", Value::of(ProviderNumber { value }))
            .unwrap();
        session.run_until_idle(100);
        assert_eq!(
            session
                .take_output("result")
                .unwrap()
                .unwrap()
                .value()
                .get::<ConsumerNumber>()
                .unwrap()
                .number,
            value
        );
    }
}

#[cfg(feature = "derive")]
fn record_conformance() {
    use phenix_graph::Schema;
    use portable_consumer::Record as Consumer;
    use portable_provider::Record as Provider;
    let expected = Schema::record(
        "fixture/record",
        &[
            (
                "bytes",
                false,
                Schema {
                    name: "example.i64".into(),
                    revision: 1,
                    structure: "i64".into(),
                    codec: "i64-le-v1".into(),
                },
            ),
            (
                "cursor",
                true,
                Schema {
                    name: "example.utf8".into(),
                    revision: 1,
                    structure: "string".into(),
                    codec: "utf8-v1".into(),
                },
            ),
            (
                "type",
                false,
                Schema {
                    name: "example.bool".into(),
                    revision: 1,
                    structure: "bool".into(),
                    codec: "bool-u8-v1".into(),
                },
            ),
        ],
    );
    // Fixed le64 field framing, then payload. None and Some("") differ only by tag.
    let none: &[u8] = &[
        8, 0, 0, 0, 0, 0, 0, 0, 42, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0,
        0, 0, 0, 1,
    ];
    let mut empty = none.to_vec();
    empty[24] = 1;
    let text: &[u8] = &[
        8, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 3, 0, 0, 0, 0, 0, 0, 0, 1,
        104, 105, 1, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    let mut malformed: Vec<Vec<u8>> = (0..none.len()).map(|n| none[..n].to_vec()).collect();
    malformed.extend((0..text.len()).map(|n| text[..n].to_vec()));
    let mut invalid_tag = none.to_vec();
    invalid_tag[24] = 2;
    malformed.push(invalid_tag);
    let mut absent_payload = text.to_vec();
    absent_payload[24] = 0;
    malformed.push(absent_payload);
    let mut missing_tag = none.to_vec();
    missing_tag[16] = 0;
    missing_tag.remove(24);
    malformed.push(missing_tag);
    let mut invalid_utf8 = text.to_vec();
    invalid_utf8[25] = 255;
    malformed.push(invalid_utf8);
    let mut overflow = none.to_vec();
    overflow[..8].fill(255);
    malformed.push(overflow);
    let mut trailing = none.to_vec();
    trailing.push(0);
    malformed.push(trailing);
    let malformed: Vec<_> = malformed.iter().map(Vec::as_slice).collect();
    check(
        &expected,
        &[
            GoldenVector {
                name: "none",
                value: Provider {
                    bytes: 42,
                    cursor: None,
                    r#type: true,
                },
                bytes: none,
            },
            GoldenVector {
                name: "empty",
                value: Provider {
                    bytes: 42,
                    cursor: Some(String::new()),
                    r#type: true,
                },
                bytes: &empty,
            },
            GoldenVector {
                name: "text",
                value: Provider {
                    bytes: -1,
                    cursor: Some("hi".into()),
                    r#type: false,
                },
                bytes: text,
            },
        ],
        &malformed,
    )
    .unwrap();
    check(
        &expected,
        &[
            GoldenVector {
                name: "none",
                value: Consumer {
                    number: 42,
                    text: None,
                    flag: true,
                },
                bytes: none,
            },
            GoldenVector {
                name: "empty",
                value: Consumer {
                    number: 42,
                    text: Some(String::new()),
                    flag: true,
                },
                bytes: &empty,
            },
            GoldenVector {
                name: "text",
                value: Consumer {
                    number: -1,
                    text: Some("hi".into()),
                    flag: false,
                },
                bytes: text,
            },
        ],
        &malformed,
    )
    .unwrap();
    assert_ne!(
        std::any::TypeId::of::<Provider>(),
        std::any::TypeId::of::<Consumer>()
    );
}
