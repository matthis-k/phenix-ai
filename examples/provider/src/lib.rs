#[phenix_sdk::plugin(id = "example.provider", version = "1.0.0", define(example_contracts::TransformContract), define(example_contracts::ReadCountContract), provide(example_contracts::TransformContract, slot = "Transform"), provide(example_contracts::ReadCountContract, slot = "ReadCount"))]
mod provider {
    use example_contracts::{Count, ReadCount, Text, Transform, TransformError};
    use phenix_sdk::{CallError, CallResult, Host};
    #[derive(Default)]
    pub struct State {
        calls: u64,
    }
    #[provide(Transform)]
    fn apply(state: &mut State, _: &Host<'_>, request: Text) -> CallResult<Text, TransformError> {
        if request.text.is_empty() {
            return Err(CallError::Domain(TransformError::Empty));
        }
        if request.text == "panic" {
            panic!("fixture boundary panic");
        }
        state.calls += 1;
        Ok(Text {
            text: request.text.to_uppercase(),
        })
    }
    #[provide(ReadCount)]
    fn count(state: &mut State, _: &Host<'_>, _: Count) -> CallResult<Count, TransformError> {
        Ok(Count { count: state.calls })
    }
}

// Test witness: offline inspection must not run ELF constructors.
extern "C" fn witness() {
    if let Ok(path) = std::env::var("PHENIX_CONSTRUCTOR_WITNESS") {
        let _ = std::fs::write(path, b"loaded");
    }
}
#[used]
#[link_section = ".init_array"]
static WITNESS: extern "C" fn() = witness;
