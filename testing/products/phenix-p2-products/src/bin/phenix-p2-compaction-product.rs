//! Independent P2-3c native provider acceptance across SQLite restarts.
use phenix_advanced_contracts::{
    CheckpointKey, CheckpointStoreError, CheckpointStoreProblem, CompactionCheckpoint,
};
use phenix_basic_contracts::{EventKind, EventPage, JournalQuery, Scope};
use phenix_contract::{Authority, Portable, Value};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection, SlotBinding},
    Outcome,
};
use phenix_p2_compaction_replay::{
    compacted_view, publish_verified_checkpoint, restore_original_events, span_for_events,
    ContextPart,
};
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
            grant("root", CONTRACT, "latest"),
            grant("root", "phenix.basic.journal@1", "read"),
            grant(STORE, "phenix.basic.storage@1", "read"),
            grant(STORE, "phenix.basic.storage@1", "transact"),
            grant(STORE, "phenix.basic.journal@1", "read"),
            grant(
                "phenix.basic.event-journal",
                "phenix.basic.storage@1",
                "read",
            ),
        ];
        let composition = Composition {
            format: "phenix.composition.v3".into(),
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
        let root = kernel.admit_default_root().unwrap();
        root.invoke(&input).unwrap()
    }

    fn journal(&self) -> EventPage {
        // Read source data from the real selected journal. The 128-event
        // boundary is not an end-of-stream signal; verify every cursor.
        const PAGE_EVENTS: u64 = 128;
        const MAX_EVENTS: usize = 1024;
        let mut after = 0_u64;
        let mut events = Vec::new();
        loop {
            let result = self.invoke(
                "phenix.basic.journal@1",
                "read",
                "phenix.basic.event-journal",
                JournalQuery {
                    session: SESSION.into(),
                    after,
                    limit: PAGE_EVENTS,
                }
                .to_value(),
            );
            let Outcome::Success(value) = result else {
                panic!("source journal missing: {result:?}");
            };
            let page = EventPage::from_value(value).unwrap();
            assert!(page.events.len() <= PAGE_EVENTS as usize);
            if page.events.is_empty() {
                assert_eq!(page.next, after, "empty journal page advanced cursor");
                return EventPage {
                    events,
                    next: after,
                };
            }
            let mut next = after;
            for event in &page.events {
                next = next.checked_add(1).expect("journal cursor overflow");
                assert_eq!(event.sequence, next, "noncontiguous source journal");
                assert_eq!(event.session, SESSION, "foreign session in journal");
            }
            assert_eq!(page.next, next, "invalid journal page cursor");
            events.extend(page.events);
            assert!(events.len() <= MAX_EVENTS, "source exceeds checkpoint cap");
            after = next;
        }
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
        assert_eq!(
            CheckpointStoreError::from_value(value).unwrap().problem,
            code
        );
    }
}

fn make_checkpoint(
    page: &EventPage,
    id: &str,
    revision: u64,
    parent: Option<&str>,
) -> CompactionCheckpoint {
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
    fixture.deny(
        "latest",
        Scope {
            session: SESSION.into(),
        }
        .to_value(),
        CheckpointStoreProblem::NotFound,
    );
    assert_eq!(
        fixture.accept::<CompactionCheckpoint>("put", first.to_value()),
        first
    );
    assert_eq!(
        fixture.accept::<CompactionCheckpoint>("put", second.to_value()),
        second
    );
    assert_eq!(
        fixture.accept::<CompactionCheckpoint>(
            "latest",
            Scope {
                session: SESSION.into()
            }
            .to_value()
        ),
        second,
    );
    assert_eq!(
        fixture.accept::<CompactionCheckpoint>("get", key("cp1").to_value()),
        first
    );
    let mut stale = make_checkpoint(&page, "stale", 2, Some("cp2"));
    fixture.deny("put", stale.to_value(), CheckpointStoreProblem::Conflict);
    stale.revision = 3;
    stale.input.digest_sha256 = "f".repeat(64);
    fixture.deny(
        "put",
        stale.to_value(),
        CheckpointStoreProblem::SourceChanged,
    );
    let mut foreign = first.clone();
    foreign.session = "s2".into();
    fixture.deny(
        "put",
        foreign.to_value(),
        CheckpointStoreProblem::ScopeDenied,
    );
    fixture.deny(
        "get",
        CheckpointKey {
            session: "s2".into(),
            id: "cp1".into(),
        }
        .to_value(),
        CheckpointStoreProblem::ScopeDenied,
    );
    fixture.deny(
        "latest",
        Scope {
            session: "s2".into(),
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
    assert_eq!(
        fixture.accept::<CompactionCheckpoint>(
            "latest",
            Scope {
                session: SESSION.into()
            }
            .to_value()
        ),
        second,
    );
    assert_eq!(first.revision, 1);
    assert_eq!(second.revision, 2);
    assert_eq!(second.parent_checkpoint.as_deref(), Some("cp1"));
    fixture.deny(
        "get",
        key("missing").to_value(),
        CheckpointStoreProblem::NotFound,
    );
    // A *fresh* native kernel separately reads the authoritative Basic
    // journal after the checkpoint has been retrieved from durable Storage.
    // The derived view neither mutates source records nor executes any tool.
    let source = fixture.journal();
    let projection = compacted_view(&second, &source.events, 8192).unwrap();
    let summary_cited = matches!(
        projection.parts.first(),
        Some(ContextPart::Summary { sources, text, .. })
            if sources == &second.summarized && text == &second.summary
    );
    let original_tail = &source.events[2..];
    let exact_originals = projection.parts.len() == original_tail.len() + 1
        && projection.parts[1..]
            .iter()
            .zip(original_tail)
            .all(|(part, event)| *part == ContextPart::Original(event.clone()));
    let expanded = restore_original_events(&second, &source.events).unwrap();
    assert_eq!(expanded, source.events);
    assert!(summary_cited && exact_originals);
    serde_json::json!({
        "restored": true,
        "revisions": [first.revision, second.revision],
        "latest_checkpoint_id": second.id,
        "view_original_source_verified": expanded == source.events,
        "view_summary_cited": summary_cited,
        "view_uncompacted_events_exact": exact_originals,
        "view_visible_payload_bytes": projection.visible_payload_bytes
    })
}
fn action_live_seed(fixture: &Fixture) -> serde_json::Value {
    // A separate native process commits one source-backed checkpoint over an
    // entire completed Basic turn. The Advanced Model adapter must read the
    // same SQLite ledger and journal through its own selected imports.
    let page = fixture.journal();
    let first_answer = page
        .events
        .iter()
        .position(|event| event.kind == EventKind::Answer)
        .expect("completed first turn")
        + 1;
    assert!(first_answer > 1 && first_answer < page.events.len());
    let source = &page.events;
    let checkpoint = CompactionCheckpoint {
        id: "cp-live".into(),
        session: SESSION.into(),
        revision: 1,
        parent_checkpoint: None,
        input: span_for_events(source).unwrap(),
        summarized: vec![span_for_events(&source[..first_answer]).unwrap()],
        retained: vec![span_for_events(&source[first_answer..]).unwrap()],
        excluded: vec![],
        summary: "The user said seed-one; the assistant replied fixture:seed-one.".into(),
        policy: "verified-model-request".into(),
        policy_version: 1,
        model: None,
        hydration_sources: vec![],
    };
    let stored: CompactionCheckpoint = fixture.accept("put", checkpoint.to_value());
    assert_eq!(stored, checkpoint);
    serde_json::json!({"live_checkpoint": stored.id, "revision": stored.revision})
}

fn action_publish(fixture: &Fixture) -> serde_json::Value {
    // One native root per selected Store operation. The source provider and
    // SQLite ledger are real: the reusable policy only checks CAS results.
    fn latest(fixture: &Fixture) -> Result<Option<CompactionCheckpoint>, String> {
        match fixture.checkpoint(
            "latest",
            Scope {
                session: SESSION.into(),
            }
            .to_value(),
        ) {
            Outcome::Success(value) => CompactionCheckpoint::from_value(value)
                .map(Some)
                .map_err(|failure| format!("{failure:?}")),
            Outcome::Domain(value) => {
                let failure = CheckpointStoreError::from_value(value)
                    .map_err(|reason| format!("{reason:?}"))?;
                if failure.problem == CheckpointStoreProblem::NotFound {
                    Ok(None)
                } else {
                    Err(format!("{failure:?}"))
                }
            }
        }
    }

    fn put(
        fixture: &Fixture,
        candidate: &CompactionCheckpoint,
    ) -> Result<CompactionCheckpoint, String> {
        match fixture.checkpoint("put", candidate.to_value()) {
            Outcome::Success(value) => {
                CompactionCheckpoint::from_value(value).map_err(|failure| format!("{failure:?}"))
            }
            other => Err(format!("{other:?}")),
        }
    }

    let page = fixture.journal();
    let first_complete_turn = page
        .events
        .iter()
        .position(|event| event.kind == EventKind::Answer)
        .expect("first completed native Basic turn")
        + 1;
    assert!(first_complete_turn > 1 && first_complete_turn < page.events.len());
    let original = &page.events;
    let candidate = CompactionCheckpoint {
        id: "cp-published".into(),
        session: SESSION.into(),
        revision: 1,
        parent_checkpoint: None,
        input: span_for_events(original).unwrap(),
        summarized: vec![span_for_events(&original[..first_complete_turn]).unwrap()],
        retained: vec![span_for_events(&original[first_complete_turn..]).unwrap()],
        excluded: vec![],
        summary: format!(
            "Source user prompt: {}; source answer: {}",
            original[0].text,
            original[first_complete_turn - 1].text,
        ),
        policy: "p2-verified-completed-turns".into(),
        policy_version: 1,
        model: None,
        hydration_sources: vec![],
    };
    let published =
        publish_verified_checkpoint(&candidate, || latest(fixture), |next| put(fixture, next))
            .expect("selected Store denied verified CAS publication or readback");
    assert_eq!(published.checkpoint, candidate);
    let restored = latest(fixture).unwrap().expect("published source missing");
    assert_eq!(restored, candidate);
    serde_json::json!({
        "created": published.created,
        "checkpoint_id": restored.id,
        "revision": restored.revision,
        "source_bound": restored.input == span_for_events(&page.events).unwrap(),
    })
}

fn action_missing_source(fixture: &Fixture) -> serde_json::Value {
    fixture.deny(
        "get",
        key("cp2").to_value(),
        CheckpointStoreProblem::SourceChanged,
    );
    fixture.deny(
        "latest",
        Scope {
            session: SESSION.into(),
        }
        .to_value(),
        CheckpointStoreProblem::SourceChanged,
    );
    serde_json::json!({"stale_checkpoint_denied": true})
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(
        args.len(),
        8,
        "STORE JOURNAL SQLITE BASIC-DEFS ADV-DEFS ACTION OUTPUT"
    );
    let fixture = Fixture {
        artifacts: args[1..=3].to_vec(),
        definitions: args[4..=5].to_vec(),
    };
    let report = match args[6].as_str() {
        "write" => action_write(&fixture),
        "restart" => action_restart(&fixture),
        "source-missing" => action_missing_source(&fixture),
        "live-seed" => action_live_seed(&fixture),
        "publish" => action_publish(&fixture),
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
