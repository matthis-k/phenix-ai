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

#[event(id = "example.graph.slow")]
#[derive(Debug, Clone, Portable)]
pub struct SlowStart {
    pub text: String,
}

#[action(
    id = "example.graph.submit-slow",
    event = "example.graph.slow",
    description = "Run a pending source with an independent source and a join",
    requires = "example.graph.slow@1/run,example.graph.task@1/run"
)]
pub struct SubmitSlow;

const GRAPH: phenix_sdk::StaticGraphContribution = phenix_sdk::StaticGraphContribution {
    plugin: "example.graph-contracts",
    events: &[PartialStart::EVENT, SlowStart::EVENT, Start::EVENT],
    actions: &[Submit::ACTION, SubmitPartial::ACTION, SubmitSlow::ACTION],
    nodes: &[],
};

#[phenix_sdk::plugin(
    id = "example.graph-contracts",
    version = "1.0.0",
    profile = "definitions",
    define(Task),
    define(SlowTask),
    graph(super::GRAPH)
)]
pub mod definitions {
    use super::{SlowTask, Task};
}

#[contract(id = "example.graph.task@1")]
pub trait Task {
    fn run(&mut self, input: Start) -> Result<String, String>;
}

#[contract(id = "example.graph.consume@1")]
pub trait Consume {
    fn run(&mut self, input: String) -> Result<String, String>;
}

#[contract(id = "example.graph.reference-source@1")]
pub trait ReferenceSource {
    fn run(&mut self, input: Start) -> Result<phenix_sdk::live::ValueRef<String>, String>;
}

#[contract(id = "example.graph.reference-consumer@1")]
pub trait ReferenceConsumer {
    fn run(&mut self, input: phenix_sdk::live::ValueRef<String>) -> Result<String, String>;
}

#[contract(id = "example.graph.slow@1")]
pub trait SlowTask {
    fn run(&mut self, input: Start) -> Result<String, String>;
}

/// The graph kernel sends one filtered change batch per watcher per epoch.
#[derive(Debug, Clone, Portable)]
pub struct ChangeNotice {
    pub epoch: u64,
    pub properties: Vec<String>,
}

#[contract(id = "example.graph.changed@1")]
pub trait StateHook {
    fn run(&mut self, input: ChangeNotice) -> Result<String, String>;
}

#[contract(id = "example.graph.pending-change@1")]
pub trait PendingStateHook {
    fn run(&mut self, input: ChangeNotice) -> Result<String, String>;
}

#[property(owner = "example.graph-a", id = "result", get, set)]
pub type ResultA = String;

#[property(owner = "example.graph-b", id = "result", get, set)]
pub type ResultB = String;

/// An imported watch capability. The graph grant alone is not an import.
#[property(owner = "example.graph-a", id = "signal", get, watch)]
pub type SignalA = String;

#[property(owner = "example.graph-a", id = "pending_signal", get, watch)]
pub type PendingSignalA = String;
