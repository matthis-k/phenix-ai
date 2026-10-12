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
        Ok(input.text)
    }
}
