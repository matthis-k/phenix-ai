#[phenix_sdk::plugin(
    id = "example.other",
    version = "1.0.0",
    define(example_contracts::OtherContract),
    define(example_contracts::TransformContract),
    provide(example_contracts::OtherContract, slot = "Other"),
    require(
        example_contracts::TransformContract,
        slot = "Provider",
        provider = "example.provider"
    )
)]
mod other {
    use example_contracts::{Other, Text, Transform, TransformError};
    use phenix_sdk::{CallResult, Host};
    #[derive(Default)]
    pub struct State;
    type Provider = Transform;
    #[provide(Other)]
    fn run(_: &mut State, host: &Host<'_>, request: Text) -> CallResult<Text, TransformError> {
        host.invoke::<Provider>("Provider", &request)
    }
}
