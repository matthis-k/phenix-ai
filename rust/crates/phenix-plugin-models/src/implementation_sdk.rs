pub use phenix_core::{
    model_inference_service, ModelInferenceRequest, ModelInferenceResponse, MODEL_INFERENCE_SERVICE,
};
use phenix_core::{
    Authority, CallableId, CapabilityId, ComponentInterface, DurableSchema, PluginContext,
    PluginExecution, PluginHost, PluginId, PluginInstance, PluginManifest, ResourceNamespace,
    RoutingProfileId, ServiceContribution, ServiceId, TransactionOp,
};
pub use phenix_sdk::{
    model_routing_service, ModelCommand, ModelResponse, ModelTarget, RoutingProfile,
    RoutingProfileDescriptor, MODEL_ROUTING_SERVICE,
};
use std::collections::BTreeSet;

use crate::routing_service::{RoutingServiceState, ROUTING_RUNTIME_KEY};

const MODEL_ROUTING_PLUGIN: &str = "phenix.models";
const MODEL_NAMESPACE: &str = "phenix.models.state";
const PERSISTENCE_SCHEMA: &str = "kernel.persistence.schema";
const PERSISTENCE_READ: &str = "kernel.persistence.read";
const PERSISTENCE_WRITE: &str = "kernel.persistence.write";
const PROFILE_INDEX: &str = "index/profiles";

type ModelContext<'host, 'runtime, 'state> =
    PluginContext<'host, 'runtime, (), (), &'state mut BTreeSet<PluginId>>;

fn context<'host, 'runtime, 'state>(
    host: &'host PluginHost<'runtime>,
    authenticated: &'state mut BTreeSet<PluginId>,
) -> ModelContext<'host, 'runtime, 'state> {
    PluginContext::new(host, (), (), authenticated)
}

#[must_use]
pub fn model_routing_manifest(maximum_authority: Authority) -> PluginManifest {
    let persistence = Authority::new([
        capability(PERSISTENCE_SCHEMA),
        capability(PERSISTENCE_READ),
        capability(PERSISTENCE_WRITE),
    ]);
    let maximum_authority = Authority::new(
        maximum_authority
            .capabilities()
            .cloned()
            .chain(persistence.capabilities().cloned()),
    );
    PluginManifest {
        id: PluginId::parse(MODEL_ROUTING_PLUGIN).expect("static plugin id is valid"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: phenix_core::ServiceRole::Terminal,
            service: model_routing_service(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        resource_namespaces: vec![model_namespace()],
        maximum_authority,
    }
}

#[must_use]
pub fn model_routing_factory() -> Box<dyn PluginInstance> {
    Box::new(ModelRoutingPlugin::default())
}

fn model_namespace() -> ResourceNamespace {
    ResourceNamespace::parse(MODEL_NAMESPACE).expect("static namespace is valid")
}

fn capability(value: &str) -> CapabilityId {
    CapabilityId::parse(value).expect("static capability is valid")
}

#[derive(Default)]
struct ModelRoutingPlugin {
    authenticated: BTreeSet<PluginId>,
    routing: RoutingServiceState,
}

impl PluginInstance for ModelRoutingPlugin {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        let snapshot = {
            let context = context(host, &mut self.authenticated);
            context
                .kernel
                .register_durable_schema(&DurableSchema::new(model_namespace(), 1))
                .map_err(|error| error.to_string())?;
            context
                .kernel
                .read_durable(&model_namespace(), ROUTING_RUNTIME_KEY)
                .map_err(|error| error.to_string())?
        };
        self.routing = RoutingServiceState::restore(snapshot.as_deref())
            .map_err(|error| format!("invalid durable routing state: {error:?}"))?;
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &model_routing_service() {
            return Err(format!("unsupported model routing service: {service}"));
        }
        let mut context = context(host, &mut self.authenticated);
        let interface = crate::ModelRoutingInterface::interface_id();
        let command = context
            .kernel
            .decode_projected::<ModelCommand>(&interface, input)
            .map_err(|error| error.to_string())?;
        let response = handle(&mut context, &mut self.routing, command)?;
        context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string())
    }
}

fn handle(
    context: &mut ModelContext<'_, '_, '_>,
    routing: &mut RoutingServiceState,
    command: ModelCommand,
) -> Result<ModelResponse, String> {
    let mutates_runtime = matches!(
        &command,
        ModelCommand::PublishCapabilities { .. } | ModelCommand::RecordEvidence { .. }
    );
    let previous_runtime = if mutates_runtime {
        read_raw(context, ROUTING_RUNTIME_KEY)?
    } else {
        None
    };

    if let Some(response) = routing.handle_state_command(command.clone(), |id| read_profile(context, id)) {
        let response = response?;
        if mutates_runtime {
            persist_runtime_state(context, routing, previous_runtime)?;
        }
        return Ok(response);
    }

    match command {
        ModelCommand::RegisterProfile { profile } => {
            insert_profile(context, &profile)?;
            Ok(ModelResponse::Profile {
                profile: Some(profile),
            })
        }
        ModelCommand::GetProfile { id } => Ok(ModelResponse::Profile {
            profile: read_profile(context, &id)?,
        }),
        ModelCommand::ListProfiles => Ok(ModelResponse::Profiles {
            profiles: load_profiles(context)?
                .into_iter()
                .map(|profile| descriptor(&profile))
                .collect(),
        }),
        ModelCommand::SetProviderAuthenticated {
            provider_plugin,
            authenticated,
        } => {
            if authenticated {
                context.plugin.state.insert(provider_plugin.clone());
            } else {
                context.plugin.state.remove(&provider_plugin);
            }
            Ok(ModelResponse::Authentication {
                provider_plugin,
                authenticated,
            })
        }
        ModelCommand::Resolve {
            profile_id,
            callable_id,
        } => Ok(ModelResponse::Target {
            target: resolve_compat_target(context, &profile_id, callable_id.as_ref())?,
        }),
        ModelCommand::Invoke {
            profile_id,
            callable_id,
            input,
            tools,
        } => {
            let target = resolve_compat_target(context, &profile_id, callable_id.as_ref())?;
            if !context.plugin.state.contains(&target.provider_plugin) {
                return Err(format!(
                    "provider authentication required: {}",
                    target.provider_plugin
                ));
            }
            let request = ModelInferenceRequest {
                model: target.model.clone(),
                input,
                options: target.options.clone(),
                tools,
            };
            let input = context
                .kernel
                .encode_value(&request)
                .map_err(|error| error.to_string())?;
            let output = context
                .kernel
                .invoke_service_abi(
                    &model_inference_service(),
                    &input,
                    context.call.authority,
                    Some(&target.provider_plugin),
                )
                .map_err(|error| error.to_string())?;
            let response = context
                .kernel
                .decode_projected::<ModelInferenceResponse>(
                    &phenix_core::ModelInferenceInterface::interface_id(),
                    &output,
                )
                .map_err(|error| error.to_string())?;
            Ok(ModelResponse::Inference { target, response })
        }
        ModelCommand::PublishCapabilities { .. }
        | ModelCommand::ListCandidates { .. }
        | ModelCommand::ResolveWithRequirements { .. }
        | ModelCommand::RecordEvidence { .. } => {
            Err("routing state command was not handled".into())
        }
    }
}

fn persist_runtime_state(
    context: &ModelContext<'_, '_, '_>,
    routing: &mut RoutingServiceState,
    previous: Option<Vec<u8>>,
) -> Result<(), String> {
    let next = match routing.snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            *routing = RoutingServiceState::restore(previous.as_deref())
                .map_err(|restore| format!("routing state rollback failed: {restore:?}"))?;
            return Err(format!("routing state snapshot failed: {error:?}"));
        }
    };
    let result = context.kernel.transact_durable(
        &model_namespace(),
        &[
            TransactionOp::AssertValue {
                key: ROUTING_RUNTIME_KEY.into(),
                expected: previous.clone(),
            },
            TransactionOp::Put {
                key: ROUTING_RUNTIME_KEY.into(),
                value: next,
            },
        ],
    );
    if let Err(error) = result {
        *routing = RoutingServiceState::restore(previous.as_deref())
            .map_err(|restore| format!("routing state rollback failed: {restore:?}"))?;
        return Err(error.to_string());
    }
    Ok(())
}

fn descriptor(profile: &RoutingProfile) -> RoutingProfileDescriptor {
    let mut providers = BTreeSet::from([profile.default_target.provider_plugin.clone()]);
    providers.extend(
        profile
            .fallback_targets
            .iter()
            .chain(profile.callable_targets.values())
            .map(|target| target.provider_plugin.clone()),
    );
    RoutingProfileDescriptor {
        id: profile.id.clone(),
        providers: providers.into_iter().collect(),
    }
}

fn resolve_compat_target(
    context: &ModelContext<'_, '_, '_>,
    profile_id: &RoutingProfileId,
    callable_id: Option<&CallableId>,
) -> Result<ModelTarget, String> {
    let profile = read_profile(context, profile_id)?
        .ok_or_else(|| format!("unknown routing profile: {profile_id}"))?;
    Ok(callable_id
        .and_then(|callable| profile.callable_targets.get(callable))
        .unwrap_or(&profile.default_target)
        .clone())
}

fn insert_profile(
    context: &ModelContext<'_, '_, '_>,
    profile: &RoutingProfile,
) -> Result<(), String> {
    let key = profile_key(&profile.id);
    let old_index = read_raw(context, PROFILE_INDEX)?;
    let mut ids: Vec<RoutingProfileId> = old_index
        .as_deref()
        .map(|value| serde_json::from_slice(value).map_err(|error| error.to_string()))
        .transpose()?
        .unwrap_or_default();
    if ids.contains(&profile.id) || read_raw(context, &key)?.is_some() {
        return Err(format!(
            "routing profile already registered: {}",
            profile.id
        ));
    }
    ids.push(profile.id.clone());
    ids.sort();
    context
        .kernel
        .transact_durable(
            &model_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: key.clone(),
                    expected: None,
                },
                TransactionOp::AssertValue {
                    key: PROFILE_INDEX.into(),
                    expected: old_index,
                },
                TransactionOp::Put {
                    key,
                    value: serde_json::to_vec(profile).map_err(|error| error.to_string())?,
                },
                TransactionOp::Put {
                    key: PROFILE_INDEX.into(),
                    value: serde_json::to_vec(&ids).map_err(|error| error.to_string())?,
                },
            ],
        )
        .map_err(|error| error.to_string())
}

fn read_profile(
    context: &ModelContext<'_, '_, '_>,
    id: &RoutingProfileId,
) -> Result<Option<RoutingProfile>, String> {
    read_raw(context, &profile_key(id))?
        .map(|value| serde_json::from_slice(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn load_profiles(context: &ModelContext<'_, '_, '_>) -> Result<Vec<RoutingProfile>, String> {
    let ids: Vec<RoutingProfileId> = read_raw(context, PROFILE_INDEX)?
        .as_deref()
        .map(|value| serde_json::from_slice(value).map_err(|error| error.to_string()))
        .transpose()?
        .unwrap_or_default();
    ids.into_iter()
        .map(|id| {
            read_profile(context, &id)?.ok_or_else(|| format!("missing routing profile: {id}"))
        })
        .collect()
}

fn read_raw(context: &ModelContext<'_, '_, '_>, key: &str) -> Result<Option<Vec<u8>>, String> {
    context
        .kernel
        .read_durable(&model_namespace(), key)
        .map_err(|error| error.to_string())
}

fn profile_key(id: &RoutingProfileId) -> String {
    format!("profile/{id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{
        CapabilityGenerationId, Kernel, KernelConfig, LocalPersistence, ModelId, ModelToolDescriptor,
        PhenixValue, Project,
    };
    use phenix_sdk::{
        CapacityKnowledge, ContextControl, ContextDemand, EffectiveModelCapabilities, ModelLimits,
        RouteSelectionPolicy, RoutingEstimateMode, RoutingRequirements,
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
                provider_metadata: BTreeMap::from([(
                    "provider".into(),
                    serde_json::json!("fixture.provider").into(),
                )]),
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

    fn capabilities(target: ModelTarget, context_window_tokens: u64) -> EffectiveModelCapabilities {
        EffectiveModelCapabilities {
            target,
            generation: CapabilityGenerationId::parse("generation-1").unwrap(),
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
        let mut kernel =
            Kernel::with_persistence(KernelConfig::new([manifest]).unwrap(), persistence);
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

    fn invoke(kernel: &mut Kernel, command: ModelCommand) -> Result<ModelResponse, String> {
        let output = kernel
            .invoke(
                &model_routing_service(),
                &serde_json::to_vec(&PhenixValue::from(&command)).unwrap(),
                &routing_authority(),
                None,
            )
            .map_err(|error| error.to_string())?;
        let output: PhenixValue =
            serde_json::from_slice(&output).map_err(|error| error.to_string())?;
        ModelResponse::try_from(Project(&output)).map_err(|error| error.to_string())
    }

    mod profile_store {
        use super::*;

        #[test]
        fn immutable_profile_survives_restart_and_describes_all_providers() {
            let path = temp_db("routing-profile-store");
            let profile = profile();
            {
                let mut kernel = kernel_with(&path);
                invoke(
                    &mut kernel,
                    ModelCommand::RegisterProfile {
                        profile: profile.clone(),
                    },
                )
                .unwrap();
                assert!(invoke(
                    &mut kernel,
                    ModelCommand::RegisterProfile {
                        profile: profile.clone(),
                    },
                )
                .unwrap_err()
                .contains("already registered"));
            }
            let mut restored = kernel_with(&path);
            let response = invoke(&mut restored, ModelCommand::ListProfiles).unwrap();
            let ModelResponse::Profiles { profiles } = response else {
                panic!("expected profiles response");
            };
            assert_eq!(profiles.len(), 1);
            assert_eq!(
                profiles[0].providers,
                vec![
                    PluginId::parse("provider.default").unwrap(),
                    PluginId::parse("provider.fallback").unwrap(),
                    PluginId::parse("provider.scout").unwrap(),
                ]
            );
            let _ = fs::remove_file(path);
        }
    }

    mod smart_selection {
        use super::*;

        fn requirements() -> RoutingRequirements {
            RoutingRequirements {
                context: ContextDemand {
                    mandatory_input_tokens: 1_000,
                    reducible_input_tokens: 500,
                    output_reserve_tokens: 500,
                    required_capabilities: BTreeSet::new(),
                },
                required_capabilities: BTreeSet::new(),
                require_known_capacity: true,
            }
        }

        fn policy() -> RouteSelectionPolicy {
            RouteSelectionPolicy {
                revision: "route-policy-1".into(),
                estimates: RoutingEstimateMode::Ignore,
                max_candidate_attempts: 2,
            }
        }

        #[test]
        fn hard_capacity_rejects_primary_before_fallback_selection() {
            let path = temp_db("routing-smart-selection");
            let profile = profile();
            let mut kernel = kernel_with(&path);
            invoke(
                &mut kernel,
                ModelCommand::RegisterProfile {
                    profile: profile.clone(),
                },
            )
            .unwrap();
            for capabilities in [
                capabilities(profile.default_target.clone(), 1_500),
                capabilities(profile.fallback_targets[0].clone(), 8_000),
            ] {
                invoke(
                    &mut kernel,
                    ModelCommand::PublishCapabilities { capabilities },
                )
                .unwrap();
            }
            let response = invoke(
                &mut kernel,
                ModelCommand::ResolveWithRequirements {
                    profile_id: profile.id,
                    callable_id: None,
                    requirements: requirements(),
                    policy: policy(),
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
                invoke(
                    &mut kernel,
                    ModelCommand::RegisterProfile {
                        profile: profile.clone(),
                    },
                )
                .unwrap();
                invoke(
                    &mut kernel,
                    ModelCommand::PublishCapabilities {
                        capabilities: capabilities(profile.default_target.clone(), 8_000),
                    },
                )
                .unwrap();
                invoke(
                    &mut kernel,
                    ModelCommand::PublishCapabilities {
                        capabilities: capabilities(profile.fallback_targets[0].clone(), 8_000),
                    },
                )
                .unwrap();
            }
            let mut restored = kernel_with(&path);
            let response = invoke(
                &mut restored,
                ModelCommand::ListCandidates {
                    profile_id: profile.id,
                    callable_id: None,
                },
            )
            .unwrap();
            let ModelResponse::Candidates { candidates } = response else {
                panic!("expected routing candidates");
            };
            assert_eq!(candidates.len(), 2);
            assert_eq!(candidates[0].capabilities.target.model.as_str(), "root");
            assert_eq!(candidates[1].capabilities.target.model.as_str(), "fallback");
            let _ = fs::remove_file(path);
        }
    }

    mod provider_dispatch {
        use super::*;

        #[test]
        fn compatibility_invoke_uses_provider_abi_and_process_local_auth() {
            let path = temp_db("routing-provider-dispatch");
            let profile = RoutingProfile {
                id: RoutingProfileId::parse("default").unwrap(),
                default_target: target("fixture.provider", "fixture"),
                fallback_targets: Vec::new(),
                callable_targets: BTreeMap::new(),
            };
            {
                let mut kernel = kernel_with_provider(&path);
                invoke(
                    &mut kernel,
                    ModelCommand::RegisterProfile {
                        profile: profile.clone(),
                    },
                )
                .unwrap();
                invoke(
                    &mut kernel,
                    ModelCommand::SetProviderAuthenticated {
                        provider_plugin: PluginId::parse("fixture.provider").unwrap(),
                        authenticated: true,
                    },
                )
                .unwrap();
                let response = invoke(
                    &mut kernel,
                    ModelCommand::Invoke {
                        profile_id: profile.id.clone(),
                        callable_id: None,
                        input: b"hello".to_vec().into(),
                        tools: Vec::<ModelToolDescriptor>::new(),
                    },
                )
                .unwrap();
                assert!(matches!(
                    response,
                    ModelResponse::Inference { target, response }
                        if target.provider_plugin.as_str() == "fixture.provider"
                            && response.output.as_ref() == b"hello"
                ));
            }
            let mut restored = kernel_with_provider(&path);
            assert!(invoke(
                &mut restored,
                ModelCommand::Invoke {
                    profile_id: profile.id,
                    callable_id: None,
                    input: b"hello".to_vec().into(),
                    tools: Vec::new(),
                },
            )
            .unwrap_err()
            .contains("authentication required"));
            let _ = fs::remove_file(path);
        }
    }
}
