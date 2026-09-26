use crate::{memory_context_service, memory_factory, memory_manifest, memory_service};
use phenix_core::{Kernel, KernelConfig, LocalPersistence, PhenixValue, Project, ServiceId};
use phenix_sdk::{
    AssociationObservationSource, CandidateCompleteness, ContextAnchor, ContextNeed,
    MemoryAssociationObservation, MemoryCommand, MemoryContextAssociation, MemoryContextCandidate,
    MemoryContextCommand, MemoryContextMatch, MemoryContextRecallRequest, MemoryContextResponse,
    MemoryKind, MemoryRecord, MemoryResponse, MemoryScope, MemorySourceReference, RecallEvidence,
    RecallResolution,
};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn temp_db(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "phenix-memory-context-{name}-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

fn kernel(path: &PathBuf) -> Kernel {
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

fn invoke_memory(kernel: &mut Kernel, command: MemoryCommand) -> MemoryResponse {
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
    MemoryResponse::try_from(Project(&output)).unwrap()
}

fn invoke_context(
    kernel: &mut Kernel,
    command: MemoryContextCommand,
) -> Result<MemoryContextResponse, String> {
    let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
    let output = kernel
        .invoke(
            &memory_context_service(),
            &input,
            &memory_manifest().maximum_authority,
            None,
        )
        .map_err(|error| error.to_string())?;
    let output: PhenixValue = serde_json::from_slice(&output).map_err(|error| error.to_string())?;
    MemoryContextResponse::try_from(Project(&output)).map_err(|error| error.to_string())
}

fn source() -> MemorySourceReference {
    MemorySourceReference {
        service: ServiceId::parse("fixture.history@1").unwrap(),
        resource: "turn/1".into(),
        start: None,
        end: None,
    }
}

fn record() -> MemoryRecord {
    MemoryRecord {
        id: "memory-phenix".into(),
        kind: MemoryKind::Fact,
        scope: MemoryScope::Workspace {
            workspace_id: "phenix".into(),
        },
        content: "Phenix pull request work and repository tasks".into(),
        source_refs: vec![source()],
        supporting_dependencies: Vec::new(),
        supersedes: Vec::new(),
        valid_from: None,
        valid_until: None,
        created_at: 10,
    }
}

fn observation(event_id: &str) -> MemoryAssociationObservation {
    MemoryAssociationObservation {
        event_id: event_id.into(),
        request_id: "request-1".into(),
        source: AssociationObservationSource::RootAdmission,
        association: MemoryContextAssociation {
            memory_id: "memory-phenix".into(),
            anchor: ContextAnchor::Project {
                key: "phenix".into(),
            },
            source_refs: vec![source()],
            observed_at: 20,
        },
    }
}

mod association_persistence {
    use super::*;

    #[test]
    fn observation_deduplication_survives_plugin_restart() {
        let path = temp_db("association-restart");
        {
            let mut kernel = kernel(&path);
            invoke_memory(&mut kernel, MemoryCommand::Record { record: record() });
            let first = invoke_context(
                &mut kernel,
                MemoryContextCommand::Observe {
                    observation: observation("event-1"),
                },
            )
            .unwrap();
            assert!(matches!(
                first,
                MemoryContextResponse::Observed {
                    duplicate: false,
                    ..
                }
            ));
        }

        let mut restored = kernel(&path);
        let duplicate = invoke_context(
            &mut restored,
            MemoryContextCommand::Observe {
                observation: observation("event-1"),
            },
        )
        .unwrap();
        assert!(matches!(
            duplicate,
            MemoryContextResponse::Observed {
                duplicate: true,
                ..
            }
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn observation_requires_existing_memory_and_exact_provenance() {
        let path = temp_db("association-validation");
        let mut kernel = kernel(&path);
        let error = invoke_context(
            &mut kernel,
            MemoryContextCommand::Observe {
                observation: observation("event-missing"),
            },
        )
        .unwrap_err();
        assert!(error.contains("unknown memory"));
        let _ = fs::remove_file(path);
    }
}

mod deterministic_recall {
    use super::*;

    fn seed(kernel: &mut Kernel) {
        invoke_memory(kernel, MemoryCommand::Record { record: record() });
        invoke_context(
            kernel,
            MemoryContextCommand::Observe {
                observation: observation("event-1"),
            },
        )
        .unwrap();
    }

    fn request(known: Vec<ContextAnchor>) -> MemoryContextRecallRequest {
        MemoryContextRecallRequest {
            request_id: "recall-1".into(),
            scopes: vec![MemoryScope::Workspace {
                workspace_id: "phenix".into(),
            }],
            prompt: "work on phenix pull requests".into(),
            known,
            needs: vec![ContextNeed::Project {
                query: "phenix".into(),
            }],
            at: 30,
            limit: 5,
        }
    }

    #[test]
    fn lexical_current_memory_returns_associated_candidate() {
        let path = temp_db("recall-lexical");
        let mut kernel = kernel(&path);
        seed(&mut kernel);
        let response = invoke_context(
            &mut kernel,
            MemoryContextCommand::Recall {
                request: request(Vec::new()),
            },
        )
        .unwrap();
        let MemoryContextResponse::Recall {
            candidates,
            completeness,
        } = response
        else {
            panic!("expected recall response");
        };
        assert_eq!(completeness, CandidateCompleteness::Complete);
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].signals.contains(&MemoryContextMatch::Lexical));
        assert_eq!(candidates[0].evidence_class(), 2);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn conflicting_known_project_filters_candidate_before_ranking() {
        let path = temp_db("recall-anchor-compatibility");
        let mut kernel = kernel(&path);
        seed(&mut kernel);
        let response = invoke_context(
            &mut kernel,
            MemoryContextCommand::Recall {
                request: request(vec![ContextAnchor::Project {
                    key: "other-project".into(),
                }]),
            },
        )
        .unwrap();
        let MemoryContextResponse::Recall { candidates, .. } = response else {
            panic!("expected recall response");
        };
        assert!(candidates.is_empty());
        let _ = fs::remove_file(path);
    }
}

mod resolution_boundary {
    use super::*;

    fn evidence(live_validated: bool) -> RecallEvidence {
        RecallEvidence {
            candidate: MemoryContextCandidate {
                memory_id: "memory-phenix".into(),
                anchor: ContextAnchor::Project {
                    key: "phenix".into(),
                },
                source_refs: vec![source()],
                signals: vec![MemoryContextMatch::Lexical],
                observation_count: 1,
                confirmed_recoveries: 0,
                last_observed_at: 20,
            },
            resolved_needs: vec![ContextNeed::Project {
                query: "phenix".into(),
            }],
            missing_needs: Vec::new(),
            completeness: CandidateCompleteness::Complete,
            query_relevant: true,
            live_validated,
        }
    }

    #[test]
    fn resolver_requires_external_live_validation() {
        let path = temp_db("resolution-live-validation");
        let mut kernel = kernel(&path);
        let not_validated = invoke_context(
            &mut kernel,
            MemoryContextCommand::Resolve {
                evidence: vec![evidence(false)],
            },
        )
        .unwrap();
        assert_eq!(
            not_validated,
            MemoryContextResponse::Resolution {
                resolution: RecallResolution::NotFound,
            }
        );

        let validated = invoke_context(
            &mut kernel,
            MemoryContextCommand::Resolve {
                evidence: vec![evidence(true)],
            },
        )
        .unwrap();
        assert!(matches!(
            validated,
            MemoryContextResponse::Resolution {
                resolution: RecallResolution::Unique { .. }
            }
        ));
        let _ = fs::remove_file(path);
    }
}
