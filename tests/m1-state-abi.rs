//! Rust host linked to an independently compiled C producer; no kernel runtime.
use phenix_contract::{abi::*, *};
use std::{
    ffi::c_void,
    sync::atomic::{AtomicUsize, Ordering},
};
unsafe extern "C" {
    fn phenix_native_v3() -> *const Header;
    fn m1_fixture_releases() -> usize;
    fn m1_fixture_bad_reply(kind: u32) -> Reply;
}
static CALLS: AtomicUsize = AtomicUsize::new(0);
static RELEASES: AtomicUsize = AtomicUsize::new(0);
unsafe extern "C" fn released(ptr: *mut u8, len: usize) {
    RELEASES.fetch_add(1, Ordering::SeqCst);
    drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)) });
}
fn owned(value: Value) -> Reply {
    let mut r = Reply::value(0, value);
    r.release = Some(released);
    r
}
fn location() -> ValueLocation {
    ValueLocation {
        generation: GenerationDigest::from_bytes(&[1; 32]).unwrap(),
        owner: OwnerId::from_bytes(&[2; 32]).unwrap(),
        cell: ValueId::from_bytes(&[3; 16]).unwrap(),
        schema: SchemaId::from_bytes(&[4; 32]).unwrap(),
    }
}
unsafe extern "C" fn state(scope: u64, opcode: u32, bytes: Bytes) -> Reply {
    assert_eq!(scope, 7);
    let request = StateRequest::decode(opcode, unsafe { bytes.read() }.unwrap()).unwrap();
    let index = CALLS.fetch_add(1, Ordering::SeqCst);
    match (index, request) {
        (0, StateRequest::Create { property, initial }) => {
            assert_eq!(property, "counter");
            assert_eq!(initial, Value::U64(0));
            owned(Value::Ref(Box::new(location())))
        }
        (1, StateRequest::Read(r)) => {
            assert_eq!(r, location());
            owned(Value::List(vec![
                Value::U64(0),
                Value::U64(0),
                Value::U64(0),
            ]))
        }
        _ => panic!("initialization ordering/request mismatch"),
    }
}
unsafe extern "C" fn invoke(scope: u64, slot: Bytes, op: Bytes, request: Bytes) -> Reply {
    assert_eq!(scope, 7);
    assert_eq!(unsafe { slot.read() }.unwrap(), b"counter");
    assert_eq!(unsafe { op.read() }.unwrap(), b"read");
    assert_eq!(
        Value::decode(unsafe { request.read() }.unwrap()).unwrap(),
        Value::Null
    );
    assert_eq!(CALLS.fetch_add(1, Ordering::SeqCst), 2);
    owned(Value::Null)
}
unsafe extern "C" fn wake(ticket: u64, permit: u64) -> u32 {
    assert_eq!((ticket, permit), (11, 13));
    assert_eq!(CALLS.fetch_add(1, Ordering::SeqCst), 3);
    0
}
fn main() {
    let table = unsafe { StatePluginTable::negotiate(phenix_native_v3()) }.unwrap();
    let host = StateHostApi {
        header: Header::state(std::mem::size_of::<StateHostApi>()),
        scope: 7,
        ticket: 11,
        permit: 13,
        invoke: Some(invoke),
        state: Some(state),
        wake: Some(wake),
    };
    unsafe { StateHostApi::negotiate(&host.header) }.unwrap();
    let mut instance: *mut c_void = std::ptr::null_mut();
    assert_eq!(
        unsafe { table.create.unwrap()(&mut instance).consume() }.unwrap(),
        (0, Value::Null)
    );
    assert_eq!(CALLS.load(Ordering::SeqCst), 0);
    assert!(!instance.is_null());
    assert_eq!(
        unsafe { table.initialize.unwrap()(instance, &host).consume() }.unwrap(),
        (0, Value::Null)
    );
    let empty = Bytes::new(&[]);
    assert_eq!(
        unsafe { table.invoke.unwrap()(instance, &host, empty, empty).consume() }.unwrap(),
        (0, Value::Ref(Box::new(location())))
    );
    assert_eq!(
        unsafe { table.poll.unwrap()(instance, &host, empty).consume() }.unwrap(),
        (3, Value::Null)
    );
    assert_eq!(
        unsafe { table.cancel.unwrap()(instance, &host, empty).consume() }.unwrap(),
        (4, Value::Null)
    );
    assert_eq!(
        unsafe { table.stop.unwrap()(instance).consume() }.unwrap(),
        (0, Value::Null)
    );
    assert_eq!(CALLS.load(Ordering::SeqCst), 4);
    assert_eq!(RELEASES.load(Ordering::SeqCst), 3);
    assert_eq!(unsafe { m1_fixture_releases() }, 6);
    for kind in 0..4 {
        assert!(unsafe { m1_fixture_bad_reply(kind).consume() }.is_err());
    }
    assert_eq!(unsafe { m1_fixture_releases() }, 9);
    assert!(unsafe { m1_fixture_bad_reply(4).consume() }.is_err());
    assert_eq!(unsafe { m1_fixture_releases() }, 10);
    assert!(unsafe { Reply::value(0, Value::Null).consume() }.is_err());
    for status in [3, 4] {
        assert!(unsafe { Reply::value(status, Value::U64(1)).consume() }.is_err());
    }
    let mut rejected = 0;
    assert!(unsafe { StatePluginTable::negotiate(std::ptr::null()) }.is_err());
    rejected += 1;
    for header in [
        Header {
            major: 2,
            ..table.header
        },
        Header {
            minor: 1,
            ..table.header
        },
        Header {
            features: 1,
            ..table.header
        },
        Header {
            table_bytes: 16,
            ..table.header
        },
        Header {
            table_bytes: 65,
            ..table.header
        },
    ] {
        // Only the header exists here. Reading any later member would be invalid.
        assert!(unsafe { StatePluginTable::negotiate(&header) }.is_err());
        rejected += 1;
    }
    for n in 0..6 {
        let mut bad = table;
        match n {
            0 => bad.create = None,
            1 => bad.initialize = None,
            2 => bad.invoke = None,
            3 => bad.stop = None,
            4 => bad.poll = None,
            _ => bad.cancel = None,
        };
        assert!(unsafe { StatePluginTable::negotiate(&bad.header) }.is_err());
        rejected += 1;
    }
    assert!(unsafe { StateHostApi::negotiate(std::ptr::null()) }.is_err());
    rejected += 1;
    for n in 0..4 {
        let mut bad = host;
        match n {
            0 => bad.invoke = None,
            1 => bad.state = None,
            2 => bad.wake = None,
            _ => bad.scope = 0,
        };
        assert!(unsafe { StateHostApi::negotiate(&bad.header) }.is_err());
        rejected += 1;
    }
    println!("{{\"native_language\":\"C\",\"host_language\":\"Rust\",\"abi_major\":3,\"host_callbacks\":4,\"c_releases\":10,\"rust_releases\":3,\"rejected_tables\":{rejected},\"live_state_proven\":false}}");
}
