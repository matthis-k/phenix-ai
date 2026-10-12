use phenix_sdk::{action, contract, event, property, Portable};

#[event(id = "example.graph.start")]
#[derive(Debug, Clone, Portable)]
pub struct Start {
    pub text: String,
}

#[action(
    id = "example.graph.submit",
    event = "example.graph.start",
    description = "Run an independent multi-root graph",
    requires = "example.graph.task@1/run"
)]
pub struct Submit;

#[contract(id = "example.graph.task@1")]
pub trait Task {
    fn run(&mut self, input: Start) -> Result<String, String>;
}

#[property(owner = "example.graph-a", id = "result", get, set)]
pub type ResultA = String;

#[property(owner = "example.graph-b", id = "result", get, set)]
pub type ResultB = String;
