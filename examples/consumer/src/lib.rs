#[phenix_sdk::plugin(
    id = "example.consumer",
    version = "1.0.0",
    define(example_contracts::RunContract),
    define(example_contracts::TransformContract),
    provide(example_contracts::RunContract, slot = "Run"),
    require(example_contracts::TransformContract, slot = "Provider")
)]
mod consumer {
    use example_contracts::{Count, ReadCount, Run, Text, Transform, TransformError};
    use phenix_sdk::{CallError, CallResult, Code, Failure, Host};
    pub struct State;
    impl Default for State {
        fn default() -> Self {
            super::lifecycle("consumer:create");
            Self
        }
    }
    impl Drop for State {
        fn drop(&mut self) {
            super::lifecycle("consumer:stop");
        }
    }
    type Provider = Transform;
    #[provide(Run)]
    fn run(_: &mut State, host: &Host<'_>, request: Text) -> CallResult<Text, TransformError> {
        if request.text == "handled-denial" {
            // ReadCount is deliberately not a declared import. Typed SDK calls
            // still need host authorization, even when a plugin handles failure.
            match host.invoke::<ReadCount>("undeclared", &Count { count: 0 }) {
                Err(CallError::Kernel(failure)) if failure.code == Code::Denied => {}
                _ => return Err(Failure::new(Code::PluginFailed, "expected denied import").into()),
            }
            return Ok(Text {
                text: "handled".into(),
            });
        }
        if request.text == "replace-failure" {
            let error = host
                .invoke::<Provider>(
                    "Provider",
                    &Text {
                        text: "panic".into(),
                    },
                )
                .unwrap_err();
            if !matches!(error, CallError::Kernel(_)) {
                return Err(Failure::new(Code::PluginFailed, "expected host failure").into());
            }
            return Err(Failure::new(Code::PluginFailed, "consumer replacement failure").into());
        }
        host.invoke::<Provider>("Provider", &request)
    }
}

// Optional conformance witness; absent in the ordinary demo deployment.
fn lifecycle(event: &str) {
    use std::io::Write;
    if let Ok(path) = std::env::var("PHENIX_LIFECYCLE_WITNESS") {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        writeln!(file, "{event}").unwrap();
    }
}
