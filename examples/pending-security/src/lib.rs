//! Adversarial ABI producer. Raw commands bypass SDK handle protections.
use example_contracts::{PendingConsumerContract, PendingRunContract, TransformContract};
use phenix_sdk::{abi::*, *};
use std::ffi::c_void;

const CHILD: &str = "example.pending@1/run";
const SERVICE: &str = "example.transform.apply@1/apply";
const POLL: &str = "phenix.host.pending.poll.v2";
const CANCEL: &str = "phenix.host.pending.cancel.v2";
const BEGIN: &str = "phenix.host.pending.begin.v2";
const METADATA: MetadataBytes = descriptor::static_metadata::metadata(
    "example.pending-security",
    "1.0.0",
    MetadataExecution::StatePending,
    [
        PendingConsumerContract::CONTRACT,
        PendingRunContract::CONTRACT,
        TransformContract::CONTRACT,
    ],
    [
        import("Worker", PendingRunContract::CONTRACT),
        StaticImport {
            cardinality: Cardinality::Many { minimum: 1 },
            ..import("Sibling", PendingRunContract::CONTRACT)
        },
        import("Service", TransformContract::CONTRACT),
    ],
    [StaticExport {
        slot: "parent",
        contract: "example.pending-consumer@1",
        revision: 1,
    }],
    [],
);
const fn import(slot: &'static str, contract: StaticContract) -> StaticImport {
    StaticImport {
        slot,
        contract: contract.contract,
        provider: None,
        revision_min: 1,
        revision_max: 1,
        cardinality: Cardinality::One,
    }
}
#[used]
#[no_mangle]
#[link_section = ".phenix"]
static PHENIX_DESCRIPTOR: [u8; NATIVE_SECTION_BYTES] = METADATA.native_section();

#[derive(Default)]
struct State {
    child: u64,
    resource: Option<String>,
}

fn command(api: &StateHostApi, slot: &str, key: &str, value: Value) -> (u32, Value) {
    let bytes = value.encode().unwrap();
    unsafe {
        api.invoke.unwrap()(
            api.scope,
            Bytes::new(slot.as_bytes()),
            Bytes::new(key.as_bytes()),
            Bytes::new(&bytes),
        )
        .consume()
        .unwrap()
    }
}
fn handle(operation: &str, ticket: u64) -> Value {
    Value::map([
        ("operation", Value::String(operation.into())),
        ("ticket", Value::U64(ticket)),
    ])
}
fn reject(reply: (u32, Value), status: u32, code: Code) {
    assert_eq!(reply.0, status);
    assert_eq!(Failure::from_value(reply.1).unwrap().code, code);
}
unsafe extern "C" fn create(out: *mut *mut c_void) -> Reply {
    boundary(|| {
        unsafe { *out = Box::into_raw(Box::new(State::default())).cast() };
        Reply::value(0, Value::Null)
    })
}
unsafe extern "C" fn initialize(_: *mut c_void, _: *const StateHostApi) -> Reply {
    Reply::value(0, Value::Null)
}
unsafe extern "C" fn invoke(
    ptr: *mut c_void,
    api: *const StateHostApi,
    _: Bytes,
    input: Bytes,
) -> Reply {
    boundary(|| {
        let api = unsafe { &*api };
        let state = unsafe { &mut *ptr.cast::<State>() };
        let Value::Map(input) = Value::decode(unsafe { input.read() }.unwrap()).unwrap() else {
            panic!("expected request")
        };
        state.resource = if input["text"] == Value::String("red".into()) {
            Some("red".into())
        } else {
            None
        };
        assert_eq!(
            command(api, "", RESOURCE_CONTEXT, Value::Null),
            (0, state.resource.clone().map_or(Value::Null, Value::String))
        );
        reject(
            command(api, "Worker", RESOURCE_CONTEXT, Value::Null),
            2,
            Code::SchemaRejected,
        );
        let begin = Value::map([
            ("operation", Value::String(CHILD.into())),
            (
                "request",
                Value::Bytes(
                    Value::map([("text", Value::String("hello".into()))])
                        .encode()
                        .unwrap(),
                ),
            ),
        ]);
        reject(
            command(api, "missing", BEGIN, begin.clone()),
            5,
            Code::MissingBinding,
        );
        reject(
            command(api, "Sibling#1", BEGIN, begin.clone()),
            5,
            Code::MissingBinding,
        );
        reject(
            command(api, "Worker", BEGIN, Value::Null),
            5,
            Code::SchemaRejected,
        );
        let mut forged_operation = begin.clone();
        if let Value::Map(fields) = &mut forged_operation {
            fields.insert("operation".into(), Value::String(SERVICE.into()));
        }
        reject(
            command(api, "Worker", BEGIN, forged_operation),
            5,
            Code::MissingBinding,
        );
        let mut malformed_request = begin.clone();
        if let Value::Map(fields) = &mut malformed_request {
            fields.insert("request".into(), Value::Bytes(vec![255]));
        }
        reject(
            command(api, "Worker", BEGIN, malformed_request),
            5,
            Code::SchemaRejected,
        );
        let switched = ScopedRequest {
            authority: Authority {
                operations: [CHILD.into(), SERVICE.into()].into(),
                resources: ["red".into(), "blue".into()].into(),
            },
            resource: Some("blue".into()),
        };
        let scoped = switched
            .encode(
                CHILD,
                Value::map([("text", Value::String("hello".into()))])
                    .encode()
                    .unwrap(),
            )
            .unwrap();
        reject(
            command(
                api,
                "Worker",
                "phenix.host.pending.scoped.v2",
                Value::decode(&scoped).unwrap(),
            ),
            5,
            Code::Denied,
        );
        let (status, Value::U64(child)) = command(api, "Worker", BEGIN, begin) else {
            panic!("child did not begin")
        };
        assert_eq!(status, 3);
        state.child = child;
        for command_key in [POLL, CANCEL] {
            for (slot, operation, ticket) in [
                ("missing", CHILD, child),
                ("Sibling#0", CHILD, child),
                ("Worker", SERVICE, child),
                ("Service", SERVICE, child),
                ("Worker", CHILD, api.ticket),
                ("Worker", CHILD, u64::MAX),
            ] {
                reject(
                    command(api, slot, command_key, handle(operation, ticket)),
                    5,
                    Code::ScopeRejected,
                );
            }
            reject(
                command(api, "Worker", command_key, Value::Null),
                5,
                Code::SchemaRejected,
            );
        }
        // Authentication precedes all borrowed-buffer reads, including on workers.
        let callback = api.invoke.unwrap();
        let invalid = Bytes {
            ptr: std::ptr::null(),
            len: 1,
        };
        reject(
            unsafe { callback(0, invalid, invalid, invalid).consume().unwrap() },
            2,
            Code::ScopeRejected,
        );
        let scope = api.scope;
        std::thread::spawn(move || {
            let invalid = Bytes {
                ptr: std::ptr::null(),
                len: 1,
            };
            reject(
                unsafe {
                    callback(scope, invalid, invalid, invalid)
                        .consume()
                        .unwrap()
                },
                2,
                Code::ScopeRejected,
            );
        })
        .join()
        .unwrap();
        Reply::value(3, Value::Null)
    })
}
unsafe extern "C" fn poll(ptr: *mut c_void, api: *const StateHostApi, _: Bytes) -> Reply {
    boundary(|| {
        let api = unsafe { &*api };
        let child = unsafe { (*ptr.cast::<State>()).child };
        let resource = unsafe { (*ptr.cast::<State>()).resource.clone() };
        assert_eq!(
            command(api, "", RESOURCE_CONTEXT, Value::Null),
            (0, resource.map_or(Value::Null, Value::String))
        );
        let (status, value) = command(api, "Worker", POLL, handle(CHILD, child));
        // Rejected forgeries left the real child alive and its result undelivered.
        assert_eq!(status, 0);
        assert_eq!(value, Value::map([("text", Value::String("HELLO".into()))]));
        reject(
            command(api, "Worker", POLL, handle(CHILD, child)),
            5,
            Code::ScopeRejected,
        );
        Reply::value(0, value)
    })
}
unsafe extern "C" fn cancel(_: *mut c_void, _: *const StateHostApi, _: Bytes) -> Reply {
    Reply::value(4, Value::Null)
}
unsafe extern "C" fn stop(ptr: *mut c_void) -> Reply {
    unsafe { drop(Box::from_raw(ptr.cast::<State>())) };
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
#[no_mangle]
pub extern "C" fn phenix_native_v3() -> *const Header {
    &TABLE.header
}
