use phenix_core::{
    Authority, CapabilityGenerationId, ComponentManifest, ModelId, PhenixValue, PluginExecution,
    PluginHost, PluginId, PluginInstance, PluginManifest, Project, RoutingProfileId,
    ServiceContribution, ServiceId, ValueError,
};
use phenix_harness::{default_suite_authority, HarnessBuilder, PhenixHarness};
use phenix_plugin_catalog::{
    artifact_component_manifest, model_inference_service, planning_component_manifest,
    ArtifactCommand, ArtifactResponse, CliProbeRequest, ContextCommand, ContextResponse,
    DebugCommand, DebugResponse, ExecutionAuthority, ExecutionCommand, ExecutionResponse,
    FrontendCommand, FrontendResponse, HookCommand, HookResponse, JobCommand, JobResponse,
    LanguageCommand, LanguageResponse, ModelCommand, ModelInferenceRequest, ModelInferenceResponse,
    ModelResponse, ModelTarget, PlanningCommand, PlanningResponse, RepositoryWorkSnapshot,
    RoutingProfile, SessionCommand, SessionResponse, SessionTreeCommand, SessionTreeResponse,
    WorkspaceCommand, WorkspaceResponse,
};
use phenix_sdk::{
    CapacityKnowledge, ContextControl, DelegationResourcePolicy, EffectiveModelCapabilities,
    ExecutionResourceCommand, ExecutionResourceResponse, InvocationCommand, InvocationIntent,
    InvocationParams, InvocationRequest, ModelLimits, RootBudgetLedger, RootBudgetLimits,
    RouteSelectionPolicy, RoutingEstimateMode, StepRunnerResponse, UsagePolicy,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

fn invoke(harness: &mut PhenixHarness, service: &str, input: Value) -> Value {
    match service {
        "phenix.repository.worker-queue@1" => {
            let request: RepositoryWorkSnapshot = serde_json::from_value(input).unwrap();
            let response = invoke_structural_value(harness, service, &request);
            let (tag, payload) = response.variant().unwrap();
            match tag.as_str() {
                "Selected" => json!({
                    "pr_number": payload.get("pr_number").unwrap().exact::<u64>().unwrap(),
                    "reason": payload.get("reason").unwrap().exact::<String>().unwrap(),
                }),
                "Empty" => Value::Null,
                other => panic!("unexpected repository worker response: {other}"),
            }
        }
        "phenix.sessions@1" => {
            invoke_structural_json::<SessionCommand, SessionResponse>(harness, service, input)
        }
        "phenix.session-tree@1" => {
            invoke_structural_json::<SessionTreeCommand, SessionTreeResponse>(
                harness, service, input,
            )
        }
        "phenix.artifacts@1" => {
            invoke_component_structural_json::<ArtifactCommand, ArtifactResponse>(
                harness,
                artifact_component_manifest(),
                service,
                input,
            )
        }
        "phenix.context@1" => {
            invoke_structural_json::<ContextCommand, ContextResponse>(harness, service, input)
        }
        "phenix.execution@1" => {
            invoke_structural_json::<ExecutionCommand, ExecutionResponse>(harness, service, input)
        }
        "phenix.language@1" => {
            invoke_structural_json::<LanguageCommand, LanguageResponse>(harness, service, input)
        }
        "phenix.planning@1" => {
            invoke_component_structural_json::<PlanningCommand, PlanningResponse>(
                harness,
                planning_component_manifest(),
                service,
                input,
            )
        }
        "phenix.models.routing@1" => {
            invoke_structural_json::<ModelCommand, ModelResponse>(harness, service, input)
        }
        "phenix.jobs@1" => invoke_component_structural_json::<JobCommand, JobResponse>(
            harness,
            phenix_plugin_catalog::job_component_manifest(),
            service,
            input,
        ),
        "phenix.frontend-services@1" => {
            invoke_structural_json::<FrontendCommand, FrontendResponse>(harness, service, input)
        }
        "phenix.hooks@1" => {
            invoke_structural_json::<HookCommand, HookResponse>(harness, service, input)
        }
        "phenix.workspace@1" => {
            invoke_structural_json::<WorkspaceCommand, WorkspaceResponse>(harness, service, input)
        }
        "phenix.debug@1" => {
            invoke_structural_json::<DebugCommand, DebugResponse>(harness, service, input)
        }
        other => panic!("unsupported structural test service: {other}"),
    }
}

fn invoke_structural_value<Request>(
    harness: &mut PhenixHarness,
    service: &str,
    request: &Request,
) -> PhenixValue
where
    for<'value> PhenixValue: From<&'value Request>,
{
    let service = ServiceId::parse(service).unwrap();
    let output = harness
        .invoke(
            &service,
            &serde_json::to_vec(&PhenixValue::from(request)).unwrap(),
            &default_suite_authority(),
            None,
        )
        .unwrap_or_else(|error| panic!("{service}: {error}"));
    serde_json::from_slice(&output).unwrap_or_else(|error| panic!("{service}: {error}"))
}

fn invoke_structural_json<Request, Response>(
    harness: &mut PhenixHarness,
    service: &str,
    input: Value,
) -> Value
where
    Request: DeserializeOwned,
    for<'value> PhenixValue: From<&'value Request>,
    for<'value> Response: TryFrom<Project<&'value PhenixValue>, Error = ValueError> + Serialize,
{
    let request = serde_json::from_value(input).unwrap();
    serde_json::to_value(invoke_structural::<Request, Response>(
        harness, service, &request,
    ))
    .unwrap()
}

fn invoke_component_structural_json<Request, Response>(
    harness: &mut PhenixHarness,
    component: ComponentManifest,
    service: &str,
    input: Value,
) -> Value
where
    Request: DeserializeOwned,
    for<'value> PhenixValue: From<&'value Request>,
    for<'value> Response: TryFrom<Project<&'value PhenixValue>, Error = ValueError> + Serialize,
{
    let request = serde_json::from_value(input).unwrap();
    let service = ServiceId::parse(service).unwrap();
    let output = harness
        .kernel_mut()
        .invoke_component(
            &component.id,
            &service,
            &serde_json::to_vec(&PhenixValue::from(&request)).unwrap(),
            &default_suite_authority(),
            &component.owner,
        )
        .unwrap_or_else(|error| panic!("{service}: {error}"));
    let output: PhenixValue =
        serde_json::from_slice(&output).unwrap_or_else(|error| panic!("{service}: {error}"));
    let response =
        Response::try_from(Project(&output)).unwrap_or_else(|error| panic!("{service}: {error}"));
    serde_json::to_value(response).unwrap()
}

fn invoke_structural<Request, Response>(
    harness: &mut PhenixHarness,
    service: &str,
    request: &Request,
) -> Response
where
    for<'value> PhenixValue: From<&'value Request>,
    for<'value> Response: TryFrom<Project<&'value PhenixValue>, Error = ValueError>,
{
    let service = ServiceId::parse(service).unwrap();
    let output = harness
        .invoke(
            &service,
            &serde_json::to_vec(&PhenixValue::from(request)).unwrap(),
            &default_suite_authority(),
            None,
        )
        .unwrap_or_else(|error| panic!("{service}: {error}"));
    let output: PhenixValue =
        serde_json::from_slice(&output).unwrap_or_else(|error| panic!("{service}: {error}"));
    Response::try_from(Project(&output)).unwrap_or_else(|error| panic!("{service}: {error}"))
}

fn invoke_value_raw(
    harness: &mut PhenixHarness,
    service: &ServiceId,
    request: &PhenixValue,
) -> PhenixValue {
    let output = harness
        .invoke(
            service,
            &serde_json::to_vec(request).unwrap(),
            &default_suite_authority(),
            None,
        )
        .unwrap();
    serde_json::from_slice(&output).unwrap()
}

fn fixture_manifest(id: &str, service: ServiceId) -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(id).unwrap(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: phenix_core::ServiceRole::Terminal,
            service,
            priority: 100,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

struct ModelProvider;

impl PluginInstance for ModelProvider {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &model_inference_service() {
            return Err(format!("unsupported fixture model service: {service}"));
        }
        let value: PhenixValue =
            serde_json::from_slice(input).map_err(|error| error.to_string())?;
        let request =
            ModelInferenceRequest::try_from(Project(&value)).map_err(|error| error.to_string())?;
        let response = ModelInferenceResponse {
            output: [b"answer:".as_slice(), request.input.as_slice()]
                .concat()
                .into(),
            provider_metadata: BTreeMap::from([(
                "model".into(),
                PhenixValue::String(request.model.as_str().to_owned()),
            )]),
            tool_calls: Vec::new(),
        };
        serde_json::to_vec(&PhenixValue::from(&response)).map_err(|error| error.to_string())
    }
}

struct EchoTool;

impl PluginInstance for EchoTool {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        _service: &ServiceId,
        input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        Ok(input.to_vec())
    }
}

#[test]
fn supported_harness_routes_first_party_domains_through_kernel_services() {
    let mut harness = PhenixHarness::default_suite().unwrap();
    harness.activate().unwrap();

    let repository = invoke(
        &mut harness,
        "phenix.repository.worker-queue@1",
        json!({"pull_requests": [], "issues": []}),
    );
    assert!(repository.is_null());

    let sessions = invoke(
        &mut harness,
        "phenix.sessions@1",
        json!({"operation": "list"}),
    );
    assert_eq!(sessions["result"], "sessions");

    invoke(
        &mut harness,
        "phenix.sessions@1",
        json!({"operation": "create", "id": "root"}),
    );
    invoke(
        &mut harness,
        "phenix.sessions@1",
        json!({"operation": "create", "id": "child"}),
    );
    let lineage = invoke(
        &mut harness,
        "phenix.session-tree@1",
        json!({
            "operation": "link",
            "session_id": "child",
            "parent_session_id": "root"
        }),
    );
    assert_eq!(lineage["result"], "lineage");
    assert_eq!(lineage["lineage"]["session_id"], "child");
    assert_eq!(lineage["lineage"]["parent_session_id"], "root");
    let parent = invoke(
        &mut harness,
        "phenix.session-tree@1",
        json!({"operation": "parent", "session_id": "child"}),
    );
    assert_eq!(parent["result"], "parent");
    assert_eq!(parent["parent_session_id"], "root");

    let artifact = invoke(
        &mut harness,
        "phenix.artifacts@1",
        json!({
            "operation": "get",
            "id": "missing",
            "content_identity": "missing"
        }),
    );
    assert_eq!(artifact["response"], "artifact");
    assert!(artifact["artifact"].is_null());

    let context = invoke(
        &mut harness,
        "phenix.context@1",
        json!({"operation": "list"}),
    );
    assert_eq!(context["response"], "resources");

    let execution = invoke(
        &mut harness,
        "phenix.execution@1",
        json!({"operation": "runnable_tasks"}),
    );
    assert_eq!(execution["response"], "runnable_tasks");

    let language = invoke(
        &mut harness,
        "phenix.language@1",
        json!({"operation": "current_diagnostics", "workspace_id": "system"}),
    );
    assert_eq!(language["response"], "diagnostics");

    let planning = invoke(
        &mut harness,
        "phenix.planning@1",
        json!({"operation": "search_history", "objective_id": null, "query": ""}),
    );
    assert_eq!(planning["response"], "history");

    let models = invoke(
        &mut harness,
        "phenix.models.routing@1",
        json!({"operation": "list_profiles"}),
    );
    assert_eq!(models["kind"], "profiles");

    let jobs = invoke(&mut harness, "phenix.jobs@1", json!({"operation": "list"}));
    assert_eq!(jobs["response"], "resources");

    let frontends = invoke(
        &mut harness,
        "phenix.frontend-services@1",
        json!({"operation": "catalog"}),
    );
    assert_eq!(frontends["response"], "providers");

    let hooks = invoke(
        &mut harness,
        "phenix.hooks@1",
        json!({"operation": "get_configuration", "revision": "missing"}),
    );
    assert_eq!(hooks["response"], "configuration");
    assert!(hooks["configuration"].is_null());

    let workspace = invoke(
        &mut harness,
        "phenix.workspace@1",
        json!({"operation": "search", "needle": "definitely-not-a-phenix-match", "path": null, "case_sensitive": true}),
    );
    assert_eq!(workspace["response"], "search");

    let debug = invoke(
        &mut harness,
        "phenix.debug@1",
        json!({"operation": "snapshot"}),
    );
    assert_eq!(debug["response"], "snapshot");
    assert!(debug["snapshot"]["services"]
        .as_object()
        .unwrap()
        .values()
        .all(|entry| entry["state"] == "available"));

    let error = serde_json::from_value::<CliProbeRequest>(json!({
        "name": "not-a-supported-cli"
    }))
    .unwrap_err();
    assert!(error.to_string().contains("unsupported CLI probe target"));
}

#[test]
fn supported_harness_routes_model_inference_and_tool_calls_through_plugins() {
    let provider = "fixture.model-provider";
    let tool_service = ServiceId::parse("fixture.echo@1").unwrap();
    let mut builder = HarnessBuilder::with_default_suite().unwrap();
    builder
        .add_embedded(
            fixture_manifest(provider, model_inference_service()),
            || Box::new(ModelProvider),
        )
        .unwrap();
    builder
        .add_embedded(
            fixture_manifest("fixture.echo-tool", tool_service.clone()),
            || Box::new(EchoTool),
        )
        .unwrap();
    let mut harness = builder.build().unwrap();
    harness.activate().unwrap();

    let profile = RoutingProfile {
        id: RoutingProfileId::parse("parity").unwrap(),
        default_target: ModelTarget {
            provider_plugin: PluginId::parse(provider).unwrap(),
            model: ModelId::parse("fixture-model").unwrap(),
            options: BTreeMap::new(),
        },
        fallback_targets: Vec::new(),
        callable_targets: BTreeMap::new(),
    };
    let _: ModelResponse = invoke_structural(
        &mut harness,
        "phenix.models.routing@1",
        &ModelCommand::RegisterProfile {
            profile: profile.clone(),
        },
    );
    let _: ModelResponse = invoke_structural(
        &mut harness,
        "phenix.models.routing@1",
        &ModelCommand::PublishCapabilities {
            capabilities: EffectiveModelCapabilities {
                target: profile.default_target.clone(),
                generation: CapabilityGenerationId::parse("fixture-generation").unwrap(),
                context: ContextControl::ReplaceableTurns,
                capacity: CapacityKnowledge::Known {
                    limits: ModelLimits {
                        context_window_tokens: 16_000,
                        max_output_tokens: Some(2_000),
                    },
                },
                optional: BTreeSet::new(),
            },
        },
    );
    let _: ModelResponse = invoke_structural(
        &mut harness,
        "phenix.models.routing@1",
        &ModelCommand::SetProviderAuthenticated {
            provider_plugin: PluginId::parse(provider).unwrap(),
            authenticated: true,
        },
    );

    let _: ExecutionResponse = invoke_structural(
        &mut harness,
        "phenix.execution@1",
        &ExecutionCommand::CreateExecution {
            id: "root".into(),
            requested_authority: ExecutionAuthority::new(Vec::<String>::new()),
        },
    );
    let _: ExecutionResourceResponse = invoke_structural(
        &mut harness,
        "phenix.execution.resources@1",
        &ExecutionResourceCommand::RegisterRootBudget {
            ledger: RootBudgetLedger {
                root_execution_id: "root".into(),
                limits: RootBudgetLimits {
                    fresh_input_tokens: 8_000,
                    output_tokens: 2_000,
                    cost_microunits: None,
                    attempts: 4,
                },
                reservations: BTreeMap::new(),
            },
        },
    );

    let response: StepRunnerResponse = invoke_structural(
        &mut harness,
        "phenix.invocation@1",
        &InvocationCommand::Invoke {
            request: InvocationRequest {
                execution_id: "root".into(),
                parent_attempt_id: None,
                callable_id: None,
                input: b"hello".to_vec().into(),
                tools: Vec::new(),
            },
            params: InvocationParams {
                profile_id: RoutingProfileId::parse("parity").unwrap(),
                policy: UsagePolicy {
                    revision: "fixture-policy".into(),
                    max_fresh_input_tokens: 4_000,
                    max_output_tokens: 512,
                    max_cost_microunits: None,
                    max_retries: 0,
                    max_tool_result_bytes: 64 * 1024,
                    max_tool_schemas: 4,
                    max_skills: 4,
                    require_known_capacity: true,
                    delegation: DelegationResourcePolicy::default(),
                },
                intent: InvocationIntent {
                    output_reserve_tokens: 256,
                    required_context_capabilities: BTreeSet::new(),
                    required_capabilities: BTreeSet::new(),
                    required_tools: BTreeSet::new(),
                    optional_tools: BTreeSet::new(),
                    required_skills: BTreeSet::new(),
                    optional_skills: BTreeSet::new(),
                    requested_reasoning: None,
                    deadline_at_ms: None,
                },
                route_policy: RouteSelectionPolicy {
                    revision: "fixture-route-policy".into(),
                    estimates: RoutingEstimateMode::Ignore,
                    max_candidate_attempts: 2,
                },
            },
        },
    );
    let StepRunnerResponse::Completed {
        attempt, output, ..
    } = response;
    let route = attempt
        .route
        .as_ref()
        .expect("central invocation bound a route");
    assert_eq!(route.target.provider_plugin.as_str(), provider);
    assert_eq!(route.target.model.as_str(), "fixture-model");
    let text = String::from_utf8(output.as_ref().to_vec()).unwrap();
    assert!(text.starts_with("answer:"));
    assert!(text.contains("hello"));
    assert_eq!(
        attempt.plan.routing.context.mandatory_input_tokens,
        attempt
            .plan
            .context
            .mandatory_input_tokens
            .saturating_add(5)
    );
    assert_eq!(
        attempt.plan.reservation.input_tokens,
        attempt.plan.context.total_input_tokens().saturating_add(5)
    );

    let _: ExecutionResponse = invoke_structural(
        &mut harness,
        "phenix.execution@1",
        &ExecutionCommand::RegisterCallable {
            id: "echo".into(),
            service: tool_service.to_string(),
            required_authority: ExecutionAuthority::new(Vec::<String>::new()),
        },
    );
    let output: ExecutionResponse = invoke_structural(
        &mut harness,
        "phenix.execution@1",
        &ExecutionCommand::InvokeCallable {
            execution_id: "root".into(),
            callable_id: "echo".into(),
            input: br#"{"value":"hello"}"#.to_vec(),
        },
    );
    match output {
        ExecutionResponse::Invocation { output } => {
            assert_eq!(output, br#"{"value":"hello"}"#);
        }
        other => panic!("unexpected execution response: {other:?}"),
    }
}

#[test]
fn hook_behavior_is_omittable_and_replaceable_through_harness_composition() {
    let hook_service = ServiceId::parse("phenix.hooks@1").unwrap();
    let selected = BTreeSet::new();

    let mut without_hooks = HarnessBuilder::with_selected_suite(&selected)
        .unwrap()
        .build()
        .unwrap();
    without_hooks.activate().unwrap();
    let request = HookCommand::GetConfiguration {
        revision: "missing".into(),
    };
    let error = without_hooks
        .invoke(
            &hook_service,
            &serde_json::to_vec(&PhenixValue::from(&request)).unwrap(),
            &default_suite_authority(),
            None,
        )
        .unwrap_err();
    assert!(error.to_string().contains("no eligible provider"));

    let mut replacement_builder = HarnessBuilder::with_selected_suite(&selected).unwrap();
    replacement_builder
        .add_embedded(
            fixture_manifest("fixture.hooks", hook_service.clone()),
            || Box::new(EchoTool),
        )
        .unwrap();
    let mut replacement = replacement_builder.build().unwrap();
    replacement.activate().unwrap();
    let request = PhenixValue::Bool(true);
    assert_eq!(
        invoke_value_raw(&mut replacement, &hook_service, &request),
        request
    );
}
