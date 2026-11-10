#![cfg(feature = "derive")]

use phenix_graph::{
    Deployment, FindResult, GraphBuilder, GraphUnit, InputInterface, Layout, LocalPortLayout,
    Module, ReportRetention, SinglePort, TypedModulePackage, Value,
};

#[derive(Clone, LocalPortLayout, phenix_graph::InputFields)]
struct AArgs {
    first: i64,
    second: i64,
}

#[derive(Clone, LocalPortLayout, phenix_graph::InputFields)]
struct FArgs {
    x: i64,
    y: i64,
}

type FInterface = SinglePort<FArgs, phenix_graph::LocalFlatten<FArgs>>;
type FPackage = TypedModulePackage<FInterface, i64>;

fn partial_exposure() -> FPackage {
    let mut f = Module::new(
        "partial-exposure",
        InputInterface::single(Layout::<FArgs>::fields()).unwrap(),
    )
    .unwrap();
    let a = f
        .node("a", |x: AArgs| x.first + x.second)
        .unwrap()
        .input(Layout::fields())
        .unwrap();
    let b = f.node("b", |x: i64| x * 3).unwrap();

    f.connect(
        b.output(),
        a.inlet().field(AArgs::SLOT_SECOND).target().unwrap(),
    )
    .unwrap();
    f.expose(
        f.inlet().field(FArgs::SLOT_X),
        a.inlet().field(AArgs::SLOT_FIRST).target().unwrap(),
    )
    .unwrap();
    f.expose(f.inlet().field(FArgs::SLOT_Y), b.inlet()).unwrap();
    f.finish(a.output()).unwrap()
}

fn direct() -> FPackage {
    let mut f = Module::new(
        "direct",
        InputInterface::single(Layout::<FArgs>::fields()).unwrap(),
    )
    .unwrap();
    let node = f
        .node("single", |x: FArgs| x.x + x.y * 3)
        .unwrap()
        .input(Layout::fields())
        .unwrap();
    f.expose(
        f.inlet().field(FArgs::SLOT_X),
        node.inlet().field(FArgs::SLOT_X).target().unwrap(),
    )
    .unwrap();
    f.expose(
        f.inlet().field(FArgs::SLOT_Y),
        node.inlet().field(FArgs::SLOT_Y).target().unwrap(),
    )
    .unwrap();
    f.finish(node.output()).unwrap()
}

fn wrap(name: &str, child: &FPackage) -> FPackage {
    let mut f = Module::new(
        name,
        InputInterface::single(Layout::<FArgs>::fields()).unwrap(),
    )
    .unwrap();
    let child = f.instantiate("child", child).unwrap();
    for key in [FArgs::SLOT_X, FArgs::SLOT_Y] {
        f.expose(
            f.inlet().field(key),
            child.inlet().field(key).target().unwrap(),
        )
        .unwrap();
    }
    f.finish(child.output()).unwrap()
}

#[test]
fn partial_exposure_preserves_internal_edge_and_declared_boundary_only() {
    let mut graph = GraphBuilder::new("root");
    let f = graph.instantiate("f", &partial_exposure()).unwrap();
    assert!(f.inlet().field(FArgs::SLOT_X).target().is_ok());
    assert!(f.inlet().field(FArgs::SLOT_Y).target().is_ok());
    assert!(graph.discover("f/a").is_err());
    let hidden = f
        .inlet()
        .field(phenix_graph::SlotField::<FArgs, i64>::new("second"))
        .target();
    assert!(
        hidden.is_err(),
        "internal A.second is not in the public layout"
    );
    let x = graph
        .expose_input("x", f.inlet().field(FArgs::SLOT_X).target().unwrap())
        .unwrap();
    let y = graph
        .expose_input("y", f.inlet().field(FArgs::SLOT_Y).target().unwrap())
        .unwrap();
    let trigger = graph.trigger("run", [x, y]).unwrap();
    let compiled = graph.compile().unwrap();
    assert_eq!(compiled.physical_node_count(), 2);
    let Some(FindResult::Module(module)) = compiled.find("f") else {
        panic!("module boundary missing")
    };
    let shape = module.inspect().unwrap().shape;
    assert_eq!(shape.inputs().count(), 2);
    let ports: Vec<_> = module.input_ports().collect();
    assert_eq!(ports.len(), 2);
    assert!(ports.iter().all(|port| port.port_name() == "run"));
    assert_eq!(ports[0].slot_name(), "x");
    assert_eq!(ports[1].slot_name(), "y");
    let x_targets = module.resolved_input_targets(ports[0].id()).unwrap();
    let y_targets = module.resolved_input_targets(ports[1].id()).unwrap();
    assert_eq!(x_targets.len(), 1);
    assert_eq!(y_targets.len(), 1);
    assert_ne!(x_targets[0].node_id(), y_targets[0].node_id());
    assert!(module.resolved_output_source().is_some());
    let report = Deployment::new(compiled)
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap()
        .fire(
            trigger.get(),
            vec![Value::local(5_i64), Value::local(7_i64)],
        )
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.publications().count(), 1);
    assert_eq!(
        report.publications().next().unwrap().borrow::<i64>(),
        Some(&26)
    );
}

#[test]
fn nested_repeated_modules_and_a_node_share_the_same_public_interface() {
    let composite = wrap("outer", &wrap("inner", &partial_exposure()));
    let direct = direct();
    let mut graph = GraphBuilder::new("root");
    let first = graph.instantiate("first", &composite).unwrap();
    let second = graph.instantiate("second", &composite).unwrap();
    let direct_module = graph.instantiate("direct", &direct).unwrap();
    let node = graph
        .node("single-node", |x: FArgs| x.x + x.y * 3)
        .unwrap()
        .input(Layout::fields())
        .unwrap();

    let first_shape = graph.inspect("first").unwrap().inspect().unwrap().shape;
    let second_shape = graph.inspect("second").unwrap().inspect().unwrap().shape;
    let direct_shape = graph.inspect("direct").unwrap().inspect().unwrap().shape;
    let node_shape = graph
        .inspect("single-node")
        .unwrap()
        .inspect()
        .unwrap()
        .shape;
    assert_eq!(first_shape, second_shape);
    assert_eq!(first_shape, direct_shape);
    assert_eq!(first_shape, node_shape);

    let mut triggers = Vec::new();
    for (name, inlet) in [
        ("first", first.inlet()),
        ("second", second.inlet()),
        ("direct", direct_module.inlet()),
    ] {
        let x = graph
            .expose_input(
                format!("{name}-x"),
                inlet.field(FArgs::SLOT_X).target().unwrap(),
            )
            .unwrap();
        let y = graph
            .expose_input(
                format!("{name}-y"),
                inlet.field(FArgs::SLOT_Y).target().unwrap(),
            )
            .unwrap();
        triggers.push(graph.trigger(name, [x, y]).unwrap());
    }
    let x = graph
        .expose_input(
            "node-x",
            node.inlet().field(FArgs::SLOT_X).target().unwrap(),
        )
        .unwrap();
    let y = graph
        .expose_input(
            "node-y",
            node.inlet().field(FArgs::SLOT_Y).target().unwrap(),
        )
        .unwrap();
    triggers.push(graph.trigger("single-node", [x, y]).unwrap());

    let compiled = graph.compile().unwrap();
    assert_eq!(compiled.physical_node_count(), 6);
    let engine = Deployment::new(compiled)
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap();
    for trigger in triggers {
        let report = engine
            .fire(
                trigger.get(),
                vec![Value::local(11_i64), Value::local(2_i64)],
            )
            .unwrap();
        assert!(report.success(), "{:?}", report.error_messages());
        assert_eq!(report.publications().count(), 1);
        assert_eq!(
            report.publications().next().unwrap().borrow::<i64>(),
            Some(&17)
        );
    }
}

#[derive(Clone, LocalPortLayout, phenix_graph::InputFields)]
struct DArgs {
    a: i64,
    b: i64,
    c: i64,
    bias: i64,
}

#[test]
fn three_independent_children_feed_a_join_with_external_bias() {
    let mut f = Module::new(
        "abcd",
        InputInterface::single(Layout::<DArgs>::fields()).unwrap(),
    )
    .unwrap();
    let a = f.node("a", |x: i64| x + 1).unwrap();
    let b = f.node("b", |x: i64| x * 2).unwrap();
    let c = f.node("c", |x: i64| x * 3).unwrap();
    let d = f
        .node("d", |x: DArgs| x.a + x.b + x.c + x.bias)
        .unwrap()
        .input(Layout::fields())
        .unwrap();
    f.connect(a.output(), d.inlet().field(DArgs::SLOT_A).target().unwrap())
        .unwrap();
    f.connect(b.output(), d.inlet().field(DArgs::SLOT_B).target().unwrap())
        .unwrap();
    f.connect(c.output(), d.inlet().field(DArgs::SLOT_C).target().unwrap())
        .unwrap();
    for (key, child) in [
        (DArgs::SLOT_A, a.inlet()),
        (DArgs::SLOT_B, b.inlet()),
        (DArgs::SLOT_C, c.inlet()),
    ] {
        f.expose(f.inlet().field(key), child).unwrap();
    }
    f.expose(
        f.inlet().field(DArgs::SLOT_BIAS),
        d.inlet().field(DArgs::SLOT_BIAS).target().unwrap(),
    )
    .unwrap();
    let package = f.finish(d.output()).unwrap();

    let mut graph = GraphBuilder::new("root");
    let f = graph.instantiate("f", &package).unwrap();
    let mut inputs = Vec::new();
    for key in [
        DArgs::SLOT_A,
        DArgs::SLOT_B,
        DArgs::SLOT_C,
        DArgs::SLOT_BIAS,
    ] {
        inputs.push(
            graph
                .expose_input(key.name(), f.inlet().field(key).target().unwrap())
                .unwrap(),
        );
    }
    let trigger = graph.trigger("run", inputs).unwrap();
    let compiled = graph.compile().unwrap();
    assert_eq!(compiled.physical_node_count(), 4);
    let report = Deployment::new(compiled)
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap()
        .fire(
            trigger.get(),
            [1_i64, 2, 3, 4].into_iter().map(Value::local).collect(),
        )
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.publications().count(), 1);
    assert_eq!(
        report.publications().next().unwrap().borrow::<i64>(),
        Some(&19)
    );
}

#[test]
fn a_projected_single_port_does_not_join_its_internal_targets() {
    let mut module = Module::new(
        "independent",
        InputInterface::single(Layout::<FArgs>::fields()).unwrap(),
    )
    .unwrap();
    let a = module.node("a", |x: i64| x + 1).unwrap();
    let b = module.node("b", |x: i64| x + 2).unwrap();
    module
        .expose(module.inlet().field(FArgs::SLOT_X), a.inlet())
        .unwrap();
    module
        .expose(module.inlet().field(FArgs::SLOT_Y), b.inlet())
        .unwrap();
    let package = module.finish(a.output()).unwrap();
    let mut graph = GraphBuilder::new("root");
    let child = graph.instantiate("child", &package).unwrap();
    let left = graph
        .expose_input("left", child.inlet().field(FArgs::SLOT_X).target().unwrap())
        .unwrap();
    let trigger = graph.trigger("left_only", [left]).unwrap();
    let compiled = graph.compile().unwrap();
    assert_eq!(compiled.physical_node_count(), 2);
    let report = Deployment::new(compiled)
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap()
        .fire(trigger.get(), vec![Value::local(7_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.publications().count(), 1);
    assert_eq!(
        report.publications().next().unwrap().borrow::<i64>(),
        Some(&8)
    );
}
