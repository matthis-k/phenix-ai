#![cfg(feature = "derive")]
use phenix_graph::Node;

use phenix_graph::ReportRetention;
use phenix_graph::Deployment;
use phenix_graph::{
    Atomic, BufferMode, ContractValue, DeepFlatten, Flatten, FlattenTo, InputLayout,
    AllSameCohort, GraphBuilder, Layout, ModuleDefBuilder, ModuleTriggerTarget, OutputSchema,
    OutputSlot, Port, PortLayout, Store, TypedPort, Value,
};

#[derive(ContractValue, PortLayout)]
#[contract(schema = "example.add-args@1")]
struct AddArgs {
    left: i32,
    right: i32,
}

fn add(args: AddArgs) -> i32 { args.left + args.right }
fn square(value: i32) -> i32 { value * value }
fn increment(value: i32) -> i32 { value + 1 }
fn decrement(value: i32) -> i32 { value - 1 }
fn positive(value: i32) -> Option<i32> { (value > 0).then_some(value) }
fn parse(value: String) -> Result<i32, std::num::ParseIntError> { value.parse() }
fn request_score(request: Request) -> u64 {
    request.id + u64::from(request.address.zip) + u64::from(request.priority)
        + request.metadata.revision
}

enum ProcessorInput { Add(AddArgs), Reset, Message(String) }
struct MaybeOutput(Option<i32>);
impl OutputSchema for MaybeOutput {
    fn output_slot() -> OutputSlot { OutputSlot::local::<Option<i32>>(0).named("value") }
}
impl phenix_graph::IntoEmission for MaybeOutput {
    fn into_emission(self) -> phenix_graph::Emission {
        phenix_graph::emit_local(0, self.0)
    }
}

#[derive(ContractValue, PortLayout)]
#[contract(schema = "example.address@1")]
struct Address {
    city: String,
    zip: u32,
}

#[derive(ContractValue, PortLayout)]
#[contract(schema = "example.metadata@1")]
struct Metadata { revision: u64 }

#[derive(ContractValue, PortLayout)]
#[contract(schema = "example.request@1")]
struct Request {
    id: u64,
    address: Address,
    priority: u8,
    #[port(atomic)]
    metadata: Metadata,
}

#[allow(dead_code)]
#[derive(PortLayout)]
struct ExplicitRequest {
    id: u64,
    #[port(flatten)]
    address: Address,
    priority: u8,
    #[port(atomic)]
    metadata: Metadata,
}

#[allow(dead_code)]
#[derive(PortLayout)]
struct TupleEnvelope { pair: (i32, String) }

#[allow(dead_code)]
#[derive(PortLayout)]
struct UnitEnvelope { signal: () }

#[test]
fn ordinary_function_infers_atomic_input_and_output() {
    let mut graph = GraphBuilder::new("square");
    let square_node = graph.node("square", Node::wire(square)).unwrap();
    let request = graph.expose_input("request", square_node.slot::<i32>("value")).unwrap();
    graph.expose_output("result", square_node.output()).unwrap();
    graph.trigger("run", [request]).unwrap();
    let report = Deployment::new(graph.compile().unwrap()).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
        .fire(1, vec![Value::of(7_i32)]).unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.publications().next().unwrap().value::<i32>().unwrap(), 49);
}

#[test]
fn graph_builder_connects_typed_endpoints_symbolically() {
    let mut graph = GraphBuilder::new("connected-functions");
    let square_node = graph.node("square", Node::wire(square)).unwrap();
    let increment_node = graph.node("increment", Node::wire(increment)).unwrap();
    let request = graph.expose_input("request", square_node.slot::<i32>("value")).unwrap();
    graph.connect(square_node.output(),
        increment_node.slot::<i32>("value")).unwrap();
    graph.expose_output("result", increment_node.output()).unwrap();
    graph.trigger("run", [request]).unwrap();
    let report = Deployment::new(graph.compile().unwrap()).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
        .fire(1, vec![Value::of(6_i32)]).unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.publications().last().unwrap().value::<i32>().unwrap(), 37);
}

#[test]
fn typed_diamond_fanout_and_flattened_join_preserve_cohort() {
    let mut graph = GraphBuilder::new("typed-diamond");
    let source = graph.node("source", Node::wire(square)).unwrap();
    let left = graph.node("left", Node::wire(increment)).unwrap();
    let right = graph.node("right", Node::wire(decrement)).unwrap();
    let join = graph.node("join", Node::wire_layout(add, Layout::<AddArgs>::wire_fields())).unwrap();
    let request = graph.expose_input("request", source.slot::<i32>("value")).unwrap();
    graph.connect(source.output(), left.slot::<i32>("value")).unwrap();
    graph.connect(source.output(), right.slot::<i32>("value")).unwrap();
    graph.connect(left.output(), join.slot::<i32>("left")).unwrap();
    graph.connect(right.output(), join.slot::<i32>("right")).unwrap();
    graph.expose_output("result", join.output()).unwrap();
    graph.trigger("run", [request]).unwrap();
    let report = Deployment::new(graph.compile().unwrap()).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
        .fire(1, vec![Value::of(4_i32)]).unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.publications().last().unwrap().value::<i32>().unwrap(), 32);
}

#[test]
fn flattened_layout_reconstructs_the_original_argument() {
    let mut graph = GraphBuilder::new("flatten-add");
    let adder = graph.node("adder", Node::wire_layout(add, Layout::<AddArgs>::wire_fields())).unwrap();
    let left = graph.expose_input("left", adder.slot::<i32>("left")).unwrap();
    let right = graph.expose_input("right", adder.slot::<i32>("right")).unwrap();
    graph.expose_output("sum", adder.output()).unwrap();
    graph.trigger("run", [left, right]).unwrap();
    let report = Deployment::new(graph.compile().unwrap()).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
        .fire(1, vec![Value::of(12_i32), Value::of(30_i32)]).unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.publications().next().unwrap().value::<i32>().unwrap(), 42);
}

#[test]
fn recursive_and_atomic_field_directives_have_stable_paths() {
    let slots = <Flatten<Request> as InputLayout>::slots().unwrap();
    let names: Vec<_> = slots.iter().map(|slot| slot.name.as_deref().unwrap()).collect();
    assert_eq!(names, ["id", "address", "priority", "metadata"]);
    let depth_two = <FlattenTo<Request, 2> as InputLayout>::slots().unwrap();
    assert_eq!(depth_two.iter().map(|slot| slot.name.as_deref().unwrap()).collect::<Vec<_>>(),
        ["id", "address.city", "address.zip", "priority", "metadata"]);
    let deep = <DeepFlatten<Request> as InputLayout>::slots().unwrap();
    assert_eq!(deep.iter().map(|slot| slot.name.as_deref().unwrap()).collect::<Vec<_>>(),
        ["id", "address.city", "address.zip", "priority", "metadata"]);
    let explicit = <Flatten<ExplicitRequest> as InputLayout>::slots().unwrap();
    assert_eq!(explicit.iter().map(|slot| slot.name.as_deref().unwrap()).collect::<Vec<_>>(),
        ["id", "address.city", "address.zip", "priority", "metadata"]);
    let atomic = <Atomic<Request> as InputLayout>::slots().unwrap();
    assert_eq!(atomic.len(), 1);
    assert_eq!(atomic[0].name.as_deref(), Some("value"));
    let nested_tuple = <Flatten<(i32, String)> as InputLayout>::slots().unwrap();
    assert_eq!(nested_tuple.iter().map(|s| s.name.as_deref().unwrap()).collect::<Vec<_>>(), ["0", "1"]);
    let tuple_in_struct = <FlattenTo<TupleEnvelope, 2> as InputLayout>::slots().unwrap();
    assert_eq!(tuple_in_struct.iter().map(|s| s.name.as_deref().unwrap()).collect::<Vec<_>>(),
        ["pair.0", "pair.1"]);
    let unit_field = <Flatten<UnitEnvelope> as InputLayout>::slots().unwrap();
    assert_eq!(unit_field.iter().map(|s| s.name.as_deref().unwrap()).collect::<Vec<_>>(), ["signal"]);
    let port = Port::for_layout::<Flatten<AddArgs>>(0, "run", AllSameCohort).unwrap()
        .slot_buffer("left", BufferMode::Immediate).unwrap();
    assert_eq!(port.slots[0].buffer, BufferMode::Immediate);
    assert_eq!(port.slots[1].buffer, BufferMode::Deferred);
}

#[test]
fn bounded_flat_layout_reconstructs_nested_request_without_cloning() {
    let mut graph = GraphBuilder::new("request-layout");
    let request = graph.node("request", Node::wire_layout(request_score, Layout::<Request>::wire_flatten_to::<2>())).unwrap();
    let id = graph.expose_input("id", request.slot::<u64>("id")).unwrap();
    let city = graph.expose_input("city", request.slot::<String>("address.city")).unwrap();
    let zip = graph.expose_input("zip", request.slot::<u32>("address.zip")).unwrap();
    let priority = graph.expose_input("priority", request.slot::<u8>("priority")).unwrap();
    let metadata = graph.expose_input("metadata", request.slot::<Metadata>("metadata")).unwrap();
    graph.expose_output("score", request.output()).unwrap();
    graph.trigger("run", [id, city, zip, priority, metadata]).unwrap();
    let report = Deployment::new(graph.compile().unwrap()).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
        .fire(1, vec![Value::of(10_u64), Value::of("Paris".to_owned()), Value::of(75001_u32),
            Value::of(2_u8), Value::of(Metadata { revision: 30 })]).unwrap();
    assert!(report.success(), "{:?}", report.error_messages());
    assert_eq!(report.publications().next().unwrap().value::<u64>().unwrap(), 75043);
}

#[test]
fn explicit_action_decides_when_to_emit_or_fail_without_interpreting_payload_types() {
    // The returned Option is *handled by the node*, not by the output
    // system. A local plain Option<T> would itself be broadcast as data.
    let optional = phenix_graph::NodeTemplate::with_action(
        0,
        vec![Port::for_layout::<Atomic<i32>>(0, "run", AllSameCohort).unwrap()],
        vec![OutputSlot::typed::<i32>(0).named("value")],
        |mut ctx: phenix_graph::InvocationCtx| {
            let input = ctx.take_prepared::<i32>()?;
            Ok(positive(input).into_iter().map(|value| phenix_graph::emit(0, value)).collect())
        },
    );
    let mut module = ModuleDefBuilder::new("optional");
    let filter = module.register("filter", optional).unwrap();
    module.export_input("request", vec![filter.slot::<i32>("value")]).unwrap();
    module.export_output("result", filter.output()).unwrap();
    module.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
    let mut catalog = phenix_graph::ModuleCatalog::new();
    catalog.add(module.build()).unwrap();
    let graph = catalog.compile("optional").unwrap();
    let report = Deployment::new(graph).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
        .fire(1, vec![Value::of(-1_i32)]).unwrap();
    assert!(report.success());
    assert_eq!(report.publications().count(), 0);

    // Likewise, Result only signals an invocation error because this
    // *action* explicitly maps the domain error to NodeAction's Err.
    let parser = phenix_graph::NodeTemplate::with_action(
        0,
        vec![Port::for_layout::<Atomic<String>>(0, "run", AllSameCohort).unwrap()],
        vec![OutputSlot::typed::<i32>(0).named("value")],
        |mut ctx: phenix_graph::InvocationCtx| {
            let input = ctx.take_prepared::<String>()?;
            let parsed = parse(input).map_err(|error| error.to_string())?;
            Ok(vec![phenix_graph::emit(0, parsed)])
        },
    );
    let mut module = ModuleDefBuilder::new("fallible");
    let parser = module.register("parse", parser).unwrap();
    module.export_input("request", vec![parser.slot::<String>("value")]).unwrap();
    module.export_output("result", parser.output()).unwrap();
    module.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
    let mut catalog = phenix_graph::ModuleCatalog::new();
    catalog.add(module.build()).unwrap();
    let graph = catalog.compile("fallible").unwrap();
    let good = Deployment::new(std::sync::Arc::clone(&graph)).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
        .fire(1, vec![Value::of("23".to_owned())]).unwrap();
    assert!(good.success());
    assert_eq!(good.publications().next().unwrap().value::<i32>().unwrap(), 23);
    let bad = Deployment::new(graph).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap()
        .fire(1, vec![Value::of("nope".to_owned())]).unwrap();
    assert!(!bad.success());
}

#[test]
fn heterogeneous_ports_explicitly_dispatch_to_one_event_handler() {
    let ports = vec![
        TypedPort::new::<Flatten<AddArgs>, _>(0, "add", AllSameCohort, ProcessorInput::Add).unwrap(),
        TypedPort::new::<Atomic<()>, _>(1, "reset", AllSameCohort, |_| ProcessorInput::Reset).unwrap(),
        TypedPort::new::<Atomic<String>, _>(2, "message", AllSameCohort, ProcessorInput::Message).unwrap(),
    ];
    let node = phenix_graph::NodeTemplate::multi_port::<ProcessorInput, MaybeOutput, _>(ports,
        |input, _ctx| Ok(MaybeOutput(match input {
            ProcessorInput::Add(args) => Some(args.left + args.right),
            ProcessorInput::Reset => None,
            ProcessorInput::Message(text) => { let _ = text.len(); None },
        })));
    let mut module = ModuleDefBuilder::new("multi-port");
    let processor = module.register("processor", node).unwrap();
    module.export_input("left", vec![processor.input::<i32>("add", "left")]).unwrap();
    module.export_input("right", vec![processor.input::<i32>("add", "right")]).unwrap();
    module.export_input("reset", vec![processor.input::<()>("reset", "value")]).unwrap();
    module.export_input("message", vec![processor.input::<String>("message", "value")]).unwrap();
    // The typed node publishes Option<i32> as a local payload. This test
    // inspects publications directly; no wire-only public module export is
    // needed (or valid for a local Option<i32> output).
    module.trigger(1, vec![ModuleTriggerTarget::PublicInput(0), ModuleTriggerTarget::PublicInput(1)]);
    module.trigger(2, vec![ModuleTriggerTarget::PublicInput(2)]);
    module.trigger(3, vec![ModuleTriggerTarget::PublicInput(3)]);
    let mut catalog = phenix_graph::ModuleCatalog::new();
    catalog.add(module.build()).unwrap();
    let graph = catalog.compile("multi-port").unwrap();
    let engine = Deployment::new(graph).store(Store::new()).report_retention(ReportRetention::RetainPayloads).build().unwrap();
    let sum = engine.fire(1, vec![Value::of(20_i32), Value::of(22_i32)]).unwrap();
    assert!(sum.success(), "{:?}", sum.error_messages());
    assert_eq!(sum.publications().next().unwrap().borrow::<Option<i32>>(), Some(&Some(42)));
    let reset = engine.fire(2, vec![Value::of(())]).unwrap();
    assert!(reset.success());
    assert_eq!(reset.publications().next().unwrap().borrow::<Option<i32>>(), Some(&None));
    let message = engine.fire(3, vec![Value::of("hello".to_owned())]).unwrap();
    assert!(message.success());
    assert_eq!(message.publications().next().unwrap().borrow::<Option<i32>>(), Some(&None));
}

