use phenix_core::{
    Authority, CapabilityGenerationId, ComponentExport, ComponentId, ComponentInterface,
    ComponentManifest, Kernel, KernelConfig, LocalPersistence, ModelId, ModelInferenceRequest,
    ModelInferenceResponse, PhenixValue, PluginContext, PluginExecution, PluginHost, PluginId,
    PluginInstance, PluginManifest, Project, ResolvedHarness, ResolvedHarnessActivation,
    ServiceContribution, ServiceId, ServiceRole, ValueError,
};
use phenix_plugin_context::{context_component_manifest, context_factory, context_manifest};
use phenix_plugin_execution::{
    execution_component_manifest, execution_factory, execution_manifest,
};
use phenix_plugin_models::{
    model_inference_service, model_routing_component_manifest, model_routing_factory,
    model_routing_manifest,
};
use phenix_plugin_step_runner::{
    step_runner_component_manifest, step_runner_factory, step_runner_manifest,
};
use phenix_sdk::{
    default_invocation_service, execution_resource_service, execution_service,
    invocation_clock_service, invocation_defaults_service, invocation_service,
    step_attempt_service, CapacityKnowledge, ContextControl, DefaultInvocationCommand,
    DelegationResourcePolicy, EffectiveModelCapabilities, ExecutionAuthority, ExecutionCommand,
    ExecutionResourceCommand, ExecutionResourceResponse, InvocationClockInterface,
    InvocationClockResponse, InvocationCommand, InvocationDefaultsInterface,
    InvocationDefaultsResponse, InvocationIntent, InvocationParams, InvocationRequest, ModelCommand,
    ModelLimits, ModelResponse, ModelTarget, RouteSelectionPolicy, RoutingEstimateMode,
    RoutingProfile, StepAttemptCommand, StepAttemptResponse, StepRunnerResponse, UsagePolicy,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

const SUPPORT_PLUGIN: &str = "fixture.invocation-support";
const SUPPORT_COMPONENT: &str = "fixture.invocation-support";

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
                tool_calls: Vec::new(),
            })
            .map_err(|error| error.to_string())
    }
}

struct InvocationSupport {
    defaults: bool,
}

impl PluginInstance for InvocationSupport {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        _input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        let context = PluginContext::new(host, (), (), ());
        if service == &invocation_clock_service() {
            return context
                .kernel
                .encode_value(&InvocationClockResponse::Time { now_ms: 1_000 })
                .map_err(|error| error.to_string());
        }
        if self.defaults && service == &invocation_defaults_service() {
            return context
                .kernel
                .encode_value(&InvocationDefaultsResponse::Params {
                    params: params("fixture-default-policy"),
                })
                .map_err(|error| error.to_string());
        }
        Err(format!("unsupported invocation support service: {service}"))
    }
}

fn authority() -> Authority {
    Authority::new([
        phenix_core::CapabilityId::parse("kernel.persistence.schema").unwrap(),
        phenix_core::CapabilityId::parse("kernel.persistence.read").unwrap(),
        phenix_core::CapabilityId::parse("kernel.persistence.write").unwrap(),
    ])
}

fn temp_db(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "phenix-invocation-{name}-{}-{nonce}.sqlite",
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

fn support_manifest(defaults: bool) -> PluginManifest {
    let mut services = vec![ServiceContribution {
        role: ServiceRole::Terminal,
        service: invocation_clock_service(),
        priority: 100,
        required_authority: Authority::default(),
    }];
    if defaults {
        services.push(ServiceContribution {
            role: ServiceRole::Terminal,
            service: invocation_defaults_service(),
            priority: 100,
            required_authority: Authority::default(),
        });
    }
    PluginManifest {
        id: PluginId::parse(SUPPORT_PLUGIN).unwrap(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services,
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn support_component(defaults: bool) -> ComponentManifest {
    let mut exports = vec![ComponentExport {
        interface: InvocationClockInterface::interface_id(),
        schema: InvocationClockInterface::schema(),
        priority: 100,
        required_authority: Authority::default(),
    }];
    if defaults {
        exports.push(ComponentExport {
            interface: InvocationDefaultsInterface::interface_id(),
            schema: InvocationDefaultsInterface::schema(),
            priority: 100,
            required_authority: Authority::default(),
        });
    }
    ComponentManifest {
        listeners: Vec::new(),
        id: ComponentId::parse(SUPPORT_COMPONENT).unwrap(),
        owner: PluginId::parse(SUPPORT_PLUGIN).unwrap(),
        imports: Vec::new(),
        exports,
        maximum_authority: Authority::default(),
    }
}

fn kernel(path: &PathBuf, defaults: bool) -> Kernel {
    let authority = authority();
    let execution = execution_manifest(authority.clone());
    let context = context_manifest();
    let models = model_routing_manifest(authority.clone());
    let runner = step_runner_manifest(authority.clone());
    let provider = provider_manifest();
    let support = support_manifest(defaults);
    let execution_id = execution.id.clone();
    let context_id = context.id.clone();
    let models_id = models.id.clone();
    let runner_id = runner.id.clone();
    let provider_id = provider.id.clone();
    let support_id = support.id.clone();
    let resolved = ResolvedHarness::resolve(
        [
            execution.clone(),
            context.clone(),
            models.clone(),
            runner.clone(),
            provider.clone(),
            support.clone(),
        ],
        [
            execution_component_manifest(authority.clone()),
            context_component_manifest(),
            model_routing_component_manifest(authority.clone()),
            step_runner_component_manifest(authority.clone()),
            support_component(defaults),
        ],
        [],
        &authority,
    )
    .unwrap();
    let persistence = LocalPersistence::open(path).unwrap();
    let mut kernel = Kernel::with_persistence(
        KernelConfig::new([execution, context, models, runner, provider, support]).unwrap(),
        persistence,
    );
    kernel.activate_resolved_harness(&resolved).unwrap();
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
        .register_embedded_factory(runner_id, step_runner_factory)
        .unwrap();
    kernel
        .register_embedded_factory(provider_id, || Box::new(FixtureProvider))
        .unwrap();
    kernel
        .register_embedded_factory(support_id, move || Box::new(InvocationSupport { defaults }))
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
        &ModelCommand::PublishCapabilities {
            capabilities: EffectiveModelCapabilities {
                target: target(),
                generation: CapabilityGenerationId::parse("generation-1").unwrap(),
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
    let _: ModelResponse = invoke(
        kernel,
        phenix_sdk::model_routing_service(),
        &ModelCommand::SetProviderAuthenticated {
            provider_plugin: PluginId::parse("fixture.provider").unwrap(),
            authenticated: true,
        },
    );
}

fn request() -> InvocationRequest {
    InvocationRequest {
        execution_id: "root".into(),
        parent_attempt_id: None,
        callable_id: None,
        input: b"public invocation request".to_vec().into(),
        tools: Vec::new(),
    }
}

fn params(revision: &str) -> InvocationParams {
    InvocationParams {
        profile_id: phenix_core::RoutingProfileId::parse("default").unwrap(),
        policy: UsagePolicy {
            revision: revision.into(),
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
    }
}

fn assert_completed(response: StepRunnerResponse, expected_policy: &str) {
    let StepRunnerResponse::Completed {
        attempt, output, ..
    } = response;
    assert_eq!(attempt.attribution.root_execution_id, "root");
    assert_eq!(attempt.attribution.execution_id, "root");
    assert_eq!(attempt.attribution.policy_revision, expected_policy);
    assert!(!attempt.attribution.attempt_id.is_empty());
    let text = String::from_utf8(output.as_ref().to_vec()).unwrap();
    assert!(text.contains("You are an AI agent powered by Phenix."));
    assert!(text.contains("public invocation request"));
}

#[test]
fn direct_invocation_needs_clock_but_no_defaults_provider() {
    let path = temp_db("direct");
    let mut kernel = kernel(&path, false);
    setup(&mut kernel);

    let response: StepRunnerResponse = invoke(
        &mut kernel,
        invocation_service(),
        &InvocationCommand::Invoke {
            request: request(),
            params: params("direct-policy"),
        },
    );
    assert_completed(response, "direct-policy");

    let attempts: StepAttemptResponse = invoke(
        &mut kernel,
        step_attempt_service(),
        &StepAttemptCommand::ListRoot {
            root_execution_id: "root".into(),
        },
    );
    let StepAttemptResponse::Attempts { attempts } = attempts else {
        panic!("expected attempt list");
    };
    assert_eq!(attempts.len(), 1);
    let _ = fs::remove_file(path);
}

#[test]
fn default_invocation_uses_replaceable_defaults_then_same_central_path() {
    let path = temp_db("default");
    let mut kernel = kernel(&path, true);
    setup(&mut kernel);

    let response: StepRunnerResponse = invoke(
        &mut kernel,
        default_invocation_service(),
        &DefaultInvocationCommand::Invoke { request: request() },
    );
    assert_completed(response, "fixture-default-policy");

    let _ = fs::remove_file(path);
}
