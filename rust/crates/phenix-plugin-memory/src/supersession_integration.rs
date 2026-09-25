use crate::{memory_factory, memory_manifest};
use phenix_core::{Kernel, KernelConfig, LocalPersistence, PhenixValue, ServiceId, SessionId};
use phenix_sdk::{
    memory_service, MemoryCommand, MemoryFreshness, MemoryKind, MemoryRecallQuery, MemoryRecord,
    MemoryResponse, MemoryScope, MemorySourceReference,
};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn temp_db() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "phenix-memory-supersession-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

fn kernel_with(path: &PathBuf) -> Kernel {
    let manifest = memory_manifest();
    let plugin = manifest.id.clone();
    let persistence = LocalPersistence::open(path).unwrap();
    let mut kernel = Kernel::with_persistence(KernelConfig::new([manifest]).unwrap(), persistence);
    kernel
        .register_embedded_factory(plugin, memory_factory)
        .unwrap();
    kernel.activate_all().unwrap();
    kernel
}

fn invoke(kernel: &mut Kernel, command: MemoryCommand) -> MemoryResponse {
    let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
    let output = kernel
        .invoke(
            &memory_service(),
            &input,
            &memory_manifest().maximum_authority,
            None,
        )
        .unwrap();
    let output: PhenixValue = serde_json::from_slice(&output).unwrap();
    output.project().unwrap()
}

fn scope() -> MemoryScope {
    MemoryScope::Session {
        session_id: SessionId::parse("root").unwrap(),
    }
}

fn record(id: &str, content: &str, created_at: u64) -> MemoryRecord {
    MemoryRecord {
        id: id.into(),
        kind: MemoryKind::Fact,
        scope: scope(),
        content: content.into(),
        source_refs: vec![MemorySourceReference {
            service: ServiceId::parse("fixture.history@1").unwrap(),
            resource: format!("turn/{id}"),
            start: None,
            end: None,
        }],
        supporting_dependencies: Vec::new(),
        supersedes: Vec::new(),
        valid_from: None,
        valid_until: None,
        created_at,
    }
}

#[test]
fn supersession_moves_prior_memory_to_historical_without_losing_past_recall() {
    let path = temp_db();
    let mut kernel = kernel_with(&path);
    let prior = record("prior", "old fact", 10);
    invoke(
        &mut kernel,
        MemoryCommand::Record {
            record: prior.clone(),
        },
    );

    let mut replacement = record("replacement", "new fact", 20);
    replacement.supersedes.push(prior.id.clone());
    invoke(
        &mut kernel,
        MemoryCommand::Record {
            record: replacement.clone(),
        },
    );

    let freshness = invoke(
        &mut kernel,
        MemoryCommand::GetFreshness {
            id: prior.id.clone(),
        },
    );
    assert!(matches!(
        freshness,
        MemoryResponse::Freshness { state: Some(state) }
            if state.freshness == MemoryFreshness::Historical && state.changed_at == 20
    ));

    let current = invoke(
        &mut kernel,
        MemoryCommand::Recall {
            query: MemoryRecallQuery {
                scopes: vec![scope()],
                kinds: vec![MemoryKind::Fact],
                query: "fact".into(),
                at: 25,
                limit: 10,
            },
        },
    );
    assert_eq!(
        current,
        MemoryResponse::Recall {
            records: vec![replacement]
        }
    );

    let historical = invoke(
        &mut kernel,
        MemoryCommand::Recall {
            query: MemoryRecallQuery {
                scopes: vec![scope()],
                kinds: vec![MemoryKind::Fact],
                query: "old".into(),
                at: 15,
                limit: 10,
            },
        },
    );
    assert_eq!(
        historical,
        MemoryResponse::Recall {
            records: vec![prior]
        }
    );

    let _ = fs::remove_file(path);
}

#[test]
fn supersession_starts_when_the_replacement_becomes_valid() {
    let path = temp_db();
    let mut kernel = kernel_with(&path);
    let prior = record("prior-delayed", "old delayed fact", 10);
    invoke(
        &mut kernel,
        MemoryCommand::Record {
            record: prior.clone(),
        },
    );

    let mut replacement = record("replacement-delayed", "new delayed fact", 20);
    replacement.valid_from = Some(30);
    replacement.supersedes.push(prior.id.clone());
    invoke(
        &mut kernel,
        MemoryCommand::Record {
            record: replacement.clone(),
        },
    );

    let freshness = invoke(
        &mut kernel,
        MemoryCommand::GetFreshness {
            id: prior.id.clone(),
        },
    );
    assert!(matches!(
        freshness,
        MemoryResponse::Freshness { state: Some(state) }
            if state.freshness == MemoryFreshness::Historical && state.changed_at == 30
    ));

    let before_replacement = invoke(
        &mut kernel,
        MemoryCommand::Recall {
            query: MemoryRecallQuery {
                scopes: vec![scope()],
                kinds: vec![MemoryKind::Fact],
                query: "delayed".into(),
                at: 25,
                limit: 10,
            },
        },
    );
    assert_eq!(
        before_replacement,
        MemoryResponse::Recall {
            records: vec![prior]
        }
    );

    let after_replacement = invoke(
        &mut kernel,
        MemoryCommand::Recall {
            query: MemoryRecallQuery {
                scopes: vec![scope()],
                kinds: vec![MemoryKind::Fact],
                query: "delayed".into(),
                at: 30,
                limit: 10,
            },
        },
    );
    assert_eq!(
        after_replacement,
        MemoryResponse::Recall {
            records: vec![replacement]
        }
    );

    let _ = fs::remove_file(path);
}

#[test]
fn supersession_does_not_revive_memory_that_was_already_non_current() {
    let path = temp_db();
    let mut kernel = kernel_with(&path);
    let prior = record("prior-stale", "stale old fact", 10);
    invoke(
        &mut kernel,
        MemoryCommand::Record {
            record: prior.clone(),
        },
    );
    invoke(
        &mut kernel,
        MemoryCommand::ObserveRevision {
            service: ServiceId::parse("fixture.history@1").unwrap(),
            resource: "turn/prior-stale".into(),
            revision: "rev-2".into(),
            observed_at: 15,
            limit: 10,
        },
    );

    let mut replacement = record("replacement-stale", "replacement fact", 20);
    replacement.supersedes.push(prior.id.clone());
    invoke(
        &mut kernel,
        MemoryCommand::Record {
            record: replacement,
        },
    );

    let freshness = invoke(
        &mut kernel,
        MemoryCommand::GetFreshness {
            id: prior.id.clone(),
        },
    );
    assert!(matches!(
        freshness,
        MemoryResponse::Freshness { state: Some(state) }
            if state.freshness == MemoryFreshness::Historical && state.changed_at == 15
    ));

    let between_transitions = invoke(
        &mut kernel,
        MemoryCommand::Recall {
            query: MemoryRecallQuery {
                scopes: vec![scope()],
                kinds: vec![MemoryKind::Fact],
                query: "stale".into(),
                at: 17,
                limit: 10,
            },
        },
    );
    assert_eq!(
        between_transitions,
        MemoryResponse::Recall {
            records: Vec::new()
        }
    );

    let _ = fs::remove_file(path);
}
