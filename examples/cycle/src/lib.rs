use example_contracts::{Text, TransformError};
#[phenix_sdk::operation(interface = "example.cycle@1", operation = "run")]
pub type Cycle = (Text, Text, TransformError);
#[phenix_sdk::plugin(id = "example.cycle", version = "1.0.0")]
mod cycle {
    use super::Cycle;
    use example_contracts::{Text, TransformError};
    use phenix_sdk::{CallResult, Host};
    #[derive(Default)]
    pub struct State;
    #[import]
    type Again = Cycle;
    #[export(Cycle)]
    fn run(_: &mut State, host: &Host<'_>, request: Text) -> CallResult<Text, TransformError> {
        host.invoke::<Again>(&request)
    }
}
