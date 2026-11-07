//! Declaration handles retain their exact input layout. Configuration remains
//! an authoring-time operation; execution uses the regular Deployment engine.
use phenix_graph::{Deployment, GraphBuilder, Layout, Node, ReportRetention, Value};

#[test]
fn explicit_atomic_layout_retains_the_typed_inlet() {
    let mut graph = GraphBuilder::new("atomic-layout-inlet");
    let source = graph.node("source", |x: i64| x * 2).unwrap()
        .input(Layout::atomic()).unwrap();
    let consumer = graph.node("consumer", |x: i64| x + 1).unwrap();
    graph.connect(source.output(), consumer.inlet()).unwrap();
    let request = graph.expose_input("request", source.inlet()).unwrap();
    graph.trigger("run", [request]).unwrap();
    let report = Deployment::new(graph.compile().unwrap())
        .report_retention(ReportRetention::RetainPayloads)
        .build().unwrap().fire("run", vec![Value::local(20_i64)]).unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.publications().last().unwrap().borrow::<i64>(), Some(&41));
}

#[derive(Debug)]
struct MoveOnly { value: u64 }

#[test]
fn custom_move_only_atomic_layout_also_has_a_typed_inlet() {
    let mut graph = GraphBuilder::new("move-only-inlet");
    let move_only = graph.node("consumer", Node::with_layout(
        |v: MoveOnly| v.value * 2,
        Layout::atomic(),
    )).unwrap();
    let request = graph.expose_input("request", move_only.inlet()).unwrap();
    graph.trigger("run", [request]).unwrap();
    let report = Deployment::new(graph.compile().unwrap())
        .report_retention(ReportRetention::RetainPayloads)
        .build().unwrap().fire("run", vec![Value::local(MoveOnly { value: 21 })]).unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.publications().last().unwrap().borrow::<u64>(), Some(&42));
}

#[cfg(feature = "derive")]
#[test]
fn selected_fields_retain_typed_field_endpoints() {
    use phenix_graph::LocalPortLayout;
    #[derive(Clone, LocalPortLayout)]
    struct Pair { left: i64, right: i64 }
    let mut graph = GraphBuilder::new("field-layout");
    let left = graph.node("left", |x: i64| x).unwrap();
    let right = graph.node("right", |x: i64| x).unwrap();
    let sum = graph.node("sum", |p: Pair| p.left + p.right).unwrap()
        .input(Layout::fields()).unwrap();
    graph.connect(left.output(), sum.field(Pair::SLOT_LEFT)).unwrap();
    graph.connect(right.output(), sum.field(Pair::SLOT_RIGHT)).unwrap();
    let left_in = graph.expose_input("a", left.inlet()).unwrap();
    let right_in = graph.expose_input("b", right.inlet()).unwrap();
    graph.trigger("run", [left_in, right_in]).unwrap();
    let report = Deployment::new(graph.compile().unwrap())
        .report_retention(ReportRetention::RetainPayloads)
        .build().unwrap().fire("run", vec![Value::local(19_i64), Value::local(23_i64)]).unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.publications().last().unwrap().borrow::<i64>(), Some(&42));
}

#[test]
fn signature_identity_is_explicitly_local_or_wire() {
    use phenix_graph::{DeclaredValue, NodeSignature};
    let signature = NodeSignature {
        input: DeclaredValue::local::<i64>(),
        output: DeclaredValue::wire::<i64>(),
    };
    assert!(signature.input.local_type.is_some());
    assert!(signature.output.local_type.is_none());
    assert_eq!(signature.output.schema, "example.i64");
}
