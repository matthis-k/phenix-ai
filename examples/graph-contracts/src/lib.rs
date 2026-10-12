use phenix_sdk::{action, contract, event, property, Portable};

#[event(id = "example.graph.start")]
#[derive(Debug, Clone, Portable)]
pub struct Start {
    pub text: String,
}

/// Native evidence that action resource context survives graph dispatch.
pub fn check_resource(
    host: &phenix_sdk::live::Host<'_>,
    input: &Start,
) -> Result<(), phenix_sdk::Failure> {
    if input.text == "resource" {
        assert_eq!(
            host.resource()?.as_deref(),
            Some("example.graph-a.property.result")
        );
    }
    Ok(())
}

#[action(
    id = "example.graph.submit",
    event = "example.graph.start",
    description = "Run an independent multi-root graph",
    requires = "example.graph.task@1/run"
)]
pub struct Submit;

#[event(id = "example.graph.partial")]
#[derive(Debug, Clone, Portable)]
pub struct PartialStart {
    pub text: String,
}

#[action(
    id = "example.graph.submit-partial",
    event = "example.graph.partial",
    description = "Run A and C with dormant B and D",
    requires = "example.graph.task@1/run"
)]
pub struct SubmitPartial;

const GRAPH: phenix_sdk::StaticGraphContribution = phenix_sdk::StaticGraphContribution {
    plugin: "example.graph-contracts",
    events: &[PartialStart::EVENT, Start::EVENT],
    actions: &[Submit::ACTION, SubmitPartial::ACTION],
    nodes: &[],
};

#[phenix_sdk::plugin(
    id = "example.graph-contracts",
    version = "1.0.0",
    profile = "definitions",
    define(Task),
    graph(super::GRAPH)
)]
pub mod definitions {
    use super::Task;
}

#[contract(id = "example.graph.task@1")]
pub trait Task {
    fn run(&mut self, input: Start) -> Result<String, String>;
}

#[property(owner = "example.graph-a", id = "result", get, set)]
pub type ResultA = String;

#[property(owner = "example.graph-b", id = "result", get, set)]
pub type ResultB = String;
