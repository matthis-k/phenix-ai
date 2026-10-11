#[phenix_sdk::plugin(id = "example.restricted", version = "1.0.0")]
mod restricted {
    use example_contracts::{Bump, Count, Relay, ScopedRun, Text, TransformError};
    use phenix_sdk::{Authority, CallResult, Host, Operation, ScopedRequest};
    #[derive(Default)]
    pub struct State;
    #[import]
    type Delegate = Relay;
    #[import(optional = "true")]
    type EffectLimit = Bump;
    #[export(ScopedRun)]
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
        host.invoke_scoped::<Delegate>(&scope, &request)
    }
}
