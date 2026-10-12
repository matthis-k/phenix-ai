//! A non-agent native ABI 3 fixture. Depends on the public SDK only.
use phenix_sdk::{abi::*, *};
use std::ffi::c_void;
#[used]
#[unsafe(link_section = ".phenix")]
static METADATA: [u8; NATIVE_SECTION_BYTES] = *include_bytes!(concat!(env!("OUT_DIR"), "/metadata.bin"));
#[derive(Default)]
struct State {
    counter: Option<ValueLocation>,
    notes: Option<ValueLocation>,
    pending: bool,
    previous_scope: u64,
}
fn failure(message: &str) -> Reply {
    Reply::value(2, Failure::new(Code::PluginFailed, message).to_value())
}
unsafe fn host(api: *const StateHostApi) -> Result<StateHostApi, Failure> {
    unsafe { StateHostApi::negotiate(api.cast()) }
}
fn create_cell(
    api: &StateHostApi,
    property: &str,
    initial: Value,
) -> Result<ValueLocation, Failure> {
    let r = StateRequest::Create {
        property: property.into(),
        initial,
    };
    let bytes = r.encode()?;
    let (status, value) =
        unsafe { (api.state.unwrap())(api.scope, r.opcode(), Bytes::new(&bytes)).consume() }?;
    match (status, value) {
        (0, Value::Ref(r)) => Ok(*r),
        (_, v) => Err(Failure::from_value(v)?),
    }
}
unsafe extern "C" fn create(out: *mut *mut c_void) -> Reply {
    unsafe { *out = Box::into_raw(Box::new(State::default())).cast() };
    Reply::value(0, Value::Null)
}
unsafe extern "C" fn initialize(ptr: *mut c_void, api: *const StateHostApi) -> Reply {
    let result = (|| {
        let api = unsafe { host(api) }?;
        let state = unsafe { &mut *ptr.cast::<State>() };
        if cfg!(feature = "bad-init") {
            return Ok(());
        }
        let initial = if cfg!(feature = "bad-schema") {
            Value::String("wrong".into())
        } else {
            Value::U64(0)
        };
        state.counter = Some(create_cell(&api, "counter", initial)?);
        state.notes = Some(create_cell(&api, "notes", Value::List(vec![]))?);
        Ok::<_, Failure>(())
    })();
    match result {
        Ok(()) => Reply::value(0, Value::Null),
        Err(e) => Reply::value(2, e.to_value()),
    }
}
unsafe extern "C" fn invoke(
    ptr: *mut c_void,
    api: *const StateHostApi,
    key: Bytes,
    input: Bytes,
) -> Reply {
    let result = (|| {
        let api = unsafe { host(api) }?;
        let state = unsafe { &mut *ptr.cast::<State>() };
        let key = std::str::from_utf8(unsafe { key.read() }?)
            .map_err(|_| Failure::new(Code::SchemaRejected, "key"))?;
        let request = Value::decode(unsafe { input.read() }?)?;
        match key {
            "test.refs@1/get" => Ok(Reply::value(
                0,
                Value::Ref(Box::new(state.counter.unwrap())),
            )),
            "test.refs@1/notes" => Ok(Reply::value(
                0,
                Value::Ref(Box::new(state.notes.unwrap())),
            )),
            "test.provider@1/run" if request == Value::String("pending".into()) => {
                state.pending = true;
                Ok(Reply::value(3, Value::Null))
            }
            "test.provider@1/run" => {
                let scope = if request == Value::String("expired".into()) {
                    state.previous_scope
                } else {
                    api.scope
                };
                state.previous_scope = api.scope;
                let callback = api.state.unwrap();
                let denied = if request == Value::String("expired".into()) {
                    let invalid = Bytes {
                        ptr: std::ptr::null(),
                        len: 8,
                    };
                    let (status, value) = unsafe { callback(scope, 2, invalid).consume() }?;
                    status == 2 && Failure::from_value(value)?.code == Code::ScopeRejected
                } else {
                    std::thread::spawn(move || {
                        let invalid = Bytes {
                            ptr: std::ptr::null(),
                            len: 8,
                        };
                        let (status, value) =
                            unsafe { callback(scope, 2, invalid).consume() }.unwrap();
                        status == 2
                            && Failure::from_value(value).unwrap().code == Code::ScopeRejected
                    })
                    .join()
                    .unwrap()
                };
                Ok(Reply::value(
                    0,
                    Value::List(vec![Value::U64(u64::from(denied))]),
                ))
            }
            _ => Err(Failure::new(Code::MissingBinding, "fixture operation")),
        }
    })();
    result.unwrap_or_else(|e| Reply::value(2, e.to_value()))
}
unsafe extern "C" fn poll(ptr: *mut c_void, _: *const StateHostApi, _: Bytes) -> Reply {
    let state = unsafe { &mut *ptr.cast::<State>() };
    if !state.pending {
        return failure("no pending fixture");
    }
    state.pending = false;
    Reply::value(0, Value::List(vec![Value::U64(7)]))
}
unsafe extern "C" fn cancel(ptr: *mut c_void, _: *const StateHostApi, _: Bytes) -> Reply {
    unsafe { (*ptr.cast::<State>()).pending = false };
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
        table_bytes: if cfg!(feature = "bad-table") {
            16
        } else {
            std::mem::size_of::<StatePluginTable>() as u32
        },
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
