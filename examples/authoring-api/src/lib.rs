use phenix_sdk::{contract, live::ValueRef, property};

#[contract(id = "test.authored.counter@1")]
pub trait Counter {
    fn slow(&mut self, input: String) -> Result<u64, String>;
    fn reference(&self) -> Result<ValueRef<u64>, String>;
    fn run(&mut self, input: String) -> Result<u64, String>;
}
#[contract(id = "test.authored.relay@1")]
pub trait Relay {
    fn run(&mut self, input: String) -> Result<u64, String>;
}
#[property(owner = "test.authored-provider", id = "counter", get, set, watch)]
pub type Count = u64;

#[property(owner = "test.authored-field", id = "notes", get)]
pub type Notes = Vec<String>;

#[phenix_sdk::plugin(
    id = "test.authored-api",
    version = "1.0.0",
    profile = "definitions",
    define(Counter),
    define(Relay)
)]
pub mod definitions {
    use super::*;
}
