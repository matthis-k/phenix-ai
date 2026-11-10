use phenix_graph::*;

fn run(graph: GraphBuilder, args: Vec<Value>, expected: &[i64]) {
    let report = Deployment::new(graph.compile().unwrap())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
        .fire("run", args)
        .unwrap();
    assert!(report.success(), "{:?}", report.errors());
    let values: Vec<_> = report
        .publications()
        .map(|p| *p.borrow::<i64>().unwrap())
        .collect();
    assert_eq!(values, expected);
}

#[test]
fn ordinary_input() {
    let mut graph = GraphBuilder::new("ordinary");
    let double = graph.node("double", |x: i64| x * 2).unwrap();
    let input = graph.expose_input("input", double.inlet()).unwrap();
    graph.trigger("run", [input]).unwrap();
    run(graph, vec![Value::local(21_i64)], &[42]);
}

#[test]
fn zero_input_source() {
    let mut graph = GraphBuilder::new("source");
    let source = graph.node("source", || 42_i64).unwrap();
    graph.trigger("run", [source.operation().start()]).unwrap();
    run(graph, vec![], &[42]);
}

#[cfg(feature = "derive")]
#[derive(Clone, LocalPortLayout, InputFields)]
struct Pair {
    x: i64,
    y: i64,
}

#[cfg(feature = "derive")]
#[test]
fn structured_two_fields() {
    let mut graph = GraphBuilder::new("fields");
    let sum = graph
        .node("sum", |p: Pair| p.x + p.y)
        .unwrap()
        .input(Layout::fields())
        .unwrap();
    let x = graph
        .expose_input("x", sum.inlet().field(Pair::SLOT_X).target().unwrap())
        .unwrap();
    let y = graph
        .expose_input("y", sum.inlet().field(Pair::SLOT_Y).target().unwrap())
        .unwrap();
    graph.trigger("run", [x, y]).unwrap();
    run(
        graph,
        vec![Value::local(20_i64), Value::local(22_i64)],
        &[42],
    );
}

#[cfg(feature = "derive")]
#[test]
fn independently_declared_encoded_types() {
    #[derive(Clone, ContractValue)]
    #[contract(schema = "consolidation/number")]
    struct Sender {
        number: i64,
    }
    #[derive(Clone, ContractValue)]
    #[contract(schema = "consolidation/number")]
    struct Receiver {
        number: i64,
    }
    assert_eq!(Sender::schema(), Receiver::schema());
    let mut graph = GraphBuilder::new("encoded");
    let source = graph
        .node("source", Node::wire(|x: i64| Sender { number: x }))
        .unwrap();
    let sink = graph
        .node("sink", Node::wire(|x: Receiver| x.number + 1))
        .unwrap();
    graph.connect(source.output(), sink.inlet()).unwrap();
    let input = graph.expose_input("input", source.inlet()).unwrap();
    graph.trigger("run", [input]).unwrap();
    let report = Deployment::new(graph.compile().unwrap())
        .build()
        .unwrap()
        .fire("run", vec![Value::of(41_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.errors());
    assert_eq!(
        report
            .publications()
            .last()
            .unwrap()
            .value::<i64>()
            .unwrap(),
        42
    );
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
        vec![]
    }
}

#[test]
fn custom_readiness() {
    let mut graph = GraphBuilder::new("readiness");
    let port = Port::new(
        0,
        vec![
            InputSlot::local::<i64>(0).named("left"),
            InputSlot::local::<i64>(1).named("right"),
        ],
        SelectRight,
    )
    .named("run");
    graph
        .add(
            "worker",
            NodeTemplate::with_action(
                0,
                vec![port],
                OutputSlot::local::<i64>(0).named("value"),
                |ctx: InvocationCtx| Ok(vec![emit_local(0, *ctx.shared::<i64>(1)?)]),
            ),
        )
        .unwrap();
    let left = graph.expose_input("left", "worker.in.run.left").unwrap();
    let right = graph.expose_input("right", "worker.in.run.right").unwrap();
    graph.trigger("left_only", [left]).unwrap();
    graph.trigger("run", [right]).unwrap();
    let engine = Deployment::new(graph.compile().unwrap()).build().unwrap();
    let report = engine
        .fire("left_only", vec![Value::local(10_i64)])
        .unwrap();
    assert!(report.success());
    assert_eq!(report.activation_count(), 0);
    let report = engine.fire("run", vec![Value::local(42_i64)]).unwrap();
    assert!(report.success());
    assert_eq!(report.activation_count(), 1);
    assert_eq!(
        report.publications().next().unwrap().borrow::<i64>(),
        Some(&42)
    );
}

#[test]
fn selective_routing() {
    let mut graph = GraphBuilder::new("routing");
    let left = graph.node("left", |x: i64| x + 1).unwrap();
    let right = graph
        .node("right", |_: i64| panic!("unselected route ran"))
        .unwrap();
    let target = left.inlet();
    let source = graph
        .node("source", move |x: i64| Output::to(&target, x))
        .unwrap();
    graph.connect(source.output(), left.inlet()).unwrap();
    graph.connect(source.output(), right.inlet()).unwrap();
    let input = graph.expose_input("input", source.inlet()).unwrap();
    graph.trigger("run", [input]).unwrap();
    run(graph, vec![Value::local(41_i64)], &[41, 42]);
}

#[cfg(feature = "derive")]
#[test]
fn nested_f_x_y_equals_a_x_b_y() {
    let mut b = Module::new(
        "B",
        InputInterface::single(Layout::<i64>::atomic()).unwrap(),
    )
    .unwrap();
    let double = b.node("double", |y: i64| y * 2).unwrap();
    b.expose(b.inlet(), double.inlet()).unwrap();
    let b = b.finish(double.output()).unwrap();
    let mut f = Module::new(
        "F",
        InputInterface::single(Layout::<Pair>::fields()).unwrap(),
    )
    .unwrap();
    let b = f.instantiate("B", &b).unwrap();
    let a = f
        .node("A", |p: Pair| p.x + p.y)
        .unwrap()
        .input(Layout::fields())
        .unwrap();
    f.expose(
        f.inlet().field(Pair::SLOT_X),
        a.inlet().field(Pair::SLOT_X).target().unwrap(),
    )
    .unwrap();
    f.expose(f.inlet().field(Pair::SLOT_Y), b.inlet()).unwrap();
    f.connect(b.output(), a.inlet().field(Pair::SLOT_Y).target().unwrap())
        .unwrap();
    let f = f.finish(a.output()).unwrap();
    let mut graph = GraphBuilder::new("nested");
    let f = graph.instantiate("F", &f).unwrap();
    let x = graph
        .expose_input("x", f.inlet().field(Pair::SLOT_X).target().unwrap())
        .unwrap();
    let y = graph
        .expose_input("y", f.inlet().field(Pair::SLOT_Y).target().unwrap())
        .unwrap();
    graph.trigger("run", [x, y]).unwrap();
    // Retaining B's intermediate payload would prevent owned field reconstruction in A.
    let report = Deployment::new(graph.compile().unwrap())
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap()
        .fire("run", vec![Value::local(2_i64), Value::local(20_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.errors());
    assert_eq!(report.activation_count(), 2);
    let values: Vec<_> = report
        .publications()
        .filter_map(|p| p.borrow::<i64>().copied())
        .collect();
    assert_eq!(values, [42]);
}

#[test]
fn node_to_module_replacement() {
    let mut module = Module::new(
        "implementation",
        InputInterface::single(Layout::<i64>::atomic()).unwrap(),
    )
    .unwrap();
    let add = module.node("add", |x: i64| x + 1).unwrap();
    let double = module.node("double", |x: i64| x * 2).unwrap();
    module.expose(module.inlet(), add.inlet()).unwrap();
    module.connect(add.output(), double.inlet()).unwrap();
    let package = module.finish(double.output()).unwrap();
    let mut graph = GraphBuilder::new("replacement");
    let target = graph.node("target", |x: i64| (x + 1) * 2).unwrap();
    let sink = graph.node("sink", |x: i64| x + 10).unwrap();
    graph.connect(target.output(), sink.inlet()).unwrap();
    let input = graph.expose_input("input", target.inlet()).unwrap();
    graph.trigger("run", [input]).unwrap();
    run(graph.clone(), vec![Value::local(3_i64)], &[8, 18]);
    graph.replace("target", &package).unwrap();
    assert!(graph.expose_input("stale", target.inlet()).is_err());
    run(graph, vec![Value::local(3_i64)], &[4, 8, 18]);
}

#[test]
fn multiple_contributors() {
    for reverse in [false, true] {
        let mut base = PatchBuilder::new("base");
        base.node("a", |x: i64| x + 1)
            .expose_input("input", "a")
            .trigger("run", &["input"]);
        let mut extension = PatchBuilder::new("extension");
        extension.after("base").node("b", |x: i64| x * 2);
        extension.connect("a", "b").unwrap();
        let mut planner = PatchPlanner::new();
        for patch in if reverse {
            [extension, base]
        } else {
            [base, extension]
        } {
            planner.add_patch(patch).unwrap();
        }
        let (built, resolution) = planner.build("contributors").unwrap();
        assert_eq!(resolution.selected, ["base", "extension"]);
        let report = Deployment::new(built.graph)
            .build()
            .unwrap()
            .fire("run", vec![Value::local(20_i64)])
            .unwrap();
        assert!(report.success(), "{:?}", report.errors());
        assert_eq!(
            report.publications().last().unwrap().borrow::<i64>(),
            Some(&42)
        );
    }
}

#[test]
fn finite_cycle_repeated_activations() {
    let mut graph = GraphBuilder::new("cycle");
    let step = graph
        .node("step", |x: i64| {
            if x > 0 {
                Output::all(x - 1)
            } else {
                Output::none()
            }
        })
        .unwrap();
    graph.connect(step.output(), step.inlet()).unwrap();
    let input = graph.expose_input("input", step.inlet()).unwrap();
    graph.trigger("run", [input]).unwrap();
    let engine = Deployment::new(graph.compile().unwrap())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap();
    for start in [0_i64, 3, 5, 3] {
        let report = engine.fire("run", vec![Value::local(start)]).unwrap();
        assert!(report.success(), "{:?}", report.errors());
        assert!(!report.budget_exhausted());
        assert_eq!(report.activation_count(), start as usize + 1);
        let values: Vec<_> = report
            .publications()
            .map(|p| *p.borrow::<i64>().unwrap())
            .collect();
        assert_eq!(values, (0..start).rev().collect::<Vec<_>>());
    }
}

#[test]
fn documented_rust_excerpts_are_compiled_test_source() {
    let normalize = |text: &str| text.lines().map(str::trim).collect::<Vec<_>>().join("\n");
    let source = normalize(include_str!("consolidation_api.rs"));
    let doc = include_str!("../API-EXAMPLES.md");
    let mut count = 0;
    for block in doc.split("```rust\n").skip(1) {
        let excerpt = block.split("```").next().unwrap().trim();
        assert!(
            source.contains(&normalize(excerpt)),
            "documentation drift:\n{excerpt}"
        );
        count += 1;
    }
    assert_eq!(count, 10);
}
