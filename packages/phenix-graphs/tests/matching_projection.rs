#![cfg(feature = "derive")]

use phenix_graph::*;

#[derive(Clone, LocalPortLayout, InputFields)]
struct Pair {
    left: i64,
    right: i64,
}

#[test]
fn matching_projection_preserves_structured_execution_without_a_module_action() {
    let mut module = Module::new(
        "sum",
        InputInterface::single(Layout::<Pair>::fields()).unwrap(),
    )
    .unwrap();
    let child = module
        .node("child", |p: Pair| p.left + p.right)
        .unwrap()
        .input(Layout::fields())
        .unwrap();

    // An explicit one-call projection replaces one expose per physical leaf.
    module.expose_matching(child.operation().inlets()).unwrap();
    let sum = module.finish(child.output()).unwrap();

    let mut graph = GraphBuilder::new("root");
    let instance = graph.instantiate("sum", &sum).unwrap();
    let left = graph
        .expose_input(
            "left",
            instance.inlet().field(Pair::SLOT_LEFT).target().unwrap(),
        )
        .unwrap();
    let right = graph
        .expose_input(
            "right",
            instance.inlet().field(Pair::SLOT_RIGHT).target().unwrap(),
        )
        .unwrap();
    graph.trigger("run", [left, right]).unwrap();
    let compiled = graph.compile().unwrap();
    assert_eq!(compiled.physical_node_count(), 1);

    let report = Deployment::new(compiled)
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap()
        .fire("run", vec![Value::local(4_i64), Value::local(6_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(
        report.publications().next().unwrap().borrow::<i64>(),
        Some(&10)
    );
}

#[test]
fn matching_projection_supports_fanout_to_independent_children() {
    let mut module = Module::new(
        "fanout",
        InputInterface::single(Layout::<i64>::atomic()).unwrap(),
    )
    .unwrap();
    let first = module.node("first", |x: i64| x + 1).unwrap();
    let second = module.node("second", |x: i64| x + 2).unwrap();

    module.expose_matching(first.operation().inlets()).unwrap();
    module.expose_matching(second.operation().inlets()).unwrap();
    let package = module.finish(first.output()).unwrap();

    let mut graph = GraphBuilder::new("root");
    graph.instantiate("child", &package).unwrap();
    let compiled = graph.compile().unwrap();
    assert_eq!(compiled.physical_node_count(), 2);
    let Some(FindResult::Module(child)) = compiled.find("child") else {
        panic!("missing module instance")
    };
    let inlet = child.input_ports().next().unwrap();
    assert_eq!(child.resolved_input_targets(inlet.id()).unwrap().len(), 2);
}

#[test]
fn duplicate_projection_fails_without_exposing_other_fields() {
    let mut module = Module::new(
        "duplicate",
        InputInterface::single(Layout::<Pair>::fields()).unwrap(),
    )
    .unwrap();
    let child = module
        .node("child", |p: Pair| p.left + p.right)
        .unwrap()
        .input(Layout::fields())
        .unwrap();

    module
        .expose(
            module.inlet().field(Pair::SLOT_LEFT),
            child.inlet().field(Pair::SLOT_LEFT).target().unwrap(),
        )
        .unwrap();
    assert!(module.expose_matching(child.operation().inlets()).is_err());

    // No partial commit: the right-hand leaf was not silently projected.
    assert!(module.finish(child.output()).is_err());
}

#[test]
fn matching_projection_rejects_foreign_and_stale_interfaces() {
    let mut module = Module::new(
        "scope",
        InputInterface::single(Layout::<Pair>::fields()).unwrap(),
    )
    .unwrap();
    let child = module
        .node("child", |p: Pair| p.left + p.right)
        .unwrap()
        .input(Layout::fields())
        .unwrap();

    let mut foreign = GraphBuilder::new("foreign");
    let elsewhere = foreign
        .node("child", |p: Pair| p.left + p.right)
        .unwrap()
        .input(Layout::fields())
        .unwrap();
    assert!(module.expose_matching(elsewhere.operation().inlets()).is_err());

    // Discovery is type-selectable: a caller may request the wrong owner type.
    // The graph must still compare the entire actual leaf frontier.
    module.node("impostor", |x: i64| x).unwrap();
    let mismatched = module
        .discover("impostor")
        .unwrap()
        .inlets::<SinglePort<Pair, LocalFlatten<Pair>>, LocalValue>();
    assert!(module.expose_matching(mismatched).is_err());

    let stale = child.operation().inlets();
    let child = child.input(Layout::fields()).unwrap();
    assert!(module.expose_matching(stale).is_err());

    // An invalid attempt cannot poison subsequent valid construction.
    module.expose_matching(child.operation().inlets()).unwrap();
    module.finish(child.output()).unwrap();
}
