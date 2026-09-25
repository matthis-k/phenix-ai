use crate::{execution_factory, execution_manifest, step_attempt_service};
use phenix_core::{
    Authority, CapabilityGenerationId, Kernel, KernelConfig, LocalPersistence, ModelId,
    PhenixValue, PluginId, Project,
};
use phenix_sdk::{
    AttemptOutcome, BudgetReservation, ContextDemand, DelegationResourcePolicy, ModelTarget,
    ProjectionRevision, ReasoningBudget, RetryBudget, RouteDecision, RoutingEstimate,
    RoutingRequirements, SkillProvisionBudget, StepAttemptCommand, StepAttemptPhase,
    StepAttemptResponse, StepPlan, ToolProvisionBudget, UsageAttemptKind, UsageAttribution,
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
        "phenix-step-attempt-{name}-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

fn authority() -> Authority {
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

fn invoke(kernel: &mut Kernel, command: StepAttemptCommand) -> Result<StepAttemptResponse, String> {
    let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
    let output = kernel
        .invoke(&step_attempt_service(), &input, &authority(), None)
        .map_err(|error| error.to_string())?;
    let output: PhenixValue = serde_json::from_slice(&output).map_err(|error| error.to_string())?;
    StepAttemptResponse::try_from(Project(&output)).map_err(|error| error.to_string())
}

fn plan() -> StepPlan {
    let context = ContextDemand {
        mandatory_input_tokens: 500,
        reducible_input_tokens: 250,
        output_reserve_tokens: 128,
        required_capabilities: BTreeSet::new(),
    };
    StepPlan {
        policy_revision: "policy-1".into(),
        routing: RoutingRequirements {
            context: context.clone(),
            required_capabilities: BTreeSet::new(),
            require_known_capacity: true,
        },
        context,
        reasoning: ReasoningBudget::BackendDefault,
        tools: ToolProvisionBudget {
            initial: BTreeSet::new(),
            expandable: BTreeSet::new(),
            max_schemas: 4,
            max_result_bytes: 64 * 1024,
        },
        skills: SkillProvisionBudget {
            initial: BTreeSet::new(),
            expandable: BTreeSet::new(),
            max_loaded: 4,
        },
        delegation: DelegationResourcePolicy::default(),
        retry: RetryBudget {
            max_attempts: 2,
            reserved_attempts: 2,
        },
        reservation: BudgetReservation {
            input_tokens: 750,
            output_tokens: 128,
            cost_microunits: Some(1_000),
        },
        deadline_at_ms: None,
        reducible_input_dropped_tokens: 0,
    }
}

fn attribution(
    attempt_id: &str,
    kind: UsageAttemptKind,
    parent_attempt_id: Option<&str>,
) -> UsageAttribution {
    UsageAttribution {
        root_execution_id: "root".into(),
        execution_id: "root".into(),
        attempt_id: attempt_id.into(),
        parent_attempt_id: parent_attempt_id.map(str::to_owned),
        policy_revision: "policy-1".into(),
        kind,
        task_id: None,
    }
}

fn route() -> RouteDecision {
    RouteDecision {
        target: ModelTarget {
            provider_plugin: PluginId::parse("provider.fixture").unwrap(),
            model: ModelId::parse("model.fixture").unwrap(),
            options: BTreeMap::new(),
        },
        capability_generation: CapabilityGenerationId::parse("generation-1").unwrap(),
        policy_revision: "route-policy-1".into(),
        candidate_ordinal: 0,
        estimate: None::<RoutingEstimate>,
    }
}

fn create(
    kernel: &mut Kernel,
    attempt_id: &str,
    kind: UsageAttemptKind,
    parent: Option<&str>,
) -> StepAttemptResponse {
    invoke(
        kernel,
        StepAttemptCommand::Create {
            attribution: attribution(attempt_id, kind, parent),
            plan: plan(),
        },
    )
    .unwrap()
}

fn advance_to_dispatched(kernel: &mut Kernel, attempt_id: &str) {
    invoke(
        kernel,
        StepAttemptCommand::BindReservation {
            attempt_id: attempt_id.into(),
            reservation_id: format!("reservation-{attempt_id}"),
        },
    )
    .unwrap();
    invoke(
        kernel,
        StepAttemptCommand::BindRoute {
            attempt_id: attempt_id.into(),
            decision: route(),
        },
    )
    .unwrap();
    invoke(
        kernel,
        StepAttemptCommand::BindProjection {
            attempt_id: attempt_id.into(),
            projection: ProjectionRevision {
                revision: 4,
                cache_epoch: 2,
            },
        },
    )
    .unwrap();
    invoke(
        kernel,
        StepAttemptCommand::MarkDispatched {
            attempt_id: attempt_id.into(),
            dispatch_id: format!("dispatch-{attempt_id}"),
        },
    )
    .unwrap();
}

mod transition_order {
    use super::*;

    #[test]
    fn route_cannot_bind_before_reservation() {
        let path = temp_db("transition-order");
        let mut kernel = kernel(&path);
        create(&mut kernel, "attempt-1", UsageAttemptKind::Root, None);
        let error = invoke(
            &mut kernel,
            StepAttemptCommand::BindRoute {
                attempt_id: "attempt-1".into(),
                decision: route(),
            },
        )
        .unwrap_err();
        assert!(error.contains("InvalidPhase"));
        let lookup = invoke(
            &mut kernel,
            StepAttemptCommand::Get {
                attempt_id: "attempt-1".into(),
            },
        )
        .unwrap();
        assert!(matches!(
            lookup,
            StepAttemptResponse::AttemptLookup {
                attempt: Some(ref attempt)
            } if attempt.phase == StepAttemptPhase::Planned
        ));
        let _ = fs::remove_file(path);
    }
}

mod retry_lineage {
    use super::*;

    #[test]
    fn retry_requires_settled_non_success_parent_in_same_root() {
        let path = temp_db("retry-lineage");
        let mut kernel = kernel(&path);
        create(&mut kernel, "attempt-1", UsageAttemptKind::Root, None);
        let active_error = invoke(
            &mut kernel,
            StepAttemptCommand::Create {
                attribution: attribution("retry-1", UsageAttemptKind::Retry, Some("attempt-1")),
                plan: plan(),
            },
        )
        .unwrap_err();
        assert!(active_error.contains("not settled"));

        advance_to_dispatched(&mut kernel, "attempt-1");
        invoke(
            &mut kernel,
            StepAttemptCommand::Settle {
                attempt_id: "attempt-1".into(),
                outcome: AttemptOutcome::Failed,
            },
        )
        .unwrap();
        let retry = create(
            &mut kernel,
            "retry-1",
            UsageAttemptKind::Retry,
            Some("attempt-1"),
        );
        assert!(matches!(
            retry,
            StepAttemptResponse::Attempt { ref attempt }
                if attempt.attribution.parent_attempt_id.as_deref() == Some("attempt-1")
        ));
        let _ = fs::remove_file(path);
    }
}

mod delegated_retry_identity {
    use super::*;

    #[test]
    fn retry_allocation_inherits_delegated_task_identity() {
        let path = temp_db("delegated-retry-identity");
        let mut kernel = kernel(&path);
        create(&mut kernel, "attempt-1", UsageAttemptKind::Root, None);

        let allocated = invoke(
            &mut kernel,
            StepAttemptCommand::AllocateDelegatedIdentity {
                root_execution_id: "root".into(),
                execution_id: "child-1".into(),
                parent_attempt_id: "attempt-1".into(),
                policy_revision: "policy-1".into(),
                task_id: "task-1".into(),
            },
        )
        .unwrap();
        let StepAttemptResponse::Attribution {
            attribution: delegated,
        } = allocated
        else {
            panic!("expected delegated attribution");
        };
        invoke(
            &mut kernel,
            StepAttemptCommand::Create {
                attribution: delegated.clone(),
                plan: plan(),
            },
        )
        .unwrap();
        advance_to_dispatched(&mut kernel, &delegated.attempt_id);
        invoke(
            &mut kernel,
            StepAttemptCommand::Settle {
                attempt_id: delegated.attempt_id.clone(),
                outcome: AttemptOutcome::Failed,
            },
        )
        .unwrap();

        let retry = invoke(
            &mut kernel,
            StepAttemptCommand::AllocateIdentity {
                root_execution_id: "root".into(),
                execution_id: "child-1".into(),
                parent_attempt_id: Some(delegated.attempt_id.clone()),
                policy_revision: "policy-1".into(),
                kind: UsageAttemptKind::Retry,
            },
        )
        .unwrap();
        let StepAttemptResponse::Attribution { attribution } = retry else {
            panic!("expected retry attribution");
        };
        assert_eq!(attribution.kind, UsageAttemptKind::Retry);
        assert_eq!(
            attribution.parent_attempt_id.as_deref(),
            Some(delegated.attempt_id.as_str())
        );
        assert_eq!(attribution.task_id.as_deref(), Some("task-1"));
        let _ = fs::remove_file(path);
    }
}

mod restart_identity {
    use super::*;

    #[test]
    fn partially_bound_attempt_restores_and_continues_same_identity() {
        let path = temp_db("restart-identity");
        {
            let mut kernel = kernel(&path);
            create(&mut kernel, "attempt-1", UsageAttemptKind::Root, None);
            invoke(
                &mut kernel,
                StepAttemptCommand::BindReservation {
                    attempt_id: "attempt-1".into(),
                    reservation_id: "reservation-1".into(),
                },
            )
            .unwrap();
        }
        let mut restored = kernel(&path);
        let routed = invoke(
            &mut restored,
            StepAttemptCommand::BindRoute {
                attempt_id: "attempt-1".into(),
                decision: route(),
            },
        )
        .unwrap();
        assert!(matches!(
            routed,
            StepAttemptResponse::Attempt { ref attempt }
                if attempt.phase == StepAttemptPhase::Routed
                    && attempt.reservation_id.as_deref() == Some("reservation-1")
        ));
        let _ = fs::remove_file(path);
    }
}

mod terminal_immutability {
    use super::*;

    #[test]
    fn settled_attempt_cannot_be_rewritten_or_retried_after_success() {
        let path = temp_db("terminal-immutability");
        let mut kernel = kernel(&path);
        create(&mut kernel, "attempt-1", UsageAttemptKind::Root, None);
        advance_to_dispatched(&mut kernel, "attempt-1");
        invoke(
            &mut kernel,
            StepAttemptCommand::Settle {
                attempt_id: "attempt-1".into(),
                outcome: AttemptOutcome::Succeeded,
            },
        )
        .unwrap();
        let settle_again = invoke(
            &mut kernel,
            StepAttemptCommand::Settle {
                attempt_id: "attempt-1".into(),
                outcome: AttemptOutcome::Failed,
            },
        )
        .unwrap_err();
        assert!(settle_again.contains("InvalidPhase"));
        let retry = invoke(
            &mut kernel,
            StepAttemptCommand::Create {
                attribution: attribution("retry-1", UsageAttemptKind::Retry, Some("attempt-1")),
                plan: plan(),
            },
        )
        .unwrap_err();
        assert!(retry.contains("cannot be retried"));
        let _ = fs::remove_file(path);
    }
}
