#[phenix_sdk::plugin(
    id = "example.relay",
    version = "1.0.0",
    define(example_contracts::RelayContract),
    define(example_contracts::BumpContract),
    provide(example_contracts::RelayContract, slot = "Relay"),
    require(example_contracts::BumpContract, slot = "Effect")
)]
mod relay {
    use example_contracts::{Bump, Count, Relay, Text, TransformError};
    use phenix_sdk::{Authority, CallResult, Host, ScopedRequest};
    #[derive(Default)]
    pub struct State;
    type Effect = Bump;
    #[provide(Relay)]
    fn run(_: &mut State, host: &Host<'_>, request: Text) -> CallResult<Count, TransformError> {
        if request.text == "switch" || request.text == "widen" {
            let scope = ScopedRequest {
                authority: Authority {
                    operations: [Bump::DEFINITION.key.into()].into(),
                    resources: ["red".into(), "blue".into()].into(),
                },
                resource: Some("blue".into()),
            };
            return host.invoke_scoped::<Effect>("Effect", &scope, &Count { count: 0 });
        }
        host.invoke::<Effect>("Effect", &Count { count: 0 })
    }
}
