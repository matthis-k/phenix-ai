#[phenix_sdk::plugin(id = "example.alternate", version = "1.0.0", define(example_contracts::TransformContract), provide(example_contracts::TransformContract, slot = "Transform"))]
mod alternate {
    use example_contracts::{Text, Transform, TransformError};
    use phenix_sdk::{CallResult, Host};
    #[derive(Default)]
    pub struct State;
    #[provide(Transform)]
    fn apply(_: &mut State, _: &Host<'_>, request: Text) -> CallResult<Text, TransformError> {
        Ok(request)
    }
}
