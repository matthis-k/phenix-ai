//! Independently compiled consumer. No private kernel/artifact dependency.
use phenix_sdk::{abi::*, *};
use std::ffi::c_void;
#[used]
#[unsafe(link_section = ".phenix")]
static METADATA: [u8; SECTION_BYTES] = *include_bytes!(concat!(env!("OUT_DIR"), "/metadata.bin"));
#[derive(Default)]
struct State {
    leaked: u64,
}
fn state(api: &StateHostApi, r: StateRequest) -> Result<Value, String> {
    let bytes = r.encode().map_err(|e| e.to_string())?;
    let (status, value) =
        unsafe { (api.state.unwrap())(api.scope, r.opcode(), Bytes::new(&bytes)).consume_state() }
            .map_err(|e| e.to_string())?;
    match status {
        0 => Ok(value),
        1 => {
            let e = ValueAccessError::from_value(value).map_err(|e| e.to_string())?;
            Err(match e {
                ValueAccessError::Conflict { .. } => "Conflict",
                ValueAccessError::Denied => "Denied",
                ValueAccessError::OwnerGone => "OwnerGone",
                ValueAccessError::SchemaMismatch => "SchemaMismatch",
                ValueAccessError::StaleReference => "StaleReference",
                _ => "Exhausted",
            }
            .into())
        }
        _ => Err(Failure::from_value(value)
            .map_err(|e| e.to_string())?
            .to_string()),
    }
}
fn imported(api: &StateHostApi, slot: &str, key: &str, request: Value) -> Result<Value, String> {
    let bytes = request
        .encode_profile(WireProfile::V2)
        .map_err(|e| e.to_string())?;
    let (status, value) = unsafe {
        (api.invoke.unwrap())(
            api.scope,
            Bytes::new(slot.as_bytes()),
            Bytes::new(key.as_bytes()),
            Bytes::new(&bytes),
        )
        .consume_state()
    }
    .map_err(|e| e.to_string())?;
    if status == 0 {
        Ok(value)
    } else {
        Err(format!("import status {status}: {value:?}"))
    }
}
fn reference(api: &StateHostApi, slot: &str, key: &str) -> Result<ValueLocation, String> {
    let Value::Ref(r) = imported(api, slot, key, Value::String(String::new()))? else {
        return Err("reference".into());
    };
    Ok(*r)
}
fn numbers(value: Value) -> Vec<u64> {
    let Value::List(v) = value else {
        panic!("snapshot");
    };
    v.into_iter()
        .filter_map(|v| if let Value::U64(n) = v { Some(n) } else { None })
        .collect()
}
fn update(
    api: &StateHostApi,
    r: ValueLocation,
    revision: u64,
    value: Value,
) -> Result<Value, String> {
    state(
        api,
        StateRequest::CompareExchange(CompareExchange {
            location: r,
            expected: Revision(revision),
            value,
        }),
    )
}
fn run(s: &mut State, api: &StateHostApi, command: &str) -> Result<Vec<u64>, String> {
    if command == "leaked" {
        return state(api, StateRequest::Next(s.leaked)).map(numbers);
    }
    let r = reference(api, "refs", "test.refs@1/get")?;
    let before = state(api, StateRequest::Read(r))?;
    let snapshot = numbers(before.clone());
    let revision = snapshot[0];
    let value = snapshot[2];
    match command {
        "basics" => {
            let clone = r;
            let after = numbers(update(api, clone, revision, Value::U64(value + 1))?);
            let conflict = update(api, r, revision, Value::U64(999)).unwrap_err();
            Ok(vec![
                u64::from(clone == r),
                snapshot[0],
                snapshot[2],
                after[0],
                after[2],
                u64::from(conflict == "Conflict"),
                numbers(before)[2],
            ])
        }
        "watch" | "leak" | "bootstrap-gap" => {
            let (revision, value) = if command == "bootstrap-gap" {
                update(api, r, revision, Value::U64(value + 1))?;
                (revision + 1, value + 1)
            } else {
                (revision, value)
            };
            let Value::List(v) = state(api, StateRequest::Subscribe(r))? else {
                panic!("subscribe");
            };
            let Value::U64(cursor) = v[0] else {
                panic!("cursor");
            };
            s.leaked = cursor;
            if command == "leak" {
                return Ok(vec![cursor]);
            }
            let boot = numbers(v[1].clone());
            update(api, r, revision, Value::U64(value + 1))?;
            update(api, r, revision + 1, Value::U64(value + 2))?;
            let latest = numbers(state(api, StateRequest::Next(cursor))?);
            let unchanged = state(api, StateRequest::Next(cursor))? == Value::Null;
            state(api, StateRequest::Unsubscribe(cursor))?;
            let stale = state(api, StateRequest::Next(cursor)).unwrap_err() == "StaleReference";
            Ok(vec![
                boot[0],
                boot[2],
                latest[0],
                latest[2],
                u64::from(unchanged),
                u64::from(stale),
            ])
        }
        "batch" | "bad-batch" => {
            let notes = reference(api, "refs", "test.refs@1/notes")?;
            let nr = numbers(state(api, StateRequest::Read(notes))?)[0];
            let mut edits = vec![
                CompareExchange {
                    location: r,
                    expected: Revision(revision),
                    value: Value::U64(value + 1),
                },
                CompareExchange {
                    location: notes,
                    expected: Revision(nr),
                    value: Value::List(vec![Value::String("saved".into())]),
                },
            ];
            edits.sort_by_key(|e| e.location);
            if command == "bad-batch" {
                edits.last_mut().unwrap().expected.0 += 1;
            }
            let result = state(api, StateRequest::Batch(edits));
            if command == "bad-batch" {
                return result.map(numbers);
            }
            let Value::List(batch) = result? else {
                panic!("batch");
            };
            let Value::U64(epoch) = batch[0] else {
                panic!("epoch");
            };
            let Value::List(snapshots) = &batch[1] else {
                panic!("snapshots");
            };
            Ok(vec![
                epoch,
                numbers(snapshots[0].clone())[1],
                numbers(snapshots[1].clone())[1],
                numbers(state(api, StateRequest::Read(r))?)[2],
            ])
        }
        "snapshot" | "snapshot-order" => {
            let notes = reference(api, "refs", "test.refs@1/notes")?;
            let mut locations = vec![r, notes];
            locations.sort();
            if command == "snapshot-order" {
                locations.push(locations[0]);
            }
            let result = imported(
                api,
                "",
                "phenix.host.state@1/snapshot",
                Value::List(
                    locations
                        .into_iter()
                        .map(|r| Value::Ref(Box::new(r)))
                        .collect(),
                ),
            )?;
            let Value::List(batch) = result else {
                panic!("snapshot batch");
            };
            let Value::U64(epoch) = batch[0] else {
                panic!("snapshot epoch");
            };
            let Value::List(snapshots) = &batch[1] else {
                panic!("snapshot items");
            };
            Ok(vec![
                epoch,
                numbers(snapshots[0].clone())[1],
                numbers(snapshots[1].clone())[1],
            ])
        }
        "denied" => update(api, r, revision, Value::U64(1000)).map(numbers),
        "schema" => update(api, r, revision, Value::String("invalid".into())).map(numbers),
        "many" => {
            let Value::U64(count) =
                imported(api, "many", "phenix.host.slots@1/count", Value::Null)?
            else {
                panic!("count");
            };
            let other = reference(api, "many#0", "test.refs@1/get")?;
            Ok(vec![count, u64::from(other == r)])
        }
        "many-ref-denied" => {
            let other = reference(api, "many#1", "test.refs@1/get")?;
            state(api, StateRequest::Read(other)).map(numbers)
        }
        "owner" | "cell" | "generation" | "ref-schema" => {
            let mut forged = r;
            match command {
                "owner" => forged.owner = OwnerId::from_bytes(&[9; 32]).unwrap(),
                "cell" => forged.cell = ValueId::from_bytes(&[9; 16]).unwrap(),
                "generation" => forged.generation = GenerationDigest::from_bytes(&[9; 32]).unwrap(),
                _ => forged.schema = ValueSchema::String.id().unwrap(),
            };
            state(api, StateRequest::Read(forged)).map(numbers)
        }
        _ => Ok(snapshot),
    }
}
unsafe extern "C" fn create(out: *mut *mut c_void) -> Reply {
    unsafe { *out = Box::into_raw(Box::new(State::default())).cast() };
    Reply::state_value(0, Value::Null)
}
unsafe extern "C" fn initialize(_: *mut c_void, _: *const StateHostApi) -> Reply {
    Reply::state_value(0, Value::Null)
}
unsafe extern "C" fn invoke(
    ptr: *mut c_void,
    api: *const StateHostApi,
    key: Bytes,
    input: Bytes,
) -> Reply {
    let result = (|| {
        let api = unsafe { StateHostApi::negotiate(api.cast()) }.map_err(|e| e.to_string())?;
        let key = std::str::from_utf8(unsafe { key.read() }.map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let input = Value::decode_profile(
            unsafe { input.read() }.map_err(|e| e.to_string())?,
            WireProfile::V2,
        )
        .map_err(|e| e.to_string())?;
        if key == "test.state@1/read" {
            let Value::Ref(r) = input else {
                return Err("request".into());
            };
            return Ok(Value::U64(numbers(state(&api, StateRequest::Read(*r))?)[2]));
        }
        let Value::String(command) = input else {
            return Err("request".into());
        };
        Ok(Value::List(
            run(unsafe { &mut *ptr.cast::<State>() }, &api, &command)?
                .into_iter()
                .map(Value::U64)
                .collect(),
        ))
    })();
    match result {
        Ok(v) => Reply::state_value(0, v),
        Err(e) => Reply::state_value(1, Value::String(e)),
    }
}
unsafe extern "C" fn poll(_: *mut c_void, _: *const StateHostApi, _: Bytes) -> Reply {
    Reply::state_value(0, Value::List(vec![]))
}
unsafe extern "C" fn cancel(_: *mut c_void, _: *const StateHostApi, _: Bytes) -> Reply {
    Reply::state_value(4, Value::Null)
}
unsafe extern "C" fn stop(ptr: *mut c_void) -> Reply {
    drop(unsafe { Box::from_raw(ptr.cast::<State>()) });
    Reply::state_value(0, Value::Null)
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
