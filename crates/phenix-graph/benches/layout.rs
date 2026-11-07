use phenix_graph::Deployment;
use phenix_graph::ReportRetention;
use phenix_graph::{
    AllSameCohort, Atomic, Engine, FlattenTo, InputSlot, InvocationCtx, ModuleCatalog,
    ModuleDefBuilder, ModuleTriggerTarget, NodeTemplate, OutputSchema, OutputSlot, Port,
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
    let mut module = ModuleDefBuilder::new("atomic-bench");
    let node = module
        .register("twice", NodeTemplate::function(twice))
        .unwrap();
    module
        .export_input("value", vec![node.input::<i64>("run", "value")])
        .unwrap();
    module
        .export_output("result", node.output::<i64>("value"))
        .unwrap();
    module.trigger(1, vec![ModuleTriggerTarget::PublicInput(0)]);
    let mut catalog = ModuleCatalog::new();
    catalog.add(module.build()).unwrap();
    Deployment::new(catalog.compile("atomic-bench").unwrap())
        .store(Store::new())
        .report_retention(ReportRetention::RetainPayloads)
        .build()
        .unwrap()
}

fn flattened_engine() -> Engine {
    let mut module = ModuleDefBuilder::new("flattened-bench");
    let node = module
        .register(
            "sum",
            NodeTemplate::with_input_layout::<FlattenTo<Pair, 1>, i64, _>(
                "run",
                phenix_graph::AllSameCohort,
                sum,
            )
            .unwrap(),
        )
        .unwrap();
    module
        .export_input("left", vec![node.input::<i64>("run", "left")])
        .unwrap();
    module
        .export_input("right", vec![node.input::<i64>("run", "right")])
        .unwrap();
    module
        .export_output("result", node.output::<i64>("value"))
        .unwrap();
    module.trigger(
        1,
        vec![
            ModuleTriggerTarget::PublicInput(0),
            ModuleTriggerTarget::PublicInput(1),
        ],
    );
    let mut catalog = ModuleCatalog::new();
    catalog.add(module.build()).unwrap();
    Deployment::new(catalog.compile("flattened-bench").unwrap())
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
        vec![OutputSlot::typed::<i64>(0).named("value")],
        |ctx: InvocationCtx| Ok(vec![emit(0, ctx.arg::<i64>(0)? * 2)]),
    );
    let node = module.register("twice", node).unwrap();
    module
        .export_input("value", vec![node.input::<i64>("run", "value")])
        .unwrap();
    module
        .export_output("result", node.output::<i64>("value"))
        .unwrap();
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
    let node = module.register("multi", action).unwrap();
    module
        .export_input("left", vec![node.input::<i64>("left", "value")])
        .unwrap();
    module
        .export_input("right", vec![node.input::<i64>("right", "value")])
        .unwrap();
    module
        .export_output("result", node.output::<i64>("value"))
        .unwrap();
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
