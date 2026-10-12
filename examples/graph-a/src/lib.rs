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
        writes: &[],
        join: JoinPolicy::AllActive,
        failure: FailurePolicy::FailRoot,
    }],
};

#[plugin(
    id = "example.graph-a",
    define(Task),
    provide(Task, slot = "task"),
    graph(GRAPH)
)]
mod provider {
    use super::*;
    #[provide(Task::run)]
    fn run(input: Start) -> Result<String, String> {
        if input.text == "domain" {
            return Err("domain fixture".into());
        }
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
