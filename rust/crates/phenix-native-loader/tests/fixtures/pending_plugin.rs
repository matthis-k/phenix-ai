//! Self-contained compiled native .so fixture. Intentionally does not link
//! the Rust ABI crate: this catches accidental representation assumptions.
use std::{ffi::c_void, sync::{Mutex, atomic::{AtomicUsize, Ordering}}};

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
static CALLS: Mutex<Vec<(u64, bool, bool)>> = Mutex::new(Vec::new());
static IMPORT_RESULTS: Mutex<Vec<(u64, Vec<u8>)>> = Mutex::new(Vec::new());
static HOST_ID: AtomicUsize = AtomicUsize::new(0);

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
    let host_ref=unsafe{&*host};
    HOST_ID.store(host as usize, Ordering::Release);
    if host_ref.header.major==1 && host_ref.wake.is_some() && host_ref.cancelled.is_some() {
        0
    } else {
        1
    }
}
unsafe extern "C" fn start(_: *mut c_void,_:u64)->u32 {0}
unsafe extern "C" fn begin(_: *mut c_void,request:Request)->Result {
    assert!(request.ticket.call_id!=0);
    let comp=unsafe{std::slice::from_raw_parts(request.component.ptr,request.component.len)};
    let iface=unsafe{std::slice::from_raw_parts(request.interface.ptr,request.interface.len)};
    assert!(
        (comp == b"fixture.native" && iface == b"fixture.test@1")
            || (comp == b"fixture.workflow-tool-provider"
                && iface == b"fixture.workflow-tool@1")
    );
    let input=unsafe{std::slice::from_raw_parts(request.input.ptr,request.input.len)};
    let importing=comp==b"fixture.native" && input==b"import";
    CALLS.lock().unwrap().push((request.ticket.call_id, comp == b"fixture.native", importing));
    // This notification is deliberately sent *before* begin returns to
    // verify the host does not drop early/synchronous wakeups.
    let host=HOST_ID.load(Ordering::Acquire) as *const Host;
    let host=unsafe{&*host};
    assert_eq!(unsafe{host.cancelled.unwrap()(host.context,request.ticket)},0);
    unsafe{host.wake.unwrap()(host.context,request.ticket)};
    if importing {
        let ticket=request.ticket;
        std::thread::spawn(move || {
            let host=HOST_ID.load(Ordering::Acquire) as *const Host;
            let host=unsafe{&*host};
            let interface=b"fixture.import@1";
            let input=b"ping";
            let reply=unsafe {
                host.invoke.unwrap()(
                    host.context, ticket,
                    Slice{ptr:interface.as_ptr(),len:interface.len()},
                    Slice{ptr:input.as_ptr(),len:input.len()},
                )
            };
            let bytes=if reply.payload.len==0 {
                Vec::new()
            } else {
                unsafe{std::slice::from_raw_parts(reply.payload.ptr,reply.payload.len)}.to_vec()
            };
            // SAFETY: the host pairs the reply buffer with this release hook.
            unsafe{reply.payload.release.unwrap()(reply.payload.owner,reply.payload.ptr,reply.payload.len)};
            IMPORT_RESULTS.lock().unwrap().push((ticket.call_id,bytes));
            unsafe{host.wake.unwrap()(host.context,ticket)};
        });
    }
    pending(request.ticket)
}
unsafe extern "C" fn poll(_: *mut c_void,ticket:Ticket)->Result {
    let mut calls=CALLS.lock().unwrap();
    let Some(index)=calls.iter().position(|(call, _, _)| *call==ticket.call_id) else {
        return Result {status:2,ticket,payload:buffer(b"missing call")};
    };
    let (_,simple,importing)=calls[index];
    if importing {
        let mut results=IMPORT_RESULTS.lock().unwrap();
        let Some(index_result)=results.iter().position(|(id,_)| *id==ticket.call_id) else {
            return pending(ticket);
        };
        let (_,bytes)=results.remove(index_result);
        calls.remove(index);
        return Result{status:1,ticket,payload:buffer(&bytes)};
    }
    calls.remove(index);
    Result{status:1,ticket,payload:buffer(
        if simple {
            if cfg!(phenix_native_revision_b) {
                &b"fixture finished B"[..]
            } else {
                &b"fixture finished"[..]
            }
        } else {
            br#"{"type":"string","value":"done"}"#
        }
    )}
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
