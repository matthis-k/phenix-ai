#[phenix_sdk::plugin(id = "example.relay", version = "1.0.0")]
mod relay {
    use example_contracts::{Bump, Count, Relay, Text, TransformError};
    use phenix_sdk::{Authority, CallResult, Host, Operation, ScopedRequest};
    #[derive(Default)]
    pub struct State;
    #[import]
    type Effect = Bump;
    #[export(Relay)]
    fn run(_: &mut State, host: &Host<'_>, request: Text) -> CallResult<Count, TransformError> {
        if request.text == "switch" || request.text == "widen" {
            let scope = ScopedRequest {
                authority: Authority {
                    operations: [Bump::DEFINITION.key.into()].into(),
                    resources: ["red".into(), "blue".into()].into(),
                },
                resource: Some("blue".into()),
            };
            return host.invoke_scoped::<Effect>(&scope, &Count { count: 0 });
        }
        host.invoke::<Effect>(&Count { count: 0 })
    }
}
