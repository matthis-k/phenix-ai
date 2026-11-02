use phenix_sdk::Portable;
#[derive(Debug, Portable)]
pub struct Text {
    pub text: String,
}
#[derive(Debug, Portable)]
pub enum Branch {
    Model { text: String },
    Tool { text: String },
}
#[derive(Debug, Portable)]
pub enum Error {
    Failed,
}
#[phenix_sdk::contract(id = "experiment.prepare@1")]
pub trait Prepare {
    fn run(&self, request: Text) -> Result<Branch, Error>;
}
#[phenix_sdk::contract(id = "experiment.service@1")]
pub trait Service {
    fn run(&self, request: Text) -> Result<Text, Error>;
}
#[phenix_sdk::contract(id = "experiment.complete@1")]
pub trait Complete {
    fn run(&self, request: Text) -> Result<Text, Error>;
}
