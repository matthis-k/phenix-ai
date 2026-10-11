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
        pending: bool,
    }
    #[provide(Counter::reference)]
    fn reference(
        state: &mut State,
        _: &live::Host<'_>,
        _: (),
    ) -> CallResult<live::ValueRef<u64>, String> {
        Ok(state.counter.reference()?)
    }
    #[provide(Counter::run)]
    fn run(state: &mut State, host: &live::Host<'_>, input: String) -> CallResult<u64, String> {
        match input.as_str() {
            "domain" => Err(CallError::Domain("recoverable".into())),
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
        PendingResult::Pending
    }
    #[poll(Counter::slow)]
    fn poll(state: &mut State, _: &live::Host<'_>) -> PendingResult<u64, String> {
        assert!(state.pending);
        state.pending = false;
        PendingResult::Ready(Ok(23))
    }
    #[cancel(Counter::slow)]
    fn cancel(state: &mut State, _: &live::Host<'_>) -> Result<bool, Failure> {
        state.pending = false;
        Ok(true)
    }
}
