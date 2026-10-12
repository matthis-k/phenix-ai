use example_graph_contracts::{Start, Submit, Task};
use phenix_sdk::{live, *};

const GRAPH: StaticGraphContribution = StaticGraphContribution {
    plugin: "example.graph-a",
    events: &[Start::EVENT],
    actions: &[Submit::ACTION],
    nodes: &[StaticGraphNode {
        id: "a",
        triggers: &["example.graph.start"],
        after: &[],
        selector: "",
        operation: "example.graph.task@1/run",
        writes: &["example.graph-a/result"],
        join: JoinPolicy::AllActive,
        failure: FailurePolicy::Skip,
    }],
};

#[plugin(
    id = "example.graph-a",
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
        if input.text == "domain" {
            return Err(CallError::Domain("domain fixture".into()));
        }
        let mut edit = state
            .result
            .edit(host)
            .map_err(|e| Failure::new(Code::PluginFailed, e.to_string()))?;
        *edit = format!("A:{}", input.text);
        edit.commit(host)
            .map_err(|e| Failure::new(Code::PluginFailed, e.to_string()))?;
        Ok(input.text)
    }
}

#[cfg(target_os = "linux")]
#[used]
#[link_section = ".init_array"]
static GRAPH_CONSTRUCTOR_WITNESS: extern "C" fn() = graph_constructor;

#[cfg(target_os = "linux")]
extern "C" fn graph_constructor() {
    if let Some(path) = std::env::var_os("PHENIX_GRAPH_CONSTRUCTOR_WITNESS") {
        let _ = std::fs::write(path, b"native code loaded");
    }
}
