use example_graph_contracts::{Start, Task};
use phenix_sdk::{live, *};

const GRAPH: StaticGraphContribution = StaticGraphContribution {
    plugin: "example.graph-c",
    events: &[],
    actions: &[],
    nodes: &[StaticGraphNode {
        id: "c",
        triggers: &["example.graph.start"],
        after: &["example.graph-a/a", "example.graph-b/b"],
        selector: "",
        operation: "example.graph.task@1/run",
        writes: &[],
        join: JoinPolicy::AllActive,
        failure: FailurePolicy::FailRoot,
    }],
};

#[plugin(
    id = "example.graph-c",
    define(Task),
    provide(Task, slot = "task"),
    graph(GRAPH)
)]
mod provider {
    use super::*;
    #[provide(Task::run)]
    fn run(input: Start) -> Result<String, String> {
        if let Some(path) = std::env::var_os("PHENIX_GRAPH_C_WITNESS") {
            std::fs::write(path, b"successor invoked").expect("graph-c invocation witness");
        }
        Ok(input.text)
    }
}
