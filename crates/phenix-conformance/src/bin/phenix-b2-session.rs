//! Native ABI gate for session snapshots over an independently selected
//! revisioned Storage fixture (not a durable SQLite acceptance test).
use phenix_basic_contracts::{
    encode, BasicError, ErrorCode, Message, ReconcileTool, Role, RunStatus, Scope, SessionSnapshot,
    StorageEdit, StorageTransaction, ToolCall, ToolOutcome,
};
use phenix_contract::{Authority, Portable};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection, SlotBinding},
    Outcome,
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

fn invoke<T: Portable>(
    kernel: &phenix_kernel::Kernel,
    key: &str,
    provider: &str,
    request: impl Portable,
) -> Result<Result<T, BasicError>, phenix_contract::Failure> {
    let root = kernel.admit_root(selection(key, provider))?;
    match root.invoke(&request.to_value())? {
        Outcome::Success(value) => Ok(Ok(T::from_value(value)?)),
        Outcome::Domain(value) => Ok(Err(BasicError::from_value(value)?)),
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 5, "SESSION STORAGE DEFINITIONS OUTPUT");
    let output = Path::new(&args[4]);
    fs::create_dir_all(output).unwrap();
    let open = "phenix.basic.session@1/open";
    let checkpoint = "phenix.basic.session@1/checkpoint";
    let reconcile = "phenix.basic.session@1/reconcile";
    let store_read = "phenix.basic.storage@1/read";
    let store_write = "phenix.basic.storage@1/transact";
    let session = "s1";
    let resource = "phenix.session.s1";
    let resources = BTreeSet::from([resource.to_owned()]);
    let operations = [open, checkpoint, reconcile, store_read, store_write];
    let grants = [
        ("root", open),
        ("root", checkpoint),
        ("root", reconcile),
        ("phenix.basic.session", store_read),
        ("phenix.basic.session", store_write),
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
    let mut policy = Composition {
        format: "phenix.composition.v3".into(),
        artifacts: vec![args[1].clone(), args[2].clone()],
        definitions: vec![args[3].clone()],
        entry: selection(open, "phenix.basic.session"),
        slots: vec![SlotBinding {
            caller: "phenix.basic.session".into(),
            slot: "Storage".into(),
            index: 0,
            provider: "phenix.basic.storage-fixture".into(),
            export: "Storage".into(),
        }],
        grants,
        excluded: BTreeSet::new(),
        substitutions: vec![],
        authority: Authority {
            operations: operations.into_iter().map(str::to_owned).collect(),
            resources,
        },
        resource: Some(resource.into()),
    };
    let kernel = Prepared::prepare(policy.clone()).unwrap().load().unwrap();
    let fresh: SessionSnapshot = invoke(
        &kernel,
        open,
        "phenix.basic.session",
        Scope {
            session: session.into(),
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(fresh.revision, 0);
    assert_eq!(fresh.status, RunStatus::Idle);
    let mut running = fresh.clone();
    running.run = "r1".into();
    running.status = RunStatus::Running;
    running.history.push(Message::user("prompt".into()));
    running.pending.push(ToolCall {
        id: "call-1".into(),
        tool: "uppercase".into(),
        arguments: r#""prompt""#.into(),
    });
    let saved: SessionSnapshot = invoke(&kernel, checkpoint, "phenix.basic.session", running)
        .unwrap()
        .unwrap();
    assert_eq!(saved.revision, 1);
    assert_eq!(
        invoke::<SessionSnapshot>(
            &kernel,
            open,
            "phenix.basic.session",
            Scope {
                session: session.into(),
            }
        )
        .unwrap()
        .unwrap()
        .status,
        RunStatus::Running
    );
    assert_eq!(
        invoke::<SessionSnapshot>(&kernel, checkpoint, "phenix.basic.session", fresh)
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    let denied: Result<SessionSnapshot, BasicError> = invoke(
        &kernel,
        open,
        "phenix.basic.session",
        Scope {
            session: "s2".into(),
        },
    )
    .unwrap();
    assert_eq!(denied.unwrap_err().code, ErrorCode::Denied);
    drop(kernel);

    // A fresh provider process with existing Storage state must never retry
    // a previously started tool. Seed the selected Storage fixture as if it
    // had preserved a Running checkpoint across an application restart.
    policy.grants.push(ScopedGrant {
        caller: "root".into(),
        interface: "phenix.basic.storage@1".into(),
        operation: "transact".into(),
        resources: BTreeSet::from([resource.into()]),
    });
    let kernel = Prepared::prepare(policy).unwrap().load().unwrap();
    let seeded: Vec<phenix_basic_contracts::StoredRecord> = invoke(
        &kernel,
        store_write,
        "phenix.basic.storage-fixture",
        StorageTransaction {
            session: session.into(),
            edits: vec![StorageEdit {
                key: "session.snapshot.v1".into(),
                expected: 0,
                data: encode(&saved).unwrap(),
            }],
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(seeded.len(), 1);
    // Loading a second provider instance does not prove the old process
    // exited. Opening its stored Running checkpoint is read-only.
    let restored: SessionSnapshot = invoke(
        &kernel,
        open,
        "phenix.basic.session",
        Scope {
            session: session.into(),
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(restored.status, RunStatus::Running);
    assert_eq!(restored.revision, 1);
    // This test controls the fixture's process lifetime and explicitly
    // records an interrupted tool call before reconciliation.
    let interrupted: SessionSnapshot = invoke(
        &kernel,
        checkpoint,
        "phenix.basic.session",
        SessionSnapshot {
            status: RunStatus::Indeterminate,
            ..restored
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(interrupted.status, RunStatus::Indeterminate);
    assert_eq!(interrupted.revision, 2);
    let resolved: SessionSnapshot = invoke(
        &kernel,
        reconcile,
        "phenix.basic.session",
        ReconcileTool {
            session: session.into(),
            call_id: "call-1".into(),
            outcome: ToolOutcome::Succeeded {
                text: "PROMPT".into(),
            },
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(resolved.status, RunStatus::Running);
    assert_eq!(resolved.revision, 3);
    assert!(resolved.pending.is_empty());
    assert_eq!(resolved.history.last().unwrap().text, "PROMPT");
    assert_eq!(resolved.history.last().unwrap().call_id, "call-1");

    fs::write(
        output.join("session.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "format": "phenix.b2.session.v1",
            "cases": [
                "new-session", "checkpoint-CAS", "stale-revision",
                "session-scoped-open", "open-preserves-running",
                "explicit-interruption-checkpoint", "explicit-reconciliation", "persisted-history"
            ],
            "storage_backend": "volatile-native-fixture-with-seeded-checkpoint",
            "real_restart_or_sqlite_verified": false
        }))
        .unwrap(),
    )
    .unwrap();
    println!("B2 session native gate passed (volatile Storage fixture)");
}
