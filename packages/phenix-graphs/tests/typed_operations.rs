#![cfg(feature = "derive")]

use phenix_graph::*;
use std::sync::Arc;

#[derive(Clone, LocalPortLayout, phenix_graph::InputFields)]
struct Pair {
    left: i64,
    right: i64,
}
#[derive(Clone, LocalPortLayout, phenix_graph::InputFields)]
struct Nested {
    pair: Pair,
    extra: i64,
}

fn pair_module() -> TypedModulePackage<SinglePort<Pair, LocalFlatten<Pair>>, i64> {
    let interface = InputInterface::single(Layout::<Pair>::fields()).unwrap();
    let mut module = Module::new("pair", interface).unwrap();
    let sum = module
        .node("sum", |p: Pair| p.left + p.right)
        .unwrap()
        .input(Layout::fields())
        .unwrap();
    for key in [Pair::SLOT_LEFT, Pair::SLOT_RIGHT] {
        module
            .expose(
                module.inlet().field(key),
                sum.inlet().field(key).target().unwrap(),
            )
            .unwrap();
    }
    module.finish(sum.output()).unwrap()
}

#[test]
fn nested_modules_share_atomic_and_structural_operation_api() {
    let pair = pair_module();
    let mut outer = Module::new(
        "outer",
        InputInterface::single(Layout::<Pair>::fields()).unwrap(),
    )
    .unwrap();
    let child = outer.instantiate("child", &pair).unwrap();
    for key in [Pair::SLOT_LEFT, Pair::SLOT_RIGHT] {
        outer
            .expose(
                outer.inlet().field(key),
                child.inlet().field(key).target().unwrap(),
            )
            .unwrap();
    }
    let outer = outer.finish(child.output()).unwrap();
    let mut graph = GraphBuilder::new("root");
    let instance = graph.instantiate("outer", &outer).unwrap();
    let sink = graph.node("sink", |v: i64| v * 2).unwrap();
    graph.connect(instance.output(), sink.inlet()).unwrap();
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
    assert!(graph.discover("outer/child/sum").is_err());
    let compiled = graph.compile().unwrap();
    assert_eq!(compiled.physical_node_count(), 2);
    let report = Deployment::new(compiled)
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap()
        .fire("run", vec![Value::local(3_i64), Value::local(4_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(
        report.publications().next().unwrap().borrow::<i64>(),
        Some(&14)
    );
}

#[test]
fn structural_frontier_and_revision_are_checked() {
    let mut graph = GraphBuilder::new("frontier");
    let source = graph.node("source", |x: i64| x).unwrap();
    let sink = graph
        .node("sink", |x: Nested| x.pair.left + x.extra)
        .unwrap();
    let stale = sink.output();
    let sink = sink.input(Layout::fields()).unwrap();
    assert!(sink.inlet().target().is_err());
    assert!(sink.inlet().field(Nested::SLOT_PAIR).target().is_ok());
    assert!(
        sink.inlet()
            .field(Nested::SLOT_PAIR)
            .field(Pair::SLOT_LEFT)
            .target()
            .is_err()
    );
    let stale_view = sink.inlet();
    let sink = sink.input(Layout::deep()).unwrap();
    assert!(sink.inlet().field(Nested::SLOT_PAIR).target().is_err());
    graph
        .connect(
            source.output(),
            sink.inlet()
                .field(Nested::SLOT_PAIR)
                .field(Pair::SLOT_LEFT)
                .target()
                .unwrap(),
        )
        .unwrap();
    assert!(graph.connect(stale, source.inlet()).is_err());
    let pair_source = graph.node("pair_source", |x: Pair| x).unwrap();
    assert!(
        graph
            .connect(
                pair_source.output(),
                stale_view.field(Nested::SLOT_PAIR).target().unwrap()
            )
            .is_err()
    );
}

struct Public;
impl PortInterface for Public {
    const PORT_COUNT: Option<usize> = Some(2);
}
const FIRST: InletPort<Public, i64, LocalAtomic<i64>> = InletPort::new("first");
const SECOND: InletPort<Public, i64, LocalAtomic<i64>> = InletPort::new("second");

#[test]
fn separate_public_ports_project_fanout_without_runtime_wrapper() {
    let interface = InputInterface::<Public>::new()
        .port(FIRST, Layout::atomic())
        .unwrap()
        .port(SECOND, Layout::atomic())
        .unwrap();
    let mut module = Module::new("fanout", interface).unwrap();
    let a = module.node("a", |x: i64| x + 1).unwrap();
    let b = module.node("b", |x: i64| x + 2).unwrap();
    let c = module.node("c", |x: i64| x + 3).unwrap();
    let first = module.inlets().port(FIRST);
    module.expose(first, a.inlet()).unwrap();
    assert!(
        module
            .expose(module.inlets().port(FIRST), a.inlet())
            .is_err()
    );
    module
        .expose(module.inlets().port(FIRST), b.inlet())
        .unwrap();
    module
        .expose(module.inlets().port(SECOND), c.inlet())
        .unwrap();
    let package = module.finish(a.output()).unwrap();
    let mut root = GraphBuilder::new("root");
    let instance = root.instantiate("child", &package).unwrap();
    assert!(instance.inlets().port(FIRST).target().is_ok());
    assert!(instance.inlets().port(SECOND).target().is_ok());
    let graph = root.compile().unwrap();
    assert_eq!(graph.physical_node_count(), 3);
    let Some(FindResult::Module(child)) = graph.find("child") else {
        panic!("missing child")
    };
    let first = child
        .input_ports()
        .find(|p| p.port_name() == "first")
        .unwrap();
    assert_eq!(child.resolved_input_targets(first.id()).unwrap().len(), 2);
}

#[test]
fn every_declared_public_leaf_needs_projection() {
    let mut module = Module::new(
        "missing",
        InputInterface::single(Layout::<Pair>::fields()).unwrap(),
    )
    .unwrap();
    let node = module.node("node", |x: i64| x).unwrap();
    module
        .expose(module.inlet().field(Pair::SLOT_LEFT), node.inlet())
        .unwrap();
    assert!(module.finish(node.output()).is_err());
}

#[test]
fn discovery_and_contract_projection_check_types_and_promises() {
    let mut graph = GraphBuilder::new("discovery");
    let node = graph
        .node("sum", |p: Pair| p.left + p.right)
        .unwrap()
        .input(Layout::fields())
        .unwrap();
    let contract = GraphContract::new(
        "pair",
        InterfaceShape::new()
            .input_slot("run", "left", DeclaredValue::local::<i64>())
            .unwrap()
            .input_slot("run", "right", DeclaredValue::local::<i64>())
            .unwrap()
            .output("value", DeclaredValue::local::<i64>())
            .unwrap(),
    )
    .unwrap();
    graph.implements("sum", &contract).unwrap();
    let discovered = graph.discover("sum").unwrap();
    let run = InletPort::<Pair, Pair>::new("run");
    assert!(
        discovered
            .inlets::<Pair, LocalValue>()
            .port(run.clone())
            .field(Pair::SLOT_RIGHT)
            .target()
            .is_ok()
    );
    assert!(discovered.output::<i64, EncodedValue>().is_err());
    assert!(discovered.output::<String, LocalValue>().is_err());
    let source = graph.node("source", |x: i64| x).unwrap();
    let promised = graph
        .bind_contract("sum", &contract)
        .unwrap()
        .operation()
        .unwrap();
    let run = promised.inlets::<Pair, LocalValue>().port(run);
    assert!(run.field(Pair::SLOT_LEFT).target().is_ok());
    assert!(
        run.field(SlotField::<Pair, i64>::new("private"))
            .target()
            .is_err()
    );
    assert!(promised.output::<i64, LocalValue>().is_ok());
    let old_endpoint = run.field(Pair::SLOT_LEFT).target().unwrap();
    let node = node.input(Layout::atomic()).unwrap();
    assert!(promised.output::<i64, LocalValue>().is_err());
    assert!(run.field(Pair::SLOT_LEFT).target().is_err());
    let node = node.input(Layout::fields()).unwrap();
    assert!(promised.output::<i64, LocalValue>().is_ok());
    assert!(run.field(Pair::SLOT_LEFT).target().is_ok());
    drop(run);
    drop(promised);
    assert!(graph.connect(source.output(), old_endpoint).is_err());
    let promised = graph
        .bind_contract("sum", &contract)
        .unwrap()
        .operation()
        .unwrap();
    let run = promised
        .inlets::<Pair, LocalValue>()
        .port(InletPort::<Pair, Pair>::new("run"));
    graph
        .connect(
            source.output(),
            run.field(Pair::SLOT_LEFT).target().unwrap(),
        )
        .unwrap();
    assert!(graph.connect(node.output(), source.inlet()).is_ok());
}

struct Empty;
impl NodeDefinition<(), i64> for Empty {
    type Layout = Ports<NoPorts>;
    type InputMode = LocalValue;
    type OutputMode = LocalValue;
    fn template(self: Arc<Self>) -> Result<NodeTemplate, LayoutError> {
        Ok(NodeTemplate::with_action(
            0,
            vec![],
            OutputSlot::local::<i64>(0).named("result"),
            |_| Ok(vec![emit_local(0, 42_i64)]),
        ))
    }
    fn signature() -> NodeSignature {
        NodeSignature {
            input: DeclaredValue::local::<()>(),
            output: DeclaredValue::local::<i64>(),
        }
    }
}

#[test]
fn zero_input_operation_has_one_typed_output() {
    let mut module = Module::new("empty", InputInterface::<NoPorts>::new()).unwrap();
    let node = module.node("empty", Empty).unwrap();
    let _: Inlets<NoPorts, LocalValue> = node.inlets();
    let package = module.finish(node.output()).unwrap();
    let mut graph = GraphBuilder::new("root");
    let instance = graph.instantiate("empty", &package).unwrap();
    let _: Inlets<NoPorts, LocalValue> = instance.inlets();
    let _: OutputRef<i64> = instance.output();
    assert!(graph.compile().is_ok());
}

#[test]
fn zero_argument_closure_is_not_a_unit_input_port() {
    let mut graph = GraphBuilder::new("source");
    let source = graph.node("source", || 42_i64).unwrap();
    let _: Inlets<NoPorts, LocalValue> = source.inlets();
    let _: OutputRef<i64> = source.output();
    let compiled = graph.compile().unwrap();
    let shape = compiled.find("source").unwrap().inspect().unwrap().shape;
    assert_eq!(shape.inputs().count(), 0);
    assert_eq!(
        shape.output_value().unwrap().1,
        DeclaredValue::local::<i64>()
    );
}

#[test]
fn public_structural_group_is_not_a_readiness_barrier() {
    let mut module = Module::new(
        "independent",
        InputInterface::single(Layout::<Pair>::fields()).unwrap(),
    )
    .unwrap();
    let a = module.node("a", |x: i64| x + 1).unwrap();
    let b = module.node("b", |x: i64| x + 2).unwrap();
    module
        .expose(module.inlet().field(Pair::SLOT_LEFT), a.inlet())
        .unwrap();
    module
        .expose(module.inlet().field(Pair::SLOT_RIGHT), b.inlet())
        .unwrap();
    let package = module.finish(a.output()).unwrap();
    let mut root = GraphBuilder::new("root");
    let child = root.instantiate("child", &package).unwrap();
    let left = root
        .expose_input(
            "left",
            child.inlet().field(Pair::SLOT_LEFT).target().unwrap(),
        )
        .unwrap();
    root.trigger("left_only", [left]).unwrap();
    let report = Deployment::new(root.compile().unwrap())
        .build()
        .unwrap()
        .fire("left_only", vec![Value::local(7_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
}

struct Multi;
impl NodeDefinition<(), i64> for Multi {
    type Layout = Ports<Public>;
    type InputMode = LocalValue;
    type OutputMode = EncodedValue;
    fn template(self: Arc<Self>) -> Result<NodeTemplate, LayoutError> {
        Ok(NodeTemplate::with_action(
            0,
            vec![
                Port::all(0, vec![InputSlot::local::<i64>(0).named("value")]).named("first"),
                Port::all(1, vec![InputSlot::local::<i64>(0).named("value")]).named("second"),
            ],
            OutputSlot::typed::<i64>(0).named("result"),
            |ctx: InvocationCtx| Ok(vec![emit(0, *ctx.shared::<i64>(0)? + 1)]),
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
fn custom_multiport_node_has_same_api_and_independent_modes() {
    let mut graph = GraphBuilder::new("multi");
    let multi = graph.node("multi", Multi).unwrap();
    let source = graph.node("source", |x: i64| x).unwrap();
    let sink = graph.node("sink", Node::wire(|x: i64| x)).unwrap();
    let _: OutputRef<i64, EncodedValue> = multi.output();
    let _: Input<i64, LocalValue> = multi.inlets().port(SECOND).target().unwrap();
    graph
        .connect(
            source.output(),
            multi.inlets().port(FIRST).target().unwrap(),
        )
        .unwrap();
    graph.connect(multi.output(), sink.inlet()).unwrap();
    let input = graph.expose_input("input", source.inlet()).unwrap();
    graph.trigger("run", [input]).unwrap();
    let report = Deployment::new(graph.compile().unwrap())
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap()
        .fire("run", vec![Value::local(9_i64)])
        .unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(
        report
            .publications()
            .next()
            .unwrap()
            .value::<i64>()
            .unwrap(),
        10
    );
}

struct CustomPair;
impl InputLayout for CustomPair {
    type Value = Pair;
    fn slots() -> Result<Vec<InputSlot>, LayoutError> {
        LocalFlatten::<Pair>::slots()
    }
}
impl LocalInputLayout for CustomPair {
    fn prepare_owned(values: &mut OwnedSelectedValues) -> Result<Pair, String> {
        LocalFlatten::<Pair>::prepare_owned(values)
    }
}
impl InletLayout<Pair, LocalValue> for CustomPair {
    type View = InletView<Pair>;
    fn select(view: Self::View) -> Self::View {
        view
    }
}

#[test]
fn custom_and_depth_layouts_keep_their_frontiers() {
    let mut graph = GraphBuilder::new("layouts");
    let custom = graph
        .node("custom", |x: Pair| x.left + x.right)
        .unwrap()
        .input(Layout::custom::<CustomPair>())
        .unwrap();
    assert!(custom.inlet().field(Pair::SLOT_LEFT).target().is_ok());
    assert!(custom.inlet().target().is_err());
    let depth = graph
        .node("depth", |x: Nested| x.extra)
        .unwrap()
        .input(Layout::flatten_to::<2>())
        .unwrap();
    assert!(
        depth
            .inlet()
            .field(Nested::SLOT_PAIR)
            .field(Pair::SLOT_LEFT)
            .target()
            .is_ok()
    );
    assert!(depth.inlet().field(Nested::SLOT_PAIR).target().is_err());
}

#[test]
fn module_output_mode_is_inferred_independently_of_public_input_mode() {
    let mut module = Module::new(
        "encoder",
        InputInterface::single(Layout::<i64>::atomic()).unwrap(),
    )
    .unwrap();
    let node = module.node("encode", Multi).unwrap();
    module
        .expose(module.inlet(), node.inlets().port(FIRST).target().unwrap())
        .unwrap();
    let package = module.finish(node.output()).unwrap();
    let mut root = GraphBuilder::new("root");
    let operation = root.instantiate("encoder", &package).unwrap();
    let _: Input<i64, LocalValue> = operation.inlet();
    let _: OutputRef<i64, EncodedValue> = operation.output();
    let sink = root.node("sink", Node::wire(|x: i64| x)).unwrap();
    root.connect(operation.output(), sink.inlet()).unwrap();
    assert!(root.compile().is_ok());
}

#[test]
fn single_public_interface_validates_completeness_and_actual_layout_identity() {
    assert!(Module::new("incomplete", InputInterface::<SinglePort<i64>>::new()).is_err());
    assert!(
        InputInterface::<SinglePort<i64>>::new()
            .port(
                InletPort::<SinglePort<i64>, String, LocalAtomic<String>>::new("run"),
                Layout::atomic()
            )
            .is_err()
    );
    assert!(
        InputInterface::<SinglePort<Pair, LocalFlatten<Pair>>>::new()
            .port(
                InletPort::<SinglePort<Pair, LocalFlatten<Pair>>, Pair, LocalAtomic<Pair>>::new(
                    "run"
                ),
                Layout::atomic()
            )
            .is_err()
    );
    assert!(
        Module::new(
            "incomplete-multi",
            InputInterface::<Public>::new()
                .port(FIRST, Layout::atomic())
                .unwrap()
        )
        .is_err()
    );
}

struct ZeroSlots;
impl InputLayout for ZeroSlots {
    type Value = i64;
    fn slots() -> Result<Vec<InputSlot>, LayoutError> {
        Ok(vec![])
    }
}
impl LocalInputLayout for ZeroSlots {
    fn prepare_owned(_: &mut OwnedSelectedValues) -> Result<i64, String> {
        Ok(0)
    }
}

#[test]
fn empty_public_ports_and_failed_retarget_do_not_expose_invalid_handles() {
    assert!(
        InputInterface::<SinglePort<i64, ZeroSlots>>::single(Layout::custom::<ZeroSlots>())
            .is_err()
    );
    let mut graph = GraphBuilder::new("retarget");
    let source = graph.node("source", |x: i64| x).unwrap();
    let node = graph.node("node", |x: i64| x).unwrap();
    let old = node.operation();
    assert!(node.input(Layout::custom::<ZeroSlots>()).is_err());
    graph.connect(source.output(), old.inlet()).unwrap();
    assert!(graph.discover("missing").is_err());
    graph
        .add(
            "unnamed",
            NodeTemplate::with_action(
                0,
                vec![Port::all(0, vec![InputSlot::local::<i64>(0)])],
                OutputSlot::local::<i64>(0),
                |_| Ok(vec![]),
            ),
        )
        .unwrap();
    assert!(graph.discover("unnamed").is_err());
}

#[test]
fn custom_layout_is_evaluated_once_for_each_frozen_declaration() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    struct OnceLayout;
    impl InputLayout for OnceLayout {
        type Value = Pair;
        fn slots() -> Result<Vec<InputSlot>, LayoutError> {
            CALLS.fetch_add(1, Ordering::SeqCst);
            LocalFlatten::<Pair>::slots()
        }
    }
    impl LocalInputLayout for OnceLayout {
        fn prepare_owned(values: &mut OwnedSelectedValues) -> Result<Pair, String> {
            LocalFlatten::<Pair>::prepare_owned(values)
        }
    }
    let mut graph = GraphBuilder::new("once");
    let node = graph
        .node(
            "node",
            Node::with_layout(|p: Pair| p.left, Layout::custom::<OnceLayout>()),
        )
        .unwrap();
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    let _ = graph.discover("node").unwrap();
    let node = node.input(Layout::custom::<OnceLayout>()).unwrap();
    assert_eq!(CALLS.load(Ordering::SeqCst), 2);
    let _ = node.operation();
    graph.compile().unwrap();
    assert_eq!(CALLS.load(Ordering::SeqCst), 2);
    let interface =
        InputInterface::<SinglePort<Pair, OnceLayout>>::single(Layout::custom::<OnceLayout>())
            .unwrap();
    assert_eq!(CALLS.load(Ordering::SeqCst), 3);
    let _ = Module::new("once-public", interface).unwrap();
    assert_eq!(CALLS.load(Ordering::SeqCst), 3);
}

struct DishonestRetarget;
impl NodeDefinition<Pair, i64> for DishonestRetarget {
    type Layout = CustomPair;
    type InputMode = LocalValue;
    type OutputMode = LocalValue;
    fn template(self: Arc<Self>) -> Result<NodeTemplate, LayoutError> {
        NodeTemplate::local_with_layout::<CustomPair, i64, i64, _>(|p: Pair| p.left)
    }
    fn signature() -> NodeSignature {
        NodeSignature {
            input: DeclaredValue::local::<Pair>(),
            output: DeclaredValue::local::<i64>(),
        }
    }
}
impl authoring::LocalLayoutRetarget<Pair, i64, authoring::BroadcastReturn, LocalAtomic<Pair>>
    for DishonestRetarget
{
    fn with_input_layout(self: Arc<Self>) -> Result<NodeTemplate, LayoutError> {
        self.template()
    }
}

#[test]
fn retarget_validates_selected_layout_before_revoking_the_old_revision() {
    let mut graph = GraphBuilder::new("dishonest-retarget");
    let source = graph.node("source", |x: i64| x).unwrap();
    let node = graph.node("node", DishonestRetarget).unwrap();
    let old = node.inlet().field(Pair::SLOT_LEFT).target().unwrap();
    assert!(node.input(Layout::atomic()).is_err());
    graph.connect(source.output(), old).unwrap();
}
