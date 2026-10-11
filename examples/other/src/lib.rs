#[phenix_sdk::plugin(id = "example.other", version = "1.0.0")]
mod other {
    use example_contracts::{Other, Text, Transform, TransformError};
    use phenix_sdk::{CallResult, Host};
    #[derive(Default)]
    pub struct State;
    #[import(implementation = "example.provider")]
    type Provider = Transform;
    #[export(Other)]
    fn run(_: &mut State, host: &Host<'_>, request: Text) -> CallResult<Text, TransformError> {
        host.invoke::<Provider>(&request)
    }
}
