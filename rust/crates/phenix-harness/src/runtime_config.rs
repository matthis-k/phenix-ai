use super::{default_suite_authority, PhenixRuntime};
use phenix_core::{
    Authority, CallableId, ModelFeatureGenerationId, ModelId, PhenixValue, PluginId, Project,
    RoutingProfileId, ServiceId, ValueError,
};
use phenix_plugin_catalog::{
    execution_configuration_service, model_routing_service, options_service, AgentDefinition,
    ExecutionConfigurationCommand, ExecutionConfigurationResponse, ModelCommand, ModelResponse,
    ModelTarget, OptionAssignment, OptionCommand, OptionKey, OptionResponse, OptionScope,
    OptionStartupPrecedence, OptionSubjectId, OptionValue, OrchestrationDefinition, RoutingProfile,
    COMMON_PROVIDERS,
};
use phenix_sdk::{
    CacheFeatures, CapacityKnowledge, ContextControl, EffectiveModelFeatures, FeatureSupport,
};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs,
    path::Path,
};

const RUNTIME_MODEL_FEATURE_GENERATION: &str = "runtime-config-v1";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeConfiguration {
    agents: Vec<AgentDefinition>,
    orchestrations: Vec<OrchestrationDefinition>,
    routing_profiles: Vec<RuntimeRoutingProfile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeRoutingProfile {
    id: RoutingProfileId,
    default_target: RuntimeModelTarget,
    #[serde(default)]
    fallback_targets: Vec<RuntimeModelTarget>,
    #[serde(default)]
    callable_targets: BTreeMap<CallableId, RuntimeModelTarget>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsConfiguration {
    #[serde(default)]
    global: BTreeMap<OptionKey, SettingValue>,
    #[serde(default)]
    sessions: BTreeMap<OptionSubjectId, BTreeMap<OptionKey, SettingValue>>,
    #[serde(default)]
    agents: BTreeMap<OptionSubjectId, BTreeMap<OptionKey, SettingValue>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum SettingValue {
    Bool(bool),
    Integer(i64),
    String(String),
}

impl From<SettingValue> for OptionValue {
    fn from(value: SettingValue) -> Self {
        match value {
            SettingValue::Bool(value) => Self::Bool(value),
            SettingValue::Integer(value) => Self::Integer(value),
            SettingValue::String(value) => Self::String(value),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeModelTarget {
    provider: PluginId,
    model: ModelId,
    #[serde(default, deserialize_with = "deserialize_runtime_inference")]
    inference: Option<Value>,
}

fn deserialize_runtime_inference<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    if value.is_null() {
        return Err(serde::de::Error::custom(
            "inference must be omitted instead of null",
        ));
    }
    Ok(Some(value))
}

impl RuntimeModelTarget {
    fn into_model_target(self) -> ModelTarget {
        let Self {
            provider,
            model,
            inference,
        } = self;
        let mut options = BTreeMap::new();
        if let Some(inference) = inference {
            options.insert("inference".into(), inference.into());
        }
        ModelTarget {
            provider_plugin: provider,
            model,
            options,
        }
    }
}

impl RuntimeRoutingProfile {
    fn into_routing_profile(self) -> RoutingProfile {
        RoutingProfile {
            id: self.id,
            default_target: self.default_target.into_model_target(),
            fallback_targets: self
                .fallback_targets
                .into_iter()
                .map(RuntimeModelTarget::into_model_target)
                .collect(),
            callable_targets: self
                .callable_targets
                .into_iter()
                .map(|(callable, target)| (callable, target.into_model_target()))
                .collect(),
        }
    }
}

pub(super) fn apply_default_config_directory(
    harness: &mut PhenixRuntime,
    directory: &Path,
) -> Result<(), Box<dyn Error>> {
    if !directory.is_dir() {
        return Err(format!(
            "default config directory does not exist: {}",
            directory.display()
        )
        .into());
    }
    let runtime = directory.join("runtime.json");
    if runtime.is_file() {
        apply_runtime_config(harness, &runtime)?;
    }
    Ok(())
}

pub(super) fn apply_startup_settings(
    harness: &mut PhenixRuntime,
    config_directory: Option<&Path>,
    nix_settings: Option<&Path>,
    precedence: OptionStartupPrecedence,
) -> Result<(), Box<dyn Error>> {
    let file_path = config_directory.map(|directory| directory.join("settings.json"));
    let file_settings = read_optional_settings(file_path.as_deref())?;
    let nix_settings = read_optional_settings(nix_settings)?;
    let file_values = settings_assignments(file_settings);
    let nix_values = settings_assignments(nix_settings);

    if file_values.is_empty() && nix_values.is_empty() {
        return Ok(());
    }

    // Resolve the Options provider through its interface; product code does not pin a component.
    let response: OptionResponse = invoke_projected(
        harness,
        &options_service(),
        &OptionCommand::Configure {
            file_values,
            nix_values,
            precedence,
        },
        &default_suite_authority(),
    )?;
    match response {
        OptionResponse::Configured { .. } => Ok(()),
        _ => Err("options service rejected startup settings".into()),
    }
}

fn read_optional_settings(path: Option<&Path>) -> Result<SettingsConfiguration, Box<dyn Error>> {
    let Some(path) = path else {
        return Ok(SettingsConfiguration::default());
    };
    if !path.exists() {
        return Ok(SettingsConfiguration::default());
    }
    if !path.is_file() {
        return Err(format!("settings path is not a file: {}", path.display()).into());
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn settings_assignments(settings: SettingsConfiguration) -> Vec<OptionAssignment> {
    let mut values = Vec::new();
    for (key, value) in settings.global {
        values.push(OptionAssignment {
            key,
            scope: OptionScope::Global,
            value: value.into(),
        });
    }
    for (session, settings) in settings.sessions {
        for (key, value) in settings {
            values.push(OptionAssignment {
                key,
                scope: OptionScope::Session(session.clone()),
                value: value.into(),
            });
        }
    }
    for (agent, settings) in settings.agents {
        for (key, value) in settings {
            values.push(OptionAssignment {
                key,
                scope: OptionScope::Agent(agent.clone()),
                value: value.into(),
            });
        }
    }
    values
}

fn invoke_projected<Request, Response>(
    harness: &mut PhenixRuntime,
    service: &ServiceId,
    request: &Request,
    authority: &Authority,
) -> Result<Response, Box<dyn Error>>
where
    for<'value> PhenixValue: From<&'value Request>,
    for<'value> Response: TryFrom<Project<&'value PhenixValue>, Error = ValueError>,
{
    let input = PhenixValue::from(request);
    let output = harness.invoke(service, &serde_json::to_vec(&input)?, authority, None)?;
    let output: PhenixValue = serde_json::from_slice(&output)?;
    Ok(Response::try_from(Project(&output))?)
}

pub(super) fn apply_runtime_config(
    harness: &mut PhenixRuntime,
    path: &Path,
) -> Result<(), Box<dyn Error>> {
    let bytes = fs::read(path)?;
    let configuration: RuntimeConfiguration = serde_json::from_slice(&bytes)?;
    apply_configuration(harness, configuration)
}

fn apply_configuration(
    harness: &mut PhenixRuntime,
    configuration: RuntimeConfiguration,
) -> Result<(), Box<dyn Error>> {
    let profiles = configuration
        .routing_profiles
        .into_iter()
        .map(RuntimeRoutingProfile::into_routing_profile)
        .collect::<Vec<_>>();
    let response: ExecutionConfigurationResponse = invoke_projected(
        harness,
        &execution_configuration_service(),
        &ExecutionConfigurationCommand::ConfigurePackaged {
            agents: configuration.agents,
            orchestrations: configuration.orchestrations,
            profiles,
        },
        &default_suite_authority(),
    )?;
    let ExecutionConfigurationResponse::Configured { profiles } = response else {
        return Err("execution configuration service rejected packaged configuration".into());
    };
    // Derived runtime facts are replayable after a crash. The durable definitions
    // and their ownership manifests have already committed as one transaction.
    for profile in profiles {
        publish_routing_profile_runtime_state(harness, &profile)?;
    }
    Ok(())
}
fn cache_features_for_target(target: &ModelTarget) -> CacheFeatures {
    let provider = target.provider_plugin.as_str();
    let model = target.model.as_str();
    let Some(preset) = COMMON_PROVIDERS
        .iter()
        .find(|preset| preset.id() == provider)
    else {
        return CacheFeatures::default();
    };

    match preset.id() {
        "openai-api" if model.starts_with("gpt-5.6") || model.starts_with("gpt-6") => {
            CacheFeatures {
                // Context materialization carries an exact stable-prefix byte boundary.
                breakpoint_control: FeatureSupport::Supported,
                write_policy: FeatureSupport::Supported,
                retention_hints: FeatureSupport::Supported,
                usage_reporting: FeatureSupport::Supported,
            }
        }
        "anthropic" => CacheFeatures {
            // Anthropic's top-level ephemeral control is representable at request end.
            breakpoint_control: FeatureSupport::Supported,
            write_policy: FeatureSupport::Supported,
            retention_hints: FeatureSupport::Supported,
            usage_reporting: FeatureSupport::Supported,
        },
        // A compatible protocol or gateway does not establish deployment-specific cache semantics.
        _ => CacheFeatures::default(),
    }
}

pub(crate) fn publish_routing_profile_runtime_state(
    harness: &mut PhenixRuntime,
    profile: &RoutingProfile,
) -> Result<(), Box<dyn Error>> {
    let mut targets = vec![profile.default_target.clone()];
    targets.extend(profile.fallback_targets.iter().cloned());
    targets.extend(profile.callable_targets.values().cloned());

    for target in targets {
        let cache = cache_features_for_target(&target);
        let features = EffectiveModelFeatures {
            target,
            generation: ModelFeatureGenerationId::parse(RUNTIME_MODEL_FEATURE_GENERATION)?,
            context: ContextControl::ReplaceableTurns,
            capacity: CapacityKnowledge::Unknown,
            cache,
            optional: BTreeSet::new(),
        };
        let response: ModelResponse = invoke_projected(
            harness,
            &model_routing_service(),
            &ModelCommand::PublishModelFeatures { features },
            &default_suite_authority(),
        )?;
        if !matches!(response, ModelResponse::Features { .. }) {
            return Err("model routing service rejected feature publication".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_plugin_catalog::{
        ExecutionConfigurationCommand, ModelCommand, OptionContext, OptionValueLayer,
        OptionValueSource,
    };
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn sample_runtime() -> RuntimeConfiguration {
        serde_json::from_value(json!({
            "agents": [{
                "id": "agent.scout",
                "kind": "agent",
                "description": "Inspect repository evidence.",
                "input_schema": {"type": "string", "minLength": 1},
                "output_schema": {"type": "string"},
                "capabilities": [],
                "policy": {"requires_permission": false}
            }],
            "orchestrations": [{
                "descriptor": {
                    "id": "orchestration.review",
                    "kind": "orchestration",
                    "description": "Independent review",
                    "input_schema": {"type": "string", "minLength": 1},
                    "output_schema": {"type": "string"},
                    "capabilities": [],
                    "policy": {"requires_permission": false}
                },
                "policy": "sequential",
                "nodes": [{
                    "callable": "agent.scout",
                    "objective": "Inspect the current objective."
                }]
            }],
            "routing_profiles": [{
                "id": "router.test",
                "default_target": {
                    "provider": "provider.fixture",
                    "model": "model.test",
                    "inference": {"effort": "low"}
                },
                "callable_targets": {
                    "agent.scout": {
                        "provider": "provider.fixture",
                        "model": "model.scout",
                        "inference": {"effort": "medium"}
                    }
                }
            }]
        }))
        .unwrap()
    }

    #[test]
    fn packaged_router_policy_does_not_publish_implicit_direct_model_profiles() {
        let mut harness = PhenixRuntime::default_suite().unwrap();
        harness.activate().unwrap();
        apply_configuration(&mut harness, sample_runtime()).unwrap();

        let catalog: ModelResponse = invoke_projected(
            &mut harness,
            &model_routing_service(),
            &ModelCommand::ListProfiles,
            &default_suite_authority(),
        )
        .unwrap();
        let ModelResponse::Profiles { profiles } = catalog else {
            panic!("expected routing profile catalog");
        };
        assert_eq!(
            profiles
                .iter()
                .map(|profile| profile.id.as_str())
                .collect::<Vec<_>>(),
            vec!["router.test"]
        );
    }

    #[test]
    fn direct_openai_and_anthropic_publish_only_supported_cache_features() {
        let target = |provider: &str, model: &str| ModelTarget {
            provider_plugin: PluginId::parse(provider).unwrap(),
            model: ModelId::parse(model).unwrap(),
            options: BTreeMap::new(),
        };

        let openai = cache_features_for_target(&target("openai-api", "gpt-5.6-sol"));
        assert_eq!(openai.breakpoint_control, FeatureSupport::Supported);
        assert_eq!(openai.write_policy, FeatureSupport::Supported);
        assert_eq!(openai.retention_hints, FeatureSupport::Supported);
        assert_eq!(openai.usage_reporting, FeatureSupport::Supported);

        let anthropic = cache_features_for_target(&target("anthropic", "claude-sonnet-5"));
        assert_eq!(anthropic.breakpoint_control, FeatureSupport::Supported);
        assert_eq!(anthropic.write_policy, FeatureSupport::Supported);
        assert_eq!(anthropic.retention_hints, FeatureSupport::Supported);
        assert_eq!(anthropic.usage_reporting, FeatureSupport::Supported);

        // Wire compatibility alone is not enough to claim provider cache semantics.
        let gateway = cache_features_for_target(&target("open-router", "gpt-5.6-sol"));
        assert_eq!(gateway, CacheFeatures::default());
        let older_openai = cache_features_for_target(&target("openai-api", "gpt-4.1-mini"));
        assert_eq!(older_openai, CacheFeatures::default());
    }

    #[test]
    fn runtime_model_target_projects_inference_options() {
        let target: RuntimeModelTarget = serde_json::from_value(json!({
            "provider": "provider.fixture",
            "model": "model.test",
            "inference": {"effort": "low"}
        }))
        .unwrap();
        let target = target.into_model_target();

        assert!(matches!(
            &target.options["inference"],
            PhenixValue::Map(values)
                if values.get("effort") == Some(&PhenixValue::String("low".into()))
        ));
    }

    #[test]
    fn runtime_model_target_allows_omitted_inference() {
        let target: RuntimeModelTarget = serde_json::from_value(json!({
            "provider": "provider.fixture",
            "model": "model.test"
        }))
        .unwrap();
        assert!(!target.into_model_target().options.contains_key("inference"));
    }

    #[test]
    fn runtime_model_target_rejects_null_inference() {
        let error = serde_json::from_value::<RuntimeModelTarget>(json!({
            "provider": "provider.fixture",
            "model": "model.test",
            "inference": null
        }))
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("inference must be omitted instead of null"));
    }

    #[test]
    fn runtime_model_target_rejects_removed_backend_field() {
        let error = serde_json::from_value::<RuntimeModelTarget>(json!({
            "backend": "phenix",
            "provider": "provider.fixture",
            "model": "model.test"
        }))
        .unwrap_err();
        assert!(error.to_string().contains("unknown field `backend`"));
    }

    fn invoke_configuration(
        harness: &mut PhenixRuntime,
        command: ExecutionConfigurationCommand,
    ) -> ExecutionConfigurationResponse {
        invoke_projected(
            harness,
            &execution_configuration_service(),
            &command,
            &default_suite_authority(),
        )
        .unwrap()
    }

    #[test]
    fn startup_settings_use_structural_options_boundary() {
        let directory = std::env::temp_dir().join(format!(
            "phenix-startup-settings-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("settings.json"),
            r#"{"global":{"session.auto_create":false}}"#,
        )
        .unwrap();
        let mut harness = PhenixRuntime::default_suite().unwrap();
        harness.activate().unwrap();

        apply_startup_settings(
            &mut harness,
            Some(&directory),
            None,
            OptionStartupPrecedence::Nix,
        )
        .unwrap();

        let response: OptionResponse = invoke_projected(
            &mut harness,
            &options_service(),
            &OptionCommand::Resolve {
                key: OptionKey::parse("session.auto_create").unwrap(),
                context: OptionContext::default(),
            },
            &default_suite_authority(),
        )
        .unwrap();
        assert!(matches!(
            response,
            OptionResponse::Value { option }
                if option.value == OptionValue::Bool(false)
                    && option.source == OptionValueSource::Global
                    && option.layer == OptionValueLayer::File
        ));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn runtime_configuration_rejects_foreign_profile_identity_changes() {
        let mut harness = PhenixRuntime::default_suite().unwrap();
        harness.activate().unwrap();

        let mut conflicting = sample_runtime()
            .routing_profiles
            .into_iter()
            .next()
            .unwrap()
            .into_routing_profile();
        conflicting.default_target.model = ModelId::parse("model.conflict").unwrap();
        invoke_projected::<_, ModelResponse>(
            &mut harness,
            &model_routing_service(),
            &ModelCommand::RegisterProfile {
                profile: conflicting,
            },
            &default_suite_authority(),
        )
        .unwrap();

        let error = apply_configuration(&mut harness, sample_runtime()).unwrap_err();
        assert!(error
            .to_string()
            .contains("routing profile is already owned outside packaged configuration: router.test"));
        assert!(matches!(
            invoke_configuration(
                &mut harness,
                ExecutionConfigurationCommand::GetAgent {
                    id: CallableId::parse("agent.scout").unwrap()
                }
            ),
            ExecutionConfigurationResponse::Agent { agent: None }
        ));
    }

    #[test]
    fn packaged_configuration_updates_retires_and_preserves_foreign_records_after_restart() {
        let path = std::env::temp_dir().join(format!(
            "phenix-config-{}-{}.sqlite",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut harness = PhenixRuntime::default_suite_with_persistence(
            phenix_core::LocalPersistence::open(&path).unwrap(),
        )
        .unwrap();
        harness.activate().unwrap();
        apply_configuration(&mut harness, sample_runtime()).unwrap();
        let mut foreign = sample_runtime()
            .routing_profiles
            .remove(0)
            .into_routing_profile();
        foreign.id = RoutingProfileId::parse("user.custom").unwrap();
        invoke_projected::<_, ModelResponse>(
            &mut harness,
            &model_routing_service(),
            &ModelCommand::RegisterProfile {
                profile: foreign.clone(),
            },
            &default_suite_authority(),
        )
        .unwrap();
        let mut changed = sample_runtime();
        changed.routing_profiles[0].default_target.model = ModelId::parse("model.updated").unwrap();
        let updated_agent: AgentDefinition = serde_json::from_value(json!({
            "id": "agent.scout", "kind": "agent", "description": "Updated packaged agent.",
            "input_schema": {"type":"string"}, "output_schema": {"type":"string"}, "capabilities": [], "policy": {"requires_permission":false}
        })).unwrap();
        changed.agents = vec![updated_agent.clone()];
        let mut updated_orchestration = serde_json::to_value(&changed.orchestrations[0]).unwrap();
        updated_orchestration["descriptor"]["description"] =
            json!("Updated packaged orchestration.");
        let updated_orchestration: OrchestrationDefinition =
            serde_json::from_value(updated_orchestration).unwrap();
        changed.orchestrations = vec![updated_orchestration.clone()];
        apply_configuration(&mut harness, changed).unwrap();
        drop(harness);
        let mut harness = PhenixRuntime::default_suite_with_persistence(
            phenix_core::LocalPersistence::open(&path).unwrap(),
        )
        .unwrap();
        harness.activate().unwrap();
        assert_eq!(
            invoke_configuration(
                &mut harness,
                ExecutionConfigurationCommand::GetAgent {
                    id: updated_agent.id().clone()
                }
            ),
            ExecutionConfigurationResponse::Agent {
                agent: Some(updated_agent)
            }
        );
        assert_eq!(
            invoke_configuration(
                &mut harness,
                ExecutionConfigurationCommand::GetOrchestration {
                    id: updated_orchestration.id().clone()
                }
            ),
            ExecutionConfigurationResponse::Orchestration {
                orchestration: Some(updated_orchestration)
            }
        );
        apply_configuration(
            &mut harness,
            RuntimeConfiguration {
                agents: vec![],
                orchestrations: vec![],
                routing_profiles: vec![],
            },
        )
        .unwrap();
        assert!(
            matches!(invoke_configuration(&mut harness, ExecutionConfigurationCommand::ListAgents), ExecutionConfigurationResponse::Agents { agents } if agents.is_empty())
        );
        assert!(
            matches!(invoke_configuration(&mut harness, ExecutionConfigurationCommand::ListOrchestrations), ExecutionConfigurationResponse::Orchestrations { orchestrations } if orchestrations.is_empty())
        );
        let catalog: ModelResponse = invoke_projected(
            &mut harness,
            &model_routing_service(),
            &ModelCommand::ListProfiles,
            &default_suite_authority(),
        )
        .unwrap();
        assert!(
            matches!(catalog, ModelResponse::Profiles { profiles } if profiles.len() == 1 && profiles[0].id == foreign.id)
        );
        let retained: ModelResponse = invoke_projected(
            &mut harness,
            &model_routing_service(),
            &ModelCommand::GetProfile {
                id: RoutingProfileId::parse("router.test").unwrap(),
            },
            &default_suite_authority(),
        )
        .unwrap();
        assert!(
            matches!(retained, ModelResponse::Profile { profile: Some(profile) } if profile.default_target.model.as_str() == "model.updated")
        );
        // Reapplying the same desired state after retirement is idempotent.
        apply_configuration(
            &mut harness,
            RuntimeConfiguration {
                agents: vec![],
                orchestrations: vec![],
                routing_profiles: vec![],
            },
        )
        .unwrap();
        drop(harness);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn invalid_orchestration_and_duplicate_profiles_do_not_publish_agents() {
        for duplicate in [false, true] {
            let mut harness = PhenixRuntime::default_suite().unwrap();
            harness.activate().unwrap();
            let mut config = sample_runtime();
            if duplicate {
                config
                    .routing_profiles
                    .push(sample_runtime().routing_profiles.remove(0));
            } else {
                config.agents.clear();
            }
            assert!(apply_configuration(&mut harness, config).is_err());
            assert!(
                matches!(invoke_configuration(&mut harness, ExecutionConfigurationCommand::ListAgents), ExecutionConfigurationResponse::Agents { agents } if agents.is_empty())
            );
            let catalog: ModelResponse = invoke_projected(
                &mut harness,
                &model_routing_service(),
                &ModelCommand::ListProfiles,
                &default_suite_authority(),
            )
            .unwrap();
            assert!(matches!(catalog, ModelResponse::Profiles { profiles } if profiles.is_empty()));
        }
    }

    #[test]
    fn out_of_band_owned_profile_change_blocks_configuration() {
        let mut harness = PhenixRuntime::default_suite().unwrap();
        harness.activate().unwrap();
        apply_configuration(&mut harness, sample_runtime()).unwrap();
        let expected = sample_runtime()
            .routing_profiles
            .remove(0)
            .into_routing_profile();
        let mut changed = expected.clone();
        changed.default_target.model = ModelId::parse("model.external-change").unwrap();
        invoke_projected::<_, ModelResponse>(
            &mut harness,
            &model_routing_service(),
            &ModelCommand::ReplaceProfile {
                expected,
                profile: changed.clone(),
            },
            &default_suite_authority(),
        )
        .unwrap();
        assert!(apply_configuration(&mut harness, sample_runtime())
            .unwrap_err()
            .to_string()
            .contains("changed outside configuration"));
        let retained: ModelResponse = invoke_projected(
            &mut harness,
            &model_routing_service(),
            &ModelCommand::GetProfile {
                id: changed.id.clone(),
            },
            &default_suite_authority(),
        )
        .unwrap();
        assert_eq!(
            retained,
            ModelResponse::Profile {
                profile: Some(changed)
            }
        );
    }

    #[test]
    fn later_foreign_profiles_and_callables_remain_visible() {
        let mut harness = PhenixRuntime::default_suite().unwrap();
        harness.activate().unwrap();
        let empty = || RuntimeConfiguration {
            agents: vec![],
            orchestrations: vec![],
            routing_profiles: vec![],
        };
        apply_configuration(&mut harness, empty()).unwrap();
        let sample = sample_runtime();
        let agent = sample.agents.into_iter().next().unwrap();
        invoke_configuration(
            &mut harness,
            ExecutionConfigurationCommand::RegisterAgent {
                agent: agent.clone(),
            },
        );
        let mut foreign = sample
            .routing_profiles
            .into_iter()
            .next()
            .unwrap()
            .into_routing_profile();
        foreign.id = RoutingProfileId::parse("user.foreign").unwrap();
        invoke_projected::<_, ModelResponse>(
            &mut harness,
            &model_routing_service(),
            &ModelCommand::RegisterProfile {
                profile: foreign.clone(),
            },
            &default_suite_authority(),
        )
        .unwrap();
        apply_configuration(&mut harness, empty()).unwrap();
        assert_eq!(
            invoke_configuration(&mut harness, ExecutionConfigurationCommand::ListAgents),
            ExecutionConfigurationResponse::Agents {
                agents: vec![agent]
            }
        );
        let catalog: ModelResponse = invoke_projected(
            &mut harness,
            &model_routing_service(),
            &ModelCommand::ListProfiles,
            &default_suite_authority(),
        )
        .unwrap();
        assert!(
            matches!(catalog, ModelResponse::Profiles { profiles } if profiles.len() == 1 && profiles[0].id == foreign.id)
        );
    }

    #[test]
    fn runtime_configuration_is_active_and_idempotent() {
        let mut harness = PhenixRuntime::default_suite().unwrap();
        harness.activate().unwrap();
        apply_configuration(&mut harness, sample_runtime()).unwrap();
        apply_configuration(&mut harness, sample_runtime()).unwrap();

        assert!(matches!(
            invoke_configuration(
                &mut harness,
                ExecutionConfigurationCommand::GetAgent {
                    id: CallableId::parse("agent.scout").unwrap()
                }
            ),
            ExecutionConfigurationResponse::Agent { agent: Some(_) }
        ));
        assert!(matches!(
            invoke_configuration(
                &mut harness,
                ExecutionConfigurationCommand::GetOrchestration {
                    id: CallableId::parse("orchestration.review").unwrap()
                }
            ),
            ExecutionConfigurationResponse::Orchestration {
                orchestration: Some(_)
            }
        ));

        let command = ModelCommand::GetProfile {
            id: RoutingProfileId::parse("router.test").unwrap(),
        };
        let output: ModelResponse = invoke_projected(
            &mut harness,
            &model_routing_service(),
            &command,
            &default_suite_authority(),
        )
        .unwrap();
        assert!(matches!(
            output,
            ModelResponse::Profile { profile: Some(_) }
        ));

        let candidates: ModelResponse = invoke_projected(
            &mut harness,
            &model_routing_service(),
            &ModelCommand::ListCandidates {
                profile_id: RoutingProfileId::parse("router.test").unwrap(),
                callable_id: Some(CallableId::parse("agent.scout").unwrap()),
            },
            &default_suite_authority(),
        )
        .unwrap();
        assert!(matches!(
            candidates,
            ModelResponse::Candidates { candidates } if !candidates.is_empty()
        ));
    }
}
