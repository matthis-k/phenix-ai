use example_graph_contracts::{Start, Task};
use phenix_sdk::{live, *};

const GRAPH: StaticGraphContribution = StaticGraphContribution {
    plugin: "example.graph-b",
    events: &[],
    actions: &[],
    nodes: &[StaticGraphNode {
        id: "b",
        triggers: &["example.graph.start"],
        after: &[],
        selector: "",
        operation: "example.graph.task@1/run",
        writes: &[],
        join: JoinPolicy::AllActive,
        failure: FailurePolicy::FailRoot,
    }],
};

#[plugin(
    id = "example.graph-b",
    define(Task),
    provide(Task, slot = "task"),
    graph(GRAPH)
)]
mod provider {
    use super::*;
    #[provide(Task::run)]
    fn run(input: Start) -> Result<String, String> {
        if input.text == "domain-b" {
            return Err("domain fixture b".into());
        }
        Ok(input.text)
    }
}
