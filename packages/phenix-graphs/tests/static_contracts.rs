use phenix_graph::{
    AllSameCohort, AnyOne, DeclaredValue, Deployment, GraphBuilder, GraphContract, GraphUnit,
    IncompleteGroup, InputSlot, InterfaceShape, InvocationCtx, NodeTemplate, OutputSlot, Pick,
    Port, PortPolicy, PortSnapshot, ReportRetention, Selection, Store, Value,
};
use std::sync::Arc;

#[test]
fn zero_input_source_claim_is_accepted_with_an_addressable_output() {
    let required = GraphContract::new(
        "test/source@1",
        InterfaceShape::new()
            .output("value", DeclaredValue::local::<i64>())
            .unwrap(),
    )
    .unwrap();
    let mut source = NodeTemplate::with_action(
        1,
        vec![],
        vec![OutputSlot::local::<i64>(0).named("value")],
        |_ctx: InvocationCtx| Ok(Vec::new()),
    )
    .named("source")
    .unwrap();
    source.output("value").unwrap();
    source.claims.push(required.clone());

    let mut graph = GraphBuilder::new("root");
    graph.add("source", source).unwrap();
    let view = graph.inspect("source").unwrap();
    let snapshot = view.inspect().unwrap();
    assert_eq!(snapshot.shape.inputs().count(), 0);
    assert!(required.required().same_shape(&snapshot.shape));
    required.check_shape(&view).unwrap();
    required.check_implementation(&view).unwrap();
    graph
        .bind_contract("source", &required)
        .unwrap()
        .local_output::<i64>("value")
        .unwrap();

    let compiled = graph.compile().unwrap();
    let unit = compiled.find("source").unwrap();
    required.check_implementation(&unit).unwrap();
    unit.bind_contract(&required)
        .unwrap()
        .output("value")
        .unwrap();
}

#[test]
fn zero_output_sink_claim_is_accepted_and_executes_with_an_addressable_input() {
    let required = GraphContract::new(
        "test/sink@1",
        InterfaceShape::new()
            .input_slot("run", "value", DeclaredValue::local::<i64>())
            .unwrap(),
    )
    .unwrap();
    let mut sink = NodeTemplate::with_action(
        1,
        vec![
            Port::new(
                0,
                vec![InputSlot::local::<i64>(0).named("value")],
                AllSameCohort,
            )
            .named("run"),
        ],
        vec![],
        |_ctx: InvocationCtx| Ok(Vec::new()),
    )
    .named("sink")
    .unwrap();
    sink.input("run", "value").unwrap();
    sink.claims.push(required.clone());

    let mut graph = GraphBuilder::new("root");
    graph.add("sink", sink).unwrap();
    let view = graph.inspect("sink").unwrap();
    let snapshot = view.inspect().unwrap();
    assert_eq!(snapshot.shape.outputs().count(), 0);
    assert!(required.required().same_shape(&snapshot.shape));
    required.check_shape(&view).unwrap();
    required.check_implementation(&view).unwrap();
    let input = graph
        .bind_contract("sink", &required)
        .unwrap()
        .local_input::<i64>("run", "value")
        .unwrap();
    let input = graph.expose_input("request", input).unwrap();
    graph.trigger("run", [input]).unwrap();
    let compiled = graph.compile().unwrap();
    let unit = compiled.find("sink").unwrap();
    required.check_implementation(&unit).unwrap();
    unit.bind_contract(&required)
        .unwrap()
        .input("run", "value")
        .unwrap();
    let report = Deployment::new(compiled)
        .build()
        .unwrap()
        .fire("run", vec![Value::local(7_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.invocation_count(1), 1);
    assert_eq!(report.publications().count(), 0);
}

#[test]
fn empty_actual_module_needs_a_claim_despite_equal_shape() {
    let required = GraphContract::new("test/empty@1", InterfaceShape::new()).unwrap();
    let mut graph = GraphBuilder::new("root");
    graph
        .instance("empty", &GraphBuilder::new("empty").module())
        .unwrap();
    let view = graph.inspect("empty").unwrap();
    assert!(
        required
            .required()
            .same_shape(&view.inspect().unwrap().shape)
    );
    required.check_shape(&view).unwrap();
    assert!(required.check_implementation(&view).is_err());
    assert!(graph.bind_contract("empty", &required).is_err());

    let compiled = graph.clone().compile().unwrap();
    let unit = compiled.find("empty").unwrap();
    required.check_shape(&unit).unwrap();
    assert!(required.check_implementation(&unit).is_err());
    assert!(unit.bind_contract(&required).is_err());

    graph.implements("empty", &required).unwrap();
    required
        .check_implementation(&graph.inspect("empty").unwrap())
        .unwrap();
    let compiled = graph.compile().unwrap();
    required
        .check_implementation(&compiled.find("empty").unwrap())
        .unwrap();
}

struct SelectRight;

impl PortPolicy for SelectRight {
    fn select(&self, snapshot: &PortSnapshot) -> Option<Selection> {
        let candidate = snapshot.slot_named("right")?.candidates().first()?;
        Some(Selection::new(
            candidate.cohort_id(),
            vec![Pick::new(candidate.slot_id(), candidate.delivery_id())],
        ))
    }

    fn incomplete(&self, _: &PortSnapshot) -> Vec<IncompleteGroup> {
        Vec::new()
    }
}

#[test]
fn equal_static_shapes_and_claims_do_not_promise_readiness_or_emission() {
    let required = GraphContract::new(
        "test/policy@1",
        InterfaceShape::new()
            .input_slot("run", "left", DeclaredValue::local::<i64>())
            .unwrap()
            .input_slot("run", "right", DeclaredValue::local::<i64>())
            .unwrap()
            .output("value", DeclaredValue::local::<i64>())
            .unwrap(),
    )
    .unwrap();
    let policies: Vec<(Arc<dyn PortPolicy>, usize, usize)> = vec![
        (Arc::new(AnyOne), 1, 2),
        (Arc::new(AllSameCohort), 0, 1),
        (Arc::new(SelectRight), 0, 1),
    ];
    for (policy, left_count, both_count) in policies {
        let mut template = NodeTemplate::with_action(
            1,
            vec![
                Port::new(
                    0,
                    vec![
                        InputSlot::local::<i64>(0).named("left"),
                        InputSlot::local::<i64>(1).named("right"),
                    ],
                    policy,
                )
                .named("run"),
            ],
            vec![OutputSlot::local::<i64>(0).named("value")],
            |_ctx: InvocationCtx| Ok(Vec::new()),
        );
        template.claims.push(required.clone());
        let mut graph = GraphBuilder::new("root");
        graph.add("worker", template).unwrap();
        let view = graph.inspect("worker").unwrap();
        assert!(
            required
                .required()
                .same_shape(&view.inspect().unwrap().shape)
        );
        required.check_shape(&view).unwrap();
        required.check_implementation(&view).unwrap();
        let left = graph.local_input::<i64>("worker", "run", "left").unwrap();
        let right = graph.local_input::<i64>("worker", "run", "right").unwrap();
        let left = graph.expose_input("left", left).unwrap();
        let right = graph.expose_input("right", right).unwrap();
        graph.trigger("left", [left.clone()]).unwrap();
        graph.trigger("both", [left, right]).unwrap();

        for (trigger, values, count) in [
            ("left", vec![Value::local(1_i64)], left_count),
            (
                "both",
                vec![Value::local(1_i64), Value::local(2_i64)],
                both_count,
            ),
        ] {
            let compiled = graph.clone().compile().unwrap();
            let unit = compiled.find("worker").unwrap();
            assert!(
                required
                    .required()
                    .same_shape(&unit.inspect().unwrap().shape)
            );
            required.check_implementation(&unit).unwrap();
            unit.bind_contract(&required)
                .unwrap()
                .output("value")
                .unwrap();
            let report = Deployment::new(compiled)
                .report_retention(ReportRetention::RetainPayloads)
                .build()
                .unwrap()
                .fire(trigger, values)
                .unwrap();
            if count > 0 {
                assert!(report.success(), "{:?}", report.error_messages());
            }
            assert_eq!(report.invocation_count(1), count);
            // A declared output is addressable, not a promise to publish a value.
            assert_eq!(report.publications().count(), 0);
        }
    }
}

#[test]
fn contract_claim_does_not_grant_access_to_an_existing_store_resource() {
    let required = GraphContract::new(
        "test/reader@1",
        InterfaceShape::new()
            .input_slot("run", "value", DeclaredValue::local::<i64>())
            .unwrap(),
    )
    .unwrap();
    for granted in [false, true] {
        let mut template = NodeTemplate::with_action(
            1,
            vec![
                Port::new(
                    0,
                    vec![InputSlot::local::<i64>(0).named("value")],
                    AllSameCohort,
                )
                .named("run"),
            ],
            vec![],
            move |ctx: InvocationCtx| {
                let result = ctx.read::<i64, _>("counter", |value| *value);
                if granted {
                    assert_eq!(result.unwrap(), 42);
                } else {
                    assert_eq!(result.unwrap_err(), "read of counter not granted");
                }
                Ok(Vec::new())
            },
        );
        if granted {
            template = template.reads("counter");
        }
        template.claims.push(required.clone());
        let mut graph = GraphBuilder::new("root");
        graph.add("reader", template).unwrap();
        let input = graph.local_input::<i64>("reader", "run", "value").unwrap();
        let input = graph.expose_input("request", input).unwrap();
        graph.trigger("run", [input]).unwrap();
        let compiled = graph.compile().unwrap();
        required
            .check_implementation(&compiled.find("reader").unwrap())
            .unwrap();
        let mut store = Store::new();
        store.insert("counter", 42_i64);
        let report = Deployment::new(compiled)
            .store(store)
            .build()
            .unwrap()
            .fire("run", vec![Value::local(7_i64)])
            .unwrap();
        assert!(report.success(), "{:?}", report.error_messages());
        assert_eq!(report.invocation_count(1), 1);
    }
}
