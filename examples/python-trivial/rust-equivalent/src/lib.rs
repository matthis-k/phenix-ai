#[derive(phenix_sdk::Portable)]
pub struct Text {
    pub text: String,
}

#[phenix_sdk::contract(id = "example.greeter@1")]
pub trait Api {
    fn greet(&self, request: Text) -> Result<Text, String>;
}

#[phenix_sdk::plugin(
    id = "example.rust-greeter",
    version = "1.0.0",
    define(Api),
    provide(Api, slot = "Api")
)]
mod greeter {
    use super::*;
    #[derive(Default)]
    pub struct State {
        calls: u64,
    }
    #[provide(Api::greet)]
    fn greet(state: &mut State, request: Text) -> phenix_sdk::CallResult<Text, String> {
        state.calls += 1;
        Ok(Text {
            text: format!("Hello {} ({})", request.text, state.calls),
        })
    }
}
