//! Process-boundary B2 acceptance fixture: seed SQLite in one process,
//! reopen it with new native plugin instances and explicitly reconcile tools.
use phenix_basic_contracts::{
    BasicError, ErrorCode, Event, EventKind, EventPage, JournalAppend, JournalQuery, Message,
    ReconcileTool, Role, RunStatus, Scope, SessionSnapshot, StorageEdit, StorageTransaction,
    StoredRecord, ToolCall,
};
use phenix_contract::{Authority, Blob, Portable};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection, SlotBinding},
    Outcome,
};
use std::{collections::BTreeSet, fs, path::Path};

fn selected(key: &str, provider: &str) -> Selection {
    let (interface, operation) = key.split_once('/').unwrap();
    Selection {
        interface: interface.into(),
        operation: operation.into(),
        provider: provider.into(),
    }
}
fn call<T: Portable>(
    kernel: &phenix_kernel::Kernel,
    key: &str,
    provider: &str,
    input: impl Portable,
) -> Result<Result<T, BasicError>, phenix_contract::Failure> {
    let root = kernel.admit_root(selected(key, provider))?;
    match root.invoke(&input.to_value())? {
        Outcome::Success(value) => Ok(Ok(T::from_value(value)?)),
        Outcome::Domain(value) => Ok(Err(BasicError::from_value(value)?)),
    }
}
fn event(text: &str) -> Event {
    Event {
        sequence: 0,
        session: "s1".into(),
        run: "r1".into(),
        kind: EventKind::TextDelta,
        call_id: String::new(),
        text: text.into(),
        data: Blob(vec![]),
    }
}
fn composition(args: &[String]) -> Composition {
    let open = "phenix.basic.session@1/open";
    let checkpoint = "phenix.basic.session@1/checkpoint";
    let reconcile = "phenix.basic.session@1/reconcile";
    let append = "phenix.basic.journal@1/append";
    let replay = "phenix.basic.journal@1/read";
    let read = "phenix.basic.storage@1/read";
    let write = "phenix.basic.storage@1/transact";
    let ops = [open, checkpoint, reconcile, append, replay, read, write];
    let resources = BTreeSet::from(["phenix.session.s1".to_string()]);
    let grants = [
        ("root", open), ("root", checkpoint), ("root", reconcile),
        ("root", append), ("root", replay), ("root", read), ("root", write),
        ("phenix.basic.session", read), ("phenix.basic.session", write),
        ("phenix.basic.event-journal", read), ("phenix.basic.event-journal", write),
    ]
    .into_iter()
    .map(|(caller, key)| {
        let (interface, operation) = key.split_once('/').unwrap();
        ScopedGrant {
            caller: caller.into(),
            interface: interface.into(),
            operation: operation.into(),
            resources: resources.clone(),
        }
    })
    .collect();
    Composition {
        format: "phenix.composition.v2".into(),
        artifacts: args[2..5].to_vec(),
        definitions: vec![args[5].clone()],
        entry: selected(open, "phenix.basic.session"),
        slots: ["phenix.basic.session", "phenix.basic.event-journal"]
            .into_iter()
            .map(|caller| SlotBinding {
                caller: caller.into(),
                slot: "Storage".into(),
                index: 0,
                provider: "phenix.basic.storage-sqlite".into(),
                export: "Storage".into(),
            })
            .collect(),
        grants,
        excluded: BTreeSet::new(),
        substitutions: vec![],
        authority: Authority {
            operations: ops.into_iter().map(str::to_owned).collect(),
            resources,
        },
        resource: Some("phenix.session.s1".into()),
    }
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 7, "MODE SESSION JOURNAL SQLITE DEFINITIONS OUTPUT");
    assert!(std::env::var("PHENIX_BASIC_SQLITE_PATH").is_ok());
    let kernel = Prepared::prepare(composition(&args)).unwrap().load().unwrap();
    let session = "phenix.basic.session";
    let journal = "phenix.basic.event-journal";
    let storage = "phenix.basic.storage-sqlite";
    let open = "phenix.basic.session@1/open";
    let checkpoint = "phenix.basic.session@1/checkpoint";
    let reconcile = "phenix.basic.session@1/reconcile";
    let append = "phenix.basic.journal@1/append";
    let replay = "phenix.basic.journal@1/read";
    let read = "phenix.basic.storage@1/read";
    let write = "phenix.basic.storage@1/transact";
    let output = Path::new(&args[6]);
    fs::create_dir_all(output).unwrap();
    if args[1] == "seed" {
        let fresh: SessionSnapshot =
            call(&kernel, open, session, Scope { session: "s1".into() }).unwrap().unwrap();
        assert_eq!(fresh.revision, 0);
        let mut running = fresh.clone();
        running.run = "r1".into();
        running.status = RunStatus::Running;
        running.checkpoint = 7;
        running.history.push(Message {
            role: Role::User,
            text: "prompt".into(),
            calls: vec![],
            call_id: String::new(),
            error: None,
        });
        running.pending = ["c1", "c2"].into_iter().map(|id| ToolCall {
            id: id.into(), tool: "uppercase".into(), arguments: "\"prompt\"".into(),
        }).collect();
        let saved: SessionSnapshot =
            call(&kernel, checkpoint, session, running).unwrap().unwrap();
        assert_eq!(saved.revision, 1);
        assert_eq!(call::<SessionSnapshot>(
            &kernel, checkpoint, session, fresh
        ).unwrap().unwrap_err().code, ErrorCode::Conflict);
        let count: u64 = call(&kernel, append, journal, JournalAppend {
            session: "s1".into(),
            events: vec![event("one"), event("two"), event("three")],
        }).unwrap().unwrap();
        assert_eq!(count, 3);

        let written: Vec<StoredRecord> = call(&kernel, write, storage, StorageTransaction {
            session: "s1".into(),
            edits: vec![
                StorageEdit { key: "atomic-a".into(), expected: 0, data: Blob(b"A".to_vec()) },
                StorageEdit { key: "atomic-b".into(), expected: 0, data: Blob(b"B".to_vec()) },
            ],
        }).unwrap().unwrap();
        assert_eq!(written.len(), 2);
        assert!(written.iter().all(|record| record.revision == 1));
        assert_eq!(call::<Vec<StoredRecord>>(&kernel, write, storage, StorageTransaction {
            session: "s1".into(),
            edits: vec![
                StorageEdit { key: "atomic-a".into(), expected: 1, data: Blob(b"changed".to_vec()) },
                StorageEdit { key: "atomic-b".into(), expected: 0, data: Blob(b"bad".to_vec()) },
            ],
        }).unwrap().unwrap_err().code, ErrorCode::Conflict);
        let after: StoredRecord = call(&kernel, read, storage,
            phenix_basic_contracts::RecordKey {
                session: "s1".into(), key: "atomic-a".into()
            }
        ).unwrap().unwrap();
        assert_eq!(after.data.0, b"A");
        assert_eq!(call::<SessionSnapshot>(&kernel, open, session,
            Scope { session: "s2".into() }
        ).unwrap().unwrap_err().code, ErrorCode::Denied);
    } else if args[1] == "resume" {
        // This process has no in-memory plugin instances from the seed run.
        let restored: SessionSnapshot = call(
            &kernel, open, session, Scope { session: "s1".into() }
        ).unwrap().unwrap();
        assert_eq!(restored.revision, 2);
        assert_eq!(restored.status, RunStatus::Indeterminate);
        assert_eq!(restored.checkpoint, 7);
        assert_eq!(restored.history[0].text, "prompt");
        assert_eq!(restored.pending.len(), 2);
        let a: EventPage = call(&kernel, replay, journal, JournalQuery {
            session: "s1".into(), after: 0, limit: 1
        }).unwrap().unwrap();
        let b: EventPage = call(&kernel, replay, journal, JournalQuery {
            session: "s1".into(), after: 0, limit: 2
        }).unwrap().unwrap();
        assert_eq!(a.next, 1);
        assert_eq!(b.next, 2);
        assert_eq!(a.events[0].text, "one");
        assert_eq!(b.events[1].text, "two");
        let tail: EventPage = call(&kernel, replay, journal, JournalQuery {
            session: "s1".into(), after: b.next, limit: 2
        }).unwrap().unwrap();
        assert_eq!(tail.events[0].sequence, 3);
        assert_eq!(call::<SessionSnapshot>(&kernel, reconcile, session, ReconcileTool {
            session: "s1".into(), call_id: "missing".into(),
            text: String::new(), error: None,
        }).unwrap().unwrap_err().code, ErrorCode::NotFound);
        let one: SessionSnapshot = call(&kernel, reconcile, session, ReconcileTool {
            session: "s1".into(), call_id: "c1".into(),
            text: "PROMPT".into(), error: None,
        }).unwrap().unwrap();
        assert_eq!(one.status, RunStatus::Indeterminate);
        let two: SessionSnapshot = call(&kernel, reconcile, session, ReconcileTool {
            session: "s1".into(), call_id: "c2".into(),
            text: String::new(), error: Some("failed".into()),
        }).unwrap().unwrap();
        assert_eq!(two.status, RunStatus::Running);
        assert!(two.pending.is_empty());
        assert_eq!(two.history.len(), 3);
        assert_eq!(two.history[2].call_id, "c2");
        fs::write(output.join("sqlite-restart.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "format": "phenix.b2.sqlite.v1",
                "backend": "sqlite3-file",
                "separate_processes": true,
                "cases": ["session-checkpoint", "CAS-conflict", "atomic-batch-rollback",
                    "cross-session-denial", "ordered-journal-replay",
                    "independent-observer-offsets", "indeterminate-tool-recovery",
                    "explicit-multiple-call-reconciliation"]
            })).unwrap()).unwrap();
    } else {
        panic!("expected seed or resume");
    }
}
