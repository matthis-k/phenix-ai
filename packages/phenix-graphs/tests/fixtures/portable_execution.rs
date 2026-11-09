#[cfg(feature = "derive")]
use phenix_graph::testing::portable::{GoldenVector, check};
use phenix_graph::{
    AllSameCohort, ContractValue, Deployment, GraphBuilder, InputSlot, InvocationCtx, NodeTemplate,
    OutputSlot, Port, ReportRetention, Store, Value, emit,
};
use portable_consumer::Number as ConsumerNumber;
use portable_provider::Number as ProviderNumber;

fn relay<T: ContractValue>(id: u16) -> NodeTemplate {
    NodeTemplate::with_action(
        id,
        vec![
            Port::new(
                0,
                vec![InputSlot::deferred::<T>(0).named("value")],
                AllSameCohort,
            )
            .named("run")
            .args::<T, _>(|values| values.get("value")),
        ],
        vec![OutputSlot::typed::<T>(0).named("value")],
        |mut ctx: InvocationCtx| Ok(vec![emit(0, ctx.take_prepared::<T>()?)]),
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
    let mut graph = GraphBuilder::new("independent-crates");
    graph.add("provider", relay::<ProviderNumber>(1)).unwrap();
    graph.add("consumer", relay::<ConsumerNumber>(2)).unwrap();
    let source = graph
        .wire_output::<ProviderNumber>("provider", "value")
        .unwrap();
    let sink = graph
        .wire_input::<ConsumerNumber>("consumer", "run", "value")
        .unwrap();
    graph.connect_wire(source, sink).unwrap();
    let request = graph.expose_input("request", "provider").unwrap();
    graph.expose_output("result", "consumer").unwrap();
    graph.trigger("run", [request]).unwrap();
    let engine = Deployment::new(graph.compile().unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap();
    for value in [i64::MIN, -1, 0, 1, 42, i64::MAX] {
        let report = engine
            .fire("run", vec![Value::of(ProviderNumber { value })])
            .unwrap();
        assert!(report.success(), "{:?}", report.error_messages());
        assert_eq!(report.invocation_count(1), 1);
        assert_eq!(report.invocation_count(2), 1);
        let publications: Vec<_> = report.publications().collect();
        assert_eq!(publications.len(), 2);
        assert_eq!(
            publications[1].value::<ConsumerNumber>().unwrap().number,
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
