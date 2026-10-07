use phenix_core::{
    Authority, ComponentExport, ComponentId, ComponentInterface, ComponentManifest, Kernel,
    KernelConfig, LocalPersistence, ModelFeatureGenerationId, ModelId, ModelInferenceRequest,
    ModelInferenceResponse, PhenixValue, PluginContext, PluginExecution, PluginHost, PluginId,
    PluginInstance, PluginManifest, Project, ResolvedGeneration, ResolvedGenerationActivation,
    RuntimeTraceBuffer, RuntimeTraceEvent, ServiceContribution, ServiceId, ServiceRole, ValueError,
};
use phenix_plugin_context::{context_component_manifest, context_factory, context_manifest};
use phenix_plugin_execution::{
    execution_component_manifest, execution_factory, execution_manifest,
};
use phenix_plugin_models::{
    model_inference_service, model_routing_component_manifest, model_routing_factory,
    model_routing_manifest,
};
use phenix_plugin_sessions::{session_component_manifest, session_factory, session_manifest};
use phenix_plugin_step_runner::{
    step_runner_component_manifest, step_runner_factory, step_runner_manifest,
};
use phenix_sdk::{
    CandidateCompleteness, CapacityKnowledge, ContextAnchor, ContextControl, ContextNeed,
    ContextRecoveryCommand, ContextRecoveryDecision, ContextRecoveryInterface,
    ContextRecoveryResponse, DefaultInvocationCommand, DelegationResourcePolicy,
    EffectiveModelFeatures, ExecutionAuthority, ExecutionCommand, ExecutionResourceCommand,
    ExecutionResourceResponse, InvocationClockCommand, InvocationClockInterface,
    InvocationClockResponse, InvocationDefaultsCommand, InvocationDefaultsInterface,
    InvocationDefaultsResponse, InvocationIntent, InvocationParams, InvocationRequest,
    MemoryCommand, MemoryContextCandidate, MemoryContextCommand, MemoryContextInterface,
    MemoryContextMatch, MemoryContextResponse, MemoryInterface, MemoryKind, MemoryRecord,
    MemoryResponse, MemoryScope, ModelCommand, ModelLimits, ModelResponse, ModelTarget,
    RecallResolution, RouteSelectionPolicy, RoutingEstimateMode, RoutingProfile, SessionCommand,
    SessionHistoryContentPart, SessionHistoryDraft, SessionHistoryFinishReason, SessionHistoryRole,
    SessionRecord, SessionResponse, StepRunnerResponse, UsagePolicy, context_recovery_service,
    default_invocation_service, execution_resource_service, execution_service,
    invocation_clock_service, invocation_defaults_service, memory_context_service, memory_service,
    session_service, workspace_context_id,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

const SUPPORT_PLUGIN: &str = "fixture.recovery-support";
const SUPPORT_COMPONENT: &str = "fixture.recovery-support";
const RECOVERED_MARKER: &str = "recovered-context-marker";

struct FixtureProvider;

impl PluginInstance for FixtureProvider {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &model_inference_service() {
            return Err(format!("unsupported fixture provider service: {service}"));
        }
        let context = PluginContext::new(host, (), (), ());
        let request = context
            .kernel
            .decode_projected::<ModelInferenceRequest>(
                &phenix_core::ModelInferenceInterface::interface_id(),
                input,
            )
            .map_err(|error| error.to_string())?;
        context
            .kernel
            .encode_value(&ModelInferenceResponse {
                output: request.input,
                provider_metadata: BTreeMap::new(),
                usage: Default::default(),
                tool_calls: Vec::new(),
            })
            .map_err(|error| error.to_string())
    }
}

struct RecoverySupport;

impl PluginInstance for RecoverySupport {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        let context = PluginContext::new(host, (), (), ());
        if service == &invocation_clock_service() {
            let command = context
                .kernel
                .decode_projected::<InvocationClockCommand>(
                    &InvocationClockInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            let InvocationClockCommand::Now = command;
            return context
                .kernel
                .encode_value(&InvocationClockResponse::Time { now_ms: 1_000 })
                .map_err(|error| error.to_string());
        }
        if service == &invocation_defaults_service() {
            let command = context
                .kernel
                .decode_projected::<InvocationDefaultsCommand>(
                    &InvocationDefaultsInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            let InvocationDefaultsCommand::Resolve { .. } = command else {
                return Err("fixture does not resolve helper defaults".into());
            };
            return context
                .kernel
                .encode_value(&InvocationDefaultsResponse::Params { params: params() })
                .map_err(|error| error.to_string());
        }
        if service == &context_recovery_service() {
            let command = context
                .kernel
                .decode_projected::<ContextRecoveryCommand>(
                    &ContextRecoveryInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            let ContextRecoveryCommand::Assess { request } = command;
            assert!(
                matches!(request.prompt.as_str(), "work on prs" | "recover workspace memory"),
                "unexpected recovery prompt: {}",
                request.prompt
            );
            let decision = if request.state.has_durable_session_history
                || request.state.has_explicit_resource
            {
                ContextRecoveryDecision::Sufficient
            } else {
                ContextRecoveryDecision::Missing {
                    needs: vec![ContextNeed::Task {
                        query: "work on prs".into(),
                    }],
                }
            };
            return context
                .kernel
                .encode_value(&ContextRecoveryResponse::Decision { decision })
                .map_err(|error| error.to_string());
        }
        if service == &memory_context_service() {
            let command = context
                .kernel
                .decode_projected::<MemoryContextCommand>(
                    &MemoryContextInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            let response = match command {
                MemoryContextCommand::Recall { request } => {
                    assert!(
                        matches!(request.prompt.as_str(), "work on prs" | "recover workspace memory"),
                        "unexpected recall prompt: {}",
                        request.prompt
                    );
                    if request.prompt == "recover workspace memory" {
                        let workspace_id = workspace_context_id("/workspace");
                        assert_eq!(
                            request.scopes,
                            vec![
                                MemoryScope::Global,
                                MemoryScope::Workspace {
                                    workspace_id: workspace_id.clone(),
                                },
                            ]
                        );
                        assert!(request.known.contains(&ContextAnchor::Workspace {
                            workspace_id,
                        }));
                    }
                    MemoryContextResponse::Recall {
                        candidates: vec![MemoryContextCandidate {
                            memory_id: "memory-1".into(),
                            anchor: ContextAnchor::Project {
                                key: "fixture-project".into(),
                            },
                            source_refs: Vec::new(),
                            signals: vec![MemoryContextMatch::Lexical],
                            observation_count: 1,
                            confirmed_recoveries: 0,
                            last_observed_at: 900,
                        }],
                        completeness: CandidateCompleteness::Complete,
                    }
                }
                MemoryContextCommand::Resolve { mut evidence } => {
                    assert_eq!(evidence.len(), 1);
                    MemoryContextResponse::Resolution {
                        resolution: RecallResolution::Unique {
                            winner: evidence.remove(0),
                        },
                    }
                }
                other => return Err(format!("unsupported memory context command: {other:?}")),
            };
            return context
                .kernel
                .encode_value(&response)
                .map_err(|error| error.to_string());
        }
        if service == &memory_service() {
            let command = context
                .kernel
                .decode_projected::<MemoryCommand>(&MemoryInterface::interface_id(), input)
                .map_err(|error| error.to_string())?;
            let MemoryCommand::Get { id } = command else {
                return Err("fixture memory service only supports get".into());
            };
            assert_eq!(id, "memory-1");
            return context
                .kernel
                .encode_value(&MemoryResponse::Memory {
                    record: Some(MemoryRecord {
                        id,
                        kind: MemoryKind::Fact,
                        scope: MemoryScope::Global,
                        content: RECOVERED_MARKER.into(),
                        source_refs: Vec::new(),
                        supporting_dependencies: Vec::new(),
                        supersedes: Vec::new(),
                        valid_from: None,
                        valid_until: None,
                        created_at: 100,
                    }),
                })
                .map_err(|error| error.to_string());
        }
        Err(format!("unsupported recovery support service: {service}"))
    }
}

fn authority() -> Authority {
    Authority::new([
        phenix_core::PermissionId::parse("kernel.persistence.schema").unwrap(),
        phenix_core::PermissionId::parse("kernel.persistence.read").unwrap(),
        phenix_core::PermissionId::parse("kernel.persistence.write").unwrap(),
    ])
}

fn temp_db() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "phenix-recovery-invocation-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

fn provider_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse("fixture.provider").unwrap(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: ServiceRole::Terminal,
            service: model_inference_service(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn support_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(SUPPORT_PLUGIN).unwrap(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![
            invocation_clock_service(),
            invocation_defaults_service(),
            context_recovery_service(),
            memory_context_service(),
            memory_service(),
        ]
        .into_iter()
        .map(|service| ServiceContribution {
            role: ServiceRole::Terminal,
            service,
            priority: 100,
            required_authority: Authority::default(),
        })
        .collect(),
        resource_namespaces: Vec::new(),
        maximum_authority: authority(),
    }
}

fn support_component() -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: ComponentId::parse(SUPPORT_COMPONENT).unwrap(),
        owner: PluginId::parse(SUPPORT_PLUGIN).unwrap(),
        imports: Vec::new(),
        exports: vec![
            (
                InvocationClockInterface::interface_id(),
                InvocationClockInterface::schema(),
            ),
            (
                InvocationDefaultsInterface::interface_id(),
                InvocationDefaultsInterface::schema(),
            ),
            (
                ContextRecoveryInterface::interface_id(),
                ContextRecoveryInterface::schema(),
            ),
            (
                MemoryContextInterface::interface_id(),
                MemoryContextInterface::schema(),
            ),
            (MemoryInterface::interface_id(), MemoryInterface::schema()),
        ]
        .into_iter()
        .map(|(interface, schema)| ComponentExport {
            interface,
            schema,
            priority: 100,
            required_authority: Authority::default(),
        })
        .collect(),
        maximum_authority: authority(),
    }
}

fn kernel(path: &PathBuf) -> Kernel {
    let authority = authority();
    let execution = execution_manifest(authority.clone());
    let context = context_manifest();
    let models = model_routing_manifest(authority.clone());
    let sessions = session_manifest();
    let runner = step_runner_manifest(authority.clone());
    let provider = provider_manifest();
    let support = support_manifest();
    let execution_id = execution.id.clone();
    let context_id = context.id.clone();
    let models_id = models.id.clone();
    let sessions_id = sessions.id.clone();
    let runner_id = runner.id.clone();
    let provider_id = provider.id.clone();
    let support_id = support.id.clone();
    let resolved = ResolvedGeneration::resolve(
        [
            execution.clone(),
            context.clone(),
            models.clone(),
            sessions.clone(),
            runner.clone(),
            provider.clone(),
            support.clone(),
        ],
        [
            execution_component_manifest(authority.clone()),
            context_component_manifest(),
            model_routing_component_manifest(authority.clone()),
            session_component_manifest(),
            step_runner_component_manifest(authority.clone()),
            support_component(),
        ],
        [],
        &authority,
    )
    .unwrap();
    let persistence = LocalPersistence::open(path).unwrap();
    let mut kernel = Kernel::with_persistence(
        KernelConfig::new([
            execution, context, models, sessions, runner, provider, support,
        ])
        .unwrap(),
        persistence,
    );
    kernel.activate_resolved_generation(&resolved).unwrap();
    kernel
        .register_embedded_factory(execution_id, execution_factory)
        .unwrap();
    kernel
        .register_embedded_factory(context_id, context_factory)
        .unwrap();
    kernel
        .register_embedded_factory(models_id, model_routing_factory)
        .unwrap();
    kernel
        .register_embedded_factory(sessions_id, session_factory)
        .unwrap();
    kernel
        .register_embedded_factory(runner_id, step_runner_factory)
        .unwrap();
    kernel
        .register_embedded_factory(provider_id, || Box::new(FixtureProvider))
        .unwrap();
    kernel
        .register_embedded_factory(support_id, || Box::new(RecoverySupport))
        .unwrap();
    kernel.activate_all().unwrap();
    kernel
}

fn invoke<C, R>(kernel: &mut Kernel, service: ServiceId, command: &C) -> R
where
    for<'value> PhenixValue: From<&'value C>,
    for<'value> R: TryFrom<Project<&'value PhenixValue>, Error = ValueError>,
{
    let output = kernel
        .invoke(
            &service,
            &serde_json::to_vec(&PhenixValue::from(command)).unwrap(),
            &authority(),
            None,
        )
        .unwrap();
    let output: PhenixValue = serde_json::from_slice(&output).unwrap();
    R::try_from(Project(&output)).unwrap()
}

fn target() -> ModelTarget {
    ModelTarget {
        provider_plugin: PluginId::parse("fixture.provider").unwrap(),
        model: ModelId::parse("fixture-model").unwrap(),
        options: BTreeMap::new(),
    }
}

fn params() -> InvocationParams {
    InvocationParams {
        profile_id: phenix_core::RoutingProfileId::parse("default").unwrap(),
        policy: UsagePolicy {
            revision: "recovery-policy".into(),
            max_fresh_input_tokens: Some(4_000),
            max_output_tokens: Some(512),
            max_cost_microunits: None,
            max_retries: Some(0),
            max_tool_result_bytes: Some(64 * 1024),
            max_tool_schemas: Some(4),
            max_skills: Some(4),
            require_known_capacity: true,
            delegation: DelegationResourcePolicy::default(),
        },
        intent: InvocationIntent {
            output_reserve_tokens: 256,
            required_context_features: BTreeSet::new(),
            required_features: BTreeSet::new(),
            required_tools: BTreeSet::new(),
            optional_tools: BTreeSet::new(),
            required_skills: BTreeSet::new(),
            optional_skills: BTreeSet::new(),
            requested_reasoning: None,
            deadline_at_ms: None,
        },
        route_policy: RouteSelectionPolicy {
            revision: "recovery-route-policy".into(),
            estimates: RoutingEstimateMode::Ignore,
            max_candidate_attempts: Some(2),
        },
    }
}

fn setup(kernel: &mut Kernel) {
    let _: phenix_sdk::ExecutionResponse = invoke(
        kernel,
        execution_service(),
        &ExecutionCommand::CreateExecution {
            id: "root".into(),
            requested_authority: ExecutionAuthority::new(Vec::<String>::new()),
        },
    );
    let _: ExecutionResourceResponse = invoke(
        kernel,
        execution_resource_service(),
        &ExecutionResourceCommand::RegisterRootBudget {
            ledger: phenix_sdk::RootBudgetLedger {
                root_execution_id: "root".into(),
                limits: phenix_sdk::RootBudgetLimits {
                    fresh_input_tokens: 8_000,
                    output_tokens: 2_000,
                    cost_microunits: None,
                    attempts: 4,
                },
                reservations: BTreeMap::new(),
            },
        },
    );
    let profile = RoutingProfile {
        id: phenix_core::RoutingProfileId::parse("default").unwrap(),
        default_target: target(),
        fallback_targets: Vec::new(),
        callable_targets: BTreeMap::new(),
    };
    let _: ModelResponse = invoke(
        kernel,
        phenix_sdk::model_routing_service(),
        &ModelCommand::RegisterProfile { profile },
    );
    let _: ModelResponse = invoke(
        kernel,
        phenix_sdk::model_routing_service(),
        &ModelCommand::PublishModelFeatures {
            features: EffectiveModelFeatures {
                target: target(),
                generation: ModelFeatureGenerationId::parse("generation-1").unwrap(),
                context: ContextControl::ReplaceableTurns,
                capacity: CapacityKnowledge::Known {
                    limits: ModelLimits {
                        context_window_tokens: 16_000,
                        max_output_tokens: Some(2_000),
                    },
                },
                optional: BTreeSet::new(),
                cache: Default::default(),
            },
        },
    );
}

#[test]
fn default_invocation_recovers_memory_then_materializes_the_same_invocation() {
    let path = temp_db();
    let mut kernel = kernel(&path);
    let traces = Arc::new(RuntimeTraceBuffer::default());
    kernel.set_runtime_trace_sink(traces.clone());
    setup(&mut kernel);

    let response: StepRunnerResponse = invoke(
        &mut kernel,
        default_invocation_service(),
        &DefaultInvocationCommand::Invoke {
            request: InvocationRequest {
                execution_id: "root".into(),
                session_id: None,
                parent_attempt_id: None,
                callable_id: None,
                input: b"work on prs".to_vec().into(),
                tools: Vec::new(),
                continuation: Vec::new(),
            },
        },
    );
    let StepRunnerResponse::Completed { output, .. } = response;
    let text = String::from_utf8(output.as_ref().to_vec()).unwrap();
    assert!(text.contains(RECOVERED_MARKER));
    assert!(text.contains("work on prs"));
    assert!(traces.snapshot().iter().any(|event| matches!(
        event,
        RuntimeTraceEvent::ExecutionStage {
            execution_id,
            source,
            stage,
            outcome,
            ..
        } if execution_id == "root"
            && source == "phenix.step-runner"
            && stage == "model_dispatch"
            && outcome == "completed"
    )));

    let _ = fs::remove_file(path);
}

#[test]
fn fresh_session_recovers_with_the_canonical_workspace_scope() {
    let path = temp_db();
    let mut kernel = kernel(&path);
    setup(&mut kernel);

    let session_id = phenix_core::SessionId::parse("workspace-session").unwrap();
    let _: SessionResponse = invoke(
        &mut kernel,
        session_service(),
        &SessionCommand::Create {
            session: SessionRecord::application(
                session_id.clone(),
                "/workspace".into(),
                Some("workspace recovery".into()),
            ),
        },
    );

    let response: StepRunnerResponse = invoke(
        &mut kernel,
        default_invocation_service(),
        &DefaultInvocationCommand::Invoke {
            request: InvocationRequest {
                execution_id: "workspace-root".into(),
                session_id: Some(session_id),
                parent_attempt_id: None,
                callable_id: None,
                input: b"recover workspace memory".to_vec().into(),
                tools: Vec::new(),
                continuation: Vec::new(),
            },
        },
    );
    let StepRunnerResponse::Completed { output, .. } = response;
    let text = String::from_utf8(output.as_ref().to_vec()).unwrap();
    assert!(text.contains(RECOVERED_MARKER));
    assert!(text.contains("recover workspace memory"));

    let _ = fs::remove_file(path);
}

#[test]
fn context_policy_skill_does_not_suppress_memory_recovery() {
    let path = temp_db();
    let mut kernel = kernel(&path);
    setup(&mut kernel);

    let resource_id = phenix_core::ContextResourceId::parse("skill:write").unwrap();
    let registered: phenix_sdk::ContextResponse = invoke(
        &mut kernel,
        phenix_sdk::context_service(),
        &phenix_sdk::ContextCommand::Register {
            resource_id: resource_id.clone(),
            kind: phenix_sdk::ContextResourceKind::Skill,
            source: "packaged/write/SKILL.md".into(),
            scope: phenix_sdk::ContextScope::Workspace,
            content: b"mandatory baseline skill".to_vec().into(),
        },
    );
    let phenix_sdk::ContextResponse::Registered { resource } = registered else {
        panic!("expected registered skill");
    };
    let _: phenix_sdk::ContextResponse = invoke(
        &mut kernel,
        phenix_sdk::context_service(),
        &phenix_sdk::ContextCommand::Load {
            execution_id: "root".into(),
            resource_id,
            revision: resource.descriptor.revision,
            requester: phenix_sdk::ContextInjectionRequester::ContextPolicy,
            lifetime: phenix_sdk::ContextInjectionLifetime::Execution,
            reason: "auto-load mandatory skill".into(),
        },
    );

    let response: StepRunnerResponse = invoke(
        &mut kernel,
        default_invocation_service(),
        &DefaultInvocationCommand::Invoke {
            request: InvocationRequest {
                execution_id: "root".into(),
                session_id: None,
                parent_attempt_id: None,
                callable_id: None,
                input: b"work on prs".to_vec().into(),
                tools: Vec::new(),
                continuation: Vec::new(),
            },
        },
    );
    let StepRunnerResponse::Completed { output, .. } = response;
    let text = String::from_utf8(output.as_ref().to_vec()).unwrap();
    assert!(text.contains("mandatory baseline skill"));
    assert!(text.contains(RECOVERED_MARKER));

    let _ = fs::remove_file(path);
}

#[test]
fn context_policy_project_instruction_does_not_suppress_memory_recovery() {
    let path = temp_db();
    let mut kernel = kernel(&path);
    setup(&mut kernel);

    let resource_id = phenix_core::ContextResourceId::parse("project:agents").unwrap();
    let registered: phenix_sdk::ContextResponse = invoke(
        &mut kernel,
        phenix_sdk::context_service(),
        &phenix_sdk::ContextCommand::Register {
            resource_id: resource_id.clone(),
            kind: phenix_sdk::ContextResourceKind::ProjectInstruction,
            source: "AGENTS.md".into(),
            scope: phenix_sdk::ContextScope::Workspace,
            content: b"automatic project instruction".to_vec().into(),
        },
    );
    let phenix_sdk::ContextResponse::Registered { resource } = registered else {
        panic!("expected registered project instruction");
    };
    let _: phenix_sdk::ContextResponse = invoke(
        &mut kernel,
        phenix_sdk::context_service(),
        &phenix_sdk::ContextCommand::Load {
            execution_id: "root".into(),
            resource_id,
            revision: resource.descriptor.revision,
            requester: phenix_sdk::ContextInjectionRequester::ContextPolicy,
            lifetime: phenix_sdk::ContextInjectionLifetime::Execution,
            reason: "auto-load workspace project instruction".into(),
        },
    );

    let response: StepRunnerResponse = invoke(
        &mut kernel,
        default_invocation_service(),
        &DefaultInvocationCommand::Invoke {
            request: InvocationRequest {
                execution_id: "root".into(),
                session_id: None,
                parent_attempt_id: None,
                callable_id: None,
                input: b"work on prs".to_vec().into(),
                tools: Vec::new(),
                continuation: Vec::new(),
            },
        },
    );
    let StepRunnerResponse::Completed { output, .. } = response;
    let text = String::from_utf8(output.as_ref().to_vec()).unwrap();
    assert!(text.contains("automatic project instruction"));
    assert!(text.contains(RECOVERED_MARKER));

    let _ = fs::remove_file(path);
}

#[test]
fn durable_session_history_suppresses_fallback_memory_recovery() {
    let path = temp_db();
    let mut kernel = kernel(&path);
    setup(&mut kernel);

    let session_id = phenix_core::SessionId::parse("resumed-session").unwrap();
    let _: SessionResponse = invoke(
        &mut kernel,
        session_service(),
        &SessionCommand::Create {
            session: SessionRecord::new(session_id.clone()),
        },
    );
    let _: SessionResponse = invoke(
        &mut kernel,
        session_service(),
        &SessionCommand::AppendHistory {
            id: session_id.clone(),
            entry: SessionHistoryDraft {
                role: SessionHistoryRole::Assistant,
                content: vec![SessionHistoryContentPart::Text {
                    text: "prior durable turn".into(),
                }],
                tool_calls: Vec::new(),
                tool_results: Vec::new(),
                finish_reason: Some(SessionHistoryFinishReason::Complete),
                usage: None,
                context_revision: "context-1".into(),
                instruction_revision: "instructions-1".into(),
            },
        },
    );

    let resource_id = phenix_core::ContextResourceId::parse("skill:write").unwrap();
    let registered: phenix_sdk::ContextResponse = invoke(
        &mut kernel,
        phenix_sdk::context_service(),
        &phenix_sdk::ContextCommand::Register {
            resource_id: resource_id.clone(),
            kind: phenix_sdk::ContextResourceKind::Skill,
            source: "packaged/write/SKILL.md".into(),
            scope: phenix_sdk::ContextScope::Workspace,
            content: b"mandatory baseline skill".to_vec().into(),
        },
    );
    let phenix_sdk::ContextResponse::Registered { resource } = registered else {
        panic!("expected registered skill");
    };
    let _: phenix_sdk::ContextResponse = invoke(
        &mut kernel,
        phenix_sdk::context_service(),
        &phenix_sdk::ContextCommand::Load {
            execution_id: "root".into(),
            resource_id,
            revision: resource.descriptor.revision,
            requester: phenix_sdk::ContextInjectionRequester::ContextPolicy,
            lifetime: phenix_sdk::ContextInjectionLifetime::Execution,
            reason: "auto-load mandatory skill".into(),
        },
    );

    let response: StepRunnerResponse = invoke(
        &mut kernel,
        default_invocation_service(),
        &DefaultInvocationCommand::Invoke {
            request: InvocationRequest {
                execution_id: "root".into(),
                session_id: Some(session_id),
                parent_attempt_id: None,
                callable_id: None,
                input: b"work on prs".to_vec().into(),
                tools: Vec::new(),
                continuation: Vec::new(),
            },
        },
    );
    let StepRunnerResponse::Completed { output, .. } = response;
    let text = String::from_utf8(output.as_ref().to_vec()).unwrap();
    assert!(text.contains("mandatory baseline skill"));
    assert!(text.contains("work on prs"));
    assert!(!text.contains(RECOVERED_MARKER));

    let _ = fs::remove_file(path);
}

#[test]
fn user_explicit_resource_suppresses_recovery_with_automatic_baseline_context() {
    let path = temp_db();
    let mut kernel = kernel(&path);
    setup(&mut kernel);

    for (id, requester, content) in [
        (
            "skill:write",
            phenix_sdk::ContextInjectionRequester::ContextPolicy,
            "mandatory baseline skill",
        ),
        (
            "resource:user-note",
            phenix_sdk::ContextInjectionRequester::User,
            "explicit user context",
        ),
    ] {
        let resource_id = phenix_core::ContextResourceId::parse(id).unwrap();
        let registered: phenix_sdk::ContextResponse = invoke(
            &mut kernel,
            phenix_sdk::context_service(),
            &phenix_sdk::ContextCommand::Register {
                resource_id: resource_id.clone(),
                kind: phenix_sdk::ContextResourceKind::External,
                source: id.into(),
                scope: phenix_sdk::ContextScope::Workspace,
                content: content.as_bytes().to_vec().into(),
            },
        );
        let phenix_sdk::ContextResponse::Registered { resource } = registered else {
            panic!("expected registered resource");
        };
        let _: phenix_sdk::ContextResponse = invoke(
            &mut kernel,
            phenix_sdk::context_service(),
            &phenix_sdk::ContextCommand::Load {
                execution_id: "root".into(),
                resource_id,
                revision: resource.descriptor.revision,
                requester,
                lifetime: phenix_sdk::ContextInjectionLifetime::Execution,
                reason: "recovery gate fixture".into(),
            },
        );
    }

    let response: StepRunnerResponse = invoke(
        &mut kernel,
        default_invocation_service(),
        &DefaultInvocationCommand::Invoke {
            request: InvocationRequest {
                execution_id: "root".into(),
                session_id: None,
                parent_attempt_id: None,
                callable_id: None,
                input: b"work on prs".to_vec().into(),
                tools: Vec::new(),
                continuation: Vec::new(),
            },
        },
    );
    let StepRunnerResponse::Completed { output, .. } = response;
    let text = String::from_utf8(output.as_ref().to_vec()).unwrap();
    assert!(text.contains("mandatory baseline skill"));
    assert!(text.contains("explicit user context"));
    assert!(!text.contains(RECOVERED_MARKER));

    let _ = fs::remove_file(path);
}
