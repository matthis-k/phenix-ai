//! Persistent observer offsets, including process exit around acknowledgement.
use phenix_basic_contracts::{
    BasicError, ErrorCode, Event, EventKind, EventPage, JournalAppend, JournalQuery,
    ObserverCheckpoint, ObserverRequest, RecordKey, StorageEdit, StorageTransaction, StoredRecord,
};
use phenix_contract::{Authority, Blob, Portable};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection, SlotBinding},
    Kernel, Outcome,
};
use std::{collections::BTreeSet, fs, path::Path};

fn selection(key: &str, provider: &str) -> Selection {
    let (interface, operation) = key.split_once('/').unwrap();
    Selection {
        interface: interface.into(),
        operation: operation.into(),
        provider: provider.into(),
    }
}
fn call<T: Portable>(kernel: &Kernel, key: &str, input: impl Portable) -> Result<T, BasicError> {
    let provider = if key.contains("storage@") {
        "phenix.basic.storage-sqlite"
    } else {
        "phenix.basic.event-journal"
    };
    let root = kernel.admit_root(selection(key, provider)).unwrap();
    match root.invoke(&input.to_value()).unwrap() {
        Outcome::Success(value) => Ok(T::from_value(value).unwrap()),
        Outcome::Domain(value) => Err(BasicError::from_value(value).unwrap()),
    }
}
fn observer(kernel: &Kernel, name: &str) -> ObserverCheckpoint {
    call(
        kernel,
        "phenix.basic.journal@1/observer",
        ObserverRequest {
            session: "s1".into(),
            observer: name.into(),
        },
    )
    .unwrap()
}
fn ack(
    kernel: &Kernel,
    current: &ObserverCheckpoint,
    after: u64,
) -> Result<ObserverCheckpoint, BasicError> {
    call(
        kernel,
        "phenix.basic.journal@1/checkpoint_observer",
        ObserverCheckpoint {
            after,
            ..current.clone()
        },
    )
}
fn replay(kernel: &Kernel, after: u64) -> EventPage {
    call(
        kernel,
        "phenix.basic.journal@1/read",
        JournalQuery {
            session: "s1".into(),
            after,
            limit: 10,
        },
    )
    .unwrap()
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 6, "MODE JOURNAL SQLITE DEFINITIONS OUTPUT");
    let output = Path::new(&args[5]);
    fs::create_dir_all(output).unwrap();
    let journal_ops = ["append", "read", "observer", "checkpoint_observer"];
    let storage_ops = ["read", "transact"];
    let resources = BTreeSet::from(["phenix.session.s1".to_string()]);
    let mut grants = vec![];
    let mut operations = BTreeSet::new();
    for (caller, interface, ops) in [
        ("root", "phenix.basic.journal@1", journal_ops.as_slice()),
        ("root", "phenix.basic.storage@1", storage_ops.as_slice()),
        (
            "phenix.basic.event-journal",
            "phenix.basic.storage@1",
            storage_ops.as_slice(),
        ),
    ] {
        for op in ops {
            operations.insert(format!("{interface}/{op}"));
            grants.push(ScopedGrant {
                caller: caller.into(),
                interface: interface.into(),
                operation: (*op).into(),
                resources: resources.clone(),
            });
        }
    }
    let kernel = Prepared::prepare(Composition {
        format: "phenix.composition.v2".into(),
        artifacts: args[2..4].to_vec(),
        definitions: vec![args[4].clone()],
        entry: selection("phenix.basic.journal@1/read", "phenix.basic.event-journal"),
        slots: vec![SlotBinding {
            caller: "phenix.basic.event-journal".into(),
            slot: "Storage".into(),
            index: 0,
            provider: "phenix.basic.storage-sqlite".into(),
            export: "Storage".into(),
        }],
        grants,
        excluded: BTreeSet::new(),
        substitutions: vec![],
        authority: Authority {
            operations,
            resources,
        },
        resource: Some("phenix.session.s1".into()),
    })
    .unwrap()
    .load()
    .unwrap();
    match args[1].as_str() {
        "seed" => {
            let count: u64 = call(
                &kernel,
                "phenix.basic.journal@1/append",
                JournalAppend {
                    session: "s1".into(),
                    events: ["one", "two", "three"]
                        .into_iter()
                        .map(|text| Event {
                            sequence: 0,
                            session: "s1".into(),
                            run: "r1".into(),
                            kind: EventKind::TextDelta,
                            call_id: String::new(),
                            text: text.into(),
                            data: Blob(vec![]),
                        })
                        .collect(),
                },
            )
            .unwrap();
            assert_eq!(count, 3);
            let fresh = observer(&kernel, "a");
            assert_eq!((fresh.after, fresh.revision), (0, 0));
            let a = ack(&kernel, &fresh, 1).unwrap();
            assert_eq!(
                ack(&kernel, &fresh, 2).unwrap_err().code,
                ErrorCode::Conflict
            );
            assert_eq!(
                ack(&kernel, &a, 0).unwrap_err().code,
                ErrorCode::InvalidCursor
            );
            assert_eq!(
                ack(&kernel, &a, 4).unwrap_err().code,
                ErrorCode::InvalidCursor
            );
            assert_eq!(ack(&kernel, &a, 1).unwrap(), a);
            let b = ack(&kernel, &observer(&kernel, "b"), 2).unwrap();
            assert_eq!((b.after, b.revision), (2, 1));
            assert_eq!(observer(&kernel, "a"), a);
            let denied: Result<ObserverCheckpoint, BasicError> = call(
                &kernel,
                "phenix.basic.journal@1/observer",
                ObserverRequest {
                    session: "s2".into(),
                    observer: "a".into(),
                },
            );
            assert_eq!(denied.unwrap_err().code, ErrorCode::Denied);
            let denied: Result<ObserverCheckpoint, BasicError> = call(
                &kernel,
                "phenix.basic.journal@1/checkpoint_observer",
                ObserverCheckpoint {
                    session: "s2".into(),
                    ..a.clone()
                },
            );
            assert_eq!(denied.unwrap_err().code, ErrorCode::Denied);
            let invalid: Result<ObserverCheckpoint, BasicError> = call(
                &kernel,
                "phenix.basic.journal@1/observer",
                ObserverRequest {
                    session: "s1".into(),
                    observer: "../a".into(),
                },
            );
            assert_eq!(invalid.unwrap_err().code, ErrorCode::InvalidInput);
            let records: Vec<StoredRecord> = call(
                &kernel,
                "phenix.basic.storage@1/transact",
                StorageTransaction {
                    session: "s1".into(),
                    edits: vec![StorageEdit {
                        key: "event-observer.v1.corrupt".into(),
                        expected: 0,
                        data: Blob(b"invalid checkpoint".to_vec()),
                    }],
                },
            )
            .unwrap();
            assert_eq!(records.len(), 1);
            let corrupt: Result<ObserverCheckpoint, BasicError> = call(
                &kernel,
                "phenix.basic.journal@1/observer",
                ObserverRequest {
                    session: "s1".into(),
                    observer: "corrupt".into(),
                },
            );
            assert_eq!(corrupt.unwrap_err().code, ErrorCode::StorageFailed);
            let absent: Result<StoredRecord, BasicError> = call(
                &kernel,
                "phenix.basic.storage@1/read",
                RecordKey {
                    session: "s1".into(),
                    key: "event-observer.v1.../a".into(),
                },
            );
            assert_eq!(absent.unwrap_err().code, ErrorCode::NotFound);
        }
        "crash-before" => {
            let a = observer(&kernel, "a");
            assert_eq!((a.after, a.revision), (1, 1));
            let page = replay(&kernel, a.after);
            assert_eq!(page.events[0].text, "two");
            fs::write(
                output.join("delivered-before-ack"),
                page.events[0].text.as_bytes(),
            )
            .unwrap();
            std::process::exit(71);
        }
        "verify-before" => {
            assert_eq!(observer(&kernel, "a").after, 1);
            assert_eq!(replay(&kernel, 1).events[0].text, "two");
        }
        "crash-after" => {
            let a = ack(&kernel, &observer(&kernel, "a"), 2).unwrap();
            assert_eq!((a.after, a.revision), (2, 2));
            std::process::exit(72);
        }
        "resume" => {
            let a = observer(&kernel, "a");
            let b = observer(&kernel, "b");
            assert_eq!((a.after, a.revision), (2, 2));
            assert_eq!((b.after, b.revision), (2, 1));
            let page = replay(&kernel, a.after);
            assert_eq!(page.events.len(), 1);
            assert_eq!(page.events[0].text, "three");
            assert_eq!(ack(&kernel, &b, 3).unwrap().after, 3);
            assert_eq!(observer(&kernel, "a"), a);
            fs::write(output.join("observer-restart.json"), serde_json::to_vec_pretty(&serde_json::json!({
                "format": "phenix.basic.observer-restart.v1", "cases": ["two-persistent-observers",
                    "stale-revision", "monotonic-and-tail-bounds", "cross-session-denial", "invalid-name",
                    "corrupt-record", "crash-before-ack-replay", "crash-after-ack-resume"],
                "delivery": "at-least-once"
            })).unwrap()).unwrap();
        }
        _ => panic!("unknown observer fixture mode"),
    }
}
