#![forbid(unsafe_code)]

use phenix_application_interface::types::{Content, SessionInfo};
use phenix_core::{
    Authority, Bytes, CallableId, ComponentExport, ComponentId, ComponentInterface,
    ComponentManifest, Key, LocalPersistence, ModelFeatureGenerationId, ModelId,
    ModelInferenceInterface, ModelInferenceRequest, ModelInferenceResponse, ModelToolCall,
    ModelToolResult, PhenixValue, PluginContext, PluginExecution, PluginHost, PluginId,
    PluginInstance, PluginManifest, Project, RoutingProfileId, ServiceContribution, ServiceId,
    ServiceRole, ValueCodec, model_inference_service,
};
use phenix_harness::{
    PhenixRuntime, PhenixRuntimeBuilder, application::serve_configured_application,
    default_suite_authority, model_surface_fixture::model_surface_response,
};
use phenix_sdk::{
    CapacityKnowledge, ContextControl, EffectiveModelFeatures, ModelCommand, ModelLimits,
    ModelResponse, ModelTarget, OptionCommand, OptionKey, OptionResponse, OptionScope, OptionValue,
    RoutingProfile, model_routing_service, options_service,
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
const FIXTURE_INTROSPECTION_MODEL: &str = "fixture-introspection";
const FIXTURE_INTROSPECTION_PROFILE: &str = "fixture.introspection";
const FIXTURE_GENERATION: &str = "fixture-generation";
const FIXTURE_INTROSPECTION_GENERATION: &str = "fixture-introspection-generation";
const FIXTURE_CHILD_CLOSE_FIRST: &str = "PHENIX_FIXTURE_CHILD_CLOSE_FIRST";
const FIXTURE_CHILD_CLOSE_CHILD: &str = "PHENIX_FIXTURE_CHILD_CLOSE_CHILD";
const FIXTURE_CHILD_CLOSE_SECOND: &str = "PHENIX_FIXTURE_CHILD_CLOSE_SECOND";
const FIXTURE_CHILD_CLOSE_DONE: &str = "PHENIX_FIXTURE_CHILD_CLOSE_DONE";
const FIXTURE_CHILD_DONE: &str = "PHENIX_FIXTURE_CHILD_DONE";
const FIXTURE_SECOND_DONE: &str = "PHENIX_FIXTURE_SECOND_DONE";

struct FixtureProvider;

fn fixture_response(output: &str, tool_calls: Vec<ModelToolCall>) -> ModelInferenceResponse {
    ModelInferenceResponse {
        output: Bytes::new(output.as_bytes().to_vec()),
        provider_metadata: BTreeMap::new(),
        usage: Default::default(),
        tool_calls,
    }
}

fn fixture_tool_result(
    request: &ModelInferenceRequest,
    turn_index: usize,
) -> Result<&ModelToolResult, String> {
    let turn = request
        .continuation
        .get(turn_index)
        .ok_or_else(|| format!("missing fixture tool turn {turn_index}"))?;
    if turn.tool_results.len() != 1 {
        return Err(format!(
            "fixture turn {turn_index} produced {} tool results",
            turn.tool_results.len()
        ));
    }
    let result = &turn.tool_results[0];
    if result.is_error {
        return Err(format!(
            "fixture tool {} failed: {:?}",
            result.callable_id, result.output
        ));
    }
    Ok(result)
}

fn fixture_session_call(
    call_id: &str,
    operation: &str,
    arguments: BTreeMap<String, PhenixValue>,
) -> ModelToolCall {
    ModelToolCall {
        call_id: call_id.to_owned(),
        callable_id: CallableId::parse("phenix.session").expect("static fixture callable id"),
        input: PhenixValue::Table(BTreeMap::from([
            (
                Key::parse("operation").expect("static fixture key"),
                PhenixValue::String(operation.to_owned()),
            ),
            (
                Key::parse("arguments").expect("static fixture key"),
                PhenixValue::Map(arguments),
            ),
        ])),
    }
}

fn fixture_session_orchestration(
    request: &ModelInferenceRequest,
) -> Result<Option<ModelInferenceResponse>, String> {
    let input = String::from_utf8_lossy(request.input.as_ref());

    if input.trim_end().ends_with(FIXTURE_CHILD_CLOSE_CHILD) {
        return Ok(Some(fixture_response(FIXTURE_CHILD_DONE, Vec::new())));
    }
    if input.trim_end().ends_with(FIXTURE_CHILD_CLOSE_SECOND) {
        return Ok(Some(fixture_response(FIXTURE_SECOND_DONE, Vec::new())));
    }
    if !input.contains(FIXTURE_CHILD_CLOSE_FIRST) {
        return Ok(None);
    }

    let response = match request.continuation.len() {
        0 => fixture_response(
            "create lifecycle child",
            vec![fixture_session_call(
                "fixture-create-child",
                "create",
                BTreeMap::from([
                    (
                        "working_directory".to_owned(),
                        PhenixValue::String("/workspace".to_owned()),
                    ),
                    (
                        "title".to_owned(),
                        PhenixValue::String("fixture lifecycle child".to_owned()),
                    ),
                ]),
            )],
        ),
        1 => {
            let child = SessionInfo::from_value(&fixture_tool_result(request, 0)?.output)
                .map_err(|error| error.to_string())?;
            fixture_response(
                "prompt lifecycle child",
                vec![fixture_session_call(
                    "fixture-prompt-child",
                    "prompt",
                    BTreeMap::from([
                        (
                            "session_id".to_owned(),
                            PhenixValue::String(child.session_id.to_string()),
                        ),
                        (
                            "content".to_owned(),
                            PhenixValue::List(vec![
                                Content::Text {
                                    text: FIXTURE_CHILD_CLOSE_CHILD.to_owned(),
                                }
                                .to_value(),
                            ]),
                        ),
                    ]),
                )],
            )
        }
        2 => {
            let child = SessionInfo::from_value(&fixture_tool_result(request, 0)?.output)
                .map_err(|error| error.to_string())?;
            let _ = fixture_tool_result(request, 1)?;
            fixture_response(
                "close lifecycle child",
                vec![fixture_session_call(
                    "fixture-close-child",
                    "close",
                    BTreeMap::from([(
                        "session_id".to_owned(),
                        PhenixValue::String(child.session_id.to_string()),
                    )]),
                )],
            )
        }
        3 => {
            let _ = fixture_tool_result(request, 2)?;
            fixture_response(FIXTURE_CHILD_CLOSE_DONE, Vec::new())
        }
        turns => {
            return Err(format!(
                "fixture lifecycle orchestration received {turns} continuation turns"
            ));
        }
    };
    Ok(Some(response))
}

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
        if !matches!(
            request.model.as_str(),
            FIXTURE_MODEL | FIXTURE_INTROSPECTION_MODEL
        ) {
            return Err(format!(
                "fixture provider received unexpected model {}",
                request.model
            ));
        }

        let input = String::from_utf8_lossy(request.input.as_ref());
        let session_orchestration = fixture_session_orchestration(&request)?;
        if session_orchestration.is_none()
            && let Ok(expected) = env::var("PHENIX_FIXTURE_EXPECT_INPUT")
            && !expected.is_empty()
            && !input.contains(&expected)
        {
            return Err(format!(
                "fixture model input did not contain expected marker {expected:?}"
            ));
        }
        let response = if request.model.as_str() == FIXTURE_INTROSPECTION_MODEL {
            model_surface_response(&request).map_err(|error| error.to_string())?
        } else if let Some(response) = session_orchestration {
            response
        } else {
            let response = env::var("PHENIX_FIXTURE_RESPONSE")
                .unwrap_or_else(|_| "phenix deterministic fixture response".to_owned());
            fixture_response(&response, Vec::new())
        };
        context
            .kernel
            .encode_value(&response)
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

fn fixture_target(model: &str) -> ModelTarget {
    ModelTarget {
        provider_plugin: fixture_provider_id(),
        model: ModelId::parse(model).expect("static fixture model id is valid"),
        options: BTreeMap::new(),
    }
}

fn invoke_model(
    harness: &mut PhenixRuntime,
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

fn configure_fixture(harness: &mut PhenixRuntime) -> Result<(), Box<dyn Error>> {
    let target = fixture_target(FIXTURE_MODEL);
    let introspection_target = fixture_target(FIXTURE_INTROSPECTION_MODEL);
    // Session snapshots resolve the default route before the frontend can select
    // a named fixture route. Keep the deterministic route as the default and expose
    // introspection separately so existing product fixtures retain their behavior.
    for (profile_id, profile_target) in [
        ("default", target.clone()),
        (FIXTURE_PROFILE, target.clone()),
        (FIXTURE_INTROSPECTION_PROFILE, introspection_target.clone()),
    ] {
        let profile = RoutingProfile {
            id: RoutingProfileId::parse(profile_id)?,
            default_target: profile_target,
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
                return Err("fixture routing profile identity changed".into());
            }
            ModelResponse::Profile { profile: None } => {
                match invoke_model(harness, &ModelCommand::RegisterProfile { profile })? {
                    ModelResponse::Profile { profile: Some(_) } => {}
                    other => {
                        return Err(
                            format!("fixture profile registration failed: {other:?}").into()
                        );
                    }
                }
            }
            other => return Err(format!("fixture profile lookup failed: {other:?}").into()),
        }
    }
    for (target, generation) in [
        (target, FIXTURE_GENERATION),
        (introspection_target, FIXTURE_INTROSPECTION_GENERATION),
    ] {
        match invoke_model(
            harness,
            &ModelCommand::PublishModelFeatures {
                features: EffectiveModelFeatures {
                    target,
                    generation: ModelFeatureGenerationId::parse(generation)?,
                    context: ContextControl::ReplaceableTurns,
                    capacity: CapacityKnowledge::Known {
                        limits: ModelLimits {
                            context_window_tokens: 128 * 1024,
                            max_output_tokens: Some(16 * 1024),
                        },
                    },
                    cache: Default::default(),
                    optional: BTreeSet::new(),
                },
            },
        )? {
            ModelResponse::Features { .. } => {}
            other => return Err(format!("fixture feature publication failed: {other:?}").into()),
        }
    }
    let option = OptionCommand::Set {
        key: OptionKey::parse("agent.runtime_orchestration")
            .expect("static fixture option key is valid"),
        scope: OptionScope::Global,
        value: OptionValue::Bool(true),
    };
    let input = serde_json::to_vec(&PhenixValue::from(&option))?;
    let output = harness.invoke(&options_service(), &input, &default_suite_authority(), None)?;
    let output: PhenixValue = serde_json::from_slice(&output)?;
    match OptionResponse::try_from(Project(&output))? {
        OptionResponse::Updated { .. } => {}
        other => return Err(format!("fixture orchestration option failed: {other:?}").into()),
    }

    Ok(())
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
    let mut builder = PhenixRuntimeBuilder::with_default_suite()?;
    builder.add_embedded(fixture_manifest(), || Box::new(FixtureProvider))?;
    builder.add_component(fixture_component());
    let mut harness = builder.build_with_persistence(persistence)?;
    harness.activate()?;
    configure_fixture(&mut harness)?;
    serve_configured_application(harness).await?;
    Ok(())
}
