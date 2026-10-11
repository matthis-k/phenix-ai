use example_contracts::{PendingRun, Text, Transform, TransformError};
use phenix_sdk::{CallResult, Host};
#[phenix_sdk::plugin(id = "example.pending", version = "1.0.0")]
mod plugin {
    use super::*;
    #[import]
    type Service = Transform;
    #[derive(Default)]
    pub struct State;
    #[export(PendingRun)]
    fn run(_: &mut State, host: &Host<'_>, request: Text) -> CallResult<Text, TransformError> {
        host.invoke::<Service>(&request)
    }
}
