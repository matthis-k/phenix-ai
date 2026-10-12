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
        writes: &["example.graph-b/result"],
        join: JoinPolicy::AllActive,
        failure: FailurePolicy::FailRoot,
    }],
};

#[plugin(
    id = "example.graph-b",
    define(Task),
    provide(Task, slot = "task"),
    profile = "state",
    graph(GRAPH)
)]
mod provider {
    use super::*;
    #[derive(Default)]
    struct State {
        #[get(id = "result")]
        #[set]
        result: String,
    }

    #[provide(Task::run)]
    fn run(state: &mut State, host: &live::Host<'_>, input: Start) -> CallResult<String, String> {
        example_graph_contracts::check_resource(host, &input)?;
        if input.text == "domain-b" {
            return Err(CallError::Domain("domain fixture b".into()));
        }
        let mut edit = state
            .result
            .edit(host)
            .map_err(|e| Failure::new(Code::PluginFailed, e.to_string()))?;
        *edit = format!("B:{}", input.text);
        edit.commit(host)
            .map_err(|e| Failure::new(Code::PluginFailed, e.to_string()))?;
        Ok(input.text)
    }
}
