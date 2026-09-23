#![forbid(unsafe_code)]

use phenix_core::{
    model_inference_service, Authority, Bytes, CapabilityGenerationId, ComponentExport,
    ComponentId, ComponentInterface, ComponentManifest, LocalPersistence, ModelId,
    ModelInferenceInterface, ModelInferenceRequest, ModelInferenceResponse, PhenixValue,
    PluginContext, PluginExecution, PluginHost, PluginId, PluginInstance, PluginManifest, Project,
    RoutingProfileId, ServiceContribution, ServiceId, ServiceRole,
};
use phenix_harness::{
    application::serve_configured_application, default_suite_authority, HarnessBuilder,
    PhenixHarness,
};
use phenix_sdk::{
    model_routing_service, CapacityKnowledge, ContextControl, EffectiveModelCapabilities,
    ModelCommand, ModelLimits, ModelResponse, ModelTarget, RoutingProfile,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    io,
    path::PathBuf,
};

const FIXTURE_PROVIDER: &str = "fixture.deterministic-provider";
const FIXTURE_MODEL: &str = "fixture-model";
const FIXTURE_PROFILE: &str = "fixture.deterministic";
const FIXTURE_GENERATION: &str = "fixture-generation";

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
                &ModelInferenceInterface::interface_id(),
                input,
            )
            .map_err(|error| error.to_string())?;
        if request.model.as_str() != FIXTURE_MODEL {
            return Err(format!(
                "fixture provider received unexpected model {}",
                request.model
            ));
        }

        if let Ok(expected) = env::var("PHENIX_FIXTURE_EXPECT_INPUT") {
            if !expected.is_empty()
                && !String::from_utf8_lossy(request.input.as_ref()).contains(&expected)
            {
                return Err(format!(
                    "fixture model input did not contain expected marker {expected:?}"
                ));
            }
        }
        let response = env::var("PHENIX_FIXTURE_RESPONSE")
            .unwrap_or_else(|_| "phenix deterministic fixture response".to_owned());
        context
            .kernel
            .encode_value(&ModelInferenceResponse {
                output: Bytes::new(response.into_bytes()),
                provider_metadata: BTreeMap::new(),
                usage: Default::default(),
                tool_calls: Vec::new(),
            })
            .map_err(|error| error.to_string())
    }
}

fn fixture_provider_id() -> PluginId {
    PluginId::parse(FIXTURE_PROVIDER).expect("static fixture provider id is valid")
}

fn fixture_manifest() -> PluginManifest {
    PluginManifest {
        id: fixture_provider_id(),
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

fn fixture_component() -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: ComponentId::parse(FIXTURE_PROVIDER).expect("static fixture component id is valid"),
        owner: fixture_provider_id(),
        imports: Vec::new(),
        exports: vec![ComponentExport {
            interface: ModelInferenceInterface::interface_id(),
            schema: ModelInferenceInterface::schema(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        maximum_authority: Authority::default(),
    }
}

fn fixture_target() -> ModelTarget {
    ModelTarget {
        provider_plugin: fixture_provider_id(),
        model: ModelId::parse(FIXTURE_MODEL).expect("static fixture model id is valid"),
        options: BTreeMap::new(),
    }
}

fn invoke_model(
    harness: &mut PhenixHarness,
    command: &ModelCommand,
) -> Result<ModelResponse, Box<dyn Error>> {
    let input = serde_json::to_vec(&PhenixValue::from(command))?;
    let output = harness.invoke(
        &model_routing_service(),
        &input,
        &default_suite_authority(),
        None,
    )?;
    let output: PhenixValue = serde_json::from_slice(&output)?;
    Ok(ModelResponse::try_from(Project(&output))?)
}

fn configure_fixture(harness: &mut PhenixHarness) -> Result<(), Box<dyn Error>> {
    let target = fixture_target();
    // Session snapshots resolve the default route before the frontend can select
    // the named fixture route. Both must be valid in this standalone runtime.
    for profile_id in ["default", FIXTURE_PROFILE] {
        let profile = RoutingProfile {
            id: RoutingProfileId::parse(profile_id)?,
            default_target: target.clone(),
            fallback_targets: Vec::new(),
            callable_targets: BTreeMap::new(),
        };
        match invoke_model(
            harness,
            &ModelCommand::GetProfile {
                id: profile.id.clone(),
            },
        )? {
            ModelResponse::Profile {
                profile: Some(existing),
            } if existing == profile => {}
            ModelResponse::Profile { profile: Some(_) } => {
                return Err("fixture routing profile identity changed".into())
            }
            ModelResponse::Profile { profile: None } => {
                match invoke_model(harness, &ModelCommand::RegisterProfile { profile })? {
                    ModelResponse::Profile { profile: Some(_) } => {}
                    other => {
                        return Err(format!("fixture profile registration failed: {other:?}").into())
                    }
                }
            }
            other => return Err(format!("fixture profile lookup failed: {other:?}").into()),
        }
    }
    match invoke_model(
        harness,
        &ModelCommand::PublishCapabilities {
            capabilities: EffectiveModelCapabilities {
                target,
                generation: CapabilityGenerationId::parse(FIXTURE_GENERATION)?,
                context: ContextControl::ReplaceableTurns,
                capacity: CapacityKnowledge::Known {
                    limits: ModelLimits {
                        context_window_tokens: 128 * 1024,
                        max_output_tokens: Some(16 * 1024),
                    },
                },
                optional: BTreeSet::new(),
            },
        },
    )? {
        ModelResponse::Capabilities { .. } => {}
        other => return Err(format!("fixture capability publication failed: {other:?}").into()),
    }
    match invoke_model(
        harness,
        &ModelCommand::SetProviderAuthenticated {
            provider_plugin: fixture_provider_id(),
            authenticated: true,
        },
    )? {
        ModelResponse::Authentication { .. } => Ok(()),
        other => Err(format!("fixture authentication publication failed: {other:?}").into()),
    }
}

fn state_path() -> Result<PathBuf, Box<dyn Error>> {
    env::var_os("PHENIX_STATE_DB")
        .map(PathBuf::from)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "PHENIX_STATE_DB is required for phenix-acp-fixture",
            )
        })
        .map_err(Into::into)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let state = state_path()?;
    if let Some(parent) = state.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let persistence = LocalPersistence::open(state)?;
    let mut builder = HarnessBuilder::with_default_suite()?;
    builder.add_embedded(fixture_manifest(), || Box::new(FixtureProvider))?;
    builder.add_component(fixture_component());
    let mut harness = builder.build_with_persistence(persistence)?;
    harness.activate()?;
    configure_fixture(&mut harness)?;
    serve_configured_application(harness).await?;
    Ok(())
}
