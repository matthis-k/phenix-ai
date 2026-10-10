pub use phenix_contract::*;
pub use phenix_macros::{operation,plugin,Portable};
use abi::{Bytes,HostApi,Reply};
use std::{marker::PhantomData,rc::Rc};
/// An import host borrowed for one synchronous call. It cannot cross threads.
pub struct Host<'call>{api:&'call HostApi,_serial:PhantomData<Rc<()>>}
impl<'call> Host<'call>{
    /// # Safety
    /// The host API must be issued by the kernel and remain live for this call.
    pub unsafe fn from_api(api:&'call HostApi)->Self{Self{api,_serial:PhantomData}}
    pub fn invoke<O:Operation>(&self,request:&O::Request)->CallResult<O::Response,O::Error>{
        let bytes=request.to_value().encode()?;
        let reply=unsafe{(self.api.invoke)(self.api.scope,Bytes::new(O::DEFINITION.key.as_bytes()),Bytes::new(&bytes))};
        let(status,value)=unsafe{reply.consume()}?;
        match status{0=>Ok(O::Response::from_value(value)?),1=>Err(CallError::Domain(O::Error::from_value(value)?)),_=>Err(CallError::Kernel(Failure::from_value(value)?))}
    }
}
#[doc(hidden)] pub fn reply<T:Portable,E:Portable>(result:CallResult<T,E>)->Reply{match result{Ok(v)=>Reply::value(0,v.to_value()),Err(CallError::Domain(e))=>Reply::value(1,e.to_value()),Err(CallError::Kernel(e))=>Reply::failure(e)}}
