//! Process-boundary B2 acceptance fixture: seed SQLite in one process,
//! reopen it with new native plugin instances and explicitly reconcile tools.
use phenix_basic_contracts::{
    BasicError, ErrorCode, Event, EventKind, EventPage, JournalAppend, JournalQuery, Message,
    ReconcileTool, RunStatus, Scope, SessionSnapshot, StorageEdit, StorageTransaction,
    StoredRecord, ToolCall, ToolOutcome,
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
        ("root", open),
        ("root", checkpoint),
        ("root", reconcile),
        ("root", append),
        ("root", replay),
        ("root", read),
        ("root", write),
        ("phenix.basic.session", read),
        ("phenix.basic.session", write),
        ("phenix.basic.event-journal", read),
        ("phenix.basic.event-journal", write),
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
        format: "phenix.composition.v3".into(),
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
    assert_eq!(
        args.len(),
        7,
        "MODE SESSION JOURNAL SQLITE DEFINITIONS OUTPUT"
    );
    assert!(std::env::var("PHENIX_BASIC_SQLITE_PATH").is_ok());
    let kernel = Prepared::prepare(composition(&args))
        .unwrap()
        .load()
        .unwrap();
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
        let fresh: SessionSnapshot = call(
            &kernel,
            open,
            session,
            Scope {
                session: "s1".into(),
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(fresh.revision, 0);
        let mut running = fresh.clone();
        running.run = "r1".into();
        running.status = RunStatus::Running;
        running.checkpoint = 7;
        running.history.push(Message::user("prompt"));
        running.pending = ["c1", "c2"]
            .into_iter()
            .map(|id| ToolCall {
                id: id.into(),
                tool: "uppercase".into(),
                arguments: "\"prompt\"".into(),
            })
            .collect();
        let saved: SessionSnapshot = call(&kernel, checkpoint, session, running)
            .unwrap()
            .unwrap();
        assert_eq!(saved.revision, 1);
        assert_eq!(
            call::<SessionSnapshot>(&kernel, checkpoint, session, fresh)
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        let count: u64 = call(
            &kernel,
            append,
            journal,
            JournalAppend {
                session: "s1".into(),
                events: vec![event("one"), event("two"), event("three")],
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(count, 3);

        let written: Vec<StoredRecord> = call(
            &kernel,
            write,
            storage,
            StorageTransaction {
                session: "s1".into(),
                edits: vec![
                    StorageEdit {
                        key: "atomic-a".into(),
                        expected: 0,
                        data: Blob(b"A".to_vec()),
                    },
                    StorageEdit {
                        key: "atomic-b".into(),
                        expected: 0,
                        data: Blob(b"B".to_vec()),
                    },
                ],
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(written.len(), 2);
        assert!(written.iter().all(|record| record.revision == 1));
        assert_eq!(
            call::<Vec<StoredRecord>>(
                &kernel,
                write,
                storage,
                StorageTransaction {
                    session: "s1".into(),
                    edits: vec![
                        StorageEdit {
                            key: "atomic-a".into(),
                            expected: 1,
                            data: Blob(b"changed".to_vec())
                        },
                        StorageEdit {
                            key: "atomic-b".into(),
                            expected: 0,
                            data: Blob(b"bad".to_vec())
                        },
                    ],
                }
            )
            .unwrap()
            .unwrap_err()
            .code,
            ErrorCode::Conflict
        );
        let after: StoredRecord = call(
            &kernel,
            read,
            storage,
            phenix_basic_contracts::RecordKey {
                session: "s1".into(),
                key: "atomic-a".into(),
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(after.data.0, b"A");
        assert_eq!(
            call::<SessionSnapshot>(
                &kernel,
                open,
                session,
                Scope {
                    session: "s2".into()
                }
            )
            .unwrap()
            .unwrap_err()
            .code,
            ErrorCode::Denied
        );
    } else if args[1] == "schema-rejected" {
        // Unsupported future versions, absent versioned tables, and malformed
        // unversioned tables must all fail before exposing Storage records.
        let rejected: Result<StoredRecord, BasicError> = call(
            &kernel,
            read,
            storage,
            phenix_basic_contracts::RecordKey {
                session: "s1".into(),
                key: "atomic-a".into(),
            },
        )
        .unwrap();
        assert_eq!(rejected.unwrap_err().code, ErrorCode::StorageFailed);
    } else if args[1] == "faults" {
        // The driver installs a SQLite BEFORE UPDATE trigger that aborts
        // writes to atomic-b. atomic-a is written first inside the same
        // transaction. The failed second edit must roll back the first.
        let rejected: Result<Vec<StoredRecord>, BasicError> = call(
            &kernel,
            write,
            storage,
            StorageTransaction {
                session: "s1".into(),
                edits: vec![
                    StorageEdit {
                        key: "atomic-a".into(),
                        expected: 1,
                        data: Blob(b"rolled-back".to_vec()),
                    },
                    StorageEdit {
                        key: "atomic-b".into(),
                        expected: 1,
                        data: Blob(b"trigger-rejected".to_vec()),
                    },
                ],
            },
        )
        .unwrap();
        assert_eq!(rejected.unwrap_err().code, ErrorCode::StorageFailed);

        // Validate that both values and revisions survived the failed SQL
        // statement. A later process reopens this same database to resume.
        for (key, value) in [("atomic-a", b"A".as_slice()), ("atomic-b", b"B".as_slice())] {
            let record: StoredRecord = call(
                &kernel,
                read,
                storage,
                phenix_basic_contracts::RecordKey {
                    session: "s1".into(),
                    key: key.into(),
                },
            )
            .unwrap()
            .unwrap();
            assert_eq!(record.revision, 1, "{key} revision changed on rollback");
            assert_eq!(
                record.data.0.as_slice(),
                value,
                "{key} data changed on rollback"
            );
        }

        // Overflow and duplicate edit keys fail before touching SQLite.
        for (edits, expected_code) in [
            (
                vec![StorageEdit {
                    key: "atomic-a".into(),
                    expected: u64::MAX,
                    data: Blob(b"overflow".to_vec()),
                }],
                ErrorCode::Conflict,
            ),
            (
                vec![
                    StorageEdit {
                        key: "atomic-a".into(),
                        expected: 1,
                        data: Blob(b"duplicate-1".to_vec()),
                    },
                    StorageEdit {
                        key: "atomic-a".into(),
                        expected: 1,
                        data: Blob(b"duplicate-2".to_vec()),
                    },
                ],
                ErrorCode::InvalidInput,
            ),
        ] {
            let rejected: Result<Vec<StoredRecord>, BasicError> = call(
                &kernel,
                write,
                storage,
                StorageTransaction {
                    session: "s1".into(),
                    edits,
                },
            )
            .unwrap();
            assert_eq!(rejected.unwrap_err().code, expected_code);
        }
        let unchanged: StoredRecord = call(
            &kernel,
            read,
            storage,
            phenix_basic_contracts::RecordKey {
                session: "s1".into(),
                key: "atomic-a".into(),
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(unchanged.revision, 1);
        assert_eq!(unchanged.data.0, b"A");

        fs::write(
            output.join("sqlite-faults.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "format": "phenix.b2.sqlite-faults.v1",
                "separate_process": true,
                "cases": [
                    "sql-trigger-error-rolls-back-earlier-update",
                    "both-record-revisions-unchanged",
                    "revision-overflow-before-write",
                    "duplicate-edit-key-before-write"
                ]
            }))
            .unwrap(),
        )
        .unwrap();
    } else if args[1] == "resume" {
        // This process has no in-memory plugin instances from the seed run.
        let restored: SessionSnapshot = call(
            &kernel,
            open,
            session,
            Scope {
                session: "s1".into(),
            },
        )
        .unwrap()
        .unwrap();
        // Opening a snapshot in another process does not prove the first
        // process exited. Open must not write or infer Indeterminate.
        assert_eq!(restored.revision, 1);
        assert_eq!(restored.status, RunStatus::Running);
        assert_eq!(restored.checkpoint, 7);
        // This fixture runs seed and resume sequentially, so the test driver
        // knows no owner remains. Explicitly record an interrupted tool turn.
        let interrupted: SessionSnapshot = call(
            &kernel,
            checkpoint,
            session,
            SessionSnapshot {
                status: RunStatus::Indeterminate,
                ..restored.clone()
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(interrupted.revision, 2);
        assert_eq!(interrupted.status, RunStatus::Indeterminate);
        assert_eq!(restored.history[0].text(), "prompt");
        assert_eq!(restored.pending.len(), 2);
        let a: EventPage = call(
            &kernel,
            replay,
            journal,
            JournalQuery {
                session: "s1".into(),
                after: 0,
                limit: 1,
            },
        )
        .unwrap()
        .unwrap();
        let b: EventPage = call(
            &kernel,
            replay,
            journal,
            JournalQuery {
                session: "s1".into(),
                after: 0,
                limit: 2,
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(a.next, 1);
        assert_eq!(b.next, 2);
        assert_eq!(a.events[0].text, "one");
        assert_eq!(b.events[1].text, "two");
        let tail: EventPage = call(
            &kernel,
            replay,
            journal,
            JournalQuery {
                session: "s1".into(),
                after: b.next,
                limit: 2,
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(tail.events[0].sequence, 3);
        assert_eq!(
            call::<SessionSnapshot>(
                &kernel,
                reconcile,
                session,
                ReconcileTool {
                    session: "s1".into(),
                    call_id: "missing".into(),
                    outcome: ToolOutcome::Succeeded {
                        text: String::new(),
                    },
                }
            )
            .unwrap()
            .unwrap_err()
            .code,
            ErrorCode::NotFound
        );
        let one: SessionSnapshot = call(
            &kernel,
            reconcile,
            session,
            ReconcileTool {
                session: "s1".into(),
                call_id: "c1".into(),
                outcome: ToolOutcome::Succeeded {
                    text: "PROMPT".into(),
                },
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(one.status, RunStatus::Indeterminate);
        let two: SessionSnapshot = call(
            &kernel,
            reconcile,
            session,
            ReconcileTool {
                session: "s1".into(),
                call_id: "c2".into(),
                outcome: ToolOutcome::Failed {
                    output: String::new(),
                    reason: "failed".into(),
                },
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(two.status, RunStatus::Running);
        assert!(two.pending.is_empty());
        assert_eq!(two.history.len(), 3);
        assert_eq!(two.history[2].call_id(), "c2");
        fs::write(
            output.join("sqlite-restart.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "format": "phenix.b2.sqlite.v1",
                "backend": "sqlite3-file",
                "separate_processes": true,
                "cases": ["session-checkpoint", "CAS-conflict", "atomic-batch-rollback",
                    "cross-session-denial", "ordered-journal-replay",
                    "independent-observer-offsets", "explicit-owner-confirmed-interruption",
                    "open-does-not-infer-owner-death",
                    "explicit-multiple-call-reconciliation"]
            }))
            .unwrap(),
        )
        .unwrap();
    } else {
        panic!("expected seed, faults, schema-rejected or resume");
    }
}
