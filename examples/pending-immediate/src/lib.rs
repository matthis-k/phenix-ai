use example_contracts::{PendingRun, Text, Transform, TransformError};
use phenix_sdk::{CallResult, Host};
#[phenix_sdk::plugin(id = "example.pending", version = "1.0.0", define(example_contracts::PendingRunContract), define(example_contracts::TransformContract), provide(example_contracts::PendingRunContract, slot = "PendingRun"), require(example_contracts::TransformContract, slot = "Service"))]
mod plugin {
    use super::*;
    type Service = Transform;
    #[derive(Default)]
    pub struct State;
    #[provide(PendingRun)]
    fn run(_: &mut State, host: &Host<'_>, request: Text) -> CallResult<Text, TransformError> {
        host.invoke::<Service>("Service", &request)
    }
}
