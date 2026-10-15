//! Independent P2-3c native provider acceptance across SQLite restarts.
use phenix_advanced_contracts::{
    CheckpointKey, CheckpointStoreError, CheckpointStoreProblem, CompactionCheckpoint,
};
use phenix_basic_contracts::{EventPage, JournalQuery};
use phenix_contract::{Authority, Portable, Value};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection, SlotBinding},
    Outcome,
};
use phenix_p2_compaction_replay::span_for_events;
use std::{collections::BTreeSet, fs, path::Path};

const SESSION: &str = "s1";
const RESOURCE: &str = "phenix.session.s1";
const STORE: &str = "phenix.advanced.compaction-checkpoints";
const CONTRACT: &str = "phenix.advanced.compaction-checkpoints@1";

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

struct Fixture {
    artifacts: Vec<String>,
    definitions: Vec<String>,
}
impl Fixture {
    fn invoke(&self, interface: &str, operation: &str, provider: &str, input: Value) -> Outcome {
        let grants = vec![
            grant("root", CONTRACT, "put"),
            grant("root", CONTRACT, "get"),
            grant("root", "phenix.basic.journal@1", "read"),
            grant(STORE, "phenix.basic.storage@1", "read"),
            grant(STORE, "phenix.basic.storage@1", "transact"),
            grant(STORE, "phenix.basic.journal@1", "read"),
            grant("phenix.basic.event-journal", "phenix.basic.storage@1", "read"),
        ];
        let composition = Composition {
            format: "phenix.composition.v2".into(),
            artifacts: self.artifacts.clone(),
            definitions: self.definitions.clone(),
            entry: Selection {
                interface: interface.into(),
                operation: operation.into(),
                provider: provider.into(),
            },
            slots: vec![
                slot(STORE, "Storage", "phenix.basic.storage-sqlite", "Storage"),
                slot(STORE, "Journal", "phenix.basic.event-journal", "Journal"),
                slot(
                    "phenix.basic.event-journal",
                    "Storage",
                    "phenix.basic.storage-sqlite",
                    "Storage",
                ),
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
        kernel.admit_default_root().unwrap().invoke(&input).unwrap()
    }

    fn journal(&self) -> EventPage {
        let result = self.invoke(
            "phenix.basic.journal@1",
            "read",
            "phenix.basic.event-journal",
            JournalQuery {
                session: SESSION.into(),
                after: 0,
                limit: 128,
            }
            .to_value(),
        );
        let Outcome::Success(value) = result else {
            panic!("source journal missing: {result:?}");
        };
        EventPage::from_value(value).unwrap()
    }

    fn checkpoint(&self, operation: &str, input: Value) -> Outcome {
        self.invoke(CONTRACT, operation, STORE, input)
    }

    fn accept<T: Portable>(&self, operation: &str, input: Value) -> T {
        let result = self.checkpoint(operation, input);
        let Outcome::Success(value) = result else {
            panic!("expected {operation} success, got {result:?}");
        };
        T::from_value(value).unwrap()
    }

    fn deny(&self, operation: &str, input: Value, code: CheckpointStoreProblem) {
        let actual = self.checkpoint(operation, input);
        let Outcome::Domain(value) = actual else {
            panic!("expected checkpoint {operation} denial, got {actual:?}");
        };
        assert_eq!(CheckpointStoreError::from_value(value).unwrap().problem, code);
    }
}

fn make_checkpoint(page: &EventPage, id: &str, revision: u64, parent: Option<&str>) -> CompactionCheckpoint {
    assert!(page.events.len() >= 4 && page.events.len() < 128);
    let original = &page.events;
    CompactionCheckpoint {
        id: id.into(),
        session: SESSION.into(),
        revision,
        parent_checkpoint: parent.map(str::to_owned),
        input: span_for_events(original).unwrap(),
        summarized: vec![span_for_events(&original[..2]).unwrap()],
        retained: vec![span_for_events(&original[2..]).unwrap()],
        excluded: vec![],
        summary: "Earlier completed events; rehydrate from journal".into(),
        policy: "p2-cas-verification".into(),
        policy_version: 1,
        model: None,
        hydration_sources: vec![],
    }
}
fn key(id: &str) -> CheckpointKey {
    CheckpointKey {
        session: SESSION.into(),
        id: id.into(),
    }
}
fn action_write(fixture: &Fixture) -> serde_json::Value {
    let page = fixture.journal();
    let first = make_checkpoint(&page, "cp1", 1, None);
    let second = make_checkpoint(&page, "cp2", 2, Some("cp1"));
    assert_eq!(
        fixture.accept::<CompactionCheckpoint>("put", first.to_value()),
        first
    );
    assert_eq!(
        fixture.accept::<CompactionCheckpoint>("put", second.to_value()),
        second
    );
    assert_eq!(
        fixture.accept::<CompactionCheckpoint>("get", key("cp1").to_value()),
        first
    );
    let mut stale = make_checkpoint(&page, "stale", 2, Some("cp2"));
    fixture.deny("put", stale.to_value(), CheckpointStoreProblem::Conflict);
    stale.revision = 3;
    stale.input.digest_sha256 = "f".repeat(64);
    fixture.deny("put", stale.to_value(), CheckpointStoreProblem::SourceChanged);
    let mut foreign = first.clone();
    foreign.session = "s2".into();
    fixture.deny("put", foreign.to_value(), CheckpointStoreProblem::ScopeDenied);
    fixture.deny(
        "get",
        CheckpointKey {
            session: "s2".into(),
            id: "cp1".into(),
        }
        .to_value(),
        CheckpointStoreProblem::ScopeDenied,
    );
    serde_json::json!({
        "written": 2, "verified_journal_events": page.events.len(),
        "stale_conflict": true, "tampered_source_denied": true, "foreign_session_denied": true
    })
}
fn action_restart(fixture: &Fixture) -> serde_json::Value {
    let first = fixture.accept::<CompactionCheckpoint>("get", key("cp1").to_value());
    let second = fixture.accept::<CompactionCheckpoint>("get", key("cp2").to_value());
    assert_eq!(first.revision, 1);
    assert_eq!(second.revision, 2);
    assert_eq!(second.parent_checkpoint.as_deref(), Some("cp1"));
    fixture.deny("get", key("missing").to_value(), CheckpointStoreProblem::NotFound);
    serde_json::json!({"restored": true, "revisions": [first.revision, second.revision]})
}
fn action_missing_source(fixture: &Fixture) -> serde_json::Value {
    fixture.deny("get", key("cp2").to_value(), CheckpointStoreProblem::SourceChanged);
    serde_json::json!({"stale_checkpoint_denied": true})
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 8, "STORE JOURNAL SQLITE BASIC-DEFS ADV-DEFS ACTION OUTPUT");
    let fixture = Fixture {
        artifacts: args[1..=3].to_vec(),
        definitions: args[4..=5].to_vec(),
    };
    let report = match args[6].as_str() {
        "write" => action_write(&fixture),
        "restart" => action_restart(&fixture),
        "source-missing" => action_missing_source(&fixture),
        other => panic!("unknown product action: {other}"),
    };
    let target = Path::new(&args[7]);
    fs::create_dir_all(target).unwrap();
    fs::write(
        target.join("compaction-product.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}
