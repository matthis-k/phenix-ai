use super::*;
use phenix_core::{
    InvocationOutcome, Kernel, KernelConfig, LocalPersistence, ModelFeatureGenerationId, ModelId,
    PhenixValue, Project,
};
use phenix_sdk::{
    CapacityKnowledge, ContextControl, ContextDemand, EffectiveModelFeatures, ModelDispatchCommand,
    ModelDispatchResponse, ModelLimits, RouteDecision, RouteSelectionPolicy, RoutingEstimateMode,
    RoutingRequirements,
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
            usage: Default::default(),
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
            phenix_core::CallableId::parse("agent.scout").unwrap(),
            target("provider.scout", "scout"),
        )]),
    }
}

fn features(
    target: ModelTarget,
    generation: &str,
    context_window_tokens: u64,
) -> EffectiveModelFeatures {
    EffectiveModelFeatures {
        target,
        generation: ModelFeatureGenerationId::parse(generation).unwrap(),
        context: ContextControl::ReplaceableTurns,
        capacity: CapacityKnowledge::Known {
            limits: ModelLimits {
                context_window_tokens,
                max_output_tokens: Some(2_000),
            },
        },
        cache: Default::default(),
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

fn apply_provider_catalog(
    kernel: &mut Kernel,
    provider: &str,
    profiles: Vec<RoutingProfile>,
) -> Result<(), String> {
    let response = invoke_routing(
        kernel,
        ModelCommand::PublishProviderCatalogProfiles {
            provider_plugin: PluginId::parse(provider).unwrap(),
            profiles,
        },
    )?;
    if !matches!(response, ModelResponse::Profiles { .. }) {
        return Err("expected published provider catalog profiles".into());
    }
    Ok(())
}

fn invoke_dispatch_outcome(
    kernel: &mut Kernel,
    command: ModelDispatchCommand,
) -> Result<InvocationOutcome, String> {
    let output = kernel
        .invoke(
            &model_dispatch_service(),
            &serde_json::to_vec(&PhenixValue::from(&command)).unwrap(),
            &routing_authority(),
            None,
        )
        .map_err(|error| error.to_string())?;
    let output: PhenixValue = serde_json::from_slice(&output).map_err(|error| error.to_string())?;
    Ok(InvocationOutcome::from_transport_value(output))
}

fn invoke_dispatch(
    kernel: &mut Kernel,
    command: ModelDispatchCommand,
) -> Result<ModelDispatchResponse, String> {
    match invoke_dispatch_outcome(kernel, command)? {
        InvocationOutcome::Success(output) => {
            ModelDispatchResponse::try_from(Project(&output)).map_err(|error| error.to_string())
        }
        InvocationOutcome::DomainError(output) => {
            let failure = ModelDispatchFailure::try_from(Project(&output))
                .map_err(|error| error.to_string())?;
            Err(failure.failure.message().to_owned())
        }
    }
}

fn dispatch_failure(
    kernel: &mut Kernel,
    command: ModelDispatchCommand,
) -> Result<ModelDispatchFailure, String> {
    match invoke_dispatch_outcome(kernel, command)? {
        InvocationOutcome::Success(_) => Err("expected model dispatch domain failure".into()),
        InvocationOutcome::DomainError(output) => {
            ModelDispatchFailure::try_from(Project(&output)).map_err(|error| error.to_string())
        }
    }
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

    #[test]
    fn packaged_profiles_do_not_adopt_foreign_identity() {
        let path = temp_db("routing-packaged-foreign");
        let profile = profile();
        let mut kernel = kernel_with(&path);
        invoke_routing(
            &mut kernel,
            ModelCommand::RegisterProfile {
                profile: profile.clone(),
            },
        )
        .unwrap();

        let error = invoke_routing(
            &mut kernel,
            ModelCommand::PreparePackagedProfiles {
                profiles: vec![profile],
            },
        )
        .unwrap_err();
        assert!(error.contains("already owned outside packaged configuration"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn provider_catalog_does_not_adopt_foreign_identity() {
        let path = temp_db("routing-provider-catalog-foreign");
        let provider = "provider.catalog";
        let profile = RoutingProfile {
            id: RoutingProfileId::parse("catalog.model-a").unwrap(),
            default_target: target(provider, "model-a"),
            fallback_targets: Vec::new(),
            callable_targets: BTreeMap::new(),
        };
        let mut kernel = kernel_with(&path);
        invoke_routing(
            &mut kernel,
            ModelCommand::RegisterProfile {
                profile: profile.clone(),
            },
        )
        .unwrap();

        let error = apply_provider_catalog(&mut kernel, provider, vec![profile]).unwrap_err();
        assert!(error.contains("already owned outside provider catalog"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn provider_catalog_refresh_retires_missing_models_without_deleting_durable_routes() {
        let path = temp_db("routing-provider-catalog");
        let provider = "provider.catalog";
        let fixed = |id: &str, model: &str| RoutingProfile {
            id: RoutingProfileId::parse(id).unwrap(),
            default_target: target(provider, model),
            fallback_targets: Vec::new(),
            callable_targets: BTreeMap::new(),
        };
        let first = fixed("catalog.model-a", "model-a");
        let second = fixed("catalog.model-b", "model-b");

        {
            let mut kernel = kernel_with(&path);
            apply_provider_catalog(&mut kernel, provider, vec![first.clone(), second.clone()])
                .unwrap();

            let ModelResponse::Profiles { profiles } =
                invoke_routing(&mut kernel, ModelCommand::ListProfiles).unwrap()
            else {
                panic!("expected profiles response");
            };
            assert_eq!(
                profiles
                    .iter()
                    .map(|profile| profile.id.as_str())
                    .collect::<Vec<_>>(),
                vec!["catalog.model-a", "catalog.model-b"]
            );

            apply_provider_catalog(&mut kernel, provider, vec![second.clone()]).unwrap();
            let ModelResponse::Profiles { profiles } =
                invoke_routing(&mut kernel, ModelCommand::ListProfiles).unwrap()
            else {
                panic!("expected profiles response");
            };
            assert_eq!(profiles.len(), 1);
            assert_eq!(profiles[0].id, second.id);

            assert_eq!(
                invoke_routing(
                    &mut kernel,
                    ModelCommand::GetProfile {
                        id: first.id.clone()
                    },
                )
                .unwrap(),
                ModelResponse::Profile {
                    profile: Some(first.clone())
                }
            );
        }

        let mut restored = kernel_with(&path);
        let ModelResponse::Profiles { profiles } =
            invoke_routing(&mut restored, ModelCommand::ListProfiles).unwrap()
        else {
            panic!("expected profiles response");
        };
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, second.id);
        assert_eq!(
            invoke_routing(
                &mut restored,
                ModelCommand::GetProfile {
                    id: first.id.clone()
                },
            )
            .unwrap(),
            ModelResponse::Profile {
                profile: Some(first)
            }
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn profile_replacement_is_compare_and_swap_and_survives_restart() {
        let path = temp_db("routing-profile-replacement");
        let original = profile();
        let mut replacement = original.clone();
        replacement.default_target = target("provider.default", "root-v2");

        {
            let mut kernel = kernel_with(&path);
            invoke_routing(
                &mut kernel,
                ModelCommand::RegisterProfile {
                    profile: original.clone(),
                },
            )
            .unwrap();

            let response = invoke_routing(
                &mut kernel,
                ModelCommand::ReplaceProfile {
                    expected: original.clone(),
                    profile: replacement.clone(),
                },
            )
            .unwrap();
            assert_eq!(
                response,
                ModelResponse::Profile {
                    profile: Some(replacement.clone())
                }
            );

            assert!(invoke_routing(
                &mut kernel,
                ModelCommand::ReplaceProfile {
                    expected: original,
                    profile: replacement.clone(),
                },
            )
            .unwrap_err()
            .contains("replacement conflict"));
        }

        let mut restored = kernel_with(&path);
        assert_eq!(
            invoke_routing(
                &mut restored,
                ModelCommand::GetProfile {
                    id: replacement.id.clone()
                },
            )
            .unwrap(),
            ModelResponse::Profile {
                profile: Some(replacement)
            }
        );
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
        for features in [
            features(profile.default_target.clone(), "generation-1", 1_500),
            features(profile.fallback_targets[0].clone(), "generation-1", 8_000),
        ] {
            invoke_routing(&mut kernel, ModelCommand::PublishModelFeatures { features }).unwrap();
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
                        required_features: BTreeSet::new(),
                    },
                    required_features: BTreeSet::new(),
                    require_known_capacity: true,
                },
                policy: RouteSelectionPolicy {
                    revision: "route-policy-1".into(),
                    estimates: RoutingEstimateMode::Ignore,
                    max_candidate_attempts: Some(2),
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
                    ModelCommand::PublishModelFeatures {
                        features: features(target.clone(), "generation-1", 8_000),
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

mod resolved_dispatch {
    use super::*;

    fn decision(target: ModelTarget, generation: &str) -> RouteDecision {
        RouteDecision {
            target,
            feature_generation: ModelFeatureGenerationId::parse(generation).unwrap(),
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
                session_id: None,
                decision,
                input: input.to_vec().into(),
                cache: Default::default(),
                tools: Vec::new(),
                continuation: Vec::new(),
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
    fn cache_prefix_boundary_activates_only_for_supported_target() {
        let target = target("provider.default", "root");
        let mut supported = features(target.clone(), "generation-1", 10_000);
        supported.cache.breakpoint_control = phenix_sdk::FeatureSupport::Supported;

        let requested = phenix_core::ModelCacheControl {
            explicit_prefix_bytes: Some(128),
            local_prefix_identity: Some("sha256:prefix".into()),
            ..Default::default()
        };
        let effective = effective_cache_control(requested.clone(), &supported).unwrap();
        assert_eq!(
            effective.write,
            phenix_core::ModelCacheWritePolicy::ExplicitPrefix
        );
        assert_eq!(effective.explicit_prefix_bytes, Some(128));

        let mut unsupported = supported;
        unsupported.cache.breakpoint_control = phenix_sdk::FeatureSupport::Unsupported;
        let effective = effective_cache_control(requested, &unsupported).unwrap();
        assert_eq!(
            effective.write,
            phenix_core::ModelCacheWritePolicy::ProviderDefault
        );
        assert_eq!(effective.explicit_prefix_bytes, None);
        assert_eq!(
            effective.local_prefix_identity.as_deref(),
            Some("sha256:prefix")
        );
    }

    #[test]
    fn cache_hint_does_not_change_provider_visible_context() {
        let path = temp_db("resolved-dispatch-cache-input");
        let mut kernel = kernel_with_provider(&path);
        let target = target("fixture.provider", "selected");
        let mut published = features(target.clone(), "generation-1", 8_000);
        published.cache.breakpoint_control = phenix_sdk::FeatureSupport::Supported;
        invoke_routing(
            &mut kernel,
            ModelCommand::PublishModelFeatures {
                features: published,
            },
        )
        .unwrap();

        let response = invoke_dispatch(
            &mut kernel,
            ModelDispatchCommand::PrepareResolved {
                session_id: None,
                decision: decision(target, "generation-1"),
                input: b"canonical context".to_vec().into(),
                cache: phenix_core::ModelCacheControl {
                    explicit_prefix_bytes: Some(9),
                    local_prefix_identity: Some("sha256:prefix".into()),
                    ..Default::default()
                },
                tools: Vec::new(),
                continuation: Vec::new(),
            },
        )
        .and_then(|response| match response {
            ModelDispatchResponse::Ready { prepared } => invoke_dispatch(
                &mut kernel,
                ModelDispatchCommand::InvokePrepared { prepared },
            ),
            ModelDispatchResponse::Inference { .. } => {
                Err("dispatch returned inference during preparation".into())
            }
        })
        .unwrap();

        let ModelDispatchResponse::Inference { response, .. } = response else {
            panic!("expected inference response");
        };
        assert_eq!(response.output.as_ref(), b"canonical context");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn unsupported_cache_control_preserves_context_semantics() {
        let path = temp_db("resolved-dispatch-no-cache");
        let mut kernel = kernel_with_provider(&path);
        let target = target("fixture.provider", "selected");
        invoke_routing(
            &mut kernel,
            ModelCommand::PublishModelFeatures {
                features: features(target.clone(), "generation-1", 8_000),
            },
        )
        .unwrap();

        let response = invoke_dispatch(
            &mut kernel,
            ModelDispatchCommand::PrepareResolved {
                session_id: None,
                decision: decision(target, "generation-1"),
                input: b"same context without cache support".to_vec().into(),
                cache: phenix_core::ModelCacheControl {
                    explicit_prefix_bytes: Some(12),
                    local_prefix_identity: Some("sha256:prefix".into()),
                    ..Default::default()
                },
                tools: Vec::new(),
                continuation: Vec::new(),
            },
        )
        .and_then(|response| match response {
            ModelDispatchResponse::Ready { prepared } => invoke_dispatch(
                &mut kernel,
                ModelDispatchCommand::InvokePrepared { prepared },
            ),
            ModelDispatchResponse::Inference { .. } => {
                Err("dispatch returned inference during preparation".into())
            }
        })
        .unwrap();

        let ModelDispatchResponse::Inference { response, .. } = response else {
            panic!("expected inference response");
        };
        assert_eq!(
            response.output.as_ref(),
            b"same context without cache support"
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn explicitly_required_cache_control_rejects_unsupported_target() {
        let target = target("provider.default", "root");
        let features = features(target, "generation-1", 10_000);
        let requested = phenix_core::ModelCacheControl {
            write: phenix_core::ModelCacheWritePolicy::ExplicitPrefix,
            explicit_prefix_bytes: Some(64),
            ..Default::default()
        };

        assert!(matches!(
            effective_cache_control(requested, &features),
            Err(ModelInferenceFailure::InvalidRequest { .. })
        ));
    }

    #[test]
    fn exact_selected_target_reaches_provider_unchanged() {
        let path = temp_db("resolved-dispatch-exact");
        let mut kernel = kernel_with_provider(&path);
        let target = target("fixture.provider", "selected-fallback");
        invoke_routing(
            &mut kernel,
            ModelCommand::PublishModelFeatures {
                features: features(target.clone(), "generation-1", 8_000),
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
        let target = target("fixture.provider", "selected");
        invoke_routing(
            &mut kernel,
            ModelCommand::PublishModelFeatures {
                features: features(target.clone(), "generation-1", 8_000),
            },
        )
        .unwrap();
        let decision = decision(target.clone(), "generation-1");
        invoke_routing(
            &mut kernel,
            ModelCommand::PublishModelFeatures {
                features: features(target, "generation-2", 8_000),
            },
        )
        .unwrap();
        let failure = dispatch_failure(
            &mut kernel,
            ModelDispatchCommand::PrepareResolved {
                session_id: None,
                decision,
                input: b"must-not-run".to_vec().into(),
                cache: Default::default(),
                tools: Vec::new(),
                continuation: Vec::new(),
            },
        )
        .unwrap();
        assert!(matches!(
            failure.failure,
            ModelInferenceFailure::InvalidRequest { ref message }
                if message.contains("StaleModelFeatureGeneration")
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn prepared_dispatch_is_not_rejected_when_capabilities_advance() {
        let path = temp_db("resolved-dispatch-prepared-snapshot");
        let mut kernel = kernel_with_provider(&path);
        let target = target("fixture.provider", "selected");
        invoke_routing(
            &mut kernel,
            ModelCommand::PublishModelFeatures {
                features: features(target.clone(), "generation-1", 8_000),
            },
        )
        .unwrap();
        let original = decision(target.clone(), "generation-1");
        let prepared = prepare(&mut kernel, original.clone(), b"cross-boundary").unwrap();
        invoke_routing(
            &mut kernel,
            ModelCommand::PublishModelFeatures {
                features: features(target, "generation-2", 8_000),
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
