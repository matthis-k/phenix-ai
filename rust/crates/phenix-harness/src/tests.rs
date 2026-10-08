use super::*;
use phenix_core::{
    ContextResourceId, ContextRevisionId, LayerResult, PermissionId, PhenixValue, Project,
    ServiceContribution, ServiceId, ServiceRole, SessionId,
};
use phenix_plugin_catalog::{
    ADVANCED_AGENT_CONFIGURATION, ArtifactCommand, ArtifactProvenance, ArtifactResponse,
    BASIC_AGENT_CONFIGURATION, BASIC_PRODUCT_CONFIGURATION, ContextCommand, ContextDescriptor,
    ContextResourceKind, ContextResponse, ContextScope, EfficiencyCollectionRequest,
    EfficiencyEvaluationCommand, FULL_PRODUCT_CONFIGURATION, PlanningCommand, PlanningResponse,
    RepositoryWorkSnapshot, SessionCommand, SessionResponse, artifact_manifest, artifact_service,
    context_manifest, context_service, efficiency_evaluation_service, memory_service,
    planning_manifest, planning_service, repository_work_queue_service, sdk_contribution,
    session_manifest, session_service,
};

fn plugin(value: &str) -> PluginId {
    PluginId::parse(value).unwrap()
}

fn capability(value: &str) -> PermissionId {
    PermissionId::parse(value).unwrap()
}

fn service() -> ServiceId {
    ServiceId::parse("fixture.echo@1").unwrap()
}

fn session_authority() -> Authority {
    session_manifest().maximum_authority
}

fn artifact_authority() -> Authority {
    artifact_manifest().maximum_authority
}

fn context_authority() -> Authority {
    context_manifest().maximum_authority
}

fn planning_authority() -> Authority {
    planning_manifest().maximum_authority
}

fn manifest(id: &str, priority: i32) -> PluginManifest {
    service_manifest(id, service(), priority, Authority::default())
}

fn service_manifest(
    id: &str,
    service: ServiceId,
    priority: i32,
    maximum_authority: Authority,
) -> PluginManifest {
    PluginManifest {
        id: plugin(id),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: ServiceRole::Terminal,
            service,
            priority,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority,
    }
}

fn layer_manifest(id: &str, service: ServiceId, priority: i32) -> PluginManifest {
    PluginManifest {
        id: plugin(id),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: ServiceRole::Layer,
            service,
            priority,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

struct Echo(&'static [u8]);

impl PluginInstance for Echo {
    fn start(&mut self, _host: &phenix_core::PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        _service: &ServiceId,
        _input: &[u8],
        _host: &phenix_core::PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        Ok(self.0.to_vec())
    }
}

struct LayerEcho;

impl PluginInstance for LayerEcho {
    fn start(&mut self, _host: &phenix_core::PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke_layer(
        &mut self,
        _service: &ServiceId,
        input: &[u8],
        host: &phenix_core::PluginHost<'_>,
    ) -> Result<LayerResult, String> {
        let lower = host
            .continue_service(input, host.authority())
            .map_err(|error| error.to_string())?;
        let mut output = b"layer:".to_vec();
        output.extend_from_slice(&lower);
        Ok(LayerResult::Handled(output))
    }
}

struct FixedResponse(Vec<u8>);

impl PluginInstance for FixedResponse {
    fn start(&mut self, _host: &phenix_core::PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        _service: &ServiceId,
        _input: &[u8],
        _host: &phenix_core::PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        Ok(self.0.clone())
    }
}

#[test]
fn advanced_agent_configuration_extends_basic_through_dependency_resolution() {
    let basic = PhenixRuntimeBuilder::with_selected_suite(&BTreeSet::from([
        BASIC_AGENT_CONFIGURATION.to_owned(),
    ]))
    .unwrap();
    let basic_ids = basic
        .manifests
        .iter()
        .map(|manifest| manifest.id.as_str())
        .collect::<BTreeSet<_>>();
    for required in [
        BASIC_AGENT_CONFIGURATION,
        "phenix.agent-loop",
        "phenix.basic-skills",
        "phenix.context",
        "phenix.execution",
        "phenix.harness.invocation-defaults",
        "phenix.models",
        "phenix.sessions",
        "phenix.step-runner",
    ] {
        assert!(
            basic_ids.contains(required),
            "basic configuration missed {required}"
        );
    }
    for optional in ["phenix.options", "phenix.memory", "phenix.planning"] {
        assert!(
            !basic_ids.contains(optional),
            "basic configuration unexpectedly included {optional}"
        );
    }
    basic.build().unwrap();

    let advanced = PhenixRuntimeBuilder::with_selected_suite(&BTreeSet::from([
        ADVANCED_AGENT_CONFIGURATION.to_owned(),
    ]))
    .unwrap();
    let advanced_ids = advanced
        .manifests
        .iter()
        .map(|manifest| manifest.id.as_str())
        .collect::<BTreeSet<_>>();
    for required in [
        ADVANCED_AGENT_CONFIGURATION,
        BASIC_AGENT_CONFIGURATION,
        "phenix.agent-loop",
        "phenix.options",
        "phenix.memory",
        "phenix.planning",
        "phenix.repository-workers",
        "phenix.session-tree",
        "phenix.language",
        "phenix.hooks",
        "phenix.debug",
    ] {
        assert!(
            advanced_ids.contains(required),
            "advanced configuration missed {required}"
        );
    }
    advanced.build().unwrap();
}

#[test]
fn application_tool_adapter_can_run_without_the_basic_agent_loop() {
    let selected = BTreeSet::from(["phenix.application-agent-tools".to_owned()]);
    let builder = PhenixRuntimeBuilder::with_selected_suite(&selected)
        .expect("application tools are an independently selectable plugin");
    let ids = builder
        .manifests
        .iter()
        .map(|manifest| manifest.id.as_str())
        .collect::<BTreeSet<_>>();

    assert!(ids.contains("phenix.application-agent-tools"));
    assert!(ids.contains("phenix.sessions"));
    assert!(!ids.contains("phenix.agent-loop"));
    let runtime = builder.build().expect("tools should resolve without an agent loop");
    assert!(
        !runtime
            .kernel()
            .config()
            .manifests()
            .any(|manifest| manifest.id.as_str() == "phenix.agent-loop")
    );
}

#[test]
fn standalone_memory_answers_queries_without_an_agent_or_helper_provider() {
    use phenix_sdk::{MemoryCommand, MemoryResponse};

    let selected = BTreeSet::from(["phenix.memory".to_owned()]);
    let builder = PhenixRuntimeBuilder::with_selected_suite(&selected)
        .expect("memory may be selected without an agent loop");
    let ids = builder
        .manifests
        .iter()
        .map(|manifest| manifest.id.as_str())
        .collect::<BTreeSet<_>>();
    assert!(ids.contains("phenix.memory"));
    assert!(!ids.contains("phenix.agent-loop"));
    assert!(!ids.contains("phenix.step-runner"));

    let mut runtime = builder.build().expect("memory imports are optional");
    runtime.activate().expect("standalone memory should activate");
    let input = serde_json::to_vec(&PhenixValue::from(&MemoryCommand::Get {
        id: "not-recorded".to_owned(),
    }))
    .unwrap();
    let response = runtime
        .invoke(&memory_service(), &input, &default_suite_authority(), None)
        .unwrap();
    let value: PhenixValue = serde_json::from_slice(&response).unwrap();
    let decoded = MemoryResponse::try_from(Project(&value)).unwrap();
    assert_eq!(decoded, MemoryResponse::Memory { record: None });
}

#[test]
fn basic_profile_can_run_a_foreign_agent_loop_with_first_party_tools() {
    use phenix_core::{ComponentId, ComponentInterface};
    use phenix_plugin_catalog::{agent_loop_component_manifest, agent_loop_service};
    use phenix_sdk::AgentLoopInterface;

    let selected = BTreeSet::from([BASIC_AGENT_CONFIGURATION.to_owned()]);
    let excluded = BTreeSet::from(["phenix.agent-loop".to_owned()]);
    let mut builder = PhenixRuntimeBuilder::with_selected_suite_excluding(&selected, &excluded)
        .expect("the agent profile should not force the Basic loop implementation");

    assert!(builder.manifests.iter().any(|manifest| {
        manifest.id.as_str() == "phenix.application-agent-tools"
    }));
    assert!(!builder.manifests.iter().any(|manifest| {
        manifest.id.as_str() == "phenix.agent-loop"
    }));

    let owner = plugin("fixture.foreign-agent");
    builder
        .add_embedded(
            service_manifest(
                owner.as_str(),
                agent_loop_service(),
                100,
                default_suite_authority(),
            ),
            || Box::new(Echo(b"foreign-agent")),
        )
        .unwrap();

    let mut component = agent_loop_component_manifest(default_suite_authority());
    component.id = ComponentId::parse("fixture.foreign-agent.component").unwrap();
    component.owner = owner;
    component.imports.clear();
    let component_id = component.id.clone();
    builder.add_component(component);
    builder.bind_provider(AgentLoopInterface::interface_id(), component_id.clone());

    let mut runtime = builder
        .build()
        .expect("foreign terminal loop may replace Basic without removing tools");
    assert!(runtime.resolved_generation().components().iter().any(|component| {
        component.id == component_id
    }));
    runtime.activate().expect("foreign agent graph should activate");
    assert_eq!(
        runtime
            .invoke(
                &agent_loop_service(),
                b"fixture",
                &default_suite_authority(),
                None,
            )
            .unwrap(),
        b"foreign-agent"
    );
}

#[test]
fn product_configurations_resolve_providers_and_frontend_sdk() {
    for root in [BASIC_PRODUCT_CONFIGURATION, FULL_PRODUCT_CONFIGURATION] {
        let builder =
            PhenixRuntimeBuilder::with_selected_suite(&BTreeSet::from([root.to_owned()])).unwrap();
        let ids = builder
            .manifests
            .iter()
            .map(|manifest| manifest.id.as_str())
            .collect::<BTreeSet<_>>();

        for required in [
            root,
            "phenix.agent.basic",
            "phenix.api",
            "phenix.options",
            "phenix.providers",
            "openai-api",
            "openai-codex",
            "phenix.sessions",
        ] {
            assert!(ids.contains(required), "{root} missed {required}");
        }

        let harness = builder.build().unwrap();
        harness
            .resolved_generation()
            .resolve_sdk_contributions([sdk_contribution()])
            .unwrap();
    }
}

#[test]
fn full_product_exposes_model_entry_triggers_from_its_resolved_composition() {
    let builder = PhenixRuntimeBuilder::with_selected_suite(&BTreeSet::from([
        FULL_PRODUCT_CONFIGURATION.to_owned(),
    ]))
    .unwrap();

    let callables = builder
        .entry_triggers
        .iter()
        .map(|trigger| match &trigger.trigger {
            phenix_core::EntryTriggerKind::ToolCall { callable_id, .. } => callable_id.as_str(),
        })
        .collect::<BTreeSet<_>>();

    for required in [
        "bash",
        "workspace.read",
        "workspace.search",
        "workspace.write",
        "workspace.git",
        "workspace.discover",
        "code.query",
        "memory.record",
        "memory.associate",
        "memory.query",
        "memory.recall",
    ] {
        assert!(
            callables.contains(required),
            "full product missed {required}"
        );
    }
}

#[test]
fn alternate_memory_provider_replaces_default_without_core_changes() {
    let mut builder = PhenixRuntimeBuilder::with_default_suite().unwrap();
    builder
        .add_embedded(
            service_manifest(
                "fixture.alternate-memory",
                memory_service(),
                200,
                Authority::default(),
            ),
            || Box::new(Echo(b"alternate-memory")),
        )
        .unwrap();
    let mut harness = builder.build().unwrap();
    harness.activate().unwrap();

    assert_eq!(
        harness
            .invoke(&memory_service(), b"ignored", &Authority::default(), None)
            .unwrap(),
        b"alternate-memory"
    );
}

#[test]
fn harness_builder_applies_layer_policy() {
    let service = service();
    let layer_id = plugin("fixture-layer");
    let mut builder = PhenixRuntimeBuilder::new();
    builder
        .add_embedded(manifest("terminal", 1), || Box::new(Echo(b"terminal")))
        .unwrap();
    builder
        .add_embedded(
            layer_manifest("fixture-layer", service.clone(), 100),
            || Box::new(LayerEcho),
        )
        .unwrap();
    builder.set_layer_policy(
        service.clone(),
        vec![LayerPolicy {
            plugin: layer_id,
            priority: 100,
            required: true,
            enabled: true,
        }],
    );
    let mut harness = builder.build().unwrap();
    harness.activate().unwrap();

    assert_eq!(
        harness
            .invoke(&service, b"input", &Authority::default(), None)
            .unwrap(),
        b"layer:terminal"
    );
}

#[test]
fn harness_can_exercise_resident_generation_before_promotion() {
    let service = service();
    let active_plugin = plugin("fixture.active");
    let trial_plugin = plugin("fixture.trial");
    let mut builder = PhenixRuntimeBuilder::new();
    builder
        .add_embedded(
            service_manifest(
                active_plugin.as_str(),
                service.clone(),
                100,
                Authority::default(),
            ),
            || Box::new(Echo(b"active")),
        )
        .unwrap();
    let mut harness = builder.build().unwrap();
    harness.activate().unwrap();

    let active_generation = harness.generation().clone();
    harness
        .kernel_mut()
        .preload_embedded_factory(trial_plugin.clone(), || Box::new(Echo(b"trial")));
    let candidate = ResolvedGeneration::resolve(
        [service_manifest(
            trial_plugin.as_str(),
            service.clone(),
            100,
            Authority::default(),
        )],
        [],
        [],
        &Authority::default(),
    )
    .unwrap();
    let trial_generation = candidate.generation().clone();
    let constraints = harness
        .capture_root_execution_constraints(&Authority::default(), [])
        .unwrap();

    harness
        .make_candidate_resident(candidate, &constraints)
        .unwrap();

    assert_eq!(harness.generation(), &active_generation);
    assert_eq!(
        harness
            .invoke(&service, b"input", &Authority::default(), None)
            .unwrap(),
        b"active"
    );
    assert_eq!(
        harness
            .invoke_in_generation(&trial_generation, &service, b"input", &constraints, None,)
            .unwrap(),
        b"trial"
    );

    harness
        .promote_resident(&trial_generation, &constraints)
        .unwrap();

    assert_eq!(harness.generation(), &trial_generation);
    assert_eq!(
        harness
            .invoke(&service, b"input", &Authority::default(), None)
            .unwrap(),
        b"trial"
    );
    assert!(
        harness
            .selectable_generations()
            .contains(&active_generation)
    );
    assert_eq!(
        harness
            .invoke_in_generation(&active_generation, &service, b"input", &constraints, None,)
            .unwrap(),
        b"active"
    );
}

#[test]
fn layer_policy_is_part_of_resolved_generation_identity() {
    fn generation(required: bool) -> GenerationId {
        let service = service();
        let mut builder = PhenixRuntimeBuilder::new();
        builder
            .add_embedded(manifest("terminal", 1), || Box::new(Echo(b"terminal")))
            .unwrap();
        builder
            .add_embedded(
                layer_manifest("fixture-layer", service.clone(), 100),
                || Box::new(LayerEcho),
            )
            .unwrap();
        builder.set_layer_policy(
            service,
            vec![LayerPolicy {
                plugin: plugin("fixture-layer"),
                priority: 100,
                required,
                enabled: true,
            }],
        );
        builder.build().unwrap().generation().clone()
    }

    assert_eq!(generation(true), generation(true));
    assert_ne!(generation(true), generation(false));
}

#[test]
fn efficiency_collection_requires_terminal_outcome_provider() {
    let mut harness = PhenixRuntimeBuilder::with_default_suite()
        .unwrap()
        .build()
        .unwrap();
    harness.activate().unwrap();

    let command = EfficiencyEvaluationCommand::CollectTask {
        request: EfficiencyCollectionRequest {
            task_fixture_revision: "fixture-1".into(),
            root_execution_id: "root-without-outcome-provider".into(),
            policy_revision: "policy-1".into(),
            outcome_evaluator_identity: "fixture.tests".into(),
            price_revision: "prices-1".into(),
        },
    };
    let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
    let error = harness
        .invoke(
            &efficiency_evaluation_service(),
            &input,
            &default_suite_authority(),
            None,
        )
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("terminal outcome evidence unavailable")
            || error.contains("unresolved")
            || error.contains("provider"),
        "unexpected evaluation error: {error}"
    );
}

#[test]
fn benchmark_outcomes_are_opt_in_but_selectable() {
    let benchmark = benchmark_outcome_manifest().id.as_str().to_owned();
    let default = PhenixRuntimeBuilder::with_default_suite().unwrap();
    assert!(
        !default
            .manifests
            .iter()
            .any(|manifest| manifest.id.as_str() == benchmark)
    );

    let selected =
        PhenixRuntimeBuilder::with_selected_suite(&BTreeSet::from([benchmark.clone()])).unwrap();
    assert!(
        selected
            .manifests
            .iter()
            .any(|manifest| manifest.id.as_str() == benchmark)
    );
    assert!(
        selected
            .components
            .iter()
            .any(|component| component.owner.as_str() == benchmark)
    );
}

#[test]
fn selected_efficiency_evaluation_pulls_in_execution_source() {
    let evaluation = efficiency_evaluation_manifest().id.as_str().to_owned();
    let execution = execution_manifest(default_suite_authority())
        .id
        .as_str()
        .to_owned();
    let builder = PhenixRuntimeBuilder::with_selected_suite(&BTreeSet::from([evaluation])).unwrap();

    assert!(
        builder
            .manifests
            .iter()
            .any(|manifest| manifest.id.as_str() == execution)
    );
}

#[test]
fn kernel_only_harness_has_no_userspace_plugins() {
    let mut harness = PhenixRuntime::kernel_only();
    harness.activate().unwrap();
    assert_eq!(harness.kernel().config().manifests().count(), 0);
    let input = serde_json::to_vec(&SessionCommand::Get {
        id: SessionId::parse("missing").unwrap(),
    })
    .unwrap();
    assert!(
        harness
            .invoke(&session_service(), &input, &session_authority(), None)
            .is_err()
    );
    let context = serde_json::to_vec(&ContextCommand::List).unwrap();
    assert!(
        harness
            .invoke(&context_service(), &context, &context_authority(), None)
            .is_err()
    );
    let planning = serde_json::to_vec(&PlanningCommand::GetObjective {
        id: "missing".into(),
    })
    .unwrap();
    assert!(
        harness
            .invoke(&planning_service(), &planning, &planning_authority(), None)
            .is_err()
    );
}

#[test]
fn default_harness_routes_first_party_services_through_kernel_contracts() {
    let mut harness = PhenixRuntime::default_suite().unwrap();
    harness.activate().unwrap();

    let snapshot = RepositoryWorkSnapshot {
        pull_requests: Vec::new(),
        issues: Vec::new(),
    };
    let input = serde_json::to_vec(&PhenixValue::from(&snapshot)).unwrap();
    let output = harness
        .invoke(
            &repository_work_queue_service(),
            &input,
            &Authority::default(),
            None,
        )
        .unwrap();
    serde_json::from_slice::<PhenixValue>(&output).unwrap();

    let create = serde_json::to_vec(&PhenixValue::from(&SessionCommand::Create {
        session: phenix_plugin_catalog::SessionRecord::new(SessionId::parse("session-1").unwrap()),
    }))
    .unwrap();
    let response = harness
        .invoke(&session_service(), &create, &session_authority(), None)
        .unwrap();
    let response: PhenixValue = serde_json::from_slice(&response).unwrap();
    assert!(matches!(
        SessionResponse::try_from(Project(&response)).unwrap(),
        SessionResponse::Created { .. }
    ));

    let store = serde_json::to_vec(&PhenixValue::from(&ArtifactCommand::Store {
        content: b"readme".to_vec(),
        provenance: ArtifactProvenance {
            producer: "harness-smoke".into(),
            provider_identity: None,
            configuration_identity: None,
            source_observations: BTreeMap::new(),
        },
    }))
    .unwrap();
    let artifact = artifact_component_manifest();
    let response = harness
        .kernel_mut()
        .invoke_component(
            &artifact.id,
            &artifact_service(),
            &store,
            &artifact_authority(),
            &artifact.owner,
        )
        .unwrap();
    let response: PhenixValue = serde_json::from_slice(&response).unwrap();
    assert!(matches!(
        ArtifactResponse::try_from(Project(&response)).unwrap(),
        ArtifactResponse::Stored { reused: false, .. }
    ));

    let register = serde_json::to_vec(&PhenixValue::from(&ContextCommand::Register {
        resource_id: ContextResourceId::parse("skill:review").unwrap(),
        kind: ContextResourceKind::Skill,
        source: "skills/review/SKILL.md".into(),
        scope: ContextScope::Workspace,
        content: b"review".to_vec().into(),
    }))
    .unwrap();
    let response = harness
        .invoke(&context_service(), &register, &context_authority(), None)
        .unwrap();
    let response: PhenixValue = serde_json::from_slice(&response).unwrap();
    assert!(matches!(
        ContextResponse::try_from(Project(&response)).unwrap(),
        ContextResponse::Registered { .. }
    ));

    let objective = serde_json::to_vec(&PhenixValue::from(&PlanningCommand::CreateObjective {
        id: "objective-1".into(),
        title: "Use plugin-owned planning".into(),
        parent: None,
    }))
    .unwrap();
    let planning = planning_component_manifest();
    let response = harness
        .kernel_mut()
        .invoke_component(
            &planning.id,
            &planning_service(),
            &objective,
            &planning_authority(),
            &planning.owner,
        )
        .unwrap();
    let response: PhenixValue = serde_json::from_slice(&response).unwrap();
    assert!(matches!(
        PlanningResponse::try_from(Project(&response)).unwrap(),
        PlanningResponse::Objective { objective: Some(_) }
    ));
}

#[test]
fn first_party_session_provider_is_replaceable_through_normal_resolution() {
    let alternate = serde_json::to_vec(&SessionResponse::Session { session: None }).unwrap();
    let alternate_factory = alternate.clone();
    let mut builder = PhenixRuntimeBuilder::new();
    builder.set_component_authority(session_authority());
    builder
        .add_embedded(session_manifest(), session_factory)
        .unwrap();
    builder
        .add_embedded(
            service_manifest(
                "alternate-sessions",
                session_service(),
                200,
                Authority::default(),
            ),
            move || Box::new(FixedResponse(alternate_factory.clone())),
        )
        .unwrap();
    let mut harness = builder.build().unwrap();
    harness.activate().unwrap();
    let input = serde_json::to_vec(&SessionCommand::Get {
        id: SessionId::parse("x").unwrap(),
    })
    .unwrap();
    assert_eq!(
        harness
            .invoke(&session_service(), &input, &session_authority(), None)
            .unwrap(),
        alternate
    );
}

#[test]
fn first_party_context_provider_is_replaceable_through_normal_resolution() {
    let alternate = serde_json::to_vec(&ContextResponse::Resources {
        descriptors: Vec::new(),
    })
    .unwrap();
    let alternate_factory = alternate.clone();
    let mut builder = PhenixRuntimeBuilder::new();
    builder.set_component_authority(default_suite_authority());
    builder
        .add_embedded(
            execution_manifest(default_suite_authority()),
            execution_factory,
        )
        .unwrap();
    builder
        .add_embedded(context_manifest(), context_factory)
        .unwrap();
    builder
        .add_embedded(
            service_manifest(
                "alternate-context",
                context_service(),
                200,
                Authority::default(),
            ),
            move || Box::new(FixedResponse(alternate_factory.clone())),
        )
        .unwrap();
    let mut harness = builder.build().unwrap();
    harness.activate().unwrap();
    let input = serde_json::to_vec(&ContextCommand::List).unwrap();
    assert_eq!(
        harness
            .invoke(&context_service(), &input, &Authority::default(), None)
            .unwrap(),
        alternate
    );
}

#[test]
fn mock_qml_context_provider_contributes_through_the_same_service_contract() {
    let qml_descriptor = ContextDescriptor {
        resource_id: ContextResourceId::parse("qml:Main.qml").unwrap(),
        revision: ContextRevisionId::parse("qml-revision").unwrap(),
        kind: ContextResourceKind::External,
        source: "Main.qml".into(),
        scope: ContextScope::Workspace,
        content_identity: "qml-revision".into(),
        estimated_bytes: 128,
    };
    let response = serde_json::to_vec(&ContextResponse::Resources {
        descriptors: vec![qml_descriptor.clone()],
    })
    .unwrap();
    let response_factory = response.clone();
    let mut builder = PhenixRuntimeBuilder::new();
    builder
        .add_embedded(
            service_manifest(
                "mock-qml-context",
                context_service(),
                200,
                Authority::default(),
            ),
            move || Box::new(FixedResponse(response_factory.clone())),
        )
        .unwrap();
    let mut harness = builder.build().unwrap();
    harness.activate().unwrap();
    let input = serde_json::to_vec(&ContextCommand::List).unwrap();
    let output = harness
        .invoke(&context_service(), &input, &Authority::default(), None)
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<ContextResponse>(&output).unwrap(),
        ContextResponse::Resources {
            descriptors: vec![qml_descriptor],
        }
    );
}

#[test]
fn product_policy_can_replace_provider_without_kernel_changes() {
    let mut builder = PhenixRuntimeBuilder::new();
    builder
        .add_embedded(manifest("first-party", 10), || Box::new(Echo(b"first")))
        .unwrap();
    builder
        .add_embedded(manifest("alternate", 20), || Box::new(Echo(b"alternate")))
        .unwrap();
    let mut harness = builder.build().unwrap();
    harness.activate().unwrap();

    assert_eq!(
        harness
            .invoke(&service(), b"", &Authority::default(), None)
            .unwrap(),
        b"alternate"
    );
    assert_eq!(
        harness
            .invoke(
                &service(),
                b"",
                &Authority::default(),
                Some(&plugin("first-party")),
            )
            .unwrap(),
        b"first"
    );
}

#[test]
fn omitting_provider_removes_it_from_product_composition() {
    let mut builder = PhenixRuntimeBuilder::new();
    builder
        .add_embedded(manifest("first-party", 10), || Box::new(Echo(b"first")))
        .unwrap();
    let mut harness = builder.build().unwrap();
    harness.activate().unwrap();

    assert_eq!(
        harness
            .invoke(&service(), b"", &Authority::default(), None)
            .unwrap(),
        b"first"
    );
    assert_eq!(
        harness.kernel().config().manifests().count(),
        1,
        "omitted plugins do not exist as kernel fallbacks"
    );
}

#[test]
fn first_party_state_plugins_require_only_persistence_authority() {
    for authority in [
        session_authority(),
        artifact_authority(),
        context_authority(),
        planning_authority(),
    ] {
        assert!(authority.permits(&capability("kernel.persistence.schema")));
        assert!(authority.permits(&capability("kernel.persistence.read")));
        assert!(authority.permits(&capability("kernel.persistence.write")));
        assert!(!authority.permits(&capability("fs.write")));
    }
}

fn provider_contract_fixture() -> (PhenixRuntimeBuilder, phenix_core::InterfaceId) {
    use phenix_core::{
        ComponentExport, ComponentId, ComponentImport, ComponentManifest, InterfaceId,
    };

    let interface = InterfaceId::parse("fixture.memory@1").unwrap();
    let mut builder = PhenixRuntimeBuilder::new();
    for id in ["fixture.consumer", "fixture.alpha", "fixture.beta"] {
        builder
            .add_embedded(
                PluginManifest {
                    id: plugin(id),
                    version: 1,
                    execution: PluginExecution::Embedded,
                    dependencies: Vec::new(),
                    services: Vec::new(),
                    resource_namespaces: Vec::new(),
                    maximum_authority: Authority::default(),
                },
                || Box::new(Echo(b"contract")),
            )
            .unwrap();
    }

    for (id, owner) in [
        ("fixture.alpha.component", "fixture.alpha"),
        ("fixture.beta.component", "fixture.beta"),
    ] {
        builder.add_component(ComponentManifest {
            listeners: Vec::new(),
            id: ComponentId::parse(id).unwrap(),
            owner: plugin(owner),
            imports: Vec::new(),
            exports: vec![ComponentExport {
                interface: interface.clone(),
                schema: Default::default(),
                priority: 0,
                required_authority: Authority::default(),
            }],
            maximum_authority: Authority::default(),
        });
    }

    builder.add_component(ComponentManifest {
        listeners: Vec::new(),
        id: ComponentId::parse("fixture.consumer.component").unwrap(),
        owner: plugin("fixture.consumer"),
        imports: vec![ComponentImport {
            interface: interface.clone(),
            schema: Default::default(),
            required: true,
            authority: Authority::default(),
        }],
        exports: Vec::new(),
        maximum_authority: Authority::default(),
    });
    (builder, interface)
}

fn selected_fixture_provider(
    runtime: &PhenixRuntime,
    interface: &phenix_core::InterfaceId,
) -> phenix_core::ComponentId {
    use phenix_core::ComponentId;

    runtime
        .component_graph()
        .import_handle(
            &ComponentId::parse("fixture.consumer.component").unwrap(),
            interface,
        )
        .unwrap()
        .unwrap()
        .exporter()
        .clone()
}

#[test]
fn product_builder_exposes_provider_selection_without_nix() {
    use phenix_core::{ComponentId, ProviderCompositionPolicy};

    let alpha = ComponentId::parse("fixture.alpha.component").unwrap();
    let beta = ComponentId::parse("fixture.beta.component").unwrap();

    let (default_builder, interface) = provider_contract_fixture();
    let default = default_builder.build().unwrap();
    assert_eq!(selected_fixture_provider(&default, &interface), alpha);

    let (mut selected_builder, _) = provider_contract_fixture();
    selected_builder.bind_provider(interface.clone(), beta.clone());
    let selected = selected_builder.build().unwrap();
    assert_eq!(selected_fixture_provider(&selected, &interface), beta);
    assert_ne!(default.generation(), selected.generation());

    let (mut disabled_builder, _) = provider_contract_fixture();
    disabled_builder.disable_provider(interface.clone(), alpha);
    let disabled = disabled_builder.build().unwrap();
    assert_eq!(selected_fixture_provider(&disabled, &interface), beta);

    let (mut policy_builder, _) = provider_contract_fixture();
    policy_builder.set_provider_policy(
        ProviderCompositionPolicy::new().with_explicit_binding(interface.clone(), beta.clone()),
    );
    let policy_selected = policy_builder.build().unwrap();
    assert_eq!(
        selected_fixture_provider(&policy_selected, &interface),
        beta
    );
    assert_eq!(
        selected.generation(),
        policy_selected.generation(),
        "equivalent provider composition must have the same generation identity"
    );
}

#[test]
fn product_builder_rejects_binding_a_missing_provider() {
    use phenix_core::ComponentId;

    let (mut builder, interface) = provider_contract_fixture();
    builder.bind_provider(
        interface,
        ComponentId::parse("fixture.missing.component").unwrap(),
    );

    assert!(
        matches!(builder.build(), Err(PhenixRuntimeBuildError::Resolution(_))),
        "invalid provider selection must fail during Phenix resolution"
    );
}

#[test]
fn later_provider_binding_reenables_earlier_exclusion() {
    use phenix_core::ComponentId;

    let (mut builder, interface) = provider_contract_fixture();
    let alpha = ComponentId::parse("fixture.alpha.component").unwrap();
    builder.disable_provider(interface.clone(), alpha.clone());
    builder.bind_provider(interface.clone(), alpha.clone());

    let runtime = builder.build().unwrap();
    assert_eq!(selected_fixture_provider(&runtime, &interface), alpha);
}

#[test]
fn later_provider_exclusion_rejects_an_explicit_binding() {
    use phenix_core::ComponentId;

    let (mut builder, interface) = provider_contract_fixture();
    let alpha = ComponentId::parse("fixture.alpha.component").unwrap();
    builder.bind_provider(interface.clone(), alpha.clone());
    builder.disable_provider(interface, alpha);
    let error = builder
        .build()
        .err()
        .expect("binding an excluded provider must fail");
    assert!(
        error.to_string().contains("explicitly requires provider"),
        "unexpected policy failure: {error}"
    );
}

#[test]
fn advanced_profile_can_remove_an_inherited_default_before_activation() {
    let profile = BTreeSet::from([ADVANCED_AGENT_CONFIGURATION.to_owned()]);
    let excluded = BTreeSet::from(["phenix.debug".to_owned()]);
    let builder = PhenixRuntimeBuilder::with_selected_suite_excluding(&profile, &excluded)
        .expect("profile defaults can be overridden without removing their profile");
    let ids = builder
        .manifests
        .iter()
        .map(|manifest| manifest.id.as_str())
        .collect::<BTreeSet<_>>();
    assert!(ids.contains(ADVANCED_AGENT_CONFIGURATION));
    assert!(ids.contains(BASIC_AGENT_CONFIGURATION));
    assert!(ids.contains("phenix.agent-loop"));
    assert!(
        !ids.contains("phenix.debug"),
        "excluded default must not activate"
    );

    let resolved = builder
        .build()
        .expect("optional debug default may be omitted");
    assert!(
        !resolved
            .kernel()
            .config()
            .manifests()
            .any(|manifest| manifest.id.as_str() == "phenix.debug")
    );
}

#[test]
fn full_profile_exclusions_are_not_hard_manifest_dependencies() {
    let profile = BTreeSet::from([FULL_PRODUCT_CONFIGURATION.to_owned()]);
    let excluded = BTreeSet::from(["phenix.debug".to_owned()]);
    let builder = PhenixRuntimeBuilder::with_selected_suite_excluding(&profile, &excluded)
        .expect("full profile should permit overriding an inherited debug default");
    let selected = builder
        .manifests
        .iter()
        .map(|manifest| manifest.id.as_str())
        .collect::<BTreeSet<_>>();
    assert!(selected.contains(FULL_PRODUCT_CONFIGURATION));
    assert!(selected.contains("phenix.product.basic"));
    assert!(selected.contains(ADVANCED_AGENT_CONFIGURATION));
    assert!(selected.contains(BASIC_AGENT_CONFIGURATION));
    assert!(!selected.contains("phenix.debug"));
}

#[test]
fn full_profile_can_substitute_a_contract_provider_without_loading_native_memory() {
    use phenix_core::{ComponentId, ComponentInterface};
    use phenix_plugin_catalog::memory_component_manifest;
    use phenix_sdk::MemoryInterface;

    let selected = BTreeSet::from([FULL_PRODUCT_CONFIGURATION.to_owned()]);
    let excluded = BTreeSet::from(["phenix.memory".to_owned()]);
    let mut builder = PhenixRuntimeBuilder::with_selected_suite_excluding(&selected, &excluded)
        .expect("native memory is a Full default, not a compulsory profile dependency");

    assert!(
        !builder
            .manifests
            .iter()
            .any(|manifest| manifest.id.as_str() == "phenix.memory")
    );

    let owner = plugin("fixture.external-memory");
    builder
        .add_embedded(
            PluginManifest {
                id: owner.clone(),
                version: 1,
                execution: PluginExecution::Embedded,
                dependencies: Vec::new(),
                services: Vec::new(),
                resource_namespaces: Vec::new(),
                maximum_authority: default_suite_authority(),
            },
            || Box::new(Echo(b"external")),
        )
        .unwrap();

    // A stand-in third-party contract implementation: it advertises compatible
    // exports without the native memory plugin's helper invocation dependency.
    // This tests composition, not the semantic quality of a real memory backend.
    let mut external = memory_component_manifest();
    external.id = ComponentId::parse("fixture.external-memory.component").unwrap();
    external.owner = owner;
    external.imports.clear();
    let external_id = external.id.clone();
    builder.add_component(external);
    builder.bind_provider(MemoryInterface::interface_id(), external_id.clone());

    let resolved = builder
        .build()
        .expect("Full may resolve against a foreign memory provider");
    assert!(
        !resolved
            .kernel()
            .config()
            .manifests()
            .any(|manifest| { manifest.id.as_str() == "phenix.memory" })
    );
    assert!(
        resolved
            .kernel()
            .config()
            .manifests()
            .any(|manifest| { manifest.id.as_str() == "fixture.external-memory" })
    );
    assert!(
        resolved
            .resolved_generation()
            .components()
            .iter()
            .any(|manifest| { manifest.id == external_id })
    );
}

#[test]
fn full_profile_can_omit_one_common_model_provider() {
    use phenix_plugin_catalog::COMMON_PROVIDERS;

    let available = COMMON_PROVIDERS
        .into_iter()
        .map(|provider| provider.id())
        .collect::<BTreeSet<_>>();
    assert!(available.contains("open-router"));

    let selected = BTreeSet::from([FULL_PRODUCT_CONFIGURATION.to_owned()]);
    let excluded = BTreeSet::from(["open-router".to_owned()]);
    let builder = PhenixRuntimeBuilder::with_selected_suite_excluding(&selected, &excluded)
        .expect("common provider defaults must be independently replaceable");
    let active = builder
        .manifests
        .iter()
        .map(|manifest| manifest.id.as_str())
        .collect::<BTreeSet<_>>();
    assert!(active.contains(FULL_PRODUCT_CONFIGURATION));
    assert!(active.contains("phenix.providers"));
    assert!(active.contains("openai-api"));
    assert!(!active.contains("open-router"));
    assert!(builder.build().is_ok());
}
