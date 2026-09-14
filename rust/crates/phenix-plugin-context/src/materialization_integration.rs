use crate::{context_component_manifest, context_factory, context_manifest};
use phenix_core::{
    Authority, Bytes, ContextResourceId, Kernel, KernelConfig, LocalPersistence, PhenixValue,
    Project, ResolvedHarness, ResolvedHarnessActivation,
};
use phenix_plugin_execution::{
    execution_component_manifest, execution_factory, execution_manifest,
};
use phenix_sdk::{
    context_service, execution_service, BudgetReservation, CompactionProposal,
    ContextAdmissionRequest, ContextCommand, ContextDemand, ContextInjectionLifetime,
    ContextInjectionRequester, ContextResourceKind, ContextResponse, ContextRetention,
    ContextScope, ContextSource, DelegationResourcePolicy, ExecutionAuthority, ExecutionCommand,
    ProjectionCheckpoint, ReasoningBudget, RetentionTransition, RetryBudget, RoutingRequirements,
    SkillProvisionBudget, StepPlan, ToolCallGroupReference, ToolProvisionBudget,
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

fn temp_db() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "phenix-context-materialization-{}-{nonce}.sqlite",
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
        .register_embedded_factory(execution_plugin, execution_factory)
        .unwrap();
    kernel
        .register_embedded_factory(context_plugin, context_factory)
        .unwrap();
    kernel.activate_all().unwrap();
    kernel
}

fn invoke(kernel: &mut Kernel, command: ContextCommand) -> ContextResponse {
    let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
    let output = kernel
        .invoke(&context_service(), &input, &authority(), None)
        .unwrap();
    let output: PhenixValue = serde_json::from_slice(&output).unwrap();
    ContextResponse::try_from(Project(&output)).unwrap()
}

fn create_execution(kernel: &mut Kernel) {
    let command = ExecutionCommand::CreateExecution {
        id: "exec-1".into(),
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

fn plan() -> StepPlan {
    let context = ContextDemand {
        mandatory_input_tokens: 0,
        reducible_input_tokens: 4096,
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
            input_tokens: 4096,
            output_tokens: 128,
            cost_microunits: None,
        },
        deadline_at_ms: None,
        reducible_input_dropped_tokens: 0,
    }
}

#[test]
fn committed_compaction_controls_model_facing_materialization() {
    let path = temp_db();
    let mut kernel = kernel(&path);
    create_execution(&mut kernel);

    let registered = invoke(
        &mut kernel,
        ContextCommand::Register {
            resource_id: ContextResourceId::parse("doc:readme").unwrap(),
            kind: ContextResourceKind::ProjectDocument,
            source: "README.md".into(),
            scope: ContextScope::Workspace,
            content: Bytes::from(b"large original body".to_vec()),
        },
    );
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
            reason: "test".into(),
        },
    );

    let first = invoke(
        &mut kernel,
        ContextCommand::PrepareInvocation {
            execution_id: "exec-1".into(),
            input: Bytes::from(b"request body".to_vec()),
        },
    );
    let ContextResponse::InvocationPrepared { preparation } = first else {
        panic!("expected invocation preparation");
    };
    let exact = preparation
        .candidates
        .iter()
        .find_map(|candidate| match &candidate.source {
            ContextSource::Exact { reference } => Some(reference.clone()),
            _ => None,
        })
        .expect("loaded document is an exact context candidate");
    let item_id = format!("{}@{}", exact.resource_id, exact.revision);
    let admitted = invoke(
        &mut kernel,
        ContextCommand::Admit {
            request: ContextAdmissionRequest {
                execution_id: "exec-1".into(),
                step_plan: plan(),
                candidates: preparation.candidates,
                cache_epoch: preparation.projection.cache_epoch,
            },
        },
    );
    let ContextResponse::Admission { projection, .. } = admitted else {
        panic!("expected admission");
    };

    invoke(
        &mut kernel,
        ContextCommand::PrepareCompaction {
            proposal: CompactionProposal {
                execution_id: "exec-1".into(),
                expected_projection: projection.clone(),
                next_cache_epoch: projection.cache_epoch + 1,
                transitions: vec![RetentionTransition {
                    item_id,
                    from: ContextRetention::Full,
                    to: ContextRetention::Compact,
                    recovery: Some(exact.clone()),
                }],
                checkpoint: ProjectionCheckpoint {
                    checkpoint_id: "checkpoint-1".into(),
                    execution_id: "exec-1".into(),
                    source_revision: projection,
                    content_identity: "sha256:compact-summary".into(),
                    compact_view: Bytes::from(b"compact summary".to_vec()),
                    exact_sources: vec![exact],
                    tool_groups: Vec::<ToolCallGroupReference>::new(),
                },
            },
        },
    );
    invoke(
        &mut kernel,
        ContextCommand::CommitCompaction {
            execution_id: "exec-1".into(),
            checkpoint_id: "checkpoint-1".into(),
        },
    );

    let prepared = invoke(
        &mut kernel,
        ContextCommand::PrepareInvocation {
            execution_id: "exec-1".into(),
            input: Bytes::from(b"request body".to_vec()),
        },
    );
    let ContextResponse::InvocationPrepared { preparation } = prepared else {
        panic!("expected invocation preparation");
    };
    let admitted = invoke(
        &mut kernel,
        ContextCommand::Admit {
            request: ContextAdmissionRequest {
                execution_id: "exec-1".into(),
                step_plan: plan(),
                candidates: preparation.candidates,
                cache_epoch: preparation.projection.cache_epoch,
            },
        },
    );
    let ContextResponse::Admission { projection, .. } = admitted else {
        panic!("expected readmission");
    };
    let materialized = invoke(
        &mut kernel,
        ContextCommand::MaterializeInvocation {
            execution_id: "exec-1".into(),
            input: Bytes::from(b"request body".to_vec()),
            expected_projection: projection,
        },
    );
    let ContextResponse::InvocationMaterialized { materialization } = materialized else {
        panic!("expected materialized invocation");
    };
    let text = String::from_utf8(materialization.input.as_ref().to_vec()).unwrap();
    assert!(text.contains("compact summary"));
    assert!(text.contains("request body"));
    assert!(!text.contains("large original body"));

    let _ = fs::remove_file(path);
}
