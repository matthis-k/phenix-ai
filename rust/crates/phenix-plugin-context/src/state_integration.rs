use crate::{context_component_manifest, context_factory, context_manifest};
use phenix_core::{
    Authority, Bytes, ContextResourceId, Kernel, KernelConfig, LocalPersistence, PhenixValue,
    PluginState, Project, ResolvedHarness,
};
use phenix_plugin_execution::{
    execution_component_manifest, execution_factory, execution_manifest,
};
use phenix_sdk::{
    context_service, execution_service, BudgetReservation, CachePlacement, CompactionProposal,
    ContextAdmissionRequest, ContextCandidate, ContextCheckpoint, ContextCommand, ContextDemand,
    ContextInjectionLifetime, ContextInjectionRequester, ContextProjectionForm,
    ContextResourceKind, ContextResponse, ContextRetention, ContextScope, ContextSource,
    DelegationResourcePolicy, ExecutionAuthority, ExecutionCommand, ProjectionRevision,
    ReasoningBudget, RetentionTransition, RetryBudget, RoutingRequirements, SkillProvisionBudget,
    StepPlan, ToolCallGroupReference, ToolProvisionBudget,
};
use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn authority() -> Authority {
    context_manifest().maximum_authority
}

fn temp_db(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "phenix-context-state-{name}-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

fn kernel(path: &PathBuf) -> Kernel {
    let context_manifest = context_manifest();
    let context_plugin = context_manifest.id.clone();
    let execution_manifest = execution_manifest(authority());
    let execution_plugin = execution_manifest.id.clone();
    let resolved = ResolvedHarness::resolve(
        [execution_manifest.clone(), context_manifest.clone()],
        [
            execution_component_manifest(authority()),
            context_component_manifest(),
        ],
        [],
        &authority(),
    )
    .unwrap();
    let persistence = LocalPersistence::open(path).unwrap();
    let mut kernel = Kernel::with_persistence(
        KernelConfig::new([execution_manifest, context_manifest]).unwrap(),
        persistence,
    );
    kernel.activate_resolved_harness(&resolved).unwrap();
    kernel
        .register_embedded_factory(execution_plugin.clone(), execution_factory)
        .unwrap();
    kernel
        .register_embedded_factory(context_plugin.clone(), context_factory)
        .unwrap();
    kernel.activate_all().unwrap();
    assert_eq!(kernel.state(&execution_plugin), Some(PluginState::Active));
    assert_eq!(kernel.state(&context_plugin), Some(PluginState::Active));
    kernel
}

fn invoke(kernel: &mut Kernel, command: ContextCommand) -> Result<ContextResponse, String> {
    let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
    let output = kernel
        .invoke(&context_service(), &input, &authority(), None)
        .map_err(|error| error.to_string())?;
    let output: PhenixValue = serde_json::from_slice(&output).map_err(|error| error.to_string())?;
    ContextResponse::try_from(Project(&output)).map_err(|error| error.to_string())
}

fn create_execution(kernel: &mut Kernel, id: &str) {
    let command = ExecutionCommand::CreateExecution {
        id: id.into(),
        requested_authority: ExecutionAuthority::new(Vec::<String>::new()),
    };
    kernel
        .invoke(
            &execution_service(),
            &serde_json::to_vec(&PhenixValue::from(&command)).unwrap(),
            &authority(),
            None,
        )
        .unwrap();
}

fn plan(input_tokens: u64) -> StepPlan {
    let context = ContextDemand {
        mandatory_input_tokens: input_tokens,
        reducible_input_tokens: 0,
        output_reserve_tokens: 128,
        required_capabilities: BTreeSet::new(),
    };
    StepPlan {
        policy_revision: "policy-1".into(),
        routing: RoutingRequirements {
            context: context.clone(),
            required_capabilities: BTreeSet::new(),
            require_known_capacity: false,
        },
        context,
        reasoning: ReasoningBudget::BackendDefault,
        tools: ToolProvisionBudget {
            initial: BTreeSet::new(),
            expandable: BTreeSet::new(),
            max_schemas: 0,
            max_result_bytes: 0,
        },
        skills: SkillProvisionBudget {
            initial: BTreeSet::new(),
            expandable: BTreeSet::new(),
            max_loaded: 0,
        },
        delegation: DelegationResourcePolicy::default(),
        retry: RetryBudget {
            max_attempts: 1,
            reserved_attempts: 1,
        },
        reservation: BudgetReservation {
            input_tokens,
            output_tokens: 128,
            cost_microunits: None,
        },
        deadline_at_ms: None,
        reducible_input_dropped_tokens: 0,
    }
}

fn admission(execution_id: &str) -> ContextAdmissionRequest {
    ContextAdmissionRequest {
        execution_id: execution_id.into(),
        step_plan: plan(100),
        candidates: vec![ContextCandidate {
            id: "item-1".into(),
            source: ContextSource::Inline {
                identity: "item-1".into(),
            },
            content_identity: "sha256:item-1".into(),
            content: Bytes::from(b"important context".to_vec()),
            estimated_tokens: 100,
            mandatory: true,
            retention: ContextRetention::Full,
            cache: CachePlacement::Epoch,
            recovery: None,
        }],
        cache_epoch: 1,
    }
}

fn proposal(execution_id: &str, checkpoint: &str) -> CompactionProposal {
    let revision = ProjectionRevision {
        revision: 1,
        cache_epoch: 1,
    };
    CompactionProposal {
        execution_id: execution_id.into(),
        expected_projection: revision.clone(),
        next_cache_epoch: 2,
        transitions: vec![RetentionTransition {
            item_id: "item-1".into(),
            from: ContextRetention::Full,
            to: ContextRetention::DropAllowed,
            recovery: None,
        }],
        checkpoint: ContextCheckpoint {
            checkpoint_id: checkpoint.into(),
            execution_id: execution_id.into(),
            source_revision: revision,
            content_identity: "sha256:checkpoint".into(),
            compact_view: Bytes::from(b"summary".to_vec()),
            exact_sources: Vec::new(),
            tool_groups: Vec::<ToolCallGroupReference>::new(),
        },
    }
}

mod legacy_resources {
    use super::*;

    #[test]
    fn resource_load_and_projection_still_use_exact_durable_revision() {
        let path = temp_db("legacy-resource");
        let mut kernel = kernel(&path);
        create_execution(&mut kernel, "exec-1");
        let registered = invoke(
            &mut kernel,
            ContextCommand::Register {
                resource_id: ContextResourceId::parse("skill:review").unwrap(),
                kind: ContextResourceKind::Skill,
                source: "skills/review/SKILL.md".into(),
                scope: ContextScope::Workspace,
                content: b"review exactly".to_vec().into(),
            },
        )
        .unwrap();
        let ContextResponse::Registered { resource } = registered else {
            panic!("expected registered resource");
        };
        invoke(
            &mut kernel,
            ContextCommand::Load {
                execution_id: "exec-1".into(),
                resource_id: resource.descriptor.resource_id.clone(),
                revision: resource.descriptor.revision.clone(),
                requester: ContextInjectionRequester::User,
                lifetime: ContextInjectionLifetime::Execution,
                reason: "explicit".into(),
            },
        )
        .unwrap();
        let projected = invoke(
            &mut kernel,
            ContextCommand::Project {
                execution_id: "exec-1".into(),
            },
        )
        .unwrap();
        let ContextResponse::Projection { projection } = projected else {
            panic!("expected projection");
        };
        assert_eq!(projection.entries.len(), 1);
        assert_eq!(projection.entries[0].resource, resource);
        let _ = fs::remove_file(path);
    }
}

mod admission_restart {
    use super::*;

    #[test]
    fn admitted_projection_survives_restart_and_accepts_matching_cas() {
        let path = temp_db("admission-restart");
        {
            let mut kernel = kernel(&path);
            create_execution(&mut kernel, "exec-1");
            assert!(matches!(
                invoke(
                    &mut kernel,
                    ContextCommand::Admit {
                        request: admission("exec-1"),
                    },
                )
                .unwrap(),
                ContextResponse::Admission { .. }
            ));
        }
        let mut restored = kernel(&path);
        let prepared = invoke(
            &mut restored,
            ContextCommand::PrepareCompaction {
                proposal: proposal("exec-1", "checkpoint-restart"),
            },
        )
        .unwrap();
        assert!(matches!(
            prepared,
            ContextResponse::CompactionPrepared {
                projection: ProjectionRevision {
                    revision: 1,
                    cache_epoch: 1
                },
                ..
            }
        ));
        let _ = fs::remove_file(path);
    }
}

mod compaction_cas {
    use super::*;

    #[test]
    fn prepare_and_commit_advance_one_projection_revision() {
        let path = temp_db("compaction-cas");
        let mut kernel = kernel(&path);
        create_execution(&mut kernel, "exec-1");
        invoke(
            &mut kernel,
            ContextCommand::Admit {
                request: admission("exec-1"),
            },
        )
        .unwrap();
        invoke(
            &mut kernel,
            ContextCommand::PrepareCompaction {
                proposal: proposal("exec-1", "checkpoint-1"),
            },
        )
        .unwrap();
        let committed = invoke(
            &mut kernel,
            ContextCommand::CommitCompaction {
                execution_id: "exec-1".into(),
                checkpoint_id: "checkpoint-1".into(),
            },
        )
        .unwrap();
        let ContextResponse::CompactionCommitted { commit } = committed else {
            panic!("expected compaction commit");
        };
        assert_eq!(commit.committed_projection.revision, 2);
        assert_eq!(commit.committed_projection.cache_epoch, 2);
        let _ = fs::remove_file(path);
    }
}

mod injection_invalidation {
    use super::*;

    #[test]
    fn context_injection_invalidates_prepared_compaction_in_same_owner_state() {
        let path = temp_db("injection-invalidation");
        let mut kernel = kernel(&path);
        create_execution(&mut kernel, "exec-1");
        invoke(
            &mut kernel,
            ContextCommand::Admit {
                request: admission("exec-1"),
            },
        )
        .unwrap();
        invoke(
            &mut kernel,
            ContextCommand::PrepareCompaction {
                proposal: proposal("exec-1", "checkpoint-stale"),
            },
        )
        .unwrap();
        let registered = invoke(
            &mut kernel,
            ContextCommand::Register {
                resource_id: ContextResourceId::parse("skill:steering").unwrap(),
                kind: ContextResourceKind::Skill,
                source: "skills/steering/SKILL.md".into(),
                scope: ContextScope::Workspace,
                content: b"new context".to_vec().into(),
            },
        )
        .unwrap();
        let ContextResponse::Registered { resource } = registered else {
            panic!("expected registered resource");
        };
        invoke(
            &mut kernel,
            ContextCommand::Load {
                execution_id: "exec-1".into(),
                resource_id: resource.descriptor.resource_id,
                revision: resource.descriptor.revision,
                requester: ContextInjectionRequester::User,
                lifetime: ContextInjectionLifetime::Execution,
                reason: "steering".into(),
            },
        )
        .unwrap();
        let error = invoke(
            &mut kernel,
            ContextCommand::CommitCompaction {
                execution_id: "exec-1".into(),
                checkpoint_id: "checkpoint-stale".into(),
            },
        )
        .unwrap_err();
        assert!(error.contains("UnknownPreparedCheckpoint"));
        let _ = fs::remove_file(path);
    }
}
