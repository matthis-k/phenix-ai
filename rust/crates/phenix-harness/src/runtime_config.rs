use super::{default_suite_authority, PhenixHarness};
use phenix_core::{
    Authority, CallableId, CapabilityGenerationId, ModelId, PhenixValue, PluginId, Project,
    RoutingProfileId, ServiceId, ValueError,
};
use phenix_plugin_catalog::{
    execution_configuration_service, model_routing_service, options_component_manifest,
    options_service, AgentDefinition, ExecutionConfigurationCommand,
    ExecutionConfigurationResponse, ModelCommand, ModelResponse, ModelTarget, OptionAssignment,
    OptionCommand, OptionKey, OptionResponse, OptionScope, OptionStartupPrecedence,
    OptionSubjectId, OptionValue, OrchestrationDefinition, RoutingProfile,
};
use phenix_provider_sdk::{provider_auth_service, ProviderAuthCommand, ProviderAuthResponse};
use phenix_sdk::{CapacityKnowledge, ContextControl, EffectiveModelCapabilities};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs,
    path::Path,
};

const RUNTIME_CAPABILITY_GENERATION: &str = "runtime-config-v1";

#[derive(Debug, Deserialize)]
struct RuntimeConfiguration {
    agents: Vec<AgentDefinition>,
    orchestrations: Vec<OrchestrationDefinition>,
    routing_profiles: Vec<RuntimeRoutingProfile>,
}

#[derive(Debug, Deserialize)]
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
struct RuntimeModelTarget {
    provider: PluginId,
    model: ModelId,
    #[serde(default)]
    inference: Value,
}

impl RuntimeModelTarget {
    fn into_model_target(self) -> ModelTarget {
        let Self {
            provider,
            model,
            inference,
        } = self;
        let mut options = BTreeMap::new();
        if !inference.is_null() {
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
    harness: &mut PhenixHarness,
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
    harness: &mut PhenixHarness,
    config_directory: Option<&Path>,
    nix_settings: Option<&Path>,
    precedence: OptionStartupPrecedence,
) -> Result<(), Box<dyn Error>> {
    let file_path = config_directory.map(|directory| directory.join("settings.json"));
    let file_settings = read_optional_settings(file_path.as_deref())?;
    let nix_settings = read_optional_settings(nix_settings)?;
    let file_values = settings_assignments(file_settings);
    let nix_values = settings_assignments(nix_settings);

    let component = options_component_manifest();
    if harness.component_graph().component(&component.id).is_none() {
        if file_values.is_empty() && nix_values.is_empty() {
            return Ok(());
        }
        return Err("startup settings require the phenix.options plugin".into());
    }

    let command = OptionCommand::Configure {
        file_values,
        nix_values,
        precedence,
    };
    let input = PhenixValue::from(&command);
    let output = harness.kernel_mut().invoke_component(
        &component.id,
        &options_service(),
        &serde_json::to_vec(&input)?,
        &default_suite_authority(),
        &component.owner,
    )?;
    let output: PhenixValue = serde_json::from_slice(&output)?;
    match OptionResponse::try_from(Project(&output))? {
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
    harness: &mut PhenixHarness,
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
    harness: &mut PhenixHarness,
    path: &Path,
) -> Result<(), Box<dyn Error>> {
    let bytes = fs::read(path)?;
    let configuration: RuntimeConfiguration = serde_json::from_slice(&bytes)?;
    apply_configuration(harness, configuration)
}

fn apply_configuration(
    harness: &mut PhenixHarness,
    configuration: RuntimeConfiguration,
) -> Result<(), Box<dyn Error>> {
    let mut profiles = configuration
        .routing_profiles
        .into_iter()
        .map(RuntimeRoutingProfile::into_routing_profile)
        .collect::<Vec<_>>();
    let mut direct_targets = BTreeMap::new();
    for profile in &profiles {
        for target in std::iter::once(&profile.default_target)
            .chain(profile.fallback_targets.iter())
            .chain(profile.callable_targets.values())
        {
            direct_targets
                .entry(serde_json::to_string(target)?)
                .or_insert_with(|| target.clone());
        }
    }
    for target in direct_targets.into_values() {
        profiles.push(direct_routing_profile(target)?);
    }
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
fn direct_routing_profile(target: ModelTarget) -> Result<RoutingProfile, Box<dyn Error>> {
    let encoded = serde_json::to_vec(&target)?;
    let digest = Sha256::digest(encoded);
    let suffix = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let id = RoutingProfileId::parse(format!(
        "model.{}.{}.{}",
        target.provider_plugin, target.model, suffix
    ))?;
    Ok(RoutingProfile {
        id,
        default_target: target,
        fallback_targets: Vec::new(),
        callable_targets: BTreeMap::new(),
    })
}

#[cfg(test)]
fn without_legacy_runtime_metadata(mut profile: RoutingProfile) -> RoutingProfile {
    fn normalize_target(target: &mut ModelTarget) {
        if matches!(
            target.options.get("backend"),
            Some(PhenixValue::String(backend)) if backend == "phenix"
        ) {
            target.options.remove("backend");
        }
        if matches!(target.options.get("inference"), Some(PhenixValue::Unit)) {
            target.options.remove("inference");
        }
    }

    normalize_target(&mut profile.default_target);
    for target in &mut profile.fallback_targets {
        normalize_target(target);
    }
    for target in profile.callable_targets.values_mut() {
        normalize_target(target);
    }
    profile
}

fn publish_routing_profile_runtime_state(
    harness: &mut PhenixHarness,
    profile: &RoutingProfile,
) -> Result<(), Box<dyn Error>> {
    let mut targets = vec![profile.default_target.clone()];
    targets.extend(profile.fallback_targets.iter().cloned());
    targets.extend(profile.callable_targets.values().cloned());

    for target in targets {
        publish_provider_authentication(harness, &target.provider_plugin)?;
        let capabilities = EffectiveModelCapabilities {
            target,
            generation: CapabilityGenerationId::parse(RUNTIME_CAPABILITY_GENERATION)?,
            context: ContextControl::ReplaceableTurns,
            capacity: CapacityKnowledge::Unknown,
            optional: BTreeSet::new(),
        };
        let response: ModelResponse = invoke_projected(
            harness,
            &model_routing_service(),
            &ModelCommand::PublishCapabilities { capabilities },
            &default_suite_authority(),
        )?;
        if !matches!(response, ModelResponse::Capabilities { .. }) {
            return Err("model routing service rejected capability publication".into());
        }
    }
    Ok(())
}

fn publish_provider_authentication(
    harness: &mut PhenixHarness,
    provider: &PluginId,
) -> Result<(), Box<dyn Error>> {
    let input = serde_json::to_vec(&ProviderAuthCommand::List)?;
    let authenticated = match harness.invoke(
        &provider_auth_service(),
        &input,
        &default_suite_authority(),
        Some(provider),
    ) {
        Ok(output) => match serde_json::from_slice::<ProviderAuthResponse>(&output)? {
            ProviderAuthResponse::Credentials { credentials } => !credentials.is_empty(),
            _ => false,
        },
        Err(_) => false,
    };
    let response: ModelResponse = invoke_projected(
        harness,
        &model_routing_service(),
        &ModelCommand::SetProviderAuthenticated {
            provider_plugin: provider.clone(),
            authenticated,
        },
        &default_suite_authority(),
    )?;
    if matches!(response, ModelResponse::Authentication { .. }) {
        Ok(())
    } else {
        Err("model routing service rejected provider authentication state".into())
    }
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
                    "backend": "phenix",
                    "provider": "provider.fixture",
                    "model": "model.test",
                    "inference": {"effort": "low"}
                },
                "callable_targets": {
                    "agent.scout": {
                        "backend": "phenix",
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
    fn runtime_model_target_lowers_foreign_json_before_dispatch() {
        let target: RuntimeModelTarget = serde_json::from_value(json!({
            "backend": "phenix",
            "provider": "provider.fixture",
            "model": "model.test",
            "inference": {"effort": "low"}
        }))
        .unwrap();
        let target = target.into_model_target();

        assert!(!target.options.contains_key("backend"));
        assert!(matches!(
            &target.options["inference"],
            PhenixValue::Map(values)
                if values.get("effort") == Some(&PhenixValue::String("low".into()))
        ));
    }

    fn invoke_configuration(
        harness: &mut PhenixHarness,
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
        let mut harness = PhenixHarness::default_suite().unwrap();
        harness.activate().unwrap();

        apply_startup_settings(
            &mut harness,
            Some(&directory),
            None,
            OptionStartupPrecedence::Nix,
        )
        .unwrap();

        let component = options_component_manifest();
        let command = OptionCommand::Resolve {
            key: OptionKey::parse("session.auto_create").unwrap(),
            context: OptionContext::default(),
        };
        let output = harness
            .kernel_mut()
            .invoke_component(
                &component.id,
                &options_service(),
                &serde_json::to_vec(&PhenixValue::from(&command)).unwrap(),
                &default_suite_authority(),
                &component.owner,
            )
            .unwrap();
        let output: PhenixValue = serde_json::from_slice(&output).unwrap();
        assert!(matches!(
            OptionResponse::try_from(Project(&output)).unwrap(),
            OptionResponse::Value { option }
                if option.value == OptionValue::Bool(false)
                    && option.source == OptionValueSource::Global
                    && option.layer == OptionValueLayer::File
        ));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn startup_migrates_legacy_generated_routes_outside_current_configuration() {
        let mut harness = PhenixHarness::default_suite().unwrap();
        harness.activate().unwrap();
        let mut target = sample_runtime()
            .routing_profiles
            .remove(0)
            .into_routing_profile()
            .default_target;
        target
            .options
            .insert("backend".into(), PhenixValue::String("phenix".into()));
        target.options.insert("inference".into(), PhenixValue::Unit);
        let legacy = direct_routing_profile(target).unwrap();
        let normalized = without_legacy_runtime_metadata(legacy.clone());
        let canonical = direct_routing_profile(normalized.default_target.clone()).unwrap();
        assert_ne!(legacy.id, canonical.id);
        invoke_projected::<_, ModelResponse>(
            &mut harness,
            &model_routing_service(),
            &ModelCommand::RegisterProfile {
                profile: legacy.clone(),
            },
            &default_suite_authority(),
        )
        .unwrap();
        apply_configuration(&mut harness, sample_runtime()).unwrap();
        let migrated: ModelResponse = invoke_projected(
            &mut harness,
            &model_routing_service(),
            &ModelCommand::GetProfile {
                id: legacy.id.clone(),
            },
            &default_suite_authority(),
        )
        .unwrap();
        assert_eq!(
            migrated,
            ModelResponse::Profile {
                profile: Some(normalized.clone())
            }
        );
        let candidates: ModelResponse = invoke_projected(
            &mut harness,
            &model_routing_service(),
            &ModelCommand::ListCandidates {
                profile_id: legacy.id,
                callable_id: None,
            },
            &default_suite_authority(),
        )
        .unwrap();
        assert!(
            matches!(candidates, ModelResponse::Candidates { candidates } if candidates.len() == 1 && candidates[0].capabilities.target == normalized.default_target)
        );
    }

    #[test]
    fn runtime_configuration_migrates_legacy_backend_metadata() {
        let mut harness = PhenixHarness::default_suite().unwrap();
        harness.activate().unwrap();

        let desired = sample_runtime()
            .routing_profiles
            .into_iter()
            .next()
            .unwrap()
            .into_routing_profile();
        let mut legacy = desired.clone();
        legacy
            .default_target
            .options
            .insert("backend".into(), PhenixValue::String("phenix".into()));
        for target in &mut legacy.fallback_targets {
            target
                .options
                .insert("backend".into(), PhenixValue::String("phenix".into()));
        }
        for target in legacy.callable_targets.values_mut() {
            target
                .options
                .insert("backend".into(), PhenixValue::String("phenix".into()));
        }

        let response: ModelResponse = invoke_projected(
            &mut harness,
            &model_routing_service(),
            &ModelCommand::RegisterProfile { profile: legacy },
            &default_suite_authority(),
        )
        .unwrap();
        assert!(matches!(
            response,
            ModelResponse::Profile { profile: Some(_) }
        ));

        apply_configuration(&mut harness, sample_runtime()).unwrap();

        let response: ModelResponse = invoke_projected(
            &mut harness,
            &model_routing_service(),
            &ModelCommand::GetProfile {
                id: desired.id.clone(),
            },
            &default_suite_authority(),
        )
        .unwrap();
        assert_eq!(
            response,
            ModelResponse::Profile {
                profile: Some(desired)
            }
        );
    }

    #[test]
    fn runtime_configuration_migrates_legacy_null_inference_metadata() {
        fn configuration() -> RuntimeConfiguration {
            serde_json::from_value(json!({
                "agents": [],
                "orchestrations": [],
                "routing_profiles": [{
                    "id": "router.legacy-null-inference",
                    "default_target": {
                        "backend": "phenix",
                        "provider": "provider.fixture",
                        "model": "model.test"
                    }
                }]
            }))
            .unwrap()
        }

        let mut harness = PhenixHarness::default_suite().unwrap();
        harness.activate().unwrap();

        let desired = configuration()
            .routing_profiles
            .into_iter()
            .next()
            .unwrap()
            .into_routing_profile();
        let mut legacy = desired.clone();
        legacy
            .default_target
            .options
            .insert("backend".into(), PhenixValue::String("phenix".into()));
        legacy
            .default_target
            .options
            .insert("inference".into(), PhenixValue::Unit);

        invoke_projected::<_, ModelResponse>(
            &mut harness,
            &model_routing_service(),
            &ModelCommand::RegisterProfile { profile: legacy },
            &default_suite_authority(),
        )
        .unwrap();

        apply_configuration(&mut harness, configuration()).unwrap();

        let response: ModelResponse = invoke_projected(
            &mut harness,
            &model_routing_service(),
            &ModelCommand::GetProfile {
                id: desired.id.clone(),
            },
            &default_suite_authority(),
        )
        .unwrap();
        assert_eq!(
            response,
            ModelResponse::Profile {
                profile: Some(desired)
            }
        );
    }

    #[test]
    fn runtime_configuration_rejects_nonlegacy_profile_identity_changes() {
        let mut harness = PhenixHarness::default_suite().unwrap();
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
            .contains("routing profile identity is immutable: router.test"));
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
        let mut harness = PhenixHarness::default_suite_with_persistence(
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
        let mut harness = PhenixHarness::default_suite_with_persistence(
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
            let mut harness = PhenixHarness::default_suite().unwrap();
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
        let mut harness = PhenixHarness::default_suite().unwrap();
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
    fn migrated_runtime_configuration_is_active_and_restart_safe() {
        let mut harness = PhenixHarness::default_suite().unwrap();
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
