use libloading::Library;
use phenix_artifact::VerifiedPackage;
use phenix_contract::{abi::{Bytes,Header,StateHostApi,StatePluginTable},Code,Failure,Value};
use std::ffi::c_void;
pub(crate) struct Native { table: StatePluginTable, instance: *mut c_void, _image: Library }
impl Native {
    pub(crate) fn load(package: &VerifiedPackage)->Result<Self,Failure> {
        package.recheck()?;
        let image=unsafe { Library::new(package.artifact()) }.map_err(|e|Failure::new(Code::ArtifactInvalid,e.to_string()))?;
        let entry=unsafe { image.get::<unsafe extern "C" fn()->*const Header>(b"phenix_native_v3\0") }.map_err(|e|Failure::new(Code::AbiIncompatible,e.to_string()))?;
        let ptr=unsafe { entry() };
        if ptr.is_null() || !(ptr as usize).is_multiple_of(std::mem::align_of::<StatePluginTable>()) { return Err(Failure::new(Code::AbiIncompatible,"invalid ABI header pointer")); }
        let table=unsafe { StatePluginTable::negotiate(ptr) }?;
        let mut instance=std::ptr::null_mut();
        let result=unsafe { table.create.unwrap()(&mut instance).consume() };
        if !matches!(&result,Ok((0,Value::Null))) || instance.is_null() {
            if !instance.is_null() { let _=unsafe { table.stop.unwrap()(instance).consume() }; }
            return Err(match result { Ok((2,value))=>Failure::from_value(value)?,Err(error)=>error,_=>Failure::new(Code::PluginFailed,"invalid create result") });
        }
        Ok(Self { table,instance,_image:image })
    }
    pub(crate) fn initialize(&self,scope:u64)->Result<(),Failure> {
        let host=super::runtime::state_host(scope,0,0);
        match unsafe { self.table.initialize.unwrap()(self.instance,&host).consume() }? {
            (0,Value::Null)=>Ok(()),(2,value)=>Err(Failure::from_value(value)?),_=>Err(Failure::new(Code::PluginFailed,"invalid initialization reply")),
        }
    }
    pub(crate) fn invoke(&self,scope:u64,key:&str,input:&[u8])->Result<(u32,Value),Failure> {
        let host=super::runtime::state_host(scope,0,0);
        self.pending(&host,key,Some(input),false)
    }
    pub(crate) fn pending(&self,host:&StateHostApi,key:&str,input:Option<&[u8]>,cancel:bool)->Result<(u32,Value),Failure> {
        if self.instance.is_null() { return Err(Failure::new(Code::PluginFailed,"instance already stopped")); }
        let key=Bytes::new(key.as_bytes());
        unsafe {
            let reply=match input { Some(bytes)=>self.table.invoke.unwrap()(self.instance,host,key,Bytes::new(bytes)),None if cancel=>self.table.cancel.unwrap()(self.instance,host,key),None=>self.table.poll.unwrap()(self.instance,host,key) };
            reply.consume()
        }
    }
    pub(crate) fn stop(&mut self)->Result<(),Failure> {
        let instance=std::mem::replace(&mut self.instance,std::ptr::null_mut());
        if instance.is_null() { return Err(Failure::new(Code::PluginFailed,"instance already stopped")); }
        match unsafe { self.table.stop.unwrap()(instance).consume() }? {
            (0,Value::Null)=>Ok(()),(2,value)=>Err(Failure::from_value(value)?),_=>Err(Failure::new(Code::AbiIncompatible,"invalid stop result")),
        }
    }
}
impl Drop for Native { fn drop(&mut self) { if !self.instance.is_null() { let _=self.stop(); } } }
#[cfg(test)]
mod tests {
    use super::*;
    use phenix_contract::abi::Reply;
    use std::sync::atomic::{AtomicUsize,Ordering};
    static STOPS:AtomicUsize=AtomicUsize::new(0);
    unsafe extern "C" fn stop(instance:*mut c_void)->Reply { STOPS.fetch_add(1,Ordering::SeqCst);drop(unsafe { Box::from_raw(instance.cast::<u8>()) });Reply::value(0,Value::Null) }
    unsafe extern "C" fn create(_: *mut *mut c_void)->Reply { unreachable!() }
    unsafe extern "C" fn initialize(_: *mut c_void,_:*const StateHostApi)->Reply { unreachable!() }
    unsafe extern "C" fn invoke(_: *mut c_void,_:*const StateHostApi,_:Bytes,_:Bytes)->Reply { unreachable!() }
    unsafe extern "C" fn poll(_: *mut c_void,_:*const StateHostApi,_:Bytes)->Reply { unreachable!() }
    #[test]
    fn repeated_stop_rejects_without_reentering_producer() {
        STOPS.store(0,Ordering::SeqCst);
        let mut native=Native { table:StatePluginTable { header:Header::state(std::mem::size_of::<StatePluginTable>()),create:Some(create),initialize:Some(initialize),invoke:Some(invoke),poll:Some(poll),cancel:Some(poll),stop:Some(stop) },instance:Box::into_raw(Box::new(0u8)).cast(),_image:libloading::os::unix::Library::this().into() };
        native.stop().unwrap();assert_eq!(native.stop().unwrap_err().code,Code::PluginFailed);drop(native);assert_eq!(STOPS.load(Ordering::SeqCst),1);
    }
}
