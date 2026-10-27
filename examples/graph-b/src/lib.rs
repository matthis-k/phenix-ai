use example_graph_contracts::{Consume, Start, Task};
use phenix_sdk::{live, *};

const GRAPH: StaticGraphContribution = StaticGraphContribution {
    plugin: "example.graph-b",
    events: &[],
    actions: &[],
    nodes: &[StaticGraphNode {
        id: "b",
        triggers: &["example.graph.slow", "example.graph.start"],
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
    define(Consume),
    provide(Task, slot = "task"),
    provide(Consume, slot = "consume"),
    profile = "state",
    graph(GRAPH)
)]
mod provider {
    use super::*;
    #[provide(Consume::run)]
    fn consume(_state: &mut State, input: String) -> CallResult<String, String> {
        if let Some(path) = std::env::var_os("PHENIX_GRAPH_B_CONSUME_WITNESS") {
            use std::io::Write;
            writeln!(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .unwrap(),
                "called"
            )
            .unwrap();
        }
        Ok(format!("consumed:{input}"))
    }

    #[derive(Default)]
    struct State {
        #[get(id = "result")]
        #[set]
        #[watch]
        result: String,
    }

    #[provide(Task::run)]
    fn run(state: &mut State, host: &live::Host<'_>, input: Start) -> CallResult<String, String> {
        example_graph_contracts::check_resource(host, &input)?;
        if input.text == "kernel-b" {
            return Err(Failure::new(Code::PluginFailed, "kernel failure fixture b").into());
        }
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
