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
        imports::counter::invoke::<Counter::run>(host, &input)
    }
}
