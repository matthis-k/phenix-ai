//! Batch A: one node return path, no special branch or Option/Result action
//! category, and one decoded InputLayout contract for PortArgs.
use phenix_graph::testing::{ExpectWave, Fixture};
use phenix_graph::{
    AllSameCohort, Deployment, FromSelected, GraphBuilder, InputLayout, InputSchema, InputSlot,
    InvocationCtx, LayoutError, NodeTemplate, Output, OutputSlot, Port, ReportRetention,
    SelectedValues, Value, emit,
};

#[derive(Debug, PartialEq, Eq)]
struct PairArgs {
    left: i64,
    right: i64,
}
impl InputSchema for PairArgs {
    fn slots() -> Result<Vec<InputSlot>, LayoutError> {
        Ok(vec![
            InputSlot::deferred::<i64>(0).named("left"),
            InputSlot::deferred::<i64>(1).named("right"),
        ])
    }
}
impl FromSelected for PairArgs {
    fn from_values(values: &SelectedValues<'_>) -> Result<Self, String> {
        Ok(Self {
            left: values.get("left")?,
            right: values.get("right")?,
        })
    }
}

#[test]
fn argument_type_is_its_input_layout_without_an_adapter_wrapper() {
    let slots = <PairArgs as InputLayout>::slots().unwrap();
    assert_eq!(
        slots
            .iter()
            .map(|slot| slot.name.as_deref())
            .collect::<Vec<_>>(),
        [Some("left"), Some("right")]
    );

    let mut graph = GraphBuilder::new("direct-layout");
    let _sum = graph
        .add(
            "sum",
            NodeTemplate::with_action(
                0,
                vec![Port::for_layout::<PairArgs>(0, "run", AllSameCohort).unwrap()],
                OutputSlot::typed::<i64>(0).named("value"),
                |mut ctx: InvocationCtx| {
                    let args = ctx.take_prepared::<PairArgs>()?;
                    Ok(vec![emit(0, args.left + args.right)])
                },
            ),
        )
        .unwrap();
    let left = graph
        .discover("sum")
        .unwrap()
        .inlets::<(), phenix_graph::EncodedValue>()
        .port(phenix_graph::InletPort::<(), i64>::new("run"))
        .field(phenix_graph::SlotField::<i64, i64>::new("left"))
        .target()
        .unwrap();
    let right = graph
        .discover("sum")
        .unwrap()
        .inlets::<(), phenix_graph::EncodedValue>()
        .port(phenix_graph::InletPort::<(), i64>::new("run"))
        .field(phenix_graph::SlotField::<i64, i64>::new("right"))
        .target()
        .unwrap();
    let left = graph.expose_input("left", left).unwrap();
    let right = graph.expose_input("right", right).unwrap();
    graph.trigger("calculate", [left, right]).unwrap();
    let report = Deployment::new(graph.compile().unwrap())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire("calculate", vec![Value::of(11_i64), Value::of(31_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(
        report
            .publications()
            .next()
            .unwrap()
            .value::<i64>()
            .unwrap(),
        42
    );
}

#[test]
fn one_low_level_local_function_handles_directed_returns() {
    let mut graph = GraphBuilder::new("one-local-action");
    let yes = graph.node("yes", |x: i64| x + 1).unwrap();
    let no = graph.node("no", |x: i64| x - 1).unwrap();
    let yes_target = yes.inlet();
    let no_target = no.inlet();

    // The same `NodeTemplate::local_function` used for plain values also
    // accepts Output<T>, and routing can use a decision not in the payload.
    let _router = graph
        .add(
            "router",
            NodeTemplate::local_function(move |x: i64| {
                let positive = x > 0;
                Output::to(if positive { &yes_target } else { &no_target }, x.abs())
            }),
        )
        .unwrap();
    let source = graph
        .discover("router")
        .unwrap()
        .output::<i64, phenix_graph::LocalValue>()
        .unwrap();
    graph.connect(source.clone(), yes.inlet()).unwrap();
    graph.connect(source, no.inlet()).unwrap();
    let input = graph
        .discover("router")
        .unwrap()
        .inlets::<(), phenix_graph::LocalValue>()
        .port(phenix_graph::InletPort::<(), i64>::new("run"))
        .field(phenix_graph::SlotField::<i64, i64>::new("value"))
        .target()
        .unwrap();
    let input = graph.expose_input("request", input).unwrap();
    graph.trigger("run", [input]).unwrap();

    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
    let yes_out = fixture.output("yes", "value");
    let no_out = fixture.output("no", "value");
    fixture
        .fire("run", vec![Value::local(3_i64)])
        .unwrap()
        .expect_success()
        .expect_local_at(yes_out, &4_i64)
        .expect_publication_count(no_out, 0);
    fixture
        .fire("run", vec![Value::local(-3_i64)])
        .unwrap()
        .expect_success()
        .expect_local_at(no_out, &2_i64)
        .expect_publication_count(yes_out, 0);
}

#[test]
fn no_result_or_option_unwrapping_is_performed_by_return_adapter() {
    let mut graph = GraphBuilder::new("opaque-domain-values");
    let optional = graph
        .node("optional", |x: i64| (x > 0).then_some(x))
        .unwrap();
    let result = graph
        .node("result", |x: i64| {
            if x >= 0 {
                Ok::<_, &'static str>(x)
            } else {
                Err("negative")
            }
        })
        .unwrap();
    let t0 = graph.expose_input("optional", optional.inlet()).unwrap();
    let t1 = graph.expose_input("result", result.inlet()).unwrap();
    graph.trigger("both", [t0, t1]).unwrap();
    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
    fixture
        .fire("both", vec![Value::local(-1_i64), Value::local(-2_i64)])
        .unwrap()
        .expect_success()
        .expect_local_at(fixture.output("optional", "value"), &None::<i64>)
        .expect_local_at(
            fixture.output("result", "value"),
            &Err::<i64, &'static str>("negative"),
        );
}
