//! Typed grouped provider/consumer authoring without symbolic endpoint parsing.
use phenix_graph::{
    DeclaredValue, Deployment, GraphBuilder, GraphContract, InterfaceShape, ReportRetention,
    SlotField, Value,
};

// Manual descriptors work without derives or a shared provider implementation type.
struct Request;
const LEFT: SlotField<Request, i64> = SlotField::new("pair.left");
const RIGHT: SlotField<Request, i64> = SlotField::new("pair.right");

fn main() -> Result<(), String> {
    let contract = GraphContract::new(
        "example/grouped-routes@1",
        InterfaceShape::new()
            .input_slot("request", LEFT.name(), DeclaredValue::local::<i64>())
            .map_err(|e| e.to_string())?
            .input_slot("request", RIGHT.name(), DeclaredValue::local::<i64>())
            .map_err(|e| e.to_string())?
            .output("result", DeclaredValue::local::<i64>())
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    // A provider may expose ordinary independent nodes under grouped public slots.
    // Grouping is metadata, not a new readiness barrier or runtime wrapper.
    let mut provider = GraphBuilder::new("provider");
    let left = provider
        .node("left", |x: i64| x)
        .map_err(|e| e.to_string())?;
    let right = provider
        .node("right", |x: i64| -x)
        .map_err(|e| e.to_string())?;
    provider.expose_input_field("request", LEFT, left.inlet())?;
    provider.expose_input_field("request", RIGHT, right.inlet())?;
    provider.expose_output("result", left.output())?;
    provider
        .implements_public(&contract)
        .map_err(|e| e.to_string())?;

    let mut graph = GraphBuilder::new("consumer");
    graph.instance("selected", &provider.module())?;
    let bound = graph
        .bind_contract("selected", &contract)
        .map_err(|e| e.to_string())?;
    let left = bound
        .local_field("request", LEFT)
        .map_err(|e| e.to_string())?;
    let right = bound
        .local_field("request", RIGHT)
        .map_err(|e| e.to_string())?;
    let result = bound
        .local_output::<i64>("result")
        .map_err(|e| e.to_string())?;
    // Projected handles are owned; the graph borrow ends after the last projection.
    let left = graph.expose_input_field("request", LEFT, left)?;
    let right = graph.expose_input_field("request", RIGHT, right)?;
    graph.expose_output("result", result)?;
    let trigger = graph.trigger("run", [left, right])?;
    let engine = Deployment::new(graph.compile().map_err(|e| e.to_string())?)
        .report_retention(ReportRetention::TerminalOutputs)
        .build()?;
    let report = engine.fire(
        trigger.get(),
        vec![Value::local(13_i64), Value::local(4_i64)],
    )?;
    assert!(report.success());
    let mut values: Vec<_> = report
        .publications()
        .map(|p| *p.borrow::<i64>().unwrap())
        .collect();
    values.sort();
    assert_eq!(values, [-4, 13]);
    Ok(())
}
