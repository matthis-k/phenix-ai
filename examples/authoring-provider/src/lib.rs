use example_authoring_api::Counter;
use phenix_sdk::{
    live::{self, PendingResult},
    *,
};

#[plugin(
    id = "test.authored-provider",
    version = "1.0.0",
    profile = "state",
    define(Counter),
    provide(Counter, slot = "counter")
)]
mod provider {
    use super::*;
    #[derive(Default)]
    struct State {
        #[get]
        #[set]
        #[watch]
        counter: u64,
        #[expose]
        #[watch]
        observed: u64,
        pending: bool,
        worker: Option<(std::sync::mpsc::Sender<()>, std::thread::JoinHandle<()>)>,
    }
    #[provide(Counter::reference)]
    fn reference(state: &mut State) -> CallResult<live::ValueRef<u64>, String> {
        Ok(state.counter.reference()?)
    }
    #[provide(Counter::observed_reference)]
    fn observed_reference(state: &mut State) -> CallResult<live::ValueRef<u64>, String> {
        Ok(state.reference_observed()?)
    }

    #[provide(Counter::run)]
    fn run(state: &mut State, host: &live::Host<'_>, input: String) -> CallResult<u64, String> {
        match input.as_str() {
            "domain" => Err(CallError::Domain("recoverable".into())),
            "owned-publish" => {
                state.publish_observed(host, 42).map_err(|error| {
                    Failure::new(Code::Denied, format!("owner publication: {error}"))
                })?;
                Ok(state.observed)
            }
            "owned-value" => Ok(state.observed),
            "owned-update" => {
                state
                    .update_observed(host, |previous| previous + 1)
                    .map_err(|error| {
                        Failure::new(Code::Denied, format!("owner update publication: {error:?}"))
                    })?;
                Ok(state.observed)
            }
            "owned-reject" => {
                state
                    .try_update_observed(host, |_previous| -> Result<u64, String> {
                        Err("candidate rejected".into())
                    })
                    .map_err(|error| match error {
                        live::OwnedUpdateError::Rejected(problem) => CallError::Domain(problem),
                        live::OwnedUpdateError::Publish(problem) => {
                            CallError::Kernel(Failure::new(
                                Code::Denied,
                                format!("owner update publication: {problem:?}"),
                            ))
                        }
                    })?;
                Ok(state.observed)
            }
            _ => (|| {
                let mut edit = state
                    .counter
                    .edit(host)
                    .map_err(|error| Failure::new(Code::Denied, error.to_string()))?;
                *edit = input
                    .parse()
                    .map_err(|_| CallError::Domain("invalid number".into()))?;
                let result = *edit;
                edit.commit(host)
                    .map_err(|error| Failure::new(Code::Denied, error.to_string()))?;
                Ok(result)
            })(),
        }
    }
    #[pending(Counter::slow)]
    fn slow(state: &mut State, _: &live::Host<'_>, input: String) -> PendingResult<u64, String> {
        state.pending = true;
        if input == "panic" {
            panic!("authoring fixture panic")
        }
        let (stop, wait) = std::sync::mpsc::channel();
        let (ready, started) = std::sync::mpsc::sync_channel(0);
        let worker = std::thread::spawn(move || {
            let descriptor = std::fs::File::open("/dev/null").unwrap();
            ready.send(()).unwrap();
            wait.recv().unwrap();
            drop(descriptor);
        });
        started.recv().unwrap();
        state.worker = Some((stop, worker));
        PendingResult::Pending
    }
    #[poll(Counter::slow)]
    fn poll(state: &mut State, _: &live::Host<'_>) -> PendingResult<u64, String> {
        assert!(state.pending);
        state.pending = false;
        drain(state);
        PendingResult::Ready(Ok(23))
    }
    #[cancel(Counter::slow)]
    fn cancel(state: &mut State, _: &live::Host<'_>) -> Result<bool, Failure> {
        state.pending = false;
        drain(state);
        Ok(true)
    }
    fn drain(state: &mut State) {
        if let Some((stop, worker)) = state.worker.take() {
            stop.send(()).unwrap();
            worker.join().unwrap();
        }
    }
}
