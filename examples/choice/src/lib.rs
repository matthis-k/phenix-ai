#[phenix_sdk::plugin(id = "example.choice", version = "1.0.0")]
mod choice {
    use example_contracts::{Choice, Count, ReadCount, Text, Transform, TransformError};
    use phenix_sdk::{CallResult, Host};
    #[derive(Default)]
    pub struct State;
    #[import]
    type Provider = Transform;
    #[import(optional = "true")]
    type OptionalCounter = ReadCount;
    #[export(Choice)]
    fn run(_: &mut State, host: &Host<'_>, request: Text) -> CallResult<Text, TransformError> {
        if request.text == "optional" {
            let count = host.invoke::<OptionalCounter>(&Count { count: 0 })?;
            return Ok(Text {
                text: count.count.to_string(),
            });
        }
        host.invoke::<Provider>(&request)
    }
}
