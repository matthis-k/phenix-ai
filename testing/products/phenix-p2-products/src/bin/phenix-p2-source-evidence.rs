//! Independent native P2-1 source verifier: real Basic journal, real kernel,
//! real native provider and authenticated imports. No test-only Journal plugin.
use phenix_advanced_contracts::{
    compare_source, EvidenceScope, EvidenceScopeKind, SourceMatch, SourceObservation, SourceRef,
};
use phenix_basic_contracts::{encode, Event, EventKind, EventPage, JournalQuery};
use phenix_contract::{Authority, Portable, Value};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection, SlotBinding},
    Outcome,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const SCOPE: &str = "phenix.session.s1";

fn grant(caller: &str, interface: &str, operation: &str) -> ScopedGrant {
    ScopedGrant {
        caller: caller.into(),
        interface: interface.into(),
        operation: operation.into(),
        resources: BTreeSet::from([SCOPE.into()]),
    }
}

fn binding(caller: &str, slot: &str, provider: &str, export: &str) -> SlotBinding {
    SlotBinding {
        caller: caller.into(),
        slot: slot.into(),
        index: 0,
        provider: provider.into(),
        export: export.into(),
    }
}

fn composition(
    entry_interface: &str,
    entry_operation: &str,
    entry_provider: &str,
    artifacts: &[String],
    definitions: &[String],
) -> Composition {
    let grants = vec![
        grant("root", entry_interface, entry_operation),
        grant(
            "phenix.basic.event-journal",
            "phenix.basic.storage@1",
            "read",
        ),
        grant(
            "phenix.advanced.evidence-journal",
            "phenix.basic.journal@1",
            "read",
        ),
    ];
    Composition {
        format: "phenix.composition.v3".into(),
        artifacts: artifacts.to_vec(),
        definitions: definitions.to_vec(),
        entry: Selection {
            interface: entry_interface.into(),
            operation: entry_operation.into(),
            provider: entry_provider.into(),
        },
        slots: vec![
            binding(
                "phenix.basic.event-journal",
                "Storage",
                "phenix.basic.storage-sqlite",
                "Storage",
            ),
            binding(
                "phenix.advanced.evidence-journal",
                "Journal",
                "phenix.basic.event-journal",
                "Journal",
            ),
        ],
        authority: Authority {
            operations: grants.iter().map(ScopedGrant::key).collect(),
            resources: BTreeSet::from([SCOPE.into()]),
        },
        grants,
        excluded: BTreeSet::new(),
        substitutions: vec![],
        resource: Some(SCOPE.into()),
    }
}

fn invoke(composition: Composition, payload: Value) -> Result<Outcome, phenix_contract::Failure> {
    let kernel = Prepared::prepare(composition)?.load()?;
    let root = kernel.admit_default_root()?;
    root.invoke(&payload)
}

fn actual_event(events: &[Event]) -> &Event {
    events
        .iter()
        .find(|e| e.kind == EventKind::Answer && e.text == "mock:echo:journal-reference-proof")
        .expect("Basic must have durably recorded the answer")
}

fn reference(event: &Event) -> SourceRef {
    let raw = encode(event).unwrap();
    let digest = Sha256::digest(&raw.0);
    SourceRef {
        owner: "phenix.basic.event-journal".into(),
        resource: SCOPE.into(),
        revision: event.sequence.to_string(),
        digest_sha256: format!("{digest:x}"),
        range_start: 0,
        range_end: 0,
        scope: EvidenceScope {
            kind: EvidenceScopeKind::Session,
            id: event.session.clone(),
        },
    }
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    assert_eq!(
        args.len(),
        7,
        "usage: phenix-p2-source-evidence EVIDENCE JOURNAL STORAGE BASIC-DEFS ADV-DEFS OUTPUT"
    );
    let artifacts = vec![args[1].clone(), args[2].clone(), args[3].clone()];
    let definitions = vec![args[4].clone(), args[5].clone()];
    let read = composition(
        "phenix.basic.journal@1",
        "read",
        "phenix.basic.event-journal",
        &artifacts,
        &definitions,
    );
    let query = JournalQuery {
        session: "s1".into(),
        after: 0,
        limit: 128,
    };
    let page = match invoke(read.clone(), query.to_value()).unwrap() {
        Outcome::Success(response) => EventPage::from_value(response).unwrap(),
        result => panic!("actual Basic journal read was not successful: {result:?}"),
    };
    let source = reference(actual_event(&page.events));
    let observe = composition(
        "phenix.advanced.source@1",
        "observe",
        "phenix.advanced.evidence-journal",
        &artifacts,
        &definitions,
    );
    let actual = match invoke(observe.clone(), source.to_value()).unwrap() {
        Outcome::Success(value) => SourceObservation::from_value(value).unwrap(),
        result => panic!("source verifier did not accept the journal reference: {result:?}"),
    };
    assert_eq!(compare_source(&source, &actual), SourceMatch::Current);
    let mut forged = source.clone();
    forged.digest_sha256 = "b".repeat(64);
    let tamper_status = match invoke(observe.clone(), forged.to_value()).unwrap() {
        Outcome::Success(value) => {
            let observed = SourceObservation::from_value(value).unwrap();
            compare_source(&forged, &observed)
        }
        result => panic!("source query should report actual observation: {result:?}"),
    };
    assert_eq!(tamper_status, SourceMatch::Changed);

    let mut foreign = source.clone();
    foreign.scope.id = "s2".into();
    foreign.resource = "phenix.session.s2".into();
    let denied = invoke(observe.clone(), foreign.to_value());
    assert!(
        matches!(
            &denied,
            Ok(Outcome::Domain(_))
                | Err(phenix_contract::Failure {
                    code: phenix_contract::Code::Denied,
                    ..
                })
        ),
        "cross-session source unexpectedly accepted: {denied:?}"
    );

    let mut missing = source.clone();
    missing.revision = (page.events.last().unwrap().sequence + 100).to_string();
    let absent = invoke(observe, missing.to_value());
    assert!(
        matches!(&absent, Ok(Outcome::Domain(_))),
        "missing event was accepted: {absent:?}"
    );

    let unchanged = match invoke(read, query.to_value()).unwrap() {
        Outcome::Success(value) => EventPage::from_value(value).unwrap().events == page.events,
        _ => false,
    };
    assert!(unchanged, "observing source changed the journal");

    let output = Path::new(&args[6]);
    fs::create_dir_all(output).unwrap();
    let report = serde_json::json!({
        "format": "phenix.p2.source-evidence-product.v1",
        "accepted": true,
        "current": true,
        "tampered": "changed",
        "foreign_session": "denied",
        "missing_sequence": "absent",
        "journal_unchanged": unchanged,
        "source_revision": source.revision,
        "source_digest_sha256": source.digest_sha256,
    });
    fs::write(
        output.join("source-evidence.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}
