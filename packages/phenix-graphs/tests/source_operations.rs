use phenix_graph::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn node(graph: &CompiledGraph, path: &str) -> u16 {
    match graph.find(path).unwrap() {
        FindResult::Node(node) => node.id(),
        _ => panic!("expected physical node"),
    }
}

#[test]
fn closure_source_is_portless_and_explicit_with_fanout_and_repeated_waves() {
    let mut graph = GraphBuilder::new("sources");
    let source = graph.node("source", || 42_i64).unwrap();
    let _: Operation<NoPorts, i64> = source.operation();
    let sink = graph.node("sink", |x: i64| x + 1).unwrap();
    let other = graph.node("other", |x: i64| x * 2).unwrap();
    graph.connect(source.output(), sink.inlet()).unwrap();
    graph.connect(source.output(), other.inlet()).unwrap();
    let start = graph
        .trigger("start", [source.operation().start()])
        .unwrap();
    let unrelated = graph.node("unrelated", |x: i64| x).unwrap();
    let input = graph.expose_input("input", unrelated.inlet()).unwrap();
    graph.trigger("unrelated", [input]).unwrap();
    let compiled = graph.compile().unwrap();
    assert_eq!(compiled.physical_node_count(), 4);
    match compiled.find("source").unwrap() {
        FindResult::Node(source) => {
            assert_eq!(source.input_port_count(), 0);
            assert!(source.input("run", "value").is_none());
        }
        _ => unreachable!(),
    }
    let source_id = node(&compiled, "source");
    let sink_id = node(&compiled, "sink");
    let engine = Deployment::new(compiled).build().unwrap();
    assert_eq!(engine.trigger_targets("start").unwrap().len(), 0);
    assert!(engine.fire(start, vec![Value::local(())]).is_err());
    let unrelated = engine.fire("unrelated", vec![Value::local(1_i64)]).unwrap();
    assert!(unrelated.success());
    assert_eq!(unrelated.invocation_count(source_id), 0);
    let mut previous_wave = unrelated.wave_id();
    for _ in 0..3 {
        let report = engine.fire(start, vec![]).unwrap();
        assert!(report.success(), "{:?}", report.errors());
        assert!(report.wave_id() > previous_wave);
        previous_wave = report.wave_id();
        assert_eq!(report.activation_count(), 3);
        assert_eq!(report.invocation_count(source_id), 1);
        assert_eq!(report.delivery_events().len(), 2);
        assert_eq!(
            report
                .publications()
                .find(|p| p.source_node_id() == sink_id)
                .unwrap()
                .borrow::<i64>(),
            Some(&43)
        );
        let cohorts: Vec<_> = report
            .publication_events()
            .iter()
            .map(|p| p.cohort)
            .collect();
        assert!(cohorts.iter().all(|cohort| *cohort == cohorts[0]));
    }
}

#[test]
fn source_cancellation_and_zero_budget_do_not_execute_actions() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut graph = GraphBuilder::new("cancel-source");
    let counter = calls.clone();
    let source = graph
        .node("source", move || counter.fetch_add(1, Ordering::SeqCst))
        .unwrap();
    graph
        .trigger("start", [source.operation().start()])
        .unwrap();
    let compiled = graph.compile().unwrap();
    for budget in [0, 10] {
        let engine = Deployment::new(compiled.clone())
            .max_activations(budget)
            .build()
            .unwrap();
        let mut wave = engine.begin("start", vec![]).unwrap();
        wave.cancellation().request();
        assert!(wave.next_ready().is_none());
        let report = wave.finish().unwrap();
        assert!(report.cancelled());
        assert!(!report.budget_exhausted());
        assert_eq!(report.activation_count(), 0);
        assert!(report.publication_events().is_empty());
    }
    let engine = Deployment::new(compiled.clone())
        .max_activations(0)
        .build()
        .unwrap();
    let report = engine.fire("start", vec![]).unwrap();
    assert!(report.budget_exhausted());
    assert_eq!(report.activation_count(), 0);
    assert!(report.publication_events().is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let engine = Deployment::new(compiled).build().unwrap();
    assert!(engine.fire("start", vec![]).unwrap().success());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

struct RawSource;
impl NodeDefinition<(), i64> for RawSource {
    type Layout = Ports<NoPorts>;
    type InputMode = LocalValue;
    type OutputMode = EncodedValue;

    fn template(self: Arc<Self>) -> Result<NodeTemplate, LayoutError> {
        Ok(NodeTemplate::with_action(
            0,
            vec![],
            OutputSlot::typed::<i64>(0).named("value"),
            |ctx: InvocationCtx| {
                assert_eq!(ctx.source_port_id(), None);
                assert!(!ctx.has_arg(0));
                assert!(ctx.arg::<i64>(0).is_err());
                assert!(ctx.prepared::<()>().is_err());
                Ok(vec![emit(0, 42_i64)])
            },
        ))
    }
    fn signature() -> NodeSignature {
        NodeSignature {
            input: DeclaredValue::local::<()>(),
            output: DeclaredValue::wire::<i64>(),
        }
    }
}

#[test]
fn raw_zero_input_action_has_no_port_and_empty_raw_args() {
    let mut graph = GraphBuilder::new("raw-source");
    let source = graph.node("source", RawSource).unwrap();
    let sink = graph.node("sink", Node::wire(|x: i64| x + 1)).unwrap();
    graph.connect(source.output(), sink.inlet()).unwrap();
    graph
        .trigger("start", [source.operation().start()])
        .unwrap();
    let compiled = graph.compile().unwrap();
    let sink_id = node(&compiled, "sink");
    let engine = Deployment::new(compiled).build().unwrap();
    let report = engine.fire("start", vec![]).unwrap();
    assert!(report.success(), "{:?}", report.errors());
    assert_eq!(
        report
            .publications()
            .find(|p| p.source_node_id() == sink_id)
            .unwrap()
            .value::<i64>()
            .unwrap(),
        43
    );

    let mut wave = engine.begin("start", vec![]).unwrap();
    let (ticket, context) = wave.next_ready().unwrap().into_parts();
    assert_eq!(context.source_port_id(), None);
    assert!(!wave.can_finish());
    assert_eq!(wave.in_flight_count(), 1);
    let result = wave.action(ticket).unwrap().execute(context);
    wave.complete(ticket, result).unwrap();
    assert!(wave.complete(ticket, Ok(vec![])).is_err());
    let (ticket, context) = wave.next_ready().unwrap().into_parts();
    assert_eq!(context.source_port_id(), Some(0));
    let result = wave.action(ticket).unwrap().execute(context);
    wave.complete(ticket, result).unwrap();
    assert!(wave.can_finish());
    let report = wave.finish().unwrap();
    assert!(report.success());
    assert_eq!(report.activation_count(), 2);
}

#[test]
fn nested_no_input_modules_activate_only_output_reachable_child_roots() {
    let mut child = Module::new("child", InputInterface::<NoPorts>::new()).unwrap();
    let source = child.node("source", || 42_i64).unwrap();
    let sink = child.node("sink", |x: i64| x + 1).unwrap();
    let unused = child
        .node("unused", || panic!("disconnected source executed"))
        .unwrap();
    let _ = unused;
    child.connect(source.output(), sink.inlet()).unwrap();
    let child = child.finish(sink.output()).unwrap();
    let mut outer = Module::new("outer", InputInterface::<NoPorts>::new()).unwrap();
    let child = outer.instantiate("child", &child).unwrap();
    let outer = outer.finish(child.output()).unwrap();
    let mut graph = GraphBuilder::new("module-source");
    let module = graph.instantiate("module", &outer).unwrap();
    let second = graph.instantiate("second", &outer).unwrap();
    graph.trigger("start", [module.start()]).unwrap();
    graph.trigger("second", [second.start()]).unwrap();
    let compiled = graph.compile().unwrap();
    assert_eq!(compiled.physical_node_count(), 6);
    let unused = node(&compiled, "module/child/unused");
    let first_source = node(&compiled, "module/child/source");
    let second_source = node(&compiled, "second/child/source");
    let engine = Deployment::new(compiled)
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap();
    for _ in 0..2 {
        let report = engine.fire("start", vec![]).unwrap();
        assert!(report.success(), "{:?}", report.errors());
        assert_eq!(report.activation_count(), 2);
        assert_eq!(report.invocation_count(unused), 0);
        assert_eq!(report.invocation_count(second_source), 0);
        assert_eq!(
            report.publications().next().unwrap().borrow::<i64>(),
            Some(&43)
        );
    }
    let report = engine.fire("second", vec![]).unwrap();
    assert!(report.success());
    assert_eq!(report.invocation_count(first_source), 0);
    assert_eq!(report.invocation_count(second_source), 1);
}

#[test]
fn portless_module_without_an_output_reachable_source_rejects_start() {
    let mut module = Module::new("no-root", InputInterface::<NoPorts>::new()).unwrap();
    let node = module.node("needs-input", |x: i64| x).unwrap();
    let package = module.finish(node.output()).unwrap();
    let mut graph = GraphBuilder::new("no-root-graph");
    let operation = graph.instantiate("instance", &package).unwrap();
    graph.trigger("start", [operation.start()]).unwrap();
    let error = graph.compile().err().expect("must reject an inert start");
    assert_eq!(error.phase, CompilePhase::Triggers);
    assert!(
        error
            .to_string()
            .contains("no reachable zero-input producer")
    );
}

#[test]
fn source_start_checks_scope_and_rejects_duplicates() {
    let mut graph = GraphBuilder::new("source-authority");
    let source = graph.node("source", || 42_i64).unwrap();
    let mut other = GraphBuilder::new("other");
    assert!(
        other
            .trigger("wrong", [source.operation().start()])
            .is_err()
    );
    assert!(
        graph
            .trigger(
                "duplicate",
                [source.operation().start(), source.operation().start()]
            )
            .is_err()
    );
    graph
        .trigger("start", [source.operation().start()])
        .unwrap();
    assert!(
        Deployment::new(graph.compile().unwrap())
            .build()
            .unwrap()
            .fire("start", vec![])
            .unwrap()
            .success()
    );
}

#[test]
fn conditional_delivery_is_wave_local_after_each_explicit_source_start() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut graph = GraphBuilder::new("conditional-source");
    let source = graph
        .node("source", move || calls.fetch_add(1, Ordering::SeqCst))
        .unwrap();
    let gate = graph
        .node("gate", |x: usize| {
            if x.is_multiple_of(2) {
                Output::all(x)
            } else {
                Output::none()
            }
        })
        .unwrap();
    let sink = graph.node("sink", |x: usize| x + 1).unwrap();
    graph.connect(source.output(), gate.inlet()).unwrap();
    graph.connect(gate.output(), sink.inlet()).unwrap();
    graph
        .trigger("start", [source.operation().start()])
        .unwrap();
    let compiled = graph.compile().unwrap();
    let sink = node(&compiled, "sink");
    let engine = Deployment::new(compiled).build().unwrap();
    for index in 0..4 {
        let report = engine.fire("start", vec![]).unwrap();
        assert!(report.success(), "{:?}", report.errors());
        assert_eq!(report.invocation_count(sink), usize::from(index % 2 == 0));
        assert_eq!(
            report.activation_count(),
            if index % 2 == 0 { 3 } else { 2 }
        );
    }
}

#[cfg(feature = "derive")]
#[test]
fn module_start_admits_multiple_roots_with_same_cohort_for_an_ordinary_join() {
    #[derive(Clone, LocalPortLayout, InputFields)]
    struct Pair {
        left: i64,
        right: i64,
    }
    let mut module = Module::new("join-source", InputInterface::<NoPorts>::new()).unwrap();
    let left = module.node("left", || 20_i64).unwrap();
    let right = module.node("right", || 22_i64).unwrap();
    let sum = module
        .node("sum", |p: Pair| p.left + p.right)
        .unwrap()
        .input(Layout::fields())
        .unwrap();
    module
        .connect(
            left.output(),
            sum.inlet().field(Pair::SLOT_LEFT).target().unwrap(),
        )
        .unwrap();
    module
        .connect(
            right.output(),
            sum.inlet().field(Pair::SLOT_RIGHT).target().unwrap(),
        )
        .unwrap();
    let package = module.finish(sum.output()).unwrap();
    let mut graph = GraphBuilder::new("join-root");
    let instance = graph.instantiate("instance", &package).unwrap();
    graph.trigger("start", [instance.start()]).unwrap();
    let engine = Deployment::new(graph.compile().unwrap())
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap();
    for _ in 0..2 {
        let report = engine.fire("start", vec![]).unwrap();
        assert!(report.success(), "{:?}", report.errors());
        assert_eq!(report.activation_count(), 3);
        assert_eq!(
            report.publications().next().unwrap().borrow::<i64>(),
            Some(&42)
        );
    }
}

#[test]
fn directed_source_all_or_none_is_portless_and_wave_local() {
    let calls = AtomicUsize::new(0);
    let mut graph = GraphBuilder::new("directed-source");
    let source = graph
        .node("source", move || {
            let value = calls.fetch_add(1, Ordering::SeqCst);
            if value.is_multiple_of(2) {
                Output::all(value)
            } else {
                Output::none()
            }
        })
        .unwrap();
    let _: Operation<NoPorts, usize, LocalValue, LocalValue> = source.operation();
    let sink = graph.node("sink", |x: usize| x + 1).unwrap();
    graph.connect(source.output(), sink.inlet()).unwrap();
    graph
        .trigger("start", [source.operation().start()])
        .unwrap();
    let compiled = graph.compile().unwrap();
    assert_eq!(compiled.physical_node_count(), 2);
    match compiled.find("source").unwrap() {
        FindResult::Node(source) => assert_eq!(source.input_port_count(), 0),
        _ => unreachable!(),
    }
    let sink = node(&compiled, "sink");
    let engine = Deployment::new(compiled).build().unwrap();
    for index in 0..6 {
        let report = engine.fire("start", vec![]).unwrap();
        assert!(report.success(), "{:?}", report.errors());
        if index % 2 == 0 {
            assert_eq!(report.activation_count(), 2);
            assert_eq!(report.delivery_events().len(), 1);
            assert_eq!(
                report
                    .publications()
                    .find(|p| p.source_node_id() == sink)
                    .unwrap()
                    .borrow::<usize>(),
                Some(&(index + 1))
            );
        } else {
            assert_eq!(report.activation_count(), 1);
            assert!(report.publication_events().is_empty());
            assert!(report.delivery_events().is_empty());
            assert_eq!(report.invocation_count(sink), 0);
        }
    }
}

#[test]
fn directed_source_to_selects_one_existing_edge() {
    let mut graph = GraphBuilder::new("directed-target");
    let left = graph.node("left", |x: i64| x + 1).unwrap();
    let right = graph.node("right", |x: i64| x + 100).unwrap();
    let target = left.inlet();
    let source = graph
        .node("source", move || Output::to(&target, 42_i64))
        .unwrap();
    graph.connect(source.output(), left.inlet()).unwrap();
    graph.connect(source.output(), right.inlet()).unwrap();
    graph
        .trigger("start", [source.operation().start()])
        .unwrap();
    let compiled = graph.compile().unwrap();
    let left = node(&compiled, "left");
    let right = node(&compiled, "right");
    let engine = Deployment::new(compiled).build().unwrap();
    let report = engine.fire("start", vec![]).unwrap();
    assert!(report.success(), "{:?}", report.errors());
    assert_eq!(report.activation_count(), 2);
    assert_eq!(report.invocation_count(right), 0);
    assert_eq!(report.delivery_events().len(), 1);
    assert_eq!(report.delivery_events()[0].to.node_id(), left);
    assert_eq!(
        report
            .publications()
            .find(|p| p.source_node_id() == left)
            .unwrap()
            .borrow::<i64>(),
        Some(&43)
    );
}

#[test]
fn directed_source_invalid_target_rejects_the_entire_publication() {
    for stale in [false, true] {
        let mut graph = GraphBuilder::new("invalid-source-target");
        let good = graph.node("good", |x: i64| x).unwrap();
        let bad = graph.node("bad", |x: i64| x).unwrap();
        let targets = [good.inlet(), bad.inlet()];
        let source = graph
            .node("source", move || Output::to_all(&targets, 42_i64))
            .unwrap();
        graph.connect(source.output(), good.inlet()).unwrap();
        if stale {
            graph.connect(source.output(), bad.inlet()).unwrap();
            graph
                .replace("bad", NodeTemplate::local_function(|x: i64| x))
                .unwrap();
        }
        graph
            .trigger("start", [source.operation().start()])
            .unwrap();
        let compiled = graph.compile().unwrap();
        let source = node(&compiled, "source");
        let engine = Deployment::new(compiled).build().unwrap();
        for _ in 0..2 {
            let report = engine.fire("start", vec![]).unwrap();
            assert!(
                matches!(report.errors(), [WaveError::OutputRoutingFailed { from, .. }]
                if from.node_id() == source),
                "stale={stale}: {:?}",
                report.errors()
            );
            assert_eq!(report.activation_count(), 1);
            assert!(report.publication_events().is_empty());
            assert!(report.publications().next().is_none());
            assert!(report.delivery_events().is_empty());
        }
    }
}

#[test]
fn source_manifest_identifies_roots_and_is_declaration_order_independent() {
    fn manifest(pick_b: bool, reverse: bool) -> String {
        let mut child = Module::new("child", InputInterface::<NoPorts>::new()).unwrap();
        let source = child.node("source", || 42_i64).unwrap();
        let package = child.finish(source.output()).unwrap();
        let mut graph = GraphBuilder::new("source-manifest");
        let mut sources = std::collections::BTreeMap::new();
        for name in if reverse { ["b", "a"] } else { ["a", "b"] } {
            sources.insert(name, graph.node(name, || 42_i64).unwrap().operation());
        }
        let nested = graph.instantiate("nested", &package).unwrap();
        let sink = graph.node("sink", |x: i64| x).unwrap();
        let payload = graph.expose_input("payload", sink.inlet()).unwrap();
        let mut starts = vec![sources["a"].start(), sources["b"].start(), nested.start()];
        if reverse {
            starts.reverse();
        }
        graph.trigger("all", starts).unwrap();
        graph
            .trigger("pick", [sources[if pick_b { "b" } else { "a" }].start()])
            .unwrap();
        graph.trigger("payload", [payload]).unwrap();
        graph.compile().unwrap().topology_manifest()
    }
    let one = manifest(false, false);
    assert_eq!(one, manifest(false, true));
    assert_ne!(one, manifest(true, false));
    assert!(one.contains("trigger \"all\" -> [] sources=[\"a\", \"b\", \"nested/source\"]"));
    assert!(one.contains("trigger \"pick\" -> [] sources=[\"a\"]"));
    assert!(
        one.lines()
            .any(|line| line == "trigger \"payload\" -> [[\"sink.run/value\"]]")
    );
}

#[test]
fn replacement_rejects_stale_node_and_module_start_handles() {
    for module in [false, true] {
        let mut graph = GraphBuilder::new("stale-start");
        let (start, stale) = if module {
            let mut child = Module::new("child", InputInterface::<NoPorts>::new()).unwrap();
            let source = child.node("source", || 1_i64).unwrap();
            let package = child.finish(source.output()).unwrap();
            let operation = graph.instantiate("source", &package).unwrap();
            (operation.start(), operation.start())
        } else {
            let source = graph.node("source", || 1_i64).unwrap();
            (source.operation().start(), source.operation().start())
        };
        graph.trigger("start", [start]).unwrap();
        graph
            .replace(
                "source",
                NodeTemplate::with_action(
                    0,
                    vec![],
                    OutputSlot::local::<i64>(0).named("value"),
                    |_: InvocationCtx| Ok(vec![emit_local(0, 42_i64)]),
                ),
            )
            .unwrap();
        assert!(graph.trigger("stale", [stale]).is_err());
        let compiled = graph.compile().unwrap();
        assert_eq!(compiled.physical_node_count(), 1);
        assert!(compiled.trigger("stale").is_none());
        let report = Deployment::new(compiled)
            .build()
            .unwrap()
            .fire("start", vec![])
            .unwrap();
        assert!(report.success(), "{:?}", report.errors());
        assert_eq!(report.activation_count(), 1);
        assert_eq!(
            report.publications().next().unwrap().borrow::<i64>(),
            Some(&42)
        );
    }
}

struct CohortSource(bool);
impl NodeDefinition<(), i64> for CohortSource {
    type Layout = Ports<NoPorts>;
    type InputMode = LocalValue;
    type OutputMode = LocalValue;

    fn template(self: Arc<Self>) -> Result<NodeTemplate, LayoutError> {
        let node = NodeTemplate::with_action(
            0,
            vec![],
            OutputSlot::local::<i64>(0).named("value"),
            |ctx: InvocationCtx| {
                assert_eq!(ctx.source_port_id(), None);
                assert_eq!(ctx.cohort_id(), 0);
                Ok(vec![emit_local(0, 42_i64)])
            },
        );
        Ok(if self.0 {
            node.with_fresh_cohort()
        } else {
            node
        })
    }
    fn signature() -> NodeSignature {
        NodeSignature {
            input: DeclaredValue::local::<()>(),
            output: DeclaredValue::local::<i64>(),
        }
    }
}

#[test]
fn fresh_source_cohorts_do_not_alias_inherited_or_other_fresh_roots() {
    let mut graph = GraphBuilder::new("source-cohorts");
    let mut starts = Vec::new();
    for (name, fresh) in [("inherited", false), ("fresh-a", true), ("fresh-b", true)] {
        let source = graph.node(name, CohortSource(fresh)).unwrap();
        let sink = graph.node(format!("{name}-sink"), |x: i64| x).unwrap();
        graph.connect(source.output(), sink.inlet()).unwrap();
        starts.push(source.operation().start());
    }
    graph.trigger("start", starts).unwrap();
    let compiled = graph.compile().unwrap();
    let ids = ["inherited", "fresh-a", "fresh-b"].map(|name| {
        (
            node(&compiled, name),
            node(&compiled, &format!("{name}-sink")),
        )
    });
    let engine = Deployment::new(compiled).build().unwrap();
    for reverse in [false, true] {
        let mut wave = engine.begin("start", vec![]).unwrap();
        let mut sources = Vec::new();
        for _ in 0..3 {
            let (ticket, ctx) = wave.next_ready().unwrap().into_parts();
            assert_eq!(ctx.source_port_id(), None);
            sources.push((ticket, ctx));
        }
        assert!(wave.next_ready().is_none());
        if reverse {
            sources.reverse();
        }
        for (ticket, ctx) in sources {
            let result = wave.action(ticket).unwrap().execute(ctx);
            wave.complete(ticket, result).unwrap();
        }
        while let Some(ready) = wave.next_ready() {
            let (ticket, ctx) = ready.into_parts();
            let result = wave.action(ticket).unwrap().execute(ctx);
            wave.complete(ticket, result).unwrap();
        }
        let report = wave.finish().unwrap();
        assert!(report.success(), "{:?}", report.errors());
        assert_eq!(report.activation_count(), 6);
        let cohorts = ids.map(|(source, sink)| {
            let source_cohort = report
                .publication_events()
                .iter()
                .find(|p| p.from.node_id() == source)
                .unwrap()
                .cohort;
            let sink_cohort = report
                .publication_events()
                .iter()
                .find(|p| p.from.node_id() == sink)
                .unwrap()
                .cohort;
            assert_eq!(source_cohort, sink_cohort);
            report
                .publications()
                .find(|p| p.source_node_id() == sink)
                .unwrap()
                .cohort_id()
        });
        assert_eq!(cohorts[0], 0);
        assert_ne!(cohorts[1], 0);
        assert_ne!(cohorts[2], 0);
        assert_ne!(cohorts[1], cohorts[2]);
    }
}

#[test]
fn queued_source_colors_wait_only_for_reachable_work_in_any_completion_order() {
    for emits in [false, true] {
        for unrelated_first in [false, true] {
            let mut graph = GraphBuilder::new("source-colors");
            let left = graph.node("left", || 10_i64).unwrap();
            let right = graph
                .node("right", move || {
                    if emits {
                        Output::all(20_i64)
                    } else {
                        Output::none()
                    }
                })
                .unwrap();
            let unrelated = graph.node("unrelated", Output::<i64>::none).unwrap();
            graph
                .add(
                    "sink",
                    NodeTemplate::with_action(
                        0,
                        vec![
                            Port::new(
                                0,
                                vec![InputSlot::local::<i64>(0).named("value")],
                                WhenUpstreamSettles::new(AnyOne),
                            )
                            .named("run")
                            .with_buffer(BufferMode::Deferred),
                        ],
                        OutputSlot::local::<i64>(0).named("value"),
                        |ctx: InvocationCtx| Ok(vec![emit_local(0, *ctx.shared::<i64>(0)?)]),
                    ),
                )
                .unwrap();
            graph.connect("left", "sink").unwrap();
            graph.connect("right", "sink").unwrap();
            graph
                .trigger(
                    "start",
                    [
                        left.operation().start(),
                        right.operation().start(),
                        unrelated.operation().start(),
                    ],
                )
                .unwrap();
            let compiled = graph.compile().unwrap();
            let left = node(&compiled, "left");
            let right = node(&compiled, "right");
            let unrelated = node(&compiled, "unrelated");
            let sink = node(&compiled, "sink");
            let engine = Deployment::new(compiled).build().unwrap();
            let mut wave = engine.begin("start", vec![]).unwrap();
            let (ticket, ctx) = wave.next_ready().unwrap().into_parts();
            assert_eq!(ticket.node_id(), left);
            let result = wave.action(ticket).unwrap().execute(ctx);
            wave.complete(ticket, result).unwrap();
            // The right source is still queued when the first payload arrives.
            let right_job = wave.next_ready().unwrap().into_parts();
            assert_eq!(right_job.0.node_id(), right);
            let unrelated_job = wave.next_ready().unwrap().into_parts();
            assert_eq!(unrelated_job.0.node_id(), unrelated);
            assert!(
                wave.next_ready().is_none(),
                "reachable source must delay the sink"
            );
            let mut unrelated_job = Some(unrelated_job);
            if unrelated_first {
                let (ticket, ctx) = unrelated_job.take().unwrap();
                let result = wave.action(ticket).unwrap().execute(ctx);
                wave.complete(ticket, result).unwrap();
                assert!(wave.next_ready().is_none(), "right source is still open");
            }
            let (ticket, ctx) = right_job;
            let result = wave.action(ticket).unwrap().execute(ctx);
            wave.complete(ticket, result).unwrap();
            let mut values = Vec::new();
            while let Some(ready) = wave.next_ready() {
                let (ticket, ctx) = ready.into_parts();
                assert_eq!(ticket.node_id(), sink);
                values.push(*ctx.shared::<i64>(0).unwrap());
                let result = wave.action(ticket).unwrap().execute(ctx);
                wave.complete(ticket, result).unwrap();
            }
            values.sort();
            assert_eq!(
                values,
                if emits { vec![10, 20] } else { vec![10] },
                "unrelated work must not delay the sink"
            );
            if let Some((ticket, ctx)) = unrelated_job {
                assert!(!wave.can_finish());
                let result = wave.action(ticket).unwrap().execute(ctx);
                wave.complete(ticket, result).unwrap();
            }
            let report = wave.finish().unwrap();
            assert!(report.success(), "{:?}", report.errors());
            assert_eq!(report.activation_count(), 3 + values.len());
        }
    }
}
