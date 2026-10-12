//! Independent native EventJournal and Storage ABI integration.
use phenix_basic_contracts::{Event, EventKind, EventPage, JournalAppend, JournalQuery};
use phenix_contract::Blob;
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

fn request(session: &str, run: &str, text: &str) -> Event {
    Event {
        sequence: 0,
        session: session.into(),
        run: run.into(),
        kind: EventKind::TextDelta,
        call_id: String::new(),
        text: text.into(),
        data: Blob(Vec::new()),
    }
}

fn invoke<T: Portable>(
    kernel: &phenix_kernel::Kernel,
    key: &str,
    input: impl Portable,
) -> Result<T, phenix_contract::Failure> {
    let root = kernel.admit_root(selection(key, "phenix.basic.event-journal"))?;
    let Outcome::Success(value) = root.invoke(&input.to_value())? else {
        panic!("unexpected journal domain failure");
    };
    T::from_value(value)
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 5, "JOURNAL STORAGE DEFINITIONS OUTPUT");
    let output = Path::new(&args[4]);
    fs::create_dir_all(output).unwrap();
    let append = "phenix.basic.journal@1/append";
    let read = "phenix.basic.journal@1/read";
    let store_read = "phenix.basic.storage@1/read";
    let store_write = "phenix.basic.storage@1/transact";
    let session_resource = "phenix.session.s1";
    let resources = BTreeSet::from([session_resource.to_owned()]);
    let grants = [("root", append), ("root", read),
        ("phenix.basic.event-journal", store_read),
        ("phenix.basic.event-journal", store_write)]
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
    let policy = Composition {
        format: "phenix.composition.v2".into(),
        artifacts: vec![args[1].clone(), args[2].clone()],
        definitions: vec![args[3].clone()],
        entry: selection(append, "phenix.basic.event-journal"),
        slots: vec![SlotBinding {
            caller: "phenix.basic.event-journal".into(),
            slot: "Storage".into(),
            index: 0,
            provider: "phenix.basic.storage-fixture".into(),
            export: "Storage".into(),
        }],
        grants,
        excluded: BTreeSet::new(),
        substitutions: vec![],
        authority: Authority {
            operations: [append, read, store_read, store_write]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            resources,
        },
        resource: Some(session_resource.into()),
    };
    let prepared = Prepared::prepare(policy.clone()).unwrap();
    let kernel = prepared.load().unwrap();
    let first = JournalAppend {
        session: "s1".into(),
        events: vec![
            request("s1", "r1", "one"),
            request("s1", "r1", "two"),
        ],
    };
    assert_eq!(invoke::<u64>(&kernel, append, first).unwrap(), 2);
    let a = JournalQuery {
        session: "s1".into(),
        after: 0,
        limit: 1,
    };
    let b = JournalQuery { limit: 2, ..a.clone() };
    let one = invoke::<EventPage>(&kernel, read, a).unwrap();
    let two = invoke::<EventPage>(&kernel, read, b).unwrap();
    assert_eq!(one.next, 1);
    assert_eq!(two.next, 2);
    assert_eq!(one.events[0].sequence, 1);
    assert_eq!(two.events[1].sequence, 2);
    assert_eq!(
        invoke::<u64>(&kernel, append, JournalAppend {
            session: "s1".into(),
            events: vec![request("s1", "r2", "three")],
        }).unwrap(),
        3
    );
    let third = invoke::<EventPage>(&kernel, read, JournalQuery {
        session: "s1".into(),
        after: two.next,
        limit: 2,
    }).unwrap();
    assert_eq!(third.events.len(), 1);
    assert_eq!(third.events[0].text, "three");
    // The host context is scoped to s1; requests claiming s2 must reject
    // before reaching Storage. In particular, they may not append records.
    let cross = JournalAppend {
        session: "s2".into(),
        events: vec![request("s2", "r1", "forged")],
    };
    let root = kernel.admit_default_root().unwrap();
    let Outcome::Domain(error) = root.invoke(&cross.to_value()).unwrap() else {
        panic!("cross-session append unexpectedly succeeded")
    };
    assert_eq!(
        phenix_basic_contracts::BasicError::from_value(error)
            .unwrap()
            .code,
        phenix_basic_contracts::ErrorCode::Denied
    );
    drop(root);
    let unchanged = invoke::<EventPage>(&kernel, read, JournalQuery {
        session: "s1".into(),
        after: 0,
        limit: 100,
    }).unwrap();
    assert_eq!(unchanged.events.len(), 3);
    drop(kernel);

    // Without a storage write grant, the provider cannot durably append.
    let mut denied = policy;
    denied.grants.retain(|grant| {
        grant.caller != "phenix.basic.event-journal" || grant.operation != "transact"
    });
    match Prepared::prepare(denied) {
        Err(failure) => assert_eq!(failure.code, phenix_contract::Code::Denied),
        Ok(prepared) => {
            // Import authority may be validated at admission rather than at
            // preparation. Either way no Storage write may occur.
            let kernel = prepared.load().unwrap();
            let root = kernel.admit_default_root().unwrap();
            assert_eq!(
                root.invoke(&JournalAppend {
                    session: "s1".into(),
                    events: vec![request("s1", "r1", "denied")],
                }.to_value()).unwrap_err().code,
                phenix_contract::Code::Denied
            );
        }
    }

    fs::write(
        output.join("event-journal.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "format": "phenix.b2.journal.v1",
            "cases": [
                "ordered-append", "independent-replay-offsets",
                "replay-after-append", "session-scope-denial",
                "storage-write-grant-denial"
            ],
            "backend": "volatile-native-fixture",
            "durable_backend_accepted": false
        }))
        .unwrap(),
    )
    .unwrap();
    println!("B2 journal native ABI gate passed (volatile Storage fixture)");
}
