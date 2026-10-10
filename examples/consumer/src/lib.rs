#[phenix_sdk::plugin(id = "example.consumer", version = "1.0.0")]
mod consumer {
    use example_contracts::{Run, Text, Transform, TransformError};
    use phenix_sdk::{CallResult, Host};
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
    #[import]
    type Provider = Transform;
    #[export(Run)]
    fn run(_: &mut State, host: &Host<'_>, request: Text) -> CallResult<Text, TransformError> {
        host.invoke::<Provider>(&request)
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
