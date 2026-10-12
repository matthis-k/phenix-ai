use phenix_sdk::{action, contract, event, Portable};

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
