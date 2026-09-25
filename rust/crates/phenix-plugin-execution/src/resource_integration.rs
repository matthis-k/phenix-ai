use crate::{execution_factory, execution_manifest, execution_resource_service};
use phenix_core::{
    ArtifactRevision, Authority, CapabilityGenerationId, Kernel, KernelConfig, LocalPersistence,
    ModelId, PhenixValue, PluginId, Project,
};
use phenix_sdk::{
    prepare_exploration_delegation, BudgetActual, BudgetReservation, BudgetReservationPurpose,
    BudgetReservationRequest, ContextDemand, DelegatedWorkResources, DelegatedWorkerResult,
    DelegationResourcePolicy, DelegationTaskBinding, ExecutionAuthority, ExecutionResourceCommand,
    ExecutionResourceResponse, ExplorationDelegationInput, ExplorationOpportunity, ModelTarget,
    ModelTurnUsage, ReasoningBudget, RetryBudget, RootBudgetLedger, RootBudgetLimits,
    RouteDecision, RoutingEstimate, RoutingRequirements, SkillProvisionBudget, StepPlan,
    ToolProvisionBudget, UsageQuantity, WorkerTaskRecord, WorkerTaskState,
};
use std::{
    collections::{BTreeMap, BTreeSet},
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
        "phenix-execution-resource-{name}-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

fn manifest_authority() -> Authority {
    execution_manifest(Authority::default()).maximum_authority
}

fn kernel(path: &PathBuf) -> Kernel {
    let manifest = execution_manifest(Authority::default());
    let plugin = manifest.id.clone();
    let persistence = LocalPersistence::open(path).unwrap();
    let mut kernel = Kernel::with_persistence(KernelConfig::new([manifest]).unwrap(), persistence);
    kernel
        .register_embedded_factory(plugin, execution_factory)
        .unwrap();
    kernel.activate_all().unwrap();
    kernel
}

fn invoke(
    kernel: &mut Kernel,
    command: ExecutionResourceCommand,
) -> Result<ExecutionResourceResponse, String> {
    let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
    let output = kernel
        .invoke(
            &execution_resource_service(),
            &input,
            &manifest_authority(),
            None,
        )
        .map_err(|error| error.to_string())?;
    let output: PhenixValue = serde_json::from_slice(&output).map_err(|error| error.to_string())?;
    ExecutionResourceResponse::try_from(Project(&output)).map_err(|error| error.to_string())
}

fn ledger() -> RootBudgetLedger {
    RootBudgetLedger {
        root_execution_id: "root".into(),
        limits: RootBudgetLimits {
            fresh_input_tokens: 10_000,
            output_tokens: 2_000,
            cost_microunits: Some(10_000),
            attempts: 4,
        },
        reservations: BTreeMap::new(),
    }
}

fn authority(values: &[&str]) -> ExecutionAuthority {
    ExecutionAuthority::new(values.iter().copied())
}

fn binding(child_authority: ExecutionAuthority) -> DelegationTaskBinding {
    DelegationTaskBinding {
        contract_revision: ArtifactRevision::from_content(b"contract"),
        parent_policy_revision: "policy-1".into(),
        originating_attempt_id: None,
        resources: DelegatedWorkResources {
            target: RouteDecision {
                target: ModelTarget {
                    provider_plugin: PluginId::parse("provider.fixture").unwrap(),
                    model: ModelId::parse("model.fixture").unwrap(),
                    options: BTreeMap::new(),
                },
                capability_generation: CapabilityGenerationId::parse("generation-1").unwrap(),
                policy_revision: "route-1".into(),
                candidate_ordinal: 0,
                estimate: None::<RoutingEstimate>,
            },
            authority: child_authority,
            context: Vec::new(),
            budget: BudgetReservation {
                input_tokens: 2_000,
                output_tokens: 400,
                cost_microunits: Some(2_000),
            },
            deadline_at_ms: 10_000,
            depth: 1,
            attempts: 1,
            max_result_bytes: 64 * 1024,
        },
    }
}

fn reservation(id: &str, binding: &DelegationTaskBinding) -> BudgetReservationRequest {
    BudgetReservationRequest {
        reservation_id: id.into(),
        parent_reservation_id: None,
        policy_revision: binding.parent_policy_revision.clone(),
        purpose: BudgetReservationPurpose::Delegation,
        budget: binding.resources.budget.clone(),
        attempts: binding.resources.attempts,
    }
}

fn task(id: &str, delegated_authority: ExecutionAuthority) -> WorkerTaskRecord {
    WorkerTaskRecord {
        id: id.into(),
        parent_execution: "root".into(),
        graph_generation: "generation-1".into(),
        description: format!("delegated {id}"),
        depends_on: BTreeSet::new(),
        delegated_authority,
        state: WorkerTaskState::Pending,
    }
}

fn policy(max_children: u32) -> DelegationResourcePolicy {
    DelegationResourcePolicy {
        enabled: true,
        max_depth: 2,
        max_children,
        max_attempts: 2,
        max_result_bytes: 64 * 1024,
    }
}

fn step_plan(max_children: u32) -> StepPlan {
    StepPlan {
        policy_revision: "policy-1".into(),
        routing: RoutingRequirements {
            context: ContextDemand::default(),
            required_capabilities: BTreeSet::new(),
            require_known_capacity: false,
        },
        context: ContextDemand::default(),
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
        delegation: policy(max_children),
        retry: RetryBudget {
            max_attempts: 1,
            reserved_attempts: 1,
        },
        reservation: BudgetReservation {
            input_tokens: 10_000,
            output_tokens: 2_000,
            cost_microunits: Some(10_000),
        },
        deadline_at_ms: Some(10_000),
        reducible_input_dropped_tokens: 0,
    }
}

fn result() -> DelegatedWorkerResult {
    DelegatedWorkerResult {
        findings: Vec::new(),
        evidence: Vec::new(),
        escalation: None,
        usage: ModelTurnUsage {
            fresh_input_tokens: UsageQuantity::Reported { value: 500 },
            cache_read_tokens: UsageQuantity::Unavailable,
            cache_write_tokens: UsageQuantity::Unavailable,
            output_tokens: UsageQuantity::Reported { value: 100 },
            reasoning_tokens: UsageQuantity::Unavailable,
        },
        encoded_result_bytes: 100,
    }
}

fn actual() -> BudgetActual {
    BudgetActual {
        fresh_input_tokens: 500,
        output_tokens: 100,
        cost_microunits: Some(400),
        attempts: 1,
    }
}

fn register(kernel: &mut Kernel) {
    invoke(
        kernel,
        ExecutionResourceCommand::RegisterRootBudget { ledger: ledger() },
    )
    .unwrap();
}

mod exploration_admission {
    use super::*;

    #[test]
    fn accepted_exploration_uses_atomic_delegated_admission() {
        let path = temp_db("exploration-admission");
        let mut kernel = kernel(&path);
        register(&mut kernel);

        let exploration_policy = phenix_sdk::ExplorationPolicy {
            enabled: true,
            min_parent_input_tokens_saved: 1_000,
            max_child_input_tokens: 2_000,
            max_child_output_tokens: 1_000,
            max_child_cost_microunits: Some(2_000),
            max_result_bytes: 32 * 1024,
        };
        let opportunity = ExplorationOpportunity {
            task_id: "exploration-1".into(),
            description: "inspect an isolated subsystem".into(),
            separable: true,
            requires_parent_transcript: false,
            parent_input_tokens_if_inline: 5_000,
            inline_parent_reacquisition_tokens: 0,
            delegated_parent_reacquisition_tokens: 100,
            child_input_tokens: 1_000,
            child_output_tokens: 200,
            child_cost_microunits: Some(1_000),
            expected_result_input_tokens: 500,
        };
        let decision = exploration_policy.assess(&opportunity);
        let child = authority(&["workspace.read"]);
        let admission = prepare_exploration_delegation(
            &opportunity,
            &decision,
            ExplorationDelegationInput {
                root_execution_id: "root".into(),
                parent_execution: "root".into(),
                graph_generation: "generation-1".into(),
                parent_reservation_id: None,
                parent_policy_revision: "policy-1".into(),
                originating_attempt_id: None,
                contract_revision: ArtifactRevision::from_content(b"exploration-1"),
                target: RouteDecision {
                    target: ModelTarget {
                        provider_plugin: PluginId::parse("provider.fixture").unwrap(),
                        model: ModelId::parse("model.fixture").unwrap(),
                        options: BTreeMap::new(),
                    },
                    capability_generation: CapabilityGenerationId::parse("generation-1").unwrap(),
                    policy_revision: "route-1".into(),
                    candidate_ordinal: 0,
                    estimate: None::<RoutingEstimate>,
                },
                parent_authority: child.clone(),
                delegated_authority: child,
                context: Vec::new(),
                deadline_at_ms: 10_000,
                depth: 1,
                attempts: 1,
                existing_children: 0,
                depends_on: BTreeSet::new(),
            },
            &step_plan(2),
            0,
        )
        .unwrap();

        let response = invoke(&mut kernel, admission.into_resource_command(0)).unwrap();
        assert!(matches!(
            response,
            ExecutionResourceResponse::DelegatedTask { ref task }
                if task.task.id == "exploration-1"
                    && matches!(task.task.state, WorkerTaskState::Pending)
        ));

        let remaining = invoke(
            &mut kernel,
            ExecutionResourceCommand::Remaining {
                root_execution_id: "root".into(),
            },
        )
        .unwrap();
        let ExecutionResourceResponse::Remaining { budget } = remaining else {
            panic!("expected remaining budget");
        };
        assert_eq!(budget.fresh_input_tokens, 9_000);
        assert_eq!(budget.output_tokens, 1_800);
        assert_eq!(budget.cost_microunits, Some(9_000));
        assert_eq!(budget.attempts, 3);
        let _ = fs::remove_file(path);
    }
}

mod transaction_rollback {
    use super::*;

    #[test]
    fn failed_task_admission_does_not_consume_reserved_budget() {
        let path = temp_db("rollback");
        let mut kernel = kernel(&path);
        register(&mut kernel);
        let child = authority(&["workspace.read", "workspace.write"]);
        let binding = binding(child.clone());
        let error = invoke(
            &mut kernel,
            ExecutionResourceCommand::AdmitDelegated {
                root_execution_id: "root".into(),
                reservation: reservation("reservation-1", &binding),
                task: task("task-1", child),
                binding,
                parent_authority: authority(&["workspace.read"]),
                policy: policy(2),
                now_ms: 0,
            },
        )
        .unwrap_err();
        assert!(error.contains("AuthorityExpanded"));
        let remaining = invoke(
            &mut kernel,
            ExecutionResourceCommand::Remaining {
                root_execution_id: "root".into(),
            },
        )
        .unwrap();
        let ExecutionResourceResponse::Remaining { budget } = remaining else {
            panic!("expected remaining budget");
        };
        assert_eq!(budget.fresh_input_tokens, 10_000);
        assert_eq!(budget.attempts, 4);
        let _ = fs::remove_file(path);
    }
}

mod pre_start_cancellation {
    use super::*;

    fn admit(kernel: &mut Kernel) {
        let child = authority(&["workspace.read"]);
        let binding = binding(child.clone());
        invoke(
            kernel,
            ExecutionResourceCommand::AdmitDelegated {
                root_execution_id: "root".into(),
                reservation: reservation("reservation-1", &binding),
                task: task("task-1", child.clone()),
                binding,
                parent_authority: child,
                policy: policy(2),
                now_ms: 0,
            },
        )
        .unwrap();
    }

    #[test]
    fn admitted_child_is_visible_to_scheduler_until_started() {
        let path = temp_db("delegated-runnable");
        let mut kernel = kernel(&path);
        register(&mut kernel);
        admit(&mut kernel);

        let runnable = invoke(&mut kernel, ExecutionResourceCommand::RunnableDelegated).unwrap();
        assert_eq!(
            runnable,
            ExecutionResourceResponse::DelegatedRunnableTasks {
                task_ids: vec!["task-1".into()],
            }
        );

        invoke(
            &mut kernel,
            ExecutionResourceCommand::StartDelegated {
                task_id: "task-1".into(),
                execution_id: "child-execution".into(),
                now_ms: 1,
            },
        )
        .unwrap();
        let runnable = invoke(&mut kernel, ExecutionResourceCommand::RunnableDelegated).unwrap();
        assert_eq!(
            runnable,
            ExecutionResourceResponse::DelegatedRunnableTasks {
                task_ids: Vec::new(),
            }
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn pending_child_cancellation_releases_reserved_budget_atomically() {
        let path = temp_db("pre-start-cancel");
        let mut kernel = kernel(&path);
        register(&mut kernel);
        admit(&mut kernel);

        let cancelled = invoke(
            &mut kernel,
            ExecutionResourceCommand::CancelDelegatedBeforeStart {
                task_id: "task-1".into(),
                cause: "scheduler unavailable".into(),
            },
        )
        .unwrap();
        assert!(matches!(
            cancelled,
            ExecutionResourceResponse::DelegatedTask { ref task }
                if matches!(
                    task.task.state,
                    WorkerTaskState::Cancelled { ref cause }
                        if cause == "scheduler unavailable"
                )
        ));

        let remaining = invoke(
            &mut kernel,
            ExecutionResourceCommand::Remaining {
                root_execution_id: "root".into(),
            },
        )
        .unwrap();
        let ExecutionResourceResponse::Remaining { budget } = remaining else {
            panic!("expected remaining budget");
        };
        assert_eq!(budget.fresh_input_tokens, 10_000);
        assert_eq!(budget.output_tokens, 2_000);
        assert_eq!(budget.cost_microunits, Some(10_000));
        assert_eq!(budget.attempts, 4);

        let lookup = invoke(
            &mut kernel,
            ExecutionResourceCommand::GetDelegated {
                task_id: "task-1".into(),
            },
        )
        .unwrap();
        assert!(matches!(
            lookup,
            ExecutionResourceResponse::DelegatedTaskLookup {
                task: Some(ref task)
            } if matches!(task.task.state, WorkerTaskState::Cancelled { .. })
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn running_child_cannot_use_pre_start_cancellation() {
        let path = temp_db("pre-start-cancel-running");
        let mut kernel = kernel(&path);
        register(&mut kernel);
        admit(&mut kernel);
        invoke(
            &mut kernel,
            ExecutionResourceCommand::StartDelegated {
                task_id: "task-1".into(),
                execution_id: "child-execution".into(),
                now_ms: 1,
            },
        )
        .unwrap();

        let error = invoke(
            &mut kernel,
            ExecutionResourceCommand::CancelDelegatedBeforeStart {
                task_id: "task-1".into(),
                cause: "too late".into(),
            },
        )
        .unwrap_err();
        assert!(error.contains("InvalidState"));

        let remaining = invoke(
            &mut kernel,
            ExecutionResourceCommand::Remaining {
                root_execution_id: "root".into(),
            },
        )
        .unwrap();
        let ExecutionResourceResponse::Remaining { budget } = remaining else {
            panic!("expected remaining budget");
        };
        assert_eq!(budget.fresh_input_tokens, 8_000);
        assert_eq!(budget.output_tokens, 1_600);
        assert_eq!(budget.cost_microunits, Some(8_000));
        assert_eq!(budget.attempts, 3);
        let _ = fs::remove_file(path);
    }
}

mod delegation_policy {
    use super::*;

    #[test]
    fn max_children_is_enforced_before_second_reservation() {
        let path = temp_db("child-limit");
        let mut kernel = kernel(&path);
        register(&mut kernel);
        let child = authority(&["workspace.read"]);
        let first_binding = binding(child.clone());
        invoke(
            &mut kernel,
            ExecutionResourceCommand::AdmitDelegated {
                root_execution_id: "root".into(),
                reservation: reservation("reservation-1", &first_binding),
                task: task("task-1", child.clone()),
                binding: first_binding,
                parent_authority: child.clone(),
                policy: policy(1),
                now_ms: 0,
            },
        )
        .unwrap();
        let second_binding = binding(child.clone());
        let error = invoke(
            &mut kernel,
            ExecutionResourceCommand::AdmitDelegated {
                root_execution_id: "root".into(),
                reservation: reservation("reservation-2", &second_binding),
                task: task("task-2", child.clone()),
                binding: second_binding,
                parent_authority: child,
                policy: policy(1),
                now_ms: 0,
            },
        )
        .unwrap_err();
        assert!(error.contains("ChildLimitExceeded"));
        let _ = fs::remove_file(path);
    }
}

mod restart_settlement {
    use super::*;

    #[test]
    fn running_child_and_reservation_restore_then_settle_together() {
        let path = temp_db("restart-settlement");
        {
            let mut kernel = kernel(&path);
            register(&mut kernel);
            let child = authority(&["workspace.read"]);
            let binding = binding(child.clone());
            invoke(
                &mut kernel,
                ExecutionResourceCommand::AdmitDelegated {
                    root_execution_id: "root".into(),
                    reservation: reservation("reservation-1", &binding),
                    task: task("task-1", child.clone()),
                    binding,
                    parent_authority: child,
                    policy: policy(2),
                    now_ms: 0,
                },
            )
            .unwrap();
            invoke(
                &mut kernel,
                ExecutionResourceCommand::StartDelegated {
                    task_id: "task-1".into(),
                    execution_id: "child-execution".into(),
                    now_ms: 1,
                },
            )
            .unwrap();
        }

        let mut restored = kernel(&path);
        let lookup = invoke(
            &mut restored,
            ExecutionResourceCommand::GetDelegated {
                task_id: "task-1".into(),
            },
        )
        .unwrap();
        assert!(matches!(
            lookup,
            ExecutionResourceResponse::DelegatedTaskLookup {
                task: Some(ref task)
            } if matches!(task.task.state, WorkerTaskState::Running { .. })
        ));
        invoke(
            &mut restored,
            ExecutionResourceCommand::CompleteDelegated {
                task_id: "task-1".into(),
                execution_id: "child-execution".into(),
                result: result(),
                actual: actual(),
            },
        )
        .unwrap();
        let remaining = invoke(
            &mut restored,
            ExecutionResourceCommand::Remaining {
                root_execution_id: "root".into(),
            },
        )
        .unwrap();
        let ExecutionResourceResponse::Remaining { budget } = remaining else {
            panic!("expected remaining budget");
        };
        assert_eq!(budget.fresh_input_tokens, 9_500);
        assert_eq!(budget.output_tokens, 1_900);
        assert_eq!(budget.cost_microunits, Some(9_600));
        assert_eq!(budget.attempts, 3);
        let _ = fs::remove_file(path);
    }
}
