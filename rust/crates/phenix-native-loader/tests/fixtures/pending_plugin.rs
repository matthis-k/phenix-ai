//! Self-contained compiled native .so fixture. Intentionally does not link
//! the Rust ABI crate: this catches accidental representation assumptions.
use std::{ffi::c_void, sync::atomic::{AtomicU64, Ordering}};

#[repr(C)]
#[derive(Clone, Copy)]
struct Header { major:u16, minor:u16, table_bytes:usize, required_features:u64 }
#[repr(C)]
#[derive(Clone, Copy)]
struct Slice { ptr:*const u8, len:usize }
#[repr(C)]
#[derive(Clone, Copy)]
struct Ticket { root_id:u64, call_id:u64 }
#[repr(C)]
#[derive(Clone, Copy)]
struct Request { ticket:Ticket, scope:u64, component:Slice, interface:Slice, input:Slice }
#[repr(C)]
struct Buffer {
    ptr:*mut u8,
    len:usize,
    owner:*mut c_void,
    release:Option<unsafe extern "C" fn(*mut c_void,*mut u8,usize)>,
}
#[repr(C)]
struct Result { status:u32, ticket:Ticket, payload:Buffer }
#[repr(C)]
struct Host { header:Header, context:*mut c_void, cancelled:Option<unsafe extern "C" fn(*mut c_void,Ticket)->u32>, wake:Option<unsafe extern "C" fn(*mut c_void,Ticket)>, invoke:Option<unsafe extern "C" fn(*mut c_void,Ticket,Slice,Slice)->Result> }
#[repr(C)]
struct Plugin {
    header:Header, context:*mut c_void,
    prepare:Option<unsafe extern "C" fn(*mut c_void,*const Host,u64)->u32>,
    start:Option<unsafe extern "C" fn(*mut c_void,u64)->u32>,
    begin:Option<unsafe extern "C" fn(*mut c_void,Request)->Result>,
    poll:Option<unsafe extern "C" fn(*mut c_void,Ticket)->Result>,
    cancel:Option<unsafe extern "C" fn(*mut c_void,Ticket)>,
    stop:Option<unsafe extern "C" fn(*mut c_void,u64)->u32>,
    destroy:Option<unsafe extern "C" fn(*mut c_void,u64)>,
}
unsafe impl Sync for Plugin {}
static CALL: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" fn release(_: *mut c_void, ptr:*mut u8, len:usize) {
    if len != 0 {
        // SAFETY: allocated by Vec in terminal; capacity equals length.
        drop(unsafe { Vec::from_raw_parts(ptr,len,len) });
    }
}
fn buffer(data: &[u8])->Buffer {
    let mut bytes=data.to_vec();
    let ptr=bytes.as_mut_ptr();
    let len=bytes.len();
    std::mem::forget(bytes);
    Buffer {ptr,len,owner:std::ptr::null_mut(),release:Some(release)}
}
fn pending(ticket:Ticket)->Result {
    Result{status:3,ticket,payload:Buffer{ptr:std::ptr::null_mut(),len:0,owner:std::ptr::null_mut(),release:None}}
}
unsafe extern "C" fn prepare(_: *mut c_void, host:*const Host, _:u64)->u32 {
    // SAFETY: loader pins the host table through stop/destroy.
    let host=unsafe{&*host};
    if host.header.major==1 {0} else {1}
}
unsafe extern "C" fn start(_: *mut c_void,_:u64)->u32 {0}
unsafe extern "C" fn begin(_: *mut c_void,request:Request)->Result {
    assert!(request.ticket.call_id!=0);
    let comp=unsafe{std::slice::from_raw_parts(request.component.ptr,request.component.len)};
    let iface=unsafe{std::slice::from_raw_parts(request.interface.ptr,request.interface.len)};
    assert_eq!(comp,b"fixture.native");
    assert_eq!(iface,b"fixture.test@1");
    CALL.store(request.ticket.call_id,Ordering::Release);
    pending(request.ticket)
}
unsafe extern "C" fn poll(_: *mut c_void,ticket:Ticket)->Result {
    let expected=CALL.swap(0,Ordering::AcqRel);
    if expected!=ticket.call_id { return Result {status:2,ticket,payload:buffer(b"missing call")}; }
    Result{status:1,ticket,payload:buffer(b"fixture finished")}
}
unsafe extern "C" fn cancel(_: *mut c_void, _:Ticket){}
unsafe extern "C" fn stop(_: *mut c_void,_:u64)->u32 {0}
unsafe extern "C" fn destroy(_: *mut c_void,_:u64){}
static TABLE:Plugin=Plugin {
 header:Header{major:1,minor:0,table_bytes:std::mem::size_of::<Plugin>(),required_features:1|2},
 context:std::ptr::null_mut(),
 prepare:Some(prepare),start:Some(start),begin:Some(begin),poll:Some(poll),
 cancel:Some(cancel),stop:Some(stop),destroy:Some(destroy),
};
#[unsafe(no_mangle)]
extern "C" fn phenix_plugin_entry_v1()->*const Plugin { &TABLE }
