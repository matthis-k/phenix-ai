#![cfg(feature = "derive")]

use phenix_graph::{
    DeclaredValue, Deployment, GraphBuilder, GraphContract, GraphUnit, InterfaceShape, Layout,
    LocalPortLayout, ModulePackage, ReportRetention, SlotField, Value,
};

#[derive(Clone, LocalPortLayout, phenix_graph::InputFields)]
struct Pair {
    left: i64,
    right: i64,
}

#[derive(Clone, LocalPortLayout, phenix_graph::InputFields)]
struct Request {
    #[port(flatten)]
    pair: Pair,
}

fn contract() -> GraphContract {
    GraphContract::new(
        "grouped/subtract@1",
        InterfaceShape::new()
            .input_slot("request", "pair.left", DeclaredValue::local::<i64>())
            .unwrap()
            .input_slot("request", "pair.right", DeclaredValue::local::<i64>())
            .unwrap()
            .output("result", DeclaredValue::local::<i64>())
            .unwrap(),
    )
    .unwrap()
}

fn provider() -> ModulePackage {
    let mut graph = GraphBuilder::new("subtract");
    let node = graph
        .node("private", |x: Request| x.pair.left - x.pair.right)
        .unwrap()
        .input(Layout::deep())
        .unwrap();
    for key in [Pair::SLOT_LEFT, Pair::SLOT_RIGHT] {
        let path = Request::SLOT_PAIR.then(key);
        graph
            .expose_input_field(
                "request",
                path.clone(),
                node.inlet().field(path).target().unwrap(),
            )
            .unwrap();
    }
    graph.expose_output("result", node.output()).unwrap();
    graph.implements_public(&contract()).unwrap();
    graph.module()
}

#[test]
fn nested_grouped_paths_bind_connect_disconnect_export_and_trigger() {
    let mut wrapper = GraphBuilder::new("wrapper");
    let inner = wrapper.instance("inner", &provider()).unwrap();
    for key in [Pair::SLOT_LEFT, Pair::SLOT_RIGHT] {
        let path = Request::SLOT_PAIR.then(key);
        let input = inner
            .operation()
            .unwrap()
            .inlets::<Request, phenix_graph::LocalValue>()
            .port(phenix_graph::InletPort::<Request, Request>::new("request"))
            .field(path.clone())
            .target()
            .unwrap();
        wrapper.expose_input_field("request", path, input).unwrap();
    }
    wrapper
        .expose_output(
            "result",
            inner
                .operation()
                .unwrap()
                .output::<i64, phenix_graph::LocalValue>()
                .unwrap(),
        )
        .unwrap();
    wrapper.implements_public(&contract()).unwrap();

    let mut graph = GraphBuilder::new("consumer");
    graph.instance("service", &wrapper.module()).unwrap();
    let left = graph.node("left", |x: i64| x).unwrap();
    let right = graph.node("right", |x: i64| x).unwrap();
    // Endpoints own their declaration tokens. No tuple/block/clone of the binding is needed.
    let bound = graph.bind_contract("service", &contract()).unwrap();
    let operation = bound.operation().unwrap();
    let request = operation
        .inlets::<Request, phenix_graph::LocalValue>()
        .port(phenix_graph::InletPort::<Request, Request>::new("request"));
    let left_input = request
        .field(Request::SLOT_PAIR.then(Pair::SLOT_LEFT))
        .target()
        .unwrap();
    let right_input = request
        .field(Request::SLOT_PAIR.then(Pair::SLOT_RIGHT))
        .target()
        .unwrap();
    let result = operation.output::<i64, phenix_graph::LocalValue>().unwrap();
    drop(request);
    drop(operation);
    graph.connect(left.output(), left_input.clone()).unwrap();
    assert!(graph.connect(left.output(), left_input.clone()).is_err());
    graph.disconnect(left.output(), left_input.clone()).unwrap();
    assert!(graph.disconnect(left.output(), left_input.clone()).is_err());
    graph.connect(left.output(), left_input).unwrap();
    graph.connect(right.output(), right_input).unwrap();
    graph.expose_output("result", result).unwrap();
    graph
        .expose_input_field(
            "request",
            SlotField::<(), i64>::new("pair.left"),
            left.inlet(),
        )
        .unwrap();
    graph
        .expose_input_field(
            "request",
            SlotField::<(), i64>::new("pair.right"),
            right.inlet(),
        )
        .unwrap();
    let left = graph.public_input_slot("request", "pair.left").unwrap();
    let right = graph.public_input_slot("request", "pair.right").unwrap();
    assert_ne!(left.id(), right.id());
    assert!(graph.public_input("request").is_err());
    let trigger = graph.trigger("subtract", [left, right]).unwrap();
    let compiled = graph.compile().unwrap();
    for path in ["service", "service/inner"] {
        contract()
            .check_implementation(&compiled.find(path).unwrap())
            .unwrap();
    }
    let report = Deployment::new(compiled)
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap()
        .fire(
            trigger.get(),
            vec![Value::local(13_i64), Value::local(4_i64)],
        )
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(
        report.publications().last().unwrap().borrow::<i64>(),
        Some(&9)
    );
}

#[test]
fn duplicate_malformed_unpromised_and_wrong_type_paths_fail_without_mutation() {
    let mut graph = GraphBuilder::new("consumer");
    let service = graph.instance("service", &provider()).unwrap();
    let valid = Request::SLOT_PAIR.then(Pair::SLOT_LEFT);
    let input = service
        .operation()
        .unwrap()
        .inlets::<Request, phenix_graph::LocalValue>()
        .port(phenix_graph::InletPort::<Request, Request>::new("request"))
        .field(valid.clone())
        .target()
        .unwrap();
    graph
        .expose_input_field("request", valid.clone(), input.clone())
        .unwrap();
    assert!(
        graph
            .expose_input_field("request", valid, input.clone())
            .is_err()
    );
    assert_eq!(
        graph
            .inspect("")
            .unwrap()
            .inspect()
            .unwrap()
            .shape
            .inputs()
            .count(),
        1
    );
    for name in [
        "",
        ".",
        "..",
        "pair/missing",
        "pair\nleft",
        "pair..left",
        "missing",
    ] {
        let key = SlotField::<Request, i64>::new(name);
        assert!(
            service
                .operation()
                .unwrap()
                .inlets::<Request, phenix_graph::LocalValue>()
                .port(phenix_graph::InletPort::<Request, Request>::new("request"))
                .field(key)
                .target()
                .is_err()
        );
        let bound = graph.bind_contract("service", &contract()).unwrap();
        assert!(
            bound
                .operation()
                .unwrap()
                .inlets::<Request, phenix_graph::LocalValue>()
                .port(phenix_graph::InletPort::<Request, Request>::new("request"))
                .field(SlotField::<Request, i64>::new(name))
                .target()
                .is_err()
        );
    }
    assert!(
        service
            .operation()
            .unwrap()
            .inlets::<Request, phenix_graph::LocalValue>()
            .port(phenix_graph::InletPort::<Request, Request>::new("request"))
            .field(SlotField::<Request, bool>::new("pair.left"))
            .target()
            .is_err()
    );
    assert!(
        service
            .operation()
            .unwrap()
            .inlets::<Request, phenix_graph::EncodedValue>()
            .port(phenix_graph::InletPort::<Request, Request>::new("request"))
            .field(Request::SLOT_PAIR.then(Pair::SLOT_LEFT))
            .target()
            .is_err()
    );
    assert!(
        graph
            .expose_input_field("bad/port", Request::SLOT_PAIR.then(Pair::SLOT_LEFT), input)
            .is_err()
    );
    graph.compile().unwrap();
}

#[test]
fn wire_node_fields_export_without_raw_slot_lookup() {
    use phenix_graph::{ContractValue, Node, PortLayout};
    #[derive(ContractValue, PortLayout, phenix_graph::InputFields)]
    #[contract(schema = "grouped/wire-pair@1")]
    struct WirePair {
        left: i64,
        right: i64,
    }
    let left = WirePair::SLOT_LEFT;
    let right = WirePair::SLOT_RIGHT;
    let mut graph = GraphBuilder::new("wire-node");
    let node = graph
        .node(
            "subtract",
            Node::wire_layout(|x: WirePair| x.left - x.right, Layout::wire_fields()),
        )
        .unwrap();
    let left = graph
        .expose_input_field("request", left, node.inlet().field(left).target().unwrap())
        .unwrap();
    let right = graph
        .expose_input_field(
            "request",
            right,
            node.inlet().field(right).target().unwrap(),
        )
        .unwrap();
    let trigger = graph.trigger("run", [left, right]).unwrap();
    let report = Deployment::new(graph.compile().unwrap())
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap()
        .fire(trigger.get(), vec![Value::of(13_i64), Value::of(4_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.publications().last().unwrap().value::<i64>(), Ok(9));
}
