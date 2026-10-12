use example_contracts::*;
use phenix_sdk::{CallResult, Code, Failure, Host};

#[phenix_sdk::plugin(
    id = "example.lifecycle.a-consumer",
    version = "1.0.0",
    define(example_contracts::LifecycleRunContract),
    define(example_contracts::LifecycleServiceContract),
    provide(example_contracts::LifecycleRunContract, slot = "LifecycleRun"),
    require(example_contracts::LifecycleServiceContract, slot = "Service")
)]
mod plugin {
    use super::*;
    #[derive(Default)]
    pub struct State {
        resource: Option<std::fs::File>,
    }
    impl Drop for State {
        fn drop(&mut self) {
            let _ = hook("drop");
        }
    }
    fn trace(event: &str) {
        use std::io::Write;
        if let Ok(path) = std::env::var("PHENIX_LIFECYCLE_TRACE") {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .unwrap();
            writeln!(file, "consumer:{event}").unwrap();
        }
    }
    fn hook(stage: &str) -> Result<(), Failure> {
        trace(stage);
        let faults = std::env::var("PHENIX_LIFECYCLE_FAULTS").unwrap_or_default();
        if faults
            .split(',')
            .any(|f| f == format!("consumer:{stage}-panic"))
        {
            panic!("lifecycle fixture panic");
        }
        if faults.split(',').any(|f| f == format!("consumer:{stage}")) {
            return Err(Failure::new(
                Code::PluginFailed,
                format!("consumer:{stage}"),
            ));
        }
        Ok(())
    }
    #[start]
    fn start(state: &mut State) -> Result<(), Failure> {
        if let Ok(path) = std::env::var("PHENIX_LIFECYCLE_TRACE") {
            state.resource = Some(
                std::fs::File::create(format!("{path}.consumer-resource"))
                    .map_err(|e| Failure::new(Code::PluginFailed, e.to_string()))?,
            );
        }
        hook("start")
    }
    #[stop]
    fn stop(_: &mut State) -> Result<(), Failure> {
        hook("stop")
    }

    type Service = LifecycleService;
    #[provide(LifecycleRun)]
    fn run(_: &mut State, host: &Host<'_>, request: Text) -> CallResult<Text, TransformError> {
        host.invoke::<Service>("Service", &request)
    }
}
