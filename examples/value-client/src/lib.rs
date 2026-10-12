//! Independent typed caller, no private kernel source or provider object.
use phenix_sdk::{
    abi::*,
    live::{self, Portable},
    *,
};
use std::ffi::c_void;
#[used]
#[unsafe(link_section = ".phenix")]
static METADATA: [u8; NATIVE_SECTION_BYTES] =
    *include_bytes!(concat!(env!("OUT_DIR"), "/metadata.bin"));
struct Counter;
impl live::Operation for Counter {
    type Request = String;
    type Response = live::ValueRef<u64>;
    type Error = String;
    const KEY: &'static str = "test.refs@1/get";
}
struct Notes;
impl live::Operation for Notes {
    type Request = String;
    type Response = live::ValueRef<Vec<String>>;
    type Error = String;
    const KEY: &'static str = "test.refs@1/notes";
}
fn run(host: &live::Host<'_>, command: String) -> CallResult<Vec<u64>, String> {
    let counter = host.invoke::<Counter>("refs", &String::new())?;
    let notes = host.invoke::<Notes>("refs", &String::new())?;
    if command == "denied" {
        return Ok(vec![u64::from(matches!(
            host.compare_exchange(counter, Revision(0), &9),
            Err(live::AccessError::State(ValueAccessError::Denied))
        ))]);
    }
    let check = (|| {
        assert_eq!(host.slot_count("many")?, 2);
        let foreign = host
            .invoke::<Counter>("many#1", &String::new())
            .map_err(|e| Failure::new(Code::PluginFailed, format!("{e:?}")))?;
        assert!(matches!(
            foreign.read(host),
            Err(live::AccessError::State(ValueAccessError::Denied))
        ));
        let first = notes.read(host)?;
        let clone = first.clone();
        let (mut watch, bootstrap) = notes.watch(host)?;
        assert_eq!(bootstrap.revision, first.revision);
        let mut draft = notes.edit(host)?;
        draft.push("one".into());
        let committed = draft.commit(host)?;
        assert!(clone.value.is_empty());
        assert_eq!(committed.value.as_ref(), &vec![String::from("one")]);
        assert!(matches!(
            draft.commit(host),
            Err(live::AccessError::State(ValueAccessError::Conflict {
                expected: Revision(0),
                current: Revision(1)
            }))
        ));
        let before = notes.read(host)?;
        {
            let mut abandoned = notes.edit(host)?;
            abandoned.push("discarded".into());
        }
        assert_eq!(notes.read(host)?.revision, before.revision);
        let mut a = counter.edit(host)?;
        *a = 42;
        let mut b = notes.edit(host)?;
        b.push("two".into());
        let batch = host.batch(vec![b.batch_edit(), a.batch_edit()])?;
        assert_eq!(*batch.get(counter)?.value, 42);
        assert_eq!(
            batch.get(notes)?.value.as_ref(),
            &vec![String::from("one"), String::from("two")]
        );
        let counter_snapshot = counter.read(host)?;
        let notes_snapshot = notes.read(host)?;
        assert_eq!(counter_snapshot.epoch, batch.epoch);
        assert_eq!(notes_snapshot.epoch, batch.epoch);
        assert_eq!(*counter_snapshot.value, 42);
        assert_eq!(watch.next(host)?.unwrap().revision, notes_snapshot.revision);
        assert!(watch.next(host)?.is_none());
        watch.close(host)?;
        watch.close(host)?;
        assert!(watch.next(host).is_err());
        let snapshot = host.snapshot(&[counter])?;
        assert_eq!(snapshot.epoch, batch.epoch);
        assert_eq!(snapshot.snapshots[0].1.revision, counter_snapshot.revision);
        assert!(live::ValueRef::<String>::from_location(counter.location()).is_err());
        Ok::<_, live::AccessError>(vec![
            counter_snapshot.revision.0,
            notes_snapshot.revision.0,
            batch.epoch.0,
            u64::from(clone.value.is_empty()),
        ])
    })();
    check.map_err(|e| CallError::Kernel(Failure::new(Code::PluginFailed, e.to_string())))
}
unsafe extern "C" fn create(out: *mut *mut c_void) -> Reply {
    unsafe { *out = Box::into_raw(Box::new(())).cast() };
    Reply::value(0, Value::Null)
}
unsafe extern "C" fn initialize(_: *mut c_void, _: *const StateHostApi) -> Reply {
    Reply::value(0, Value::Null)
}
unsafe extern "C" fn invoke(
    _: *mut c_void,
    api: *const StateHostApi,
    key: Bytes,
    input: Bytes,
) -> Reply {
    let result = (|| {
        let api = unsafe { StateHostApi::negotiate(api.cast()) }?;
        let host = unsafe { live::Host::from_api(&api) };
        let key = std::str::from_utf8(unsafe { key.read() }?)
            .map_err(|_| Failure::new(Code::SchemaRejected, "key"))?;
        let value = Value::decode(unsafe { input.read() }?)?;
        match key {
            "test.state@1/run" => Ok(live::reply(run(&host, String::from_value(value)?))),
            "test.state@1/read" => {
                let reference = live::ValueRef::<u64>::from_value(value)?;
                match reference.read(&host) {
                    Ok(v) => Ok(Reply::value(0, v.value.to_value())),
                    Err(e) => Err(Failure::new(Code::PluginFailed, e.to_string())),
                }
            }
            _ => Err(Failure::new(Code::MissingBinding, "typed client operation")),
        }
    })();
    result.unwrap_or_else(|e: Failure| Reply::value(2, e.to_value()))
}
unsafe extern "C" fn poll(_: *mut c_void, _: *const StateHostApi, _: Bytes) -> Reply {
    Reply::value(
        2,
        Failure::new(Code::MissingBinding, "synchronous typed caller").to_value(),
    )
}
unsafe extern "C" fn cancel(_: *mut c_void, _: *const StateHostApi, _: Bytes) -> Reply {
    Reply::value(4, Value::Null)
}
unsafe extern "C" fn stop(ptr: *mut c_void) -> Reply {
    drop(unsafe { Box::from_raw(ptr.cast::<()>()) });
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
