use example_graph_contracts::{ResultA, ResultB, Start, Task};
use phenix_sdk::{live, *};

const GRAPH: StaticGraphContribution = StaticGraphContribution {
    plugin: "example.graph-c",
    events: &[],
    actions: &[],
    nodes: &[
        StaticGraphNode {
            id: "c",
            triggers: &[
                "example.graph.partial",
                "example.graph.slow",
                "example.graph.start",
            ],
            after: &[
                "example.graph-a/a",
                "example.graph-a/slow",
                "example.graph-b/b",
            ],
            selector: "",
            operation: "example.graph.task@1/run",
            writes: &[],
            join: JoinPolicy::AllActive,
            failure: FailurePolicy::FailRoot,
        },
        StaticGraphNode {
            id: "d",
            triggers: &["example.graph.slow", "example.graph.start"],
            after: &["example.graph-c/c"],
            selector: "",
            operation: "example.graph.task@1/run",
            writes: &[],
            join: JoinPolicy::AllActive,
            failure: FailurePolicy::FailRoot,
        },
    ],
};

#[plugin(
    id = "example.graph-c",
    define(Task),
    provide(Task, slot = "task"),
    require(ResultA, slot = "a"),
    require(ResultB, slot = "b"),
    graph(GRAPH)
)]
mod provider {
    use super::*;
    #[provide(Task::run)]
    fn run(host: &live::Host<'_>, input: Start) -> CallResult<String, String> {
        example_graph_contracts::check_resource(host, &input)?;
        let first = ResultA::get::locate(host, "a")?;
        let second = ResultB::get::locate(host, "b")?;
        let a = first
            .read(host)
            .map_err(|e| Failure::new(Code::PluginFailed, e.to_string()))?;
        let b = second
            .read(host)
            .map_err(|e| Failure::new(Code::PluginFailed, e.to_string()))?;
        let observed = format!("{}|{}", a.value.as_ref(), b.value.as_ref());
        if let Some(path) = std::env::var_os("PHENIX_GRAPH_C_WITNESS") {
            std::fs::write(path, observed.as_bytes()).expect("graph-c invocation witness");
        }
        Ok(observed)
    }
}
