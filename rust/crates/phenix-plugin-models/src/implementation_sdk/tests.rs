use super::*;
use phenix_core::{
    CapabilityGenerationId, Kernel, KernelConfig, LocalPersistence, ModelId, ModelToolDescriptor,
    PhenixValue, Project,
};
use phenix_sdk::{
    CapacityKnowledge, ContextControl, ContextDemand, EffectiveModelCapabilities,
    ModelDispatchCommand, ModelDispatchResponse, ModelLimits, RouteDecision, RouteSelectionPolicy,
    RoutingEstimateMode, RoutingRequirements,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct PlainProvider;

impl PluginInstance for PlainProvider {
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
        let response = ModelInferenceResponse {
            output: request.input,
            provider_metadata: BTreeMap::from([
                (
                    "provider".into(),
                    serde_json::json!("fixture.provider").into(),
                ),
                (
                    "model".into(),
                    serde_json::json!(request.model.as_str()).into(),
                ),
            ]),
            tool_calls: Vec::new(),
        };
        context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string())
    }
}

fn temp_db(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "phenix-{name}-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

fn target(provider: &str, model: &str) -> ModelTarget {
    ModelTarget {
        provider_plugin: PluginId::parse(provider).unwrap(),
        model: ModelId::parse(model).unwrap(),
        options: BTreeMap::new(),
    }
}

fn profile() -> RoutingProfile {
    RoutingProfile {
        id: RoutingProfileId::parse("default").unwrap(),
        default_target: target("provider.default", "root"),
        fallback_targets: vec![target("provider.fallback", "fallback")],
        callable_targets: BTreeMap::from([(
            CallableId::parse("agent.scout").unwrap(),
            target("provider.scout", "scout"),
        )]),
    }
}

fn capabilities(
    target: ModelTarget,
    generation: &str,
    context_window_tokens: u64,
) -> EffectiveModelCapabilities {
    EffectiveModelCapabilities {
        target,
        generation: CapabilityGenerationId::parse(generation).unwrap(),
        context: ContextControl::ReplaceableTurns,
        capacity: CapacityKnowledge::Known {
            limits: ModelLimits {
                context_window_tokens,
                max_output_tokens: Some(2_000),
            },
        },
        optional: BTreeSet::new(),
    }
}

fn routing_authority() -> Authority {
    model_routing_manifest(Authority::default()).maximum_authority
}

fn provider_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse("fixture.provider").unwrap(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: phenix_core::ServiceRole::Terminal,
            service: model_inference_service(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn kernel_with(path: &PathBuf) -> Kernel {
    let manifest = model_routing_manifest(Authority::default());
    let plugin = manifest.id.clone();
    let persistence = LocalPersistence::open(path).unwrap();
    let mut kernel = Kernel::with_persistence(KernelConfig::new([manifest]).unwrap(), persistence);
    kernel
        .register_embedded_factory(plugin, model_routing_factory)
        .unwrap();
    kernel.activate_all().unwrap();
    kernel
}

fn kernel_with_provider(path: &PathBuf) -> Kernel {
    let routing = model_routing_manifest(Authority::default());
    let provider = provider_manifest();
    let routing_id = routing.id.clone();
    let provider_id = provider.id.clone();
    let persistence = LocalPersistence::open(path).unwrap();
    let mut kernel =
        Kernel::with_persistence(KernelConfig::new([routing, provider]).unwrap(), persistence);
    kernel
        .register_embedded_factory(routing_id, model_routing_factory)
        .unwrap();
    kernel
        .register_embedded_factory(provider_id, || Box::new(PlainProvider))
        .unwrap();
    kernel.activate_all().unwrap();
    kernel
}

fn invoke_routing(kernel: &mut Kernel, command: ModelCommand) -> Result<ModelResponse, String> {
    let output = kernel
        .invoke(
            &model_routing_service(),
            &serde_json::to_vec(&PhenixValue::from(&command)).unwrap(),
            &routing_authority(),
            None,
        )
        .map_err(|error| error.to_string())?;
    let output: PhenixValue = serde_json::from_slice(&output).map_err(|error| error.to_string())?;
    ModelResponse::try_from(Project(&output)).map_err(|error| error.to_string())
}

fn invoke_dispatch(
    kernel: &mut Kernel,
    command: ModelDispatchCommand,
) -> Result<ModelDispatchResponse, String> {
    let output = kernel
        .invoke(
            &model_dispatch_service(),
            &serde_json::to_vec(&PhenixValue::from(&command)).unwrap(),
            &routing_authority(),
            None,
        )
        .map_err(|error| error.to_string())?;
    let output: PhenixValue = serde_json::from_slice(&output).map_err(|error| error.to_string())?;
    ModelDispatchResponse::try_from(Project(&output)).map_err(|error| error.to_string())
}

mod profile_store {
    use super::*;

    #[test]
    fn immutable_profile_survives_restart_and_describes_all_providers() {
        let path = temp_db("routing-profile-store");
        let profile = profile();
        {
            let mut kernel = kernel_with(&path);
            invoke_routing(
                &mut kernel,
                ModelCommand::RegisterProfile {
                    profile: profile.clone(),
                },
            )
            .unwrap();
            assert!(invoke_routing(
                &mut kernel,
                ModelCommand::RegisterProfile {
                    profile: profile.clone(),
                },
            )
            .unwrap_err()
            .contains("already registered"));
        }
        let mut restored = kernel_with(&path);
        let ModelResponse::Profiles { profiles } =
            invoke_routing(&mut restored, ModelCommand::ListProfiles).unwrap()
        else {
            panic!("expected profiles response");
        };
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].providers.len(), 3);
        let _ = fs::remove_file(path);
    }
}

mod smart_selection {
    use super::*;

    #[test]
    fn hard_capacity_rejects_primary_before_fallback_selection() {
        let path = temp_db("routing-smart-selection");
        let profile = profile();
        let mut kernel = kernel_with(&path);
        invoke_routing(
            &mut kernel,
            ModelCommand::RegisterProfile {
                profile: profile.clone(),
            },
        )
        .unwrap();
        for capabilities in [
            capabilities(profile.default_target.clone(), "generation-1", 1_500),
            capabilities(profile.fallback_targets[0].clone(), "generation-1", 8_000),
        ] {
            invoke_routing(
                &mut kernel,
                ModelCommand::PublishCapabilities { capabilities },
            )
            .unwrap();
        }
        let response = invoke_routing(
            &mut kernel,
            ModelCommand::ResolveWithRequirements {
                profile_id: profile.id,
                callable_id: None,
                requirements: RoutingRequirements {
                    context: ContextDemand {
                        mandatory_input_tokens: 1_000,
                        reducible_input_tokens: 500,
                        output_reserve_tokens: 500,
                        required_capabilities: BTreeSet::new(),
                    },
                    required_capabilities: BTreeSet::new(),
                    require_known_capacity: true,
                },
                policy: RouteSelectionPolicy {
                    revision: "route-policy-1".into(),
                    estimates: RoutingEstimateMode::Ignore,
                    max_candidate_attempts: 2,
                },
            },
        )
        .unwrap();
        let ModelResponse::Decision { selection } = response else {
            panic!("expected routing decision");
        };
        assert_eq!(selection.decision.target.model.as_str(), "fallback");
        assert_eq!(selection.rejected.len(), 1);
        let _ = fs::remove_file(path);
    }
}

mod runtime_persistence {
    use super::*;

    #[test]
    fn published_capabilities_survive_restart() {
        let path = temp_db("routing-runtime-persistence");
        let profile = profile();
        {
            let mut kernel = kernel_with(&path);
            invoke_routing(
                &mut kernel,
                ModelCommand::RegisterProfile {
                    profile: profile.clone(),
                },
            )
            .unwrap();
            for target in
                std::iter::once(&profile.default_target).chain(profile.fallback_targets.iter())
            {
                invoke_routing(
                    &mut kernel,
                    ModelCommand::PublishCapabilities {
                        capabilities: capabilities(target.clone(), "generation-1", 8_000),
                    },
                )
                .unwrap();
            }
        }
        let mut restored = kernel_with(&path);
        let ModelResponse::Candidates { candidates } = invoke_routing(
            &mut restored,
            ModelCommand::ListCandidates {
                profile_id: profile.id,
                callable_id: None,
            },
        )
        .unwrap() else {
            panic!("expected routing candidates");
        };
        assert_eq!(candidates.len(), 2);
        let _ = fs::remove_file(path);
    }
}

mod provider_dispatch {
    use super::*;

    #[test]
    fn legacy_invoke_is_rejected_even_when_provider_is_authenticated() {
        let path = temp_db("routing-provider-dispatch");
        let profile = RoutingProfile {
            id: RoutingProfileId::parse("default").unwrap(),
            default_target: target("fixture.provider", "fixture"),
            fallback_targets: Vec::new(),
            callable_targets: BTreeMap::new(),
        };
        let mut kernel = kernel_with_provider(&path);
        invoke_routing(
            &mut kernel,
            ModelCommand::RegisterProfile {
                profile: profile.clone(),
            },
        )
        .unwrap();
        invoke_routing(
            &mut kernel,
            ModelCommand::SetProviderAuthenticated {
                provider_plugin: PluginId::parse("fixture.provider").unwrap(),
                authenticated: true,
            },
        )
        .unwrap();
        assert!(invoke_routing(
            &mut kernel,
            ModelCommand::Invoke {
                profile_id: profile.id,
                callable_id: None,
                input: b"hello".to_vec().into(),
                tools: Vec::<ModelToolDescriptor>::new(),
            },
        )
        .unwrap_err()
        .contains("legacy model invocation is disabled"));
        let _ = fs::remove_file(path);
    }
}

mod resolved_dispatch {
    use super::*;

    fn authenticate(kernel: &mut Kernel, authenticated: bool) {
        invoke_routing(
            kernel,
            ModelCommand::SetProviderAuthenticated {
                provider_plugin: PluginId::parse("fixture.provider").unwrap(),
                authenticated,
            },
        )
        .unwrap();
    }

    fn decision(target: ModelTarget, generation: &str) -> RouteDecision {
        RouteDecision {
            target,
            capability_generation: CapabilityGenerationId::parse(generation).unwrap(),
            policy_revision: "route-policy-1".into(),
            candidate_ordinal: 0,
            estimate: None,
        }
    }

    fn prepare(
        kernel: &mut Kernel,
        decision: RouteDecision,
        input: &[u8],
    ) -> Result<phenix_sdk::PreparedDispatch, String> {
        let response = invoke_dispatch(
            kernel,
            ModelDispatchCommand::PrepareResolved {
                decision,
                input: input.to_vec().into(),
                tools: Vec::new(),
            },
        )?;
        match response {
            ModelDispatchResponse::Ready { prepared } => Ok(prepared),
            ModelDispatchResponse::Inference { .. } => {
                Err("dispatch returned inference during preparation".into())
            }
        }
    }

    #[test]
    fn exact_selected_target_reaches_provider_unchanged() {
        let path = temp_db("resolved-dispatch-exact");
        let mut kernel = kernel_with_provider(&path);
        authenticate(&mut kernel, true);
        let target = target("fixture.provider", "selected-fallback");
        invoke_routing(
            &mut kernel,
            ModelCommand::PublishCapabilities {
                capabilities: capabilities(target.clone(), "generation-1", 8_000),
            },
        )
        .unwrap();
        let decision = decision(target, "generation-1");
        let prepared = prepare(&mut kernel, decision.clone(), b"exact").unwrap();
        assert_eq!(prepared.decision(), &decision);
        let response = invoke_dispatch(
            &mut kernel,
            ModelDispatchCommand::InvokePrepared { prepared },
        )
        .unwrap();
        let ModelDispatchResponse::Inference {
            decision: returned,
            response,
        } = response
        else {
            panic!("expected inference response");
        };
        assert_eq!(returned, decision);
        assert_eq!(response.output.as_ref(), b"exact");
        assert_eq!(
            response.provider_metadata["model"],
            serde_json::json!("selected-fallback").into()
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn stale_generation_is_rejected_during_preparation() {
        let path = temp_db("resolved-dispatch-stale");
        let mut kernel = kernel_with_provider(&path);
        authenticate(&mut kernel, true);
        let target = target("fixture.provider", "selected");
        invoke_routing(
            &mut kernel,
            ModelCommand::PublishCapabilities {
                capabilities: capabilities(target.clone(), "generation-1", 8_000),
            },
        )
        .unwrap();
        let decision = decision(target.clone(), "generation-1");
        invoke_routing(
            &mut kernel,
            ModelCommand::PublishCapabilities {
                capabilities: capabilities(target, "generation-2", 8_000),
            },
        )
        .unwrap();
        let error = prepare(&mut kernel, decision, b"must-not-run").unwrap_err();
        assert!(error.contains("StaleCapabilityGeneration"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn missing_authentication_is_rejected_during_preparation() {
        let path = temp_db("resolved-dispatch-auth");
        let mut kernel = kernel_with_provider(&path);
        let target = target("fixture.provider", "selected");
        invoke_routing(
            &mut kernel,
            ModelCommand::PublishCapabilities {
                capabilities: capabilities(target.clone(), "generation-1", 8_000),
            },
        )
        .unwrap();
        let error = prepare(
            &mut kernel,
            decision(target, "generation-1"),
            b"must-not-run",
        )
        .unwrap_err();
        assert!(error.contains("authentication required"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn prepared_dispatch_is_not_rejected_when_mutable_state_changes() {
        let path = temp_db("resolved-dispatch-prepared-snapshot");
        let mut kernel = kernel_with_provider(&path);
        authenticate(&mut kernel, true);
        let target = target("fixture.provider", "selected");
        invoke_routing(
            &mut kernel,
            ModelCommand::PublishCapabilities {
                capabilities: capabilities(target.clone(), "generation-1", 8_000),
            },
        )
        .unwrap();
        let original = decision(target.clone(), "generation-1");
        let prepared = prepare(&mut kernel, original.clone(), b"cross-boundary").unwrap();

        authenticate(&mut kernel, false);
        invoke_routing(
            &mut kernel,
            ModelCommand::PublishCapabilities {
                capabilities: capabilities(target, "generation-2", 8_000),
            },
        )
        .unwrap();

        let response = invoke_dispatch(
            &mut kernel,
            ModelDispatchCommand::InvokePrepared { prepared },
        )
        .unwrap();
        let ModelDispatchResponse::Inference {
            decision: returned,
            response,
        } = response
        else {
            panic!("expected inference response");
        };
        assert_eq!(returned, original);
        assert_eq!(response.output.as_ref(), b"cross-boundary");
        let _ = fs::remove_file(path);
    }
}