use phenix_sdk::{
    abi::{Bytes, Header, Reply, StateHostApi, StatePluginTable},
    live::{self, Operation},
    *,
};
use std::sync::atomic::{AtomicUsize, Ordering};
static CREATES: AtomicUsize = AtomicUsize::new(0);
static STOPS: AtomicUsize = AtomicUsize::new(0);

#[contract(id = "test.generated@1")]
pub trait Service {
    fn reference(&self) -> Result<live::ValueRef<u64>, String>;
    fn synchronous(&self, input: String) -> Result<String, String>;
    fn run(&mut self, input: String) -> Result<String, String>;
}
#[contract(id = "test.dependency@1")]
pub trait Dependency {
    fn echo(&self, input: String) -> Result<String, String>;
}
#[property(owner = "test.generated", id = "stable_count", get, set, watch)]
pub type Counter = u64;

#[plugin(
    id = "test.generated",
    version = "1.0.0",
    profile = "state",
    define(Service),
    define(Dependency),
    provide(Service, slot = "service"),
    require(Dependency, slot = "primary"),
    require(Dependency, slot = "fallback", optional),
    require(Dependency, slot = "all", many(min = 0))
)]
mod generated {
    use super::*;
    #[derive(Default)]
    struct State {
        #[get(id = "stable_count")]
        #[set]
        #[watch]
        counter: u64,
        pending: bool,
        plain: usize,
    }
    #[start]
    fn start(state: &mut State) -> Result<(), Failure> {
        state.counter.reference()?;
        state.plain = 17;
        Ok(())
    }
    #[stop]
    fn stop(state: &mut State) -> Result<(), Failure> {
        assert_eq!(state.plain, 17);
        STOPS.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    #[provide(Service::reference)]
    fn reference(
        state: &mut State,
        _: &live::Host<'_>,
        _: (),
    ) -> CallResult<live::ValueRef<u64>, String> {
        Ok(state.counter.reference()?)
    }
    #[pending(Service::run)]
    fn run(
        state: &mut State,
        host: &live::Host<'_>,
        input: String,
    ) -> live::PendingResult<String, String> {
        match input.as_str() {
            "pending" => {
                state.pending = true;
                live::PendingResult::Pending
            }
            "panic" => {
                state.pending = true;
                panic!("fixture panic")
            }
            "failure" => {
                state.pending = true;
                live::PendingResult::Ready(Err(CallError::Kernel(Failure::new(
                    Code::PluginFailed,
                    "worker still running",
                ))))
            }
            "domain" => live::PendingResult::Ready(Err(CallError::Domain("recoverable".into()))),
            _ => live::PendingResult::Ready(imports::primary::invoke::<Dependency::echo>(
                host, &input,
            )),
        }
    }
    #[provide(Service::synchronous)]
    fn synchronous(input: String) -> CallResult<String, String> {
        match input.as_str() {
            "panic" => panic!("synchronous fixture panic"),
            _ => Err(CallError::Kernel(Failure::new(
                Code::PluginFailed,
                "returned failure",
            ))),
        }
    }
    #[poll(Service::run)]
    fn poll(state: &mut State, _: &live::Host<'_>) -> live::PendingResult<String, String> {
        assert!(state.pending);
        state.pending = false;
        live::PendingResult::Ready(Ok("drained".into()))
    }
    #[cancel(Service::run)]
    fn cancel(state: &mut State, _: &live::Host<'_>) -> Result<bool, Failure> {
        state.pending = false;
        Ok(true)
    }
}

unsafe extern "C" fn state(_: u64, opcode: u32, bytes: Bytes) -> Reply {
    let StateRequest::Create { property, initial } =
        StateRequest::decode(opcode, unsafe { bytes.read() }.unwrap()).unwrap()
    else {
        panic!("unexpected state call")
    };
    assert_eq!(property, "stable_count");
    assert_eq!(initial, Value::U64(0));
    CREATES.fetch_add(1, Ordering::SeqCst);
    Reply::value(
        0,
        Value::Ref(Box::new(ValueLocation {
            generation: GenerationDigest::from_bytes(&[1; 32]).unwrap(),
            owner: OwnerId::from_bytes(&[2; 32]).unwrap(),
            cell: ValueId::from_bytes(&[3; 16]).unwrap(),
            schema: ValueSchema::U64.id().unwrap(),
        })),
    )
}
unsafe extern "C" fn invoke(scope: u64, slot: Bytes, key: Bytes, input: Bytes) -> Reply {
    assert_eq!(scope, 9);
    assert_eq!(unsafe { slot.read() }.unwrap(), b"primary");
    assert_eq!(unsafe { key.read() }.unwrap(), b"test.dependency@1/echo");
    Reply::value(0, Value::decode(unsafe { input.read() }.unwrap()).unwrap())
}
unsafe extern "C" fn wake(_: u64, _: u64) -> u32 {
    0
}

#[test]
fn generated_native_lifecycle_pending_and_poisoning_use_v2() {
    let table = unsafe { StatePluginTable::negotiate(generated::phenix_native_v3()) }.unwrap();
    let host = StateHostApi {
        header: Header::state(std::mem::size_of::<StateHostApi>()),
        scope: 9,
        ticket: 1,
        permit: 1,
        invoke: Some(invoke),
        state: Some(state),
        wake: Some(wake),
    };
    let mut instance = std::ptr::null_mut();
    assert_eq!(
        unsafe { table.create.unwrap()(&mut instance).consume() }.unwrap(),
        (0, Value::Null)
    );
    assert!(!instance.is_null());
    assert_eq!(
        unsafe { table.initialize.unwrap()(instance, &host).consume() }.unwrap(),
        (0, Value::Null)
    );
    assert_eq!(CREATES.load(Ordering::SeqCst), 1);
    let synchronous_host = StateHostApi { ticket: 0, ..host };
    let input = Value::String("pending".into()).encode().unwrap();
    let (status, failure) = unsafe {
        table.invoke.unwrap()(
            instance,
            &synchronous_host,
            Bytes::new(Service::run::KEY.as_bytes()),
            Bytes::new(&input),
        )
        .consume()
    }
    .unwrap();
    assert_eq!(status, 5);
    assert_eq!(
        Failure::from_value(failure).unwrap().code,
        Code::AbiIncompatible
    );
    assert_eq!(
        unsafe { table.initialize.unwrap()(instance, &host).consume() }
            .unwrap()
            .0,
        2
    );
    assert_eq!(CREATES.load(Ordering::SeqCst), 1);
    let call = |text: &str| {
        let bytes = Value::String(text.into()).encode().unwrap();
        unsafe {
            table.invoke.unwrap()(
                instance,
                &host,
                Bytes::new(Service::run::KEY.as_bytes()),
                Bytes::new(&bytes),
            )
            .consume()
        }
        .unwrap()
    };
    assert_eq!(call("echo"), (0, Value::String("echo".into())));
    assert_eq!(call("domain"), (1, Value::String("recoverable".into())));
    assert_eq!(call("pending"), (3, Value::Null));
    assert_eq!(
        unsafe {
            table.cancel.unwrap()(instance, &host, Bytes::new(Service::run::KEY.as_bytes()))
                .consume()
        }
        .unwrap(),
        (4, Value::Null)
    );
    assert_eq!(call("pending"), (3, Value::Null));
    assert_eq!(
        unsafe {
            table.poll.unwrap()(instance, &host, Bytes::new(Service::run::KEY.as_bytes())).consume()
        }
        .unwrap(),
        (0, Value::String("drained".into()))
    );
    assert_eq!(call("failure").0, 2);
    assert_eq!(
        unsafe {
            table.cancel.unwrap()(instance, &host, Bytes::new(Service::run::KEY.as_bytes()))
                .consume()
        }
        .unwrap(),
        (4, Value::Null)
    );
    assert_eq!(call("panic").0, 2);
    assert_eq!(call("echo").0, 5);
    assert_eq!(
        unsafe {
            table.poll.unwrap()(instance, &host, Bytes::new(Service::run::KEY.as_bytes())).consume()
        }
        .unwrap(),
        (0, Value::String("drained".into()))
    );
    assert_eq!(call("echo").0, 5);
    assert_eq!(
        unsafe { table.stop.unwrap()(instance).consume() }.unwrap(),
        (0, Value::Null)
    );
    assert_eq!(STOPS.load(Ordering::SeqCst), 1);

    let mut instance = std::ptr::null_mut();
    unsafe { table.create.unwrap()(&mut instance).consume() }.unwrap();
    unsafe { table.initialize.unwrap()(instance, &host).consume() }.unwrap();
    // A normal synchronous failure does not poison state; a panic does.
    // The post-panic call must be rejected rather than re-entering mutated state.
    for (text, expected_message) in [
        ("failure", "returned failure"),
        ("panic", "caught state plugin panic"),
        ("failure", "instance uninitialized or poisoned"),
    ] {
        let bytes = Value::String(text.into()).encode().unwrap();
        let (status, value) = unsafe {
            table.invoke.unwrap()(
                instance,
                &host,
                Bytes::new(Service::synchronous::KEY.as_bytes()),
                Bytes::new(&bytes),
            )
            .consume()
        }
        .unwrap();
        assert_eq!(
            status, 5,
            "synchronous {text} must release its native lease"
        );
        let error = Failure::from_value(value).unwrap();
        assert_eq!(error.code, Code::PluginFailed);
        assert_eq!(error.message, expected_message);
    }
    assert_eq!(
        unsafe { table.stop.unwrap()(instance).consume() }.unwrap(),
        (0, Value::Null)
    );
    assert_eq!(STOPS.load(Ordering::SeqCst), 2);
}

#[test]
fn generated_properties_match_independent_synthetic_contracts() {
    let error = ValueSchema::Variants(
        [
            "Conflict",
            "Denied",
            "OwnerGone",
            "SchemaMismatch",
            "StaleReference",
        ]
        .map(String::from)
        .into(),
    );
    let reference = ValueSchema::Ref(ValueSchema::U64.id().unwrap());
    let expected = ContractDefinition {
        contract: "test.generated.property.stable_count@1".into(),
        revision: 1,
        operations: vec![
            OperationSchema {
                name: "get".into(),
                request: reference.clone(),
                response: ValueSchema::U64,
                error: error.clone(),
            },
            OperationSchema {
                name: "set".into(),
                request: ValueSchema::Struct(
                    [
                        ("expected".into(), ValueSchema::U64),
                        ("location".into(), reference.clone()),
                        ("value".into(), ValueSchema::U64),
                    ]
                    .into(),
                ),
                response: ValueSchema::U64,
                error: error.clone(),
            },
            OperationSchema {
                name: "watch".into(),
                request: reference,
                response: ValueSchema::U64,
                error,
            },
        ],
    };
    assert_eq!(Counter::CONTRACT.owned().unwrap(), expected);
    assert_eq!(
        generated::properties::counter::CONTRACT.owned().unwrap(),
        expected
    );
    let metadata = ContractMetadata::decode(generated::METADATA.bytes()).unwrap();
    assert_eq!(metadata.properties.len(), 1);
    assert_eq!(metadata.properties[0].id, "stable_count");
    assert_eq!(
        metadata
            .requires
            .iter()
            .map(|s| (s.slot.as_str(), s.cardinality))
            .collect::<Vec<_>>(),
        vec![
            ("all", Cardinality::Many { minimum: 0 }),
            ("fallback", Cardinality::Optional),
            ("primary", Cardinality::One)
        ]
    );
}
