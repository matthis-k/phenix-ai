//! Run with: cargo run --example multi_contributor
//! Contributions are installed out of order, planned, assembled, compiled once,
//! then executed. No graph macro, plugin loader, harness or authorization.
use phenix_graph::{
    Deployment, PatchBuilder, PatchPlanner, PatchSpec, PatchTarget, ReportRetention, Value,
};

fn main() {
    let mut planner = PatchPlanner::new();

    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("plugin.final")
                .after("plugin.model")
                .writes_all([
                    PatchTarget::Node("finish".into()),
                    PatchTarget::edge_nodes("model", "value", "finish", "run", "value"),
                    PatchTarget::PublicOutput("result".into()),
                ]),
            |graph| {
                let finish = graph
                    .node("finish", |x: i64| x * 2)
                    .map_err(|e| e.to_string())?;
                let from = graph.local_output::<i64>("model", "value")?;
                graph.connect(from, finish.inlet())?;
                graph.expose_output("result", finish.output())?;
                Ok(())
            },
        ))
        .unwrap();

    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("plugin.model")
                .after("plugin.base")
                .writes_all([
                    PatchTarget::Node("model".into()),
                    PatchTarget::edge_nodes("accept", "value", "model", "run", "value"),
                ]),
            |graph| {
                let model = graph
                    .node("model", |x: i64| x + 10)
                    .map_err(|e| e.to_string())?;
                let from = graph.local_output::<i64>("accept", "value")?;
                graph.connect(from, model.inlet())?;
                Ok(())
            },
        ))
        .unwrap();

    planner
        .add_patch(PatchBuilder::custom(
            PatchSpec::new("plugin.base").writes_all([
                PatchTarget::Node("accept".into()),
                PatchTarget::PublicInput("request".into()),
                PatchTarget::Trigger("run".into()),
            ]),
            |graph| {
                let accept = graph
                    .node("accept", |x: i64| x + 1)
                    .map_err(|e| e.to_string())?;
                let input = graph.expose_input("request", accept.inlet())?;
                graph.trigger("run", [input])?;
                Ok(())
            },
        ))
        .unwrap();

    let (artifact, resolution) = planner.build("agent").unwrap();
    assert_eq!(
        resolution.selected,
        ["plugin.base", "plugin.model", "plugin.final"]
    );
    let engine = Deployment::new(artifact.graph)
        .report_retention(ReportRetention::TerminalOutputs)
        .build()
        .unwrap();
    let result = engine.fire(1, vec![Value::local(5_i64)]).unwrap();
    assert!(result.success());
    assert_eq!(
        result.publications().last().unwrap().borrow::<i64>(),
        Some(&32)
    );
    println!("Graph result: 32; revisions: {}", artifact.revisions.len());
}
