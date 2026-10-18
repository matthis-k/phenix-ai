//! Independent native P2 EvidenceStore acceptance over packaged providers.
//! The production Basic binary seeds the journal in a separate process.
use phenix_advanced_contracts::{
    EvidenceError, EvidenceFreshness, EvidenceKey, EvidenceKind, EvidencePage, EvidenceProblem,
    EvidenceRecord, EvidenceScope, EvidenceScopeKind, EvidenceSearch, SourceRef,
};
use phenix_basic_contracts::{encode, Event, EventKind, EventPage, JournalQuery};
use phenix_contract::{Authority, Portable, Value};
use phenix_kernel::{
    resolve::{Composition, Prepared, ScopedGrant, Selection, SlotBinding},
    Outcome,
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, path::Path};

const SESSION: &str = "s1";
const RESOURCE: &str = "phenix.session.s1";
const FIRST: &str = "mock:echo:The project color was violet";
const SECOND: &str = "mock:echo:The project color is blue";

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
fn selection(interface: &str, operation: &str, provider: &str) -> Selection {
    Selection {
        interface: interface.into(),
        operation: operation.into(),
        provider: provider.into(),
    }
}
fn scope() -> EvidenceScope {
    EvidenceScope {
        kind: EvidenceScopeKind::Session,
        id: SESSION.into(),
    }
}

#[derive(Clone)]
struct Packages {
    artifacts: Vec<String>,
    definitions: Vec<String>,
    resource: String,
}
impl Packages {
    fn invoke(
        &self,
        interface: &str,
        operation: &str,
        provider: &str,
        input: Value,
    ) -> Result<Outcome, phenix_contract::Failure> {
        let mut grants = vec![
            grant("root", "phenix.basic.journal@1", "read"),
            grant("root", "phenix.advanced.evidence@1", "put"),
            grant("root", "phenix.advanced.evidence@1", "get"),
            grant("root", "phenix.advanced.evidence@1", "search"),
            grant(
                "phenix.advanced.evidence-store",
                "phenix.basic.storage@1",
                "read",
            ),
            grant(
                "phenix.advanced.evidence-store",
                "phenix.basic.storage@1",
                "transact",
            ),
            grant(
                "phenix.advanced.evidence-store",
                "phenix.advanced.source@1",
                "observe",
            ),
            grant(
                "phenix.advanced.evidence-journal",
                "phenix.basic.journal@1",
                "read",
            ),
            grant(
                "phenix.basic.event-journal",
                "phenix.basic.storage@1",
                "read",
            ),
        ];
        grants.extend([
            grant(
                "phenix.advanced.evidence-store",
                "phenix.advanced.project-membership@1",
                "get",
            ),
            grant(
                "phenix.advanced.evidence-store",
                "phenix.advanced.project-storage@1",
                "read",
            ),
            grant(
                "phenix.advanced.evidence-store",
                "phenix.advanced.project-storage@1",
                "transact",
            ),
        ]);
        for grant in &mut grants {
            grant.resources = BTreeSet::from([self.resource.clone()]);
        }
        let composition = Composition {
            format: "phenix.composition.v3".into(),
            artifacts: self.artifacts.clone(),
            definitions: self.definitions.clone(),
            entry: selection(interface, operation, provider),
            slots: vec![
                slot(
                    "phenix.advanced.evidence-store",
                    "Storage",
                    "phenix.basic.storage-sqlite",
                    "Storage",
                ),
                slot(
                    "phenix.advanced.evidence-store",
                    "Source",
                    "phenix.advanced.evidence-journal",
                    "Evidence",
                ),
                slot(
                    "phenix.advanced.evidence-journal",
                    "Journal",
                    "phenix.basic.event-journal",
                    "Journal",
                ),
                slot(
                    "phenix.basic.event-journal",
                    "Storage",
                    "phenix.basic.storage-sqlite",
                    "Storage",
                ),
                slot(
                    "phenix.advanced.evidence-store",
                    "ProjectStorage",
                    "phenix.advanced.project-storage-sqlite",
                    "ProjectStorage",
                ),
                slot(
                    "phenix.advanced.evidence-store",
                    "Membership",
                    "phenix.advanced.project-storage-sqlite",
                    "Membership",
                ),
                slot(
                    "phenix.advanced.evidence-store",
                    "ProjectSource",
                    "phenix.advanced.project-storage-sqlite",
                    "ProjectSource",
                ),
            ],
            authority: Authority {
                operations: grants.iter().map(ScopedGrant::key).collect(),
                resources: BTreeSet::from([self.resource.clone()]),
            },
            grants,
            excluded: BTreeSet::new(),
            substitutions: vec![],
            resource: Some(self.resource.clone()),
        };
        let kernel = Prepared::prepare(composition)?.load()?;
        let root = kernel.admit_default_root()?;
        root.invoke(&input)
    }

    fn journal(&self) -> EventPage {
        match self
            .invoke(
                "phenix.basic.journal@1",
                "read",
                "phenix.basic.event-journal",
                JournalQuery {
                    session: SESSION.into(),
                    after: 0,
                    limit: 128,
                }
                .to_value(),
            )
            .unwrap()
        {
            Outcome::Success(value) => EventPage::from_value(value).unwrap(),
            other => panic!("native journal must return real source events: {other:?}"),
        }
    }

    fn evidence(&self, op: &str, request: Value) -> Result<Outcome, phenix_contract::Failure> {
        self.invoke(
            "phenix.advanced.evidence@1",
            op,
            "phenix.advanced.evidence-store",
            request,
        )
    }
    fn success<T: Portable>(&self, op: &str, request: Value) -> T {
        let outcome = self.evidence(op, request).unwrap();
        let Outcome::Success(value) = outcome else {
            panic!("expected successful EvidenceStore {op}: {outcome:?}");
        };
        T::from_value(value).unwrap()
    }
    fn denied(&self, op: &str, request: Value, expected: EvidenceProblem) {
        let actual = self.evidence(op, request).unwrap();
        let Outcome::Domain(value) = actual else {
            panic!("expected typed denial for {op}, got {actual:?}");
        };
        let actual = EvidenceError::from_value(value).unwrap();
        assert_eq!(
            actual.problem, expected,
            "unexpected EvidenceStore {op} denial"
        );
    }
}

fn source(event: &Event) -> SourceRef {
    let digest = Sha256::digest(&encode(event).unwrap().0);
    SourceRef {
        owner: "phenix.basic.event-journal".into(),
        resource: RESOURCE.into(),
        revision: event.sequence.to_string(),
        digest_sha256: format!("{digest:x}"),
        range_start: 0,
        range_end: 0,
        scope: scope(),
    }
}
fn answer<'a>(page: &'a EventPage, text: &str) -> &'a Event {
    page.events
        .iter()
        .find(|e| e.kind == EventKind::Answer && e.text == text)
        .unwrap_or_else(|| panic!("missing authoritative Answer {text}"))
}
fn fact(
    statement: &str,
    source: SourceRef,
    revision: u64,
    from: u64,
    until: Option<u64>,
) -> EvidenceRecord {
    EvidenceRecord {
        id: "fact-project-color".into(),
        scope: scope(),
        kind: EvidenceKind::Fact,
        statement: statement.into(),
        sources: vec![source],
        revision,
        observed_at: 100 + revision,
        valid_from: from,
        valid_until: until,
        confidence_basis_points: 10000,
        freshness: EvidenceFreshness::Current,
        supersedes: vec![],
    }
}

fn write(packages: &Packages, first: SourceRef, second: SourceRef) -> serde_json::Value {
    let initial = fact(
        "The project color was violet",
        first.clone(),
        1,
        10,
        Some(100),
    );
    let revision = packages.success::<EvidenceRecord>("put", initial.to_value());
    assert_eq!(revision.revision, 1);
    let mut unbacked = initial.clone();
    unbacked.id = "unbacked".into();
    unbacked.sources.clear();
    packages.denied("put", unbacked.to_value(), EvidenceProblem::MissingSource);
    let mut missing = initial.clone();
    missing.id = "missing".into();
    missing.sources[0].revision = "999999".into();
    packages.denied("put", missing.to_value(), EvidenceProblem::MissingSource);
    let mut forged = initial.clone();
    forged.id = "forged".into();
    forged.sources[0].digest_sha256 = "b".repeat(64);
    packages.denied("put", forged.to_value(), EvidenceProblem::SourceChanged);
    let mut foreign = initial.clone();
    foreign.id = "foreign".into();
    foreign.scope.id = "s2".into();
    foreign.sources[0].scope.id = "s2".into();
    foreign.sources[0].resource = "phenix.session.s2".into();
    packages.denied("put", foreign.to_value(), EvidenceProblem::ScopeDenied);

    let corrected = fact("The project color is blue", second, 2, 100, None);
    let accepted = packages.success::<EvidenceRecord>("put", corrected.to_value());
    assert_eq!(accepted.revision, 2);
    packages.denied("put", initial.to_value(), EvidenceProblem::Conflict);
    let saved = packages.success::<EvidenceRecord>(
        "get",
        EvidenceKey {
            id: "fact-project-color".into(),
            scope: scope(),
        }
        .to_value(),
    );
    assert_eq!(saved, corrected);
    serde_json::json!({
        "accepted": true,
        "source_verified": true,
        "created_revision": revision.revision,
        "corrected_revision": accepted.revision,
        "stale_cas": "conflict",
        "tampered_source": "changed",
        "missing_source": "rejected",
        "unbacked_claim": "rejected",
        "cross_session": "denied",
        "source_digest_sha256": first.digest_sha256,
    })
}

fn restart(packages: &Packages, first: SourceRef) -> serde_json::Value {
    let current = packages.success::<EvidenceRecord>(
        "get",
        EvidenceKey {
            id: "fact-project-color".into(),
            scope: scope(),
        }
        .to_value(),
    );
    assert_eq!(current.revision, 2);
    let query = |as_of| {
        EvidenceSearch {
            scopes: vec![scope()],
            query: "The project color".into(),
            as_of,
            limit: 4,
        }
        .to_value()
    };
    let active = packages.success::<EvidencePage>("search", query(None));
    assert_eq!(active.records.len(), 1);
    assert_eq!(active.records[0], current);
    let historical = packages.success::<EvidencePage>("search", query(Some(50)));
    assert_eq!(historical.records.len(), 1);
    assert_eq!(
        historical.records[0].statement,
        "The project color was violet"
    );
    let mut foreign = scope();
    foreign.id = "s2".into();
    packages.denied(
        "get",
        EvidenceKey {
            id: "fact-project-color".into(),
            scope: foreign,
        }
        .to_value(),
        EvidenceProblem::ScopeDenied,
    );
    serde_json::json!({
        "accepted": true,
        "retrieved_revision": current.revision,
        "current_statement": current.statement,
        "historical_statement": historical.records[0].statement,
        "cross_session": "denied",
        "source_digest_sha256": first.digest_sha256,
    })
}

fn project_scope() -> EvidenceScope {
    EvidenceScope {
        kind: EvidenceScopeKind::Project,
        id: "alpha".into(),
    }
}
fn project_fact() -> EvidenceRecord {
    let digest = Sha256::digest(b"blue");
    let source = SourceRef {
        owner: "phenix.advanced.project-storage-sqlite".into(),
        resource: "phenix.project.alpha".into(),
        revision: "theme@1".into(),
        digest_sha256: format!("{digest:x}"),
        range_start: 0,
        range_end: 0,
        scope: project_scope(),
    };
    let mut record = fact("The project color is blue", source, 1, 1, None);
    record.scope = project_scope();
    record
}
fn project_write(packages: &Packages) -> serde_json::Value {
    let record = project_fact();
    assert_eq!(
        packages.success::<EvidenceRecord>("put", record.to_value()),
        record
    );
    serde_json::json!({"source_verified": true, "stored_by_session": "s1"})
}
fn project_recall(packages: &Packages) -> serde_json::Value {
    let key = EvidenceKey {
        id: "fact-project-color".into(),
        scope: project_scope(),
    };
    let record = packages.success::<EvidenceRecord>("get", key.to_value());
    assert_eq!(record.statement, "The project color is blue");
    let page = packages.success::<EvidencePage>(
        "search",
        EvidenceSearch {
            scopes: vec![project_scope()],
            query: "project color".into(),
            as_of: None,
            limit: 5,
        }
        .to_value(),
    );
    assert_eq!(page.records, vec![record.clone()]);
    let stranger = Packages {
        resource: "phenix.session.s3".into(),
        ..packages.clone()
    };
    stranger.denied("get", key.to_value(), EvidenceProblem::ScopeDenied);
    stranger.denied(
        "search",
        EvidenceSearch {
            scopes: vec![project_scope()],
            query: "project color".into(),
            as_of: None,
            limit: 5,
        }
        .to_value(),
        EvidenceProblem::ScopeDenied,
    );
    stranger.denied(
        "put",
        project_fact().to_value(),
        EvidenceProblem::ScopeDenied,
    );
    let mut foreign = key.clone();
    foreign.scope.id = "beta".into();
    packages.denied("get", foreign.to_value(), EvidenceProblem::ScopeDenied);
    packages.denied(
        "search",
        EvidenceSearch {
            scopes: vec![foreign.scope.clone()],
            query: "project color".into(),
            as_of: None,
            limit: 5,
        }
        .to_value(),
        EvidenceProblem::ScopeDenied,
    );
    let mut foreign_fact = project_fact();
    foreign_fact.scope.id = "beta".into();
    packages.denied("put", foreign_fact.to_value(), EvidenceProblem::ScopeDenied);
    assert_eq!(
        packages.success::<EvidenceRecord>("get", key.to_value()),
        record
    );
    serde_json::json!({"reader": "s2", "statement": record.statement,
        "unapproved_session_denied": true, "unapproved_search_denied": true,
        "unapproved_write_denied": true, "cross_project_denied": true,
        "cross_project_search_denied": true, "cross_project_write_denied": true})
}
fn project_stale(packages: &Packages) -> serde_json::Value {
    let key = EvidenceKey {
        id: "fact-project-color".into(),
        scope: project_scope(),
    };
    packages.denied("get", key.to_value(), EvidenceProblem::SourceChanged);
    packages.denied(
        "search",
        EvidenceSearch {
            scopes: vec![project_scope()],
            query: "project color".into(),
            as_of: None,
            limit: 5,
        }
        .to_value(),
        EvidenceProblem::SourceChanged,
    );
    let mut correction = project_fact();
    correction.revision = 2;
    correction.observed_at += 1;
    correction.statement = "The project color is no longer blue".into();
    packages.denied("put", correction.to_value(), EvidenceProblem::SourceChanged);
    serde_json::json!({"source_changed": true, "stale_search_denied": true,
        "stale_write_denied": true})
}
fn project_revoked(packages: &Packages) -> serde_json::Value {
    let key = EvidenceKey {
        id: "fact-project-color".into(),
        scope: project_scope(),
    };
    packages.denied("get", key.to_value(), EvidenceProblem::ScopeDenied);
    packages.denied(
        "search",
        EvidenceSearch {
            scopes: vec![project_scope()],
            query: "project color".into(),
            as_of: None,
            limit: 5,
        }
        .to_value(),
        EvidenceProblem::ScopeDenied,
    );
    packages.denied(
        "put",
        project_fact().to_value(),
        EvidenceProblem::ScopeDenied,
    );
    let retained = Packages {
        resource: RESOURCE.into(),
        ..packages.clone()
    };
    retained.denied("get", key.to_value(), EvidenceProblem::SourceChanged);
    serde_json::json!({"revoked_session_denied": true, "revoked_search_denied": true,
        "revoked_write_denied": true, "retained_session_source_changed": true})
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 10, "usage: phenix-p2-evidence-store STORE SOURCE JOURNAL SQLITE BASIC-DEFS ADV-DEFS PROJECT-STORAGE ACTION OUTPUT");
    let action = args[8].as_str();
    let resource = if action.starts_with("project-") && action != "project-write" {
        "phenix.session.s2"
    } else {
        RESOURCE
    };
    let packages = Packages {
        artifacts: vec![
            args[1].clone(),
            args[2].clone(),
            args[3].clone(),
            args[4].clone(),
            args[7].clone(),
        ],
        definitions: args[5..=6].to_vec(),
        resource: resource.into(),
    };
    let report = match action {
        "project-write" => project_write(&packages),
        "project-recall" => project_recall(&packages),
        "project-stale" => project_stale(&packages),
        "project-revoked" => project_revoked(&packages),
        "write" | "restart" => {
            let page = packages.journal();
            let first = source(answer(&page, FIRST));
            let second = source(answer(&page, SECOND));
            if action == "write" {
                write(&packages, first, second)
            } else {
                restart(&packages, first)
            }
        }
        other => panic!("unknown product action: {other}"),
    };
    let output = Path::new(&args[9]);
    fs::create_dir_all(output).unwrap();
    fs::write(
        output.join("evidence-store.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}
