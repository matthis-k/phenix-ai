use example_contracts::{Text, TransformError};
#[phenix_sdk::contract(id = "example.cycle@1")]
pub trait CycleContract { fn run(&self, request: Text) -> Result<Text, TransformError>; }
pub type Cycle = CycleContract::run;
#[phenix_sdk::plugin(id = "example.cycle", version = "1.0.0", define(super::CycleContract), provide(super::CycleContract, slot = "Cycle"), require(super::CycleContract, slot = "Again"))]
mod cycle {
    use super::Cycle;
    use example_contracts::{Text, TransformError};
    use phenix_sdk::{CallResult, Host};
    #[derive(Default)]
    pub struct State;
    type Again = Cycle;
    #[provide(Cycle)]
    fn run(_: &mut State, host: &Host<'_>, request: Text) -> CallResult<Text, TransformError> {
        host.invoke::<Again>("Again", &request)
    }
}
