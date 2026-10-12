#[phenix_sdk::plugin(id = "example.restricted", version = "1.0.0", define(example_contracts::ScopedRunContract), define(example_contracts::RelayContract), define(example_contracts::BumpContract), provide(example_contracts::ScopedRunContract, slot = "ScopedRun"), require(example_contracts::RelayContract, slot = "Delegate"), require(example_contracts::BumpContract, slot = "EffectLimit", optional))]
mod restricted {
    use example_contracts::{Bump, Count, Relay, ScopedRun, Text, TransformError};
    use phenix_sdk::{Authority, CallResult, Host, ScopedRequest};
    #[derive(Default)]
    pub struct State;
    type Delegate = Relay;
    type EffectLimit = Bump;
    #[provide(ScopedRun)]
    fn run(_: &mut State, host: &Host<'_>, request: Text) -> CallResult<Count, TransformError> {
        let resource = if request.text == "blue" {
            "blue"
        } else {
            "red"
        };
        let scope = ScopedRequest {
            authority: Authority {
                operations: [
                    Relay::DEFINITION.key.into(),
                    EffectLimit::DEFINITION.key.into(),
                ]
                .into(),
                resources: ["red".into()].into(),
            },
            resource: Some(resource.into()),
        };
        host.invoke_scoped::<Delegate>("Delegate", &scope, &request)
    }
}
