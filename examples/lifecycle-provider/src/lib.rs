use example_contracts::*;
use phenix_sdk::{CallResult, Code, Failure, Host};

#[phenix_sdk::plugin(id = "example.lifecycle.z-provider", version = "1.0.0")]
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
            writeln!(file, "provider:{event}").unwrap();
        }
    }
    fn hook(stage: &str) -> Result<(), Failure> {
        trace(stage);
        let faults = std::env::var("PHENIX_LIFECYCLE_FAULTS").unwrap_or_default();
        if faults
            .split(',')
            .any(|f| f == format!("provider:{stage}-panic"))
        {
            panic!("lifecycle fixture panic");
        }
        if faults.split(',').any(|f| f == format!("provider:{stage}")) {
            return Err(Failure::new(
                Code::PluginFailed,
                format!("provider:{stage}"),
            ));
        }
        Ok(())
    }
    #[start]
    fn start(state: &mut State) -> Result<(), Failure> {
        if let Ok(path) = std::env::var("PHENIX_LIFECYCLE_TRACE") {
            state.resource = Some(
                std::fs::File::create(format!("{path}.provider-resource"))
                    .map_err(|e| Failure::new(Code::PluginFailed, e.to_string()))?,
            );
        }
        hook("start")
    }
    #[stop]
    fn stop(_: &mut State) -> Result<(), Failure> {
        hook("stop")
    }

    #[export(LifecycleService)]
    fn echo(_: &mut State, _: &Host<'_>, request: Text) -> CallResult<Text, TransformError> {
        trace("invoke");
        if request.text == "fail" {
            return Err(Failure::new(Code::PluginFailed, "provider:invoke").into());
        }
        Ok(Text {
            text: request.text.to_uppercase(),
        })
    }
}
