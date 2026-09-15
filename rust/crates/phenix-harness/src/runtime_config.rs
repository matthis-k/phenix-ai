use crate::{default_suite_authority, PhenixHarness};
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
    backend: String,
    provider: PluginId,
    model: ModelId,
    #[serde(default)]
    inference: Value,
}

impl RuntimeModelTarget {
    fn into_model_target(self) -> ModelTarget {
        let mut options = BTreeMap::new();
        options.insert("backend".into(), PhenixValue::String(self.backend));
        options.insert("inference".into(), self.inference.into());
        ModelTarget {
            provider_plugin: self.provider,
            model: self.model,
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
    let authority = default_suite_authority();

    for agent in configuration.agents {
        if !matches!(
            invoke_projected::<_, ExecutionConfigurationResponse>(
                harness,
                &execution_configuration_service(),
                &ExecutionConfigurationCommand::RegisterAgent { agent },
                &authority,
            )?,
            ExecutionConfigurationResponse::Agent { agent: Some(_) }
        ) {
            return Err("execution configuration service rejected agent registration".into());
        }
    }

    for orchestration in configuration.orchestrations {
        if !matches!(
            invoke_projected::<_, ExecutionConfigurationResponse>(
                harness,
                &execution_configuration_service(),
                &ExecutionConfigurationCommand::RegisterOrchestration { orchestration },
                &authority,
            )?,
            ExecutionConfigurationResponse::Orchestration {
                orchestration: Some(_)
            }
        ) {
            return Err(
                "execution configuration service rejected orchestration registration".into(),
            );
        }
    }

    for profile in configuration.routing_profiles {
        ensure_routing_profile(harness, profile.into_routing_profile())?;
    }

    Ok(())
}

fn ensure_routing_profile(
    harness: &mut PhenixHarness,
    profile: RoutingProfile,
) -> Result<(), Box<dyn Error>> {
    let authority = default_suite_authority();
    let service = model_routing_service();
    let command = ModelCommand::GetProfile {
        id: profile.id.clone(),
    };
    let existing =
        match invoke_projected::<_, ModelResponse>(harness, &service, &command, &authority)? {
            ModelResponse::Profile { profile } => profile,
            _ => return Err("model routing service returned the wrong profile response".into()),
        };

    match existing {
        Some(existing) if existing == profile => {}
        Some(_) => {
            return Err(format!("routing profile identity is immutable: {}", profile.id).into())
        }
        None => {
            let command = ModelCommand::RegisterProfile {
                profile: profile.clone(),
            };
            if !matches!(
                invoke_projected::<_, ModelResponse>(harness, &service, &command, &authority)?,
                ModelResponse::Profile { profile: Some(_) }
            ) {
                return Err("model routing service rejected profile registration".into());
            }
        }
    }

    publish_routing_profile_runtime_state(harness, &profile)
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
        let target = RuntimeModelTarget {
            backend: "phenix".into(),
            provider: PluginId::parse("provider.fixture").unwrap(),
            model: ModelId::parse("model.test").unwrap(),
            inference: json!({"effort": "low"}),
        }
        .into_model_target();

        assert_eq!(
            target.options["backend"],
            PhenixValue::String("phenix".into())
        );
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
