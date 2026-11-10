use phenix_graph::Deployment;
use phenix_graph::ReportRetention;
use phenix_graph::{
    AllSameCohort, Atomic, Engine, GraphBuilder, InputSlot, InvocationCtx, Layout, ModuleCatalog,
    ModuleDefBuilder, ModuleTriggerTarget, Node, NodeTemplate, OutputSchema, OutputSlot, Port,
    PortLayoutAt, SelectedValues, Store, TypedPort, Value, emit,
};
use std::time::{Duration, Instant};

struct Pair {
    left: i64,
    right: i64,
}
impl PortLayoutAt<1> for Pair {
    fn append_slots_at(
        prefix: &str,
        slots: &mut Vec<InputSlot>,
    ) -> Result<(), phenix_graph::LayoutError> {
        for name in ["left", "right"] {
            let path = if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}.{name}")
            };
            phenix_graph::push_layout_slot(slots, path, InputSlot::deferred::<i64>)?;
        }
        Ok(())
    }
    fn from_selected_at(values: &SelectedValues<'_>, prefix: &str) -> Result<Self, String> {
        let path = |field: &str| {
            if prefix.is_empty() {
                field.to_owned()
            } else {
                format!("{prefix}.{field}")
            }
        };
        Ok(Self {
            left: values.get_path(&path("left"))?,
            right: values.get_path(&path("right"))?,
        })
    }
}

fn sum(pair: Pair) -> i64 {
    pair.left + pair.right
}
fn twice(value: i64) -> i64 {
    value * 2
}
enum Event {
    Left(i64),
    Right(i64),
}
struct EventResult(i64);
impl OutputSchema for EventResult {
    fn output_slot() -> OutputSlot {
        OutputSlot::typed::<i64>(0).named("value")
    }
}
impl phenix_graph::IntoEmission for EventResult {
    fn into_emission(self) -> phenix_graph::Emission {
        emit(0, self.0)
    }
}

fn atomic_engine() -> Engine {
    let mut graph = GraphBuilder::new("atomic-bench");
    let node = graph.node("twice", Node::wire(twice)).unwrap();
    let input = graph.expose_input("value", node.inlet()).unwrap();
    graph.expose_output("result", node.output()).unwrap();
    graph.trigger("run", [input]).unwrap();
    Deployment::new(graph.compile().unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
}

fn flattened_engine() -> Engine {
    let mut graph = GraphBuilder::new("flattened-bench");
    let node = graph
        .node(
            "sum",
            Node::wire_layout(sum, Layout::<Pair>::wire_flatten_to::<1>()),
        )
        .unwrap();
    let inlets = node
        .inlets()
        .port(phenix_graph::InletPort::<_, Pair>::new("run"));
    let left = graph
        .expose_input(
            "left",
            inlets
                .field(phenix_graph::SlotField::<Pair, i64>::new("left"))
                .target()
                .unwrap(),
        )
        .unwrap();
    let right = graph
        .expose_input(
            "right",
            inlets
                .field(phenix_graph::SlotField::<Pair, i64>::new("right"))
                .target()
                .unwrap(),
        )
        .unwrap();
    graph.expose_output("result", node.output()).unwrap();
    graph.trigger("run", [left, right]).unwrap();
    Deployment::new(graph.compile().unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
}

fn erased_action_engine() -> Engine {
    let mut module = ModuleDefBuilder::new("erased-action-bench");
    let node = NodeTemplate::with_action(
        0,
        vec![Port::all(0, vec![InputSlot::immediate::<i64>(0).named("value")]).named("run")],
        OutputSlot::typed::<i64>(0).named("value"),
        |ctx: InvocationCtx| Ok(vec![emit(0, ctx.arg::<i64>(0)? * 2)]),
    );
    module.register("twice", node).unwrap();
    module.input(
        0,
        "value",
        <i64 as phenix_graph::ContractValue>::wire_schema(),
        vec![phenix_graph::ModuleInputTarget::NodeNamed {
            node: "twice".into(),
            port: "run".into(),
            slot: "value".into(),
        }],
    );
    module.output(
        0,
        "result",
        <i64 as phenix_graph::ContractValue>::wire_schema(),
        phenix_graph::ModuleOutputSource::NodeNamed {
            node: "twice".into(),
            output: "value".into(),
        },
    );
    module.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
    let mut catalog = ModuleCatalog::new();
    catalog.add(module.build()).unwrap();
    Deployment::new(catalog.compile("erased-action-bench").unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
}

fn multi_port_engine() -> Engine {
    let ports = vec![
        TypedPort::new::<Atomic<i64>, _>(0, "left", AllSameCohort, Event::Left).unwrap(),
        TypedPort::new::<Atomic<i64>, _>(1, "right", AllSameCohort, Event::Right).unwrap(),
    ];
    let action = NodeTemplate::multi_port::<Event, EventResult, _>(ports, |event, _ctx| {
        Ok(EventResult(match event {
            Event::Left(value) | Event::Right(value) => value * 2,
        }))
    });
    let mut module = ModuleDefBuilder::new("multi-port-bench");
    module.register("multi", action).unwrap();
    for (id, port) in [(0, "left"), (1, "right")] {
        module.input(
            id,
            port,
            <i64 as phenix_graph::ContractValue>::wire_schema(),
            vec![phenix_graph::ModuleInputTarget::NodeNamed {
                node: "multi".into(),
                port: port.into(),
                slot: "value".into(),
            }],
        );
    }
    module.output(
        0,
        "result",
        <i64 as phenix_graph::ContractValue>::wire_schema(),
        phenix_graph::ModuleOutputSource::NodeNamed {
            node: "multi".into(),
            output: "value".into(),
        },
    );
    module.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
    module.trigger(2, vec![ModuleTriggerTarget::PublicInput(1)]);
    let mut catalog = ModuleCatalog::new();
    catalog.add(module.build()).unwrap();
    Deployment::new(catalog.compile("multi-port-bench").unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
}

fn measure(engine: &Engine, values: impl Fn() -> Vec<Value>, iterations: usize) -> Duration {
    let started = Instant::now();
    for _ in 0..iterations {
        let report = engine.fire(1, values()).unwrap();
        assert!(report.success());
    }
    started.elapsed()
}

fn measure_two_triggers(engine: &Engine, iterations: usize) -> Duration {
    let started = Instant::now();
    for index in 0..iterations {
        let report = engine
            .fire(if index % 2 == 0 { 1 } else { 2 }, vec![Value::of(21_i64)])
            .unwrap();
        assert!(report.success());
    }
    started.elapsed()
}

fn main() {
    let compile_start = Instant::now();
    let atomic = atomic_engine();
    let atomic_compile = compile_start.elapsed();
    let compile_start = Instant::now();
    let flattened = flattened_engine();
    let flattened_compile = compile_start.elapsed();
    let compile_start = Instant::now();
    let erased_action = erased_action_engine();
    let erased_action_compile = compile_start.elapsed();
    let compile_start = Instant::now();
    let multi_port = multi_port_engine();
    let multi_port_compile = compile_start.elapsed();
    let iterations = 20_000;
    let atomic_runtime = measure(&atomic, || vec![Value::of(21_i64)], iterations);
    let flattened_runtime = measure(
        &flattened,
        || vec![Value::of(10_i64), Value::of(11_i64)],
        iterations,
    );
    let erased_action_runtime = measure(&erased_action, || vec![Value::of(21_i64)], iterations);
    let multi_port_runtime = measure_two_triggers(&multi_port, iterations);
    println!(
        "graph compile: typed-atomic={atomic_compile:?}, flattened={flattened_compile:?}, raw-action={erased_action_compile:?}, multi-port={multi_port_compile:?}"
    );
    println!(
        "{iterations} fires: typed-atomic={atomic_runtime:?}, flattened={flattened_runtime:?}, raw-action={erased_action_runtime:?}, multi-port={multi_port_runtime:?}"
    );
}
