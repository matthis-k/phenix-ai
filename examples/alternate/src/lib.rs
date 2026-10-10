#[phenix_sdk::plugin(id = "example.alternate", version = "1.0.0")]
mod alternate {
    use example_contracts::{Text, Transform, TransformError};
    use phenix_sdk::{CallResult, Host};
    #[derive(Default)]
    pub struct State;
    #[export(Transform)]
    fn apply(_: &mut State, _: &Host<'_>, request: Text) -> CallResult<Text, TransformError> {
        Ok(request)
    }
}
