#[phenix_sdk::plugin(id="example.provider",version="1.0.0")]
mod provider {
    use example_contracts::{Count,ReadCount,Text,Transform,TransformError};
    use phenix_sdk::{CallError,CallResult,Host};
    #[derive(Default)] pub struct State {calls:u64}
    #[export(Transform)]
    fn apply(state:&mut State,_:&Host<'_>,request:Text)->CallResult<Text,TransformError>{
        if request.text.is_empty(){return Err(CallError::Domain(TransformError::Empty));}
        if request.text=="panic"{panic!("fixture boundary panic");}
        state.calls+=1;Ok(Text{text:request.text.to_uppercase()})
    }
    #[export(ReadCount)]
    fn count(state:&mut State,_:&Host<'_>,_:Count)->CallResult<Count,TransformError>{Ok(Count{count:state.calls})}
}
