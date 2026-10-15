//! Independent, packaged native CompactionStore product acceptance.
use phenix_advanced_contracts::{
    CompactionCheckpoint, CompactionCheckpointCommit, CompactionCheckpointKey,
    CompactionStoreError, CompactionStoreProblem, JournalSpan, StoredCompactionCheckpoint,
};
use phenix_basic_contracts::{Event, EventPage, JournalQuery};
use phenix_contract::{Authority, Portable, Value};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection, SlotBinding},
    Outcome,
};
use phenix_p2_compaction_replay::span_for_events;
use std::{collections::BTreeSet, fs, path::Path};

const SESSION: &str = "s1";
const RESOURCE: &str = "phenix.session.s1";
const CONTRACT: &str = "phenix.advanced.compaction-store@1";
const PROVIDER: &str = "phenix.advanced.compaction-store";

struct Packages {
    plugin: String,
    journal: String,
    storage: String,
    basic: String,
    advanced: String,
}

fn grant(caller: &str, interface: &str, operation: &str) -> ScopedGrant {
    ScopedGrant {
        caller: caller.into(),
        interface: interface.into(),
        operation: operation.into(),
        resources: BTreeSet::from([RESOURCE.into()]),
    }
}

fn slot(caller: &str, name: &str, provider: &str, export: &str) -> SlotBinding {
    SlotBinding {
        caller: caller.into(),
        slot: name.into(),
        index: 0,
        provider: provider.into(),
        export: export.into(),
    }
}

impl Packages {
    fn invoke(&self, interface: &str, operation: &str, provider: &str, input: Value) -> Outcome {
        let grants = vec![
            grant("root", CONTRACT, "commit"),
            grant("root", CONTRACT, "get"),
            grant("root", "phenix.basic.journal@1", "read"),
            grant(PROVIDER, "phenix.basic.storage@1", "read"),
            grant(PROVIDER, "phenix.basic.storage@1", "transact"),
            grant(PROVIDER, "phenix.basic.journal@1", "read"),
            grant("phenix.basic.event-journal", "phenix.basic.storage@1", "read"),
        ];
        let composition = Composition {
            format: "phenix.composition.v2".into(),
            artifacts: vec![self.plugin.clone(), self.journal.clone(), self.storage.clone()],
            definitions: vec![self.basic.clone(), self.advanced.clone()],
            entry: Selection {
                interface: interface.into(),
                operation: operation.into(),
                provider: provider.into(),
            },
            slots: vec![
                slot(PROVIDER, "Storage", "phenix.basic.storage-sqlite", "Storage"),
                slot(PROVIDER, "Journal", "phenix.basic.event-journal", "Journal"),
                slot("phenix.basic.event-journal", "Storage", "phenix.basic.storage-sqlite", "Storage"),
            ],
            authority: Authority {
                operations: grants.iter().map(ScopedGrant::key).collect(),
                resources: BTreeSet::from([RESOURCE.into()]),
            },
            grants,
            excluded: BTreeSet::new(),
            substitutions: vec![],
            resource: Some(RESOURCE.into()),
        };
        let kernel = Prepared::prepare(composition).unwrap().load().unwrap();
        let root = kernel.admit_default_root().unwrap();
        root.invoke(&input).unwrap()
    }

    fn journal(&self) -> Vec<Event> {
        let outcome = self.invoke(
            "phenix.basic.journal@1",
            "read",
            "phenix.basic.event-journal",
            JournalQuery {
                session: SESSION.into(),
                after: 0,
                limit: 4096,
            }
            .to_value(),
        );
        let Outcome::Success(value) = outcome else {
            panic!("journal read failed: {outcome:?}");
        };
        let page = EventPage::from_value(value).unwrap();
        assert!(page.events.len() >= 2, "expected at least two real Basic events");
        page.events
    }

    fn call(&self, op: &str, value: Value) -> Outcome {
        self.invoke(CONTRACT, op, PROVIDER, value)
    }

    fn good(&self, op: &str, value: Value) -> StoredCompactionCheckpoint {
        let result = self.call(op, value);
        let Outcome::Success(value) = result else {
            panic!("expected successful checkpoint {op}, got {result:?}");
        };
        StoredCompactionCheckpoint::from_value(value).unwrap()
    }

    fn deny(&self, op: &str, value: Value, expected: CompactionStoreProblem) {
        let result = self.call(op, value);
        let Outcome::Domain(value) = result else {
            panic!("expected checkpoint {op} denial, got {result:?}");
        };
        assert_eq!(CompactionStoreError::from_value(value).unwrap().problem, expected);
    }
}

fn checkpoint(events: &[Event], id: &str, revision: u64, parent: Option<&str>) -> CompactionCheckpoint {
    let input = span_for_events(events).unwrap();
    let summarized = span_for_events(&events[..1]).unwrap();
    let retained = span_for_events(&events[1..]).unwrap();
    assert_eq!(input.first_sequence, 1);
    CompactionCheckpoint {
        id: id.into(),
        session: SESSION.into(),
        revision,
        parent_checkpoint: parent.map(str::to_owned),
        input,
        summarized: vec![summarized],
        retained: vec![retained],
        excluded: Vec::<JournalSpan>::new(),
        summary: "First message summarized; original bytes remain in Basic journal".into(),
        policy: "native-product-fixture".into(),
        policy_version: 1,
        model: None,
        hydration_sources: vec![],
    }
}

fn key(id: &str) -> Value {
    CompactionCheckpointKey {
        session: SESSION.into(),
        id: id.into(),
    }.to_value()
}

fn commit(fixture: &Packages) -> serde_json::Value {
    let events = fixture.journal();
    let a = checkpoint(&events, "checkpoint-a", 1, None);
    let first = fixture.good("commit", CompactionCheckpointCommit {
        checkpoint: a,
        expected: 0,
    }.to_value());
    assert_eq!(first.storage_revision, 1);
    let b = checkpoint(&events, "checkpoint-b", 2, Some("checkpoint-a"));
    let second = fixture.good("commit", CompactionCheckpointCommit {
        checkpoint: b.clone(),
        expected: 1,
    }.to_value());
    assert_eq!(second.storage_revision, 2);
    let next = checkpoint(&events, "checkpoint-c", 3, Some("checkpoint-b"));
    fixture.deny(
        "commit",
        CompactionCheckpointCommit {
            checkpoint: next,
            expected: 1,
        }.to_value(),
        CompactionStoreProblem::Conflict,
    );
    let mut foreign = checkpoint(&events, "foreign", 3, Some("checkpoint-b"));
    foreign.session = "s2".into();
    fixture.deny(
        "commit",
        CompactionCheckpointCommit {
            checkpoint: foreign,
            expected: 2,
        }.to_value(),
        CompactionStoreProblem::ScopeDenied,
    );
    serde_json::json!({
        "first_revision": first.storage_revision,
        "second_revision": second.storage_revision,
        "stale_cas_denied": true,
        "foreign_session_denied": true
    })
}

fn restart(fixture: &Packages) -> serde_json::Value {
    let restored = fixture.good("get", key("checkpoint-b"));
    assert_eq!(restored.storage_revision, 2);
    assert_eq!(restored.checkpoint.parent_checkpoint.as_deref(), Some("checkpoint-a"));
    assert_eq!(restored.checkpoint.input, span_for_events(&fixture.journal()).unwrap());
    serde_json::json!({
        "revision": restored.storage_revision,
        "parent_checkpoint": restored.checkpoint.parent_checkpoint,
        "source_revalidated": true
    })
}

fn missing_source(fixture: &Packages) -> serde_json::Value {
    fixture.deny(
        "get",
        key("checkpoint-b"),
        CompactionStoreProblem::SourceChanged,
    );
    serde_json::json!({"hydration_denied": true})
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(
        args.len(),
        9,
        "usage: phenix-p2-compaction-product PLUGIN JOURNAL STORAGE BASIC-DEFS ADV-DEFS MODE OUT"
    );
    let fixture = Packages {
        plugin: args[1].clone(),
        journal: args[2].clone(),
        storage: args[3].clone(),
        basic: args[4].clone(),
        advanced: args[5].clone(),
    };
    let result = match args[6].as_str() {
        "commit" => commit(&fixture),
        "restart" => restart(&fixture),
        "missing-source" => missing_source(&fixture),
        other => panic!("unknown compaction fixture mode: {other}"),
    };
    let output = Path::new(&args[7]);
    fs::create_dir_all(output).unwrap();
    fs::write(output.join("compaction.json"), serde_json::to_vec_pretty(&result).unwrap()).unwrap();
}
