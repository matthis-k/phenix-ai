use crate::{
    implementation::{RECALL_EVENT, REVALIDATION_EVENT},
    memory_factory, memory_manifest,
};
use phenix_core::{
    Authority, EventEnvelope, EventFailurePolicy, EventSubscription, EventTypeId, Kernel,
    KernelConfig, LocalPersistence, PhenixValue, RoutingProfileId, ServiceId, SessionId,
    SubscriptionId, SubscriptionSpec,
};
use phenix_sdk::{
    memory_service, MemoryCommand, MemoryExpansion, MemoryKind, MemoryNode, MemoryRecallQuery,
    MemoryRecord, MemoryResponse, MemoryScope, MemorySourceReference,
};
use std::{
    fs,
    path::PathBuf,
    sync::{mpsc, Arc},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn event_subscription<F>(id: &str, event: &str, handler: F) -> EventSubscription
where
    F: Fn(&EventEnvelope, &Authority) -> Result<(), String> + Send + Sync + 'static,
{
    EventSubscription {
        spec: SubscriptionSpec {
            id: SubscriptionId::parse(id).unwrap(),
            owner: memory_manifest().id,
            event_type: EventTypeId::parse(event).unwrap(),
            event_version: 1,
            dependencies: Vec::new(),
            failure_policy: EventFailurePolicy::Ignore,
            required_authority: Authority::default(),
            maximum_authority: Authority::default(),
            kernel_policy_revision: 0,
        },
        handler: Arc::new(handler),
    }
}

fn temp_db(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "phenix-memory-{name}-{}-{nonce}.sqlite",
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

fn invoke(kernel: &mut Kernel, command: MemoryCommand) -> Result<MemoryResponse, String> {
    let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
    let output = kernel
        .invoke(
            &memory_service(),
            &input,
            &memory_manifest().maximum_authority,
            None,
        )
        .map_err(|error| error.to_string())?;
    let output: PhenixValue = serde_json::from_slice(&output).map_err(|error| error.to_string())?;
    output.project().map_err(|error| error.to_string())
}

fn scope(session: &str) -> MemoryScope {
    MemoryScope::Session {
        session_id: SessionId::parse(session).unwrap(),
    }
}

fn source(resource: &str) -> MemorySourceReference {
    MemorySourceReference {
        service: ServiceId::parse("fixture.history@1").unwrap(),
        resource: resource.into(),
        start: None,
        end: None,
    }
}

fn fact(id: &str, session: &str, content: &str, created_at: u64) -> MemoryRecord {
    MemoryRecord {
        id: id.into(),
        kind: MemoryKind::Fact,
        scope: scope(session),
        content: content.into(),
        source_refs: vec![source(&format!("turn/{id}"))],
        supersedes: Vec::new(),
        valid_from: None,
        valid_until: None,
        created_at,
    }
}

#[test]
fn durable_fact_supersession_preserves_historical_recall_and_restores() {
    let path = temp_db("supersession");
    {
        let mut kernel = kernel_with(&path);
        let old = fact("transport-a", "root", "Use transport A for clients", 10);
        invoke(
            &mut kernel,
            MemoryCommand::Record {
                record: old.clone(),
            },
        )
        .unwrap();

        let mut new = fact("transport-b", "root", "Use transport B for clients", 20);
        new.supersedes.push(old.id.clone());
        invoke(
            &mut kernel,
            MemoryCommand::Record {
                record: new.clone(),
            },
        )
        .unwrap();

        let historical = invoke(
            &mut kernel,
            MemoryCommand::Recall {
                query: MemoryRecallQuery {
                    scopes: vec![scope("root")],
                    kinds: vec![MemoryKind::Fact],
                    query: "transport".into(),
                    at: 15,
                    limit: 10,
                },
            },
        )
        .unwrap();
        assert_eq!(historical, MemoryResponse::Recall { records: vec![old] });

        let current = invoke(
            &mut kernel,
            MemoryCommand::Recall {
                query: MemoryRecallQuery {
                    scopes: vec![scope("root")],
                    kinds: vec![MemoryKind::Fact],
                    query: "transport".into(),
                    at: 25,
                    limit: 10,
                },
            },
        )
        .unwrap();
        assert_eq!(current, MemoryResponse::Recall { records: vec![new] });
    }

    {
        let mut restored = kernel_with(&path);
        let response = invoke(
            &mut restored,
            MemoryCommand::Get {
                id: "transport-b".into(),
            },
        )
        .unwrap();
        assert!(matches!(
            response,
            MemoryResponse::Memory { record: Some(record) }
                if record.id == "transport-b" && record.supersedes == vec!["transport-a"]
        ));
    }
    let _ = fs::remove_file(path);
}

#[test]
fn hierarchy_expands_existing_same_scope_children_after_restore() {
    let path = temp_db("hierarchy");
    let leaf = MemoryNode {
        id: "leaf".into(),
        scope: scope("root"),
        summary: "Exact transport decision".into(),
        children: Vec::new(),
        source_refs: vec![source("turn/42")],
        created_at: 42,
        generation: 1,
    };
    let parent = MemoryNode {
        id: "parent".into(),
        scope: scope("root"),
        summary: "Transport decisions".into(),
        children: vec![leaf.id.clone()],
        source_refs: Vec::new(),
        created_at: 43,
        generation: 2,
    };

    {
        let mut kernel = kernel_with(&path);
        invoke(
            &mut kernel,
            MemoryCommand::RecordNode { node: leaf.clone() },
        )
        .unwrap();
        invoke(
            &mut kernel,
            MemoryCommand::RecordNode {
                node: parent.clone(),
            },
        )
        .unwrap();
    }

    {
        let mut restored = kernel_with(&path);
        let response = invoke(
            &mut restored,
            MemoryCommand::ExpandNode {
                id: parent.id.clone(),
            },
        )
        .unwrap();
        assert_eq!(
            response,
            MemoryResponse::Expansion {
                expansion: Some(MemoryExpansion {
                    node: parent,
                    children: vec![leaf],
                }),
            }
        );
    }
    let _ = fs::remove_file(path);
}

#[test]
fn recall_filters_scope_before_lexical_matching() {
    let path = temp_db("scope");
    let mut kernel = kernel_with(&path);
    invoke(
        &mut kernel,
        MemoryCommand::Record {
            record: fact("root-note", "root", "shared keyword root", 10),
        },
    )
    .unwrap();
    invoke(
        &mut kernel,
        MemoryCommand::Record {
            record: fact("child-note", "child", "shared keyword child", 11),
        },
    )
    .unwrap();

    let response = invoke(
        &mut kernel,
        MemoryCommand::Recall {
            query: MemoryRecallQuery {
                scopes: vec![scope("root")],
                kinds: Vec::new(),
                query: "shared keyword".into(),
                at: 20,
                limit: 10,
            },
        },
    )
    .unwrap();
    assert!(matches!(
        response,
        MemoryResponse::Recall { records }
            if records.len() == 1 && records[0].id == "root-note"
    ));
    let _ = fs::remove_file(path);
}

#[test]
fn context_expansion_service_routes_to_checkpoint_lookup() {
    let path = temp_db("expansion-service");
    let mut kernel = kernel_with(&path);
    let command = phenix_sdk::ContextExpansionCommand::Expand {
        scope: scope("root"),
        checkpoint_id: "missing".into(),
        depth: 1,
    };
    let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
    let error = kernel
        .invoke(
            &phenix_sdk::context_expansion_service(),
            &input,
            &memory_manifest().maximum_authority,
            None,
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("missing memory value: context checkpoint missing"));
    let _ = fs::remove_file(path);
}

#[test]
fn promotion_is_an_explicit_provenance_preserving_state_transition() {
    let path = temp_db("promotion");
    let mut kernel = kernel_with(&path);
    let source_record = fact("session-fact", "root", "stable preference", 10);
    invoke(
        &mut kernel,
        MemoryCommand::Record {
            record: source_record.clone(),
        },
    )
    .unwrap();

    let response = invoke(
        &mut kernel,
        MemoryCommand::Promote {
            id: source_record.id.clone(),
            promoted_id: "global-fact".into(),
            scope: MemoryScope::Global,
            created_at: 20,
        },
    )
    .unwrap();
    let MemoryResponse::Record { record: promoted } = response else {
        panic!("promotion must return the promoted memory");
    };
    assert_eq!(promoted.scope, MemoryScope::Global);
    assert_eq!(promoted.content, source_record.content);
    assert_eq!(promoted.source_refs, source_record.source_refs);

    let original = invoke(
        &mut kernel,
        MemoryCommand::Get {
            id: source_record.id,
        },
    )
    .unwrap();
    assert!(matches!(
        original,
        MemoryResponse::Memory { record: Some(record) }
            if matches!(record.scope, MemoryScope::Session { .. })
    ));
    let _ = fs::remove_file(path);
}

#[test]
fn promotion_rejects_session_scoped_targets() {
    let path = temp_db("promotion-session-target");
    let mut kernel = kernel_with(&path);
    let source_record = fact("session-fact", "root", "stable preference", 10);
    invoke(
        &mut kernel,
        MemoryCommand::Record {
            record: source_record.clone(),
        },
    )
    .unwrap();

    let error = invoke(
        &mut kernel,
        MemoryCommand::Promote {
            id: source_record.id,
            promoted_id: "same-session-fact".into(),
            scope: MemoryScope::Session {
                session_id: SessionId::parse("root").unwrap(),
            },
            created_at: 20,
        },
    )
    .unwrap_err();
    assert!(error.contains("promotion target must outlive the source session"));
    let _ = fs::remove_file(path);
}

#[test]
fn compaction_rejects_summary_only_provenance() {
    let path = temp_db("summary-only-provenance");
    let mut kernel = kernel_with(&path);
    let command = phenix_sdk::ContextCompactionCommand::Compact {
        request: phenix_sdk::ContextCompactionRequest {
            execution_id: "execution-1".into(),
            parent_attempt_id: "attempt-1".into(),
            scope: scope("root"),
            profile_id: phenix_core::RoutingProfileId::parse("memory").unwrap(),
            configuration_revision: "config-1".into(),
            target_tokens: 128,
            items: vec![phenix_sdk::CompactContextItem {
                id: "prior-summary".into(),
                content: "already compacted summary".into(),
                source_refs: vec![MemorySourceReference {
                    service: phenix_sdk::context_compaction_service(),
                    resource: "checkpoint/prior".into(),
                    start: None,
                    end: None,
                }],
                exact: false,
            }],
        },
    };
    let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
    let error = kernel
        .invoke(
            &phenix_sdk::context_compaction_service(),
            &input,
            &memory_manifest().maximum_authority,
            None,
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("must reference raw durable evidence, not derived memory"));
    let _ = fs::remove_file(path);
}

#[test]
fn stop_and_reactivate_preserves_memory_state() {
    let path = temp_db("stop-reactivate");
    let mut kernel = kernel_with(&path);
    let plugin = memory_manifest().id;
    let record = fact("durable-memory", "root", "survives plugin restart", 10);
    invoke(
        &mut kernel,
        MemoryCommand::Record {
            record: record.clone(),
        },
    )
    .unwrap();

    kernel.stop(&plugin).unwrap();
    kernel.activate_all().unwrap();

    assert_eq!(
        invoke(
            &mut kernel,
            MemoryCommand::Get {
                id: record.id.clone()
            }
        )
        .unwrap(),
        MemoryResponse::Memory {
            record: Some(record)
        }
    );
    let _ = fs::remove_file(path);
}

#[test]
fn recall_and_revalidation_emit_observable_operation_events() {
    let (recall_observer, recall_events) = mpsc::channel();
    let (revalidation_observer, revalidation_events) = mpsc::channel();

    let path = temp_db("operation-events");
    let mut kernel = kernel_with(&path);
    kernel
        .events()
        .replace_subscriptions([
            event_subscription("memory-recall-observer", RECALL_EVENT, move |_, _| {
                recall_observer.send(()).map_err(|error| error.to_string())
            }),
            event_subscription(
                "memory-revalidation-observer",
                REVALIDATION_EVENT,
                move |_, _| {
                    revalidation_observer
                        .send(())
                        .map_err(|error| error.to_string())
                },
            ),
        ])
        .unwrap();

    invoke(
        &mut kernel,
        MemoryCommand::Record {
            record: fact("observed-fact", "root", "observable memory", 10),
        },
    )
    .unwrap();
    invoke(
        &mut kernel,
        MemoryCommand::Recall {
            query: MemoryRecallQuery {
                scopes: vec![scope("root")],
                kinds: vec![MemoryKind::Fact],
                query: "observable".into(),
                at: 11,
                limit: 10,
            },
        },
    )
    .unwrap();
    invoke(
        &mut kernel,
        MemoryCommand::Revalidate {
            id: "observed-fact".into(),
            execution_id: "execution-1".into(),
            parent_attempt_id: "attempt-1".into(),
            profile_id: RoutingProfileId::parse("memory").unwrap(),
            at: 11,
        },
    )
    .unwrap();

    recall_events.recv_timeout(Duration::from_secs(1)).unwrap();
    revalidation_events
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    let _ = fs::remove_file(path);
}
