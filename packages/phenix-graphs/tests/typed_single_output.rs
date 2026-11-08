//! Contract test for exactly one typed payload, including a unit payload per node activation.
//! Uses the production compiler, scheduler and unmodified node registration.
use phenix_graph::testing::ExpectWave;
use phenix_graph::{
    AllSameCohort, Atomic, Deployment, Emission, IntoEmission, ModuleCatalog, ModuleDefBuilder,
    ModuleTriggerTarget, NodeTemplate, OutputSchema, OutputSlot, ReportRetention, TypedPort, Value,
    emit,
};

struct Scalar(i64);
impl OutputSchema for Scalar {
    fn output_slot() -> OutputSlot {
        OutputSlot::typed::<i64>(0).named("value")
    }
}
impl IntoEmission for Scalar {
    fn into_emission(self) -> Emission {
        emit(0, self.0)
    }
}

#[test]
fn typed_action_declares_one_output_and_publishes_one_payload() {
    let mut module = ModuleDefBuilder::new("typed-single-output");
    let node = module
        .register(
            "increment",
            NodeTemplate::multi_port::<i64, Scalar, _>(
                vec![TypedPort::new::<Atomic<i64>, _>(0, "run", AllSameCohort, |n| n).unwrap()],
                |value, _ctx| Ok(Scalar(value + 1)),
            ),
        )
        .unwrap();
    module
        .export_input("request", vec![node.input::<i64>("run", "value")])
        .unwrap();
    module
        .export_output("response", node.output::<i64>("value"))
        .unwrap();
    module.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
    let mut catalog = ModuleCatalog::new();
    catalog.add(module.build()).unwrap();
    let report = Deployment::new(catalog.compile("typed-single-output").unwrap())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire(1, vec![Value::of(41_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.publication_events().len(), 1);
    assert_eq!(report.publications().next().unwrap().value::<i64>(), Ok(42));
}

#[test]
fn unit_action_publishes_unit_and_can_have_no_recipients() {
    let mut module = ModuleDefBuilder::new("typed-no-output");
    let node = module
        .register(
            "observe",
            NodeTemplate::multi_port::<i64, (), _>(
                vec![TypedPort::new::<Atomic<i64>, _>(0, "run", AllSameCohort, |n| n).unwrap()],
                |_value, _ctx| Ok(()),
            ),
        )
        .unwrap();
    module
        .export_input("request", vec![node.input::<i64>("run", "value")])
        .unwrap();
    module.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
    let mut catalog = ModuleCatalog::new();
    catalog.add(module.build()).unwrap();
    let report = Deployment::new(catalog.compile("typed-no-output").unwrap())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire(1, vec![Value::of(7_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.invocation_count(1), 1);
    assert_eq!(report.publication_events().len(), 1);
    assert_eq!(report.delivery_events().len(), 0);
    assert_eq!(report.publications().next().unwrap().value::<()>(), Ok(()));
}

#[test]
fn structured_input_can_route_one_payload_without_a_special_node_kind() {
    use phenix_graph::testing::Fixture;
    use phenix_graph::{GraphBuilder, Layout, Output};

    let mut graph = GraphBuilder::new("layout-and-routing");
    let left = graph.node("left", |value: i64| value + 1).unwrap();
    let right = graph.node("right", |value: i64| value * 2).unwrap();
    let (left_target, right_target) = (left.inlet(), right.inlet());
    let router = graph
        .node("router", move |(a, b): (i64, i64)| {
            let prefer_left = a > b;
            Output::to(
                if prefer_left {
                    &left_target
                } else {
                    &right_target
                },
                a + b,
            )
        })
        .unwrap()
        .input(Layout::fields())
        .unwrap();
    graph.connect(router.output(), left.inlet()).unwrap();
    graph.connect(router.output(), right.inlet()).unwrap();
    let a = graph
        .expose_input("a", router.local_slot::<i64>("0"))
        .unwrap();
    let b = graph
        .expose_input("b", router.local_slot::<i64>("1"))
        .unwrap();
    graph.trigger("run", [a, b]).unwrap();

    let fixture = Fixture::new(graph.compile().unwrap()).unwrap();
    fixture
        .fire("run", vec![Value::local(5_i64), Value::local(2_i64)])
        .unwrap()
        .expect_success()
        .expect_deliveries_from(
            fixture.output("router", "value"),
            &[fixture.input("left", "run", "value")],
        );
    fixture
        .fire("run", vec![Value::local(1_i64), Value::local(3_i64)])
        .unwrap()
        .expect_success()
        .expect_deliveries_from(
            fixture.output("router", "value"),
            &[fixture.input("right", "run", "value")],
        );
}
