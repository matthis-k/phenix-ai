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

#[contract(id = "example.graph.task@1")]
pub trait Task {
    fn run(&mut self, input: Start) -> Result<String, String>;
}

#[property(owner = "example.graph-a", id = "result", get, set)]
pub type ResultA = String;

#[property(owner = "example.graph-b", id = "result", get, set)]
pub type ResultB = String;
