#[phenix_sdk::plugin(
    id = "example.resources",
    version = "1.0.0",
    define(example_contracts::BumpContract),
    define(example_contracts::ResourceCountContract),
    provide(example_contracts::BumpContract, slot = "Bump"),
    provide(example_contracts::ResourceCountContract, slot = "ResourceCount")
)]
mod resources {
    use example_contracts::{Bump, Count, ResourceCount, TransformError};
    use phenix_sdk::{CallResult, Code, Failure, Host};
    use std::collections::BTreeMap;
    #[derive(Default)]
    pub struct State {
        counters: BTreeMap<String, u64>,
    }
    fn resource(host: &Host<'_>) -> Result<String, Failure> {
        host.resource()?
            .ok_or_else(|| Failure::new(Code::Denied, "named resource required"))
    }
    #[provide(Bump)]
    fn bump(state: &mut State, host: &Host<'_>, _: Count) -> CallResult<Count, TransformError> {
        let count = state.counters.entry(resource(host)?).or_default();
        *count += 1;
        Ok(Count { count: *count })
    }
    #[provide(ResourceCount)]
    fn count(state: &mut State, host: &Host<'_>, _: Count) -> CallResult<Count, TransformError> {
        Ok(Count {
            count: *state.counters.get(&resource(host)?).unwrap_or(&0),
        })
    }
}
