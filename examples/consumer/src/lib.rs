#[phenix_sdk::plugin(id="example.consumer",version="1.0.0")]
mod consumer {
    use example_contracts::{Run,Text,Transform,TransformError};
    use phenix_sdk::{CallResult,Host};
    #[derive(Default)] pub struct State;
    #[import] type Provider=Transform;
    #[export(Run)]
    fn run(_:&mut State,host:&Host<'_>,request:Text)->CallResult<Text,TransformError>{host.invoke::<Provider>(&request)}
}
