use phenix_sdk::Portable;
#[derive(Debug, PartialEq, Eq, Portable)]
pub struct Text {
    pub text: String,
}
#[derive(Debug, PartialEq, Eq, Portable)]
pub struct Count {
    pub count: u64,
}
#[derive(Debug, PartialEq, Eq, Portable)]
pub enum TransformError {
    Empty,
}
#[phenix_sdk::contract(id = "example.transform.apply@1")]
pub trait TransformContract {
    fn apply(&self, request: Text) -> Result<Text, TransformError>;
}
pub type Transform = TransformContract::apply;
#[phenix_sdk::contract(id = "example.consumer@1")]
pub trait RunContract {
    fn run(&self, request: Text) -> Result<Text, TransformError>;
}
pub type Run = RunContract::run;
#[phenix_sdk::contract(id = "example.transform.count@1")]
pub trait ReadCountContract {
    fn count(&self, request: Count) -> Result<Count, TransformError>;
}
pub type ReadCount = ReadCountContract::count;

#[phenix_sdk::contract(id = "example.choice@1")]
pub trait ChoiceContract {
    fn run(&self, request: Text) -> Result<Text, TransformError>;
}
pub type Choice = ChoiceContract::run;

#[phenix_sdk::contract(id = "example.other@1")]
pub trait OtherContract {
    fn run(&self, request: Text) -> Result<Text, TransformError>;
}
pub type Other = OtherContract::run;

#[phenix_sdk::contract(id = "example.restricted@1")]
pub trait ScopedRunContract {
    fn run(&self, request: Text) -> Result<Count, TransformError>;
}
pub type ScopedRun = ScopedRunContract::run;

#[phenix_sdk::contract(id = "example.relay@1")]
pub trait RelayContract {
    fn run(&self, request: Text) -> Result<Count, TransformError>;
}
pub type Relay = RelayContract::run;

#[phenix_sdk::contract(id = "example.resource.bump@1")]
pub trait BumpContract {
    fn bump(&self, request: Count) -> Result<Count, TransformError>;
}
pub type Bump = BumpContract::bump;

#[phenix_sdk::contract(id = "example.resource.count@1")]
pub trait ResourceCountContract {
    fn count(&self, request: Count) -> Result<Count, TransformError>;
}
pub type ResourceCount = ResourceCountContract::count;

#[phenix_sdk::contract(id = "example.lifecycle-service@1")]
pub trait LifecycleServiceContract {
    fn echo(&self, request: Text) -> Result<Text, TransformError>;
}
pub type LifecycleService = LifecycleServiceContract::echo;
#[phenix_sdk::contract(id = "example.lifecycle@1")]
pub trait LifecycleRunContract {
    fn run(&self, request: Text) -> Result<Text, TransformError>;
}
pub type LifecycleRun = LifecycleRunContract::run;

#[phenix_sdk::contract(id = "example.pending@1")]
pub trait PendingRunContract {
    fn run(&self, request: Text) -> Result<Text, TransformError>;
}
pub type PendingRun = PendingRunContract::run;

#[phenix_sdk::contract(id = "example.pending-consumer@1")]
pub trait PendingConsumerContract {
    fn run(&self, request: Text) -> Result<Text, TransformError>;
}
pub type PendingConsumer = PendingConsumerContract::run;

#[phenix_sdk::contract(id = "example.revision@1")]
pub trait RevisionContract {
    fn run(&self, request: Text) -> Result<Text, TransformError>;
}
pub type Revision = RevisionContract::run;
