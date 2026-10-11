use example_authoring_api::{Count, Counter, Relay};
use phenix_sdk::{live, *};

#[plugin(
    id = "test.authored-client",
    version = "1.0.0",
    profile = "state",
    define(Relay),
    define(Count),
    provide(Relay, slot = "relay"),
    require(Counter, slot = "counter"),
    require(Counter, slot = "fallback", optional),
    require(Counter, slot = "all", many(min = 0))
)]
mod client {
    use super::*;
    #[derive(Default)]
    struct State {}
    #[provide(Relay::run)]
    fn run(_: &mut State, host: &live::Host<'_>, input: String) -> CallResult<u64, String> {
        assert_eq!(imports::fallback::count(host)?, 0);
        assert_eq!(imports::all::count(host)?, 1);
        let first = imports::all::invoke::<Counter::reference>(host, &())?;
        let indexed = imports::all::invoke_at::<Counter::reference>(host, 0, &())?;
        assert_eq!(first.location(), indexed.location());
        assert!(matches!(
            imports::all::invoke_at::<Counter::run>(host, 1, &input),
            Err(CallError::Kernel(Failure {
                code: Code::MissingBinding,
                ..
            }))
        ));
        imports::counter::invoke::<Counter::run>(host, &input)
    }
}
