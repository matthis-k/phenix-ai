//! ABI 1.0. Pointers are borrowed for the synchronous call unless explicitly owned.
use std::ffi::c_void;
use crate::{Code,Failure,Value};
#[repr(C)] #[derive(Clone,Copy)]
pub struct Bytes {pub ptr:*const u8,pub len:usize}
impl Bytes {
    pub fn new(bytes:&[u8])->Self{Self{ptr:bytes.as_ptr(),len:bytes.len()}}
    /// # Safety
    /// Nonzero input must point to readable bytes for the whole call.
    pub unsafe fn read<'a>(self)->Result<&'a [u8],Failure>{if self.len>crate::MAX_BYTES || (self.len>0 && self.ptr.is_null()){return Err(Failure::new(Code::SchemaRejected,"invalid borrowed buffer"));}if self.len==0{Ok(&[])}else{Ok(unsafe{std::slice::from_raw_parts(self.ptr,self.len)})}}
}
#[repr(C)] pub struct Reply {pub status:u32,pub ptr:*mut u8,pub len:usize,pub release:unsafe extern "C" fn(*mut u8,usize)}
unsafe extern "C" fn release(ptr:*mut u8,len:usize){if !ptr.is_null(){drop(unsafe{Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr,len))});}}
impl Reply {
    pub fn value(status:u32,value:Value)->Self{match value.encode(){Ok(x)=>Self::bytes(status,x),Err(e)=>Self::failure(e)}}
    fn bytes(status:u32,bytes:Vec<u8>)->Self{let mut bytes=bytes.into_boxed_slice();let result=Self{status,ptr:bytes.as_mut_ptr(),len:bytes.len(),release};std::mem::forget(bytes);result}
    pub fn failure(f:Failure)->Self{Self::bytes(2,f.to_value().encode().unwrap_or_else(|_|vec![0]))}
    /// # Safety
    /// Reply must come from the trusted ABI producer with a valid release callback.
    pub unsafe fn consume(self)->Result<(u32,Value),Failure>{
        let decoded=unsafe{Bytes{ptr:self.ptr,len:self.len}.read()}.and_then(Value::decode);
        unsafe{(self.release)(self.ptr,self.len)};
        if self.status>2{return Err(Failure::new(Code::AbiIncompatible,"unknown reply status"));}
        decoded.map(|v|(self.status,v))
    }
}
#[repr(C)] #[derive(Clone,Copy)] pub struct Header{pub major:u32,pub minor:u32,pub table_bytes:u32,pub features:u32}
#[repr(C)] #[derive(Clone,Copy)] pub struct HostApi{pub scope:u64,pub invoke:unsafe extern "C" fn(u64,Bytes,Bytes)->Reply}
#[repr(C)] #[derive(Clone,Copy)] pub struct PluginTable{
    pub header:Header,
    pub create:unsafe extern "C" fn(*mut *mut c_void)->Reply,
    pub invoke:unsafe extern "C" fn(*mut c_void,*const HostApi,Bytes,Bytes)->Reply,
    pub stop:unsafe extern "C" fn(*mut c_void)->Reply,
}
pub fn boundary(f:impl FnOnce()->Reply)->Reply{std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_|Reply::failure(Failure::new(Code::PluginFailed,"caught plugin panic")))}
