//! S3a external typed cells, public SDK only. S3b replaces this fixture glue.
use phenix_sdk::{
    abi::*,
    live::{self, Portable},
    *,
};
use std::ffi::c_void;
#[used]
#[unsafe(link_section = ".phenix")]
static METADATA: [u8; NATIVE_SECTION_BYTES] = *include_bytes!(concat!(env!("OUT_DIR"), "/metadata.bin"));
#[derive(Default)]
struct State {
    counter: live::Cell<u64>,
    notes: live::Cell<Vec<String>>,
}
unsafe extern "C" fn create(out: *mut *mut c_void) -> Reply {
    unsafe { *out = Box::into_raw(Box::new(State::default())).cast() };
    Reply::value(0, Value::Null)
}
unsafe extern "C" fn initialize(ptr: *mut c_void, api: *const StateHostApi) -> Reply {
    let result = (|| {
        let api = unsafe { StateHostApi::negotiate(api.cast()) }?;
        let host = unsafe { live::Host::from_api(&api) };
        let state = unsafe { &mut *ptr.cast::<State>() };
        state
            .counter
            .bind(&host, "counter")
            .map_err(|e| Failure::new(Code::PluginFailed, e.to_string()))?;
        state
            .notes
            .bind(&host, "notes")
            .map_err(|e| Failure::new(Code::PluginFailed, e.to_string()))?;
        Ok::<_, Failure>(())
    })();
    match result {
        Ok(()) => Reply::value(0, Value::Null),
        Err(e) => Reply::value(2, e.to_value()),
    }
}
unsafe extern "C" fn invoke(
    ptr: *mut c_void,
    _: *const StateHostApi,
    key: Bytes,
    _: Bytes,
) -> Reply {
    let result = (|| {
        let state = unsafe { &*ptr.cast::<State>() };
        let key = std::str::from_utf8(unsafe { key.read() }?)
            .map_err(|_| Failure::new(Code::SchemaRejected, "key"))?;
        Ok::<_, Failure>(match key {
            "test.refs@1/get" => state.counter.reference()?.to_value(),
            "test.refs@1/notes" => state.notes.reference()?.to_value(),
            "test.provider@1/run" => Value::List(vec![]),
            _ => {
                return Err(Failure::new(
                    Code::MissingBinding,
                    "typed provider operation",
                ))
            }
        })
    })();
    match result {
        Ok(v) => Reply::value(0, v),
        Err(e) => Reply::value(2, e.to_value()),
    }
}
unsafe extern "C" fn poll(_: *mut c_void, _: *const StateHostApi, _: Bytes) -> Reply {
    Reply::value(
        2,
        Failure::new(Code::MissingBinding, "synchronous typed provider").to_value(),
    )
}
unsafe extern "C" fn cancel(_: *mut c_void, _: *const StateHostApi, _: Bytes) -> Reply {
    Reply::value(4, Value::Null)
}
unsafe extern "C" fn stop(ptr: *mut c_void) -> Reply {
    drop(unsafe { Box::from_raw(ptr.cast::<State>()) });
    Reply::value(0, Value::Null)
}
static TABLE: StatePluginTable = StatePluginTable {
    header: Header {
        major: 3,
        minor: 0,
        features: 3,
        table_bytes: std::mem::size_of::<StatePluginTable>() as u32,
    },
    create: Some(create),
    initialize: Some(initialize),
    invoke: Some(invoke),
    poll: Some(poll),
    cancel: Some(cancel),
    stop: Some(stop),
};
#[unsafe(no_mangle)]
pub extern "C" fn phenix_native_v3() -> *const Header {
    &TABLE.header
}
