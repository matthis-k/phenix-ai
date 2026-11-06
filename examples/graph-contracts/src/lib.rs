use phenix_sdk::{contract, property, Portable};

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

pub struct Submit;

#[derive(Debug, Clone, Portable)]
pub struct PartialStart {
    pub text: String,
}

pub struct SubmitPartial;

#[derive(Debug, Clone, Portable)]
pub struct SlowStart {
    pub text: String,
}

pub struct SubmitSlow;

#[phenix_sdk::plugin(
    id = "example.graph-contracts",
    version = "1.0.0",
    profile = "definitions",
    define(Task),
    define(SlowTask)
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

#[derive(Debug, Clone, Portable)]
pub struct JoinRequest {
    pub fast: String,
    pub slow: String,
}

#[contract(id = "example.graph.join-consumer@1")]
pub trait JoinConsumer {
    fn run(&mut self, input: JoinRequest) -> Result<String, String>;
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
