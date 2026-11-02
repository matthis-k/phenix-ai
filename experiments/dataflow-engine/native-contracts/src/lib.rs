use phenix_sdk::Portable;

#[derive(Clone, Debug, Portable)]
pub struct Identity {
    pub root: u64,
    pub wave: u64,
    pub activation: u64,
    pub occurrence: u64,
}

#[derive(Clone, Debug, Portable)]
pub struct Message {
    pub id: Identity,
    pub text: String,
    pub tool: bool,
    pub pending: bool,
}

#[derive(Debug, Portable)]
pub enum Branch {
    Model {
        id: Identity,
        text: String,
        tool: bool,
        pending: bool,
    },
    Tool {
        id: Identity,
        text: String,
        tool: bool,
        pending: bool,
    },
}

#[derive(Debug, Portable)]
pub struct ResultValue {
    pub id: Identity,
    pub text: String,
}

#[derive(Debug, Portable)]
pub enum Error {
    Failed,
}

#[phenix_sdk::contract(id = "experiment.prepare@1")]
pub trait Prepare {
    fn run(&self, request: Message) -> Result<Branch, Error>;
}
#[phenix_sdk::contract(id = "experiment.service@1")]
pub trait Service {
    fn run(&self, request: Message) -> Result<ResultValue, Error>;
}
#[phenix_sdk::contract(id = "experiment.complete@1")]
pub trait Complete {
    fn run(&self, request: ResultValue) -> Result<ResultValue, Error>;
}
