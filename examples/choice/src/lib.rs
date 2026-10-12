#[phenix_sdk::plugin(
    id = "example.choice",
    version = "1.0.0",
    define(example_contracts::ChoiceContract),
    define(example_contracts::TransformContract),
    define(example_contracts::ReadCountContract),
    provide(example_contracts::ChoiceContract, slot = "Choice"),
    require(example_contracts::TransformContract, slot = "Provider"),
    require(
        example_contracts::ReadCountContract,
        slot = "OptionalCounter",
        optional
    )
)]
mod choice {
    use example_contracts::{Choice, Count, ReadCount, Text, Transform, TransformError};
    use phenix_sdk::{CallResult, Host};
    #[derive(Default)]
    pub struct State;
    type Provider = Transform;
    type OptionalCounter = ReadCount;
    #[provide(Choice)]
    fn run(_: &mut State, host: &Host<'_>, request: Text) -> CallResult<Text, TransformError> {
        if request.text == "optional" {
            let count = host.invoke::<OptionalCounter>("OptionalCounter", &Count { count: 0 })?;
            return Ok(Text {
                text: count.count.to_string(),
            });
        }
        host.invoke::<Provider>("Provider", &request)
    }
}
