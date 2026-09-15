use crate::{memory_component_manifest, memory_factory, memory_manifest};
use phenix_core::{
    model_inference_service, Authority, Kernel, LocalPersistence, ModelId, PhenixValue,
    PluginExecution, PluginHost, PluginId, PluginInstance, PluginManifest, ResolvedHarness,
    ResolvedHarnessActivation, ServiceContribution, ServiceId, ServiceRole, SessionId,
};
use phenix_plugin_models::{
    model_routing_component_manifest, model_routing_factory, model_routing_manifest,
    model_routing_service, ModelCommand as RoutingCommand, ModelResponse as RoutingResponse,
    ModelTarget, RoutingProfile,
};
use phenix_plugin_sessions::{session_component_manifest, session_factory, session_manifest};
use phenix_sdk::{
    memory_service, memory_validate_callable, session_history_resource, session_service,
    MemoryCommand, MemoryFreshness, MemoryKind, MemoryRecord, MemoryResponse, MemoryScope,
    MemorySourceReference, SessionCommand, SessionHistoryContentPart, SessionHistoryDraft,
    SessionHistoryFinishReason, SessionHistoryRole, SessionResponse,
};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

const FAILING_PROVIDER: &str = "fixture.memory-validation-failure";

fn temp_db() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "phenix-memory-revalidation-failure-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

fn kernel_with(path: &PathBuf) -> Kernel {
    let memory = memory_manifest();
    let routing = model_routing_manifest(Authority::default());
    let sessions = session_manifest();
    let provider = fixture_provider_manifest();
    let resolved = ResolvedHarness::resolve(
        [
            memory.clone(),
            routing.clone(),
            sessions.clone(),
            provider.clone(),
        ],
        [
            memory_component_manifest(),
            model_routing_component_manifest(Authority::default()),
            session_component_manifest(),
        ],
        [],
        &memory.maximum_authority,
    )
    .unwrap();
    let persistence = LocalPersistence::open(path).unwrap();
    let mut kernel = Kernel::with_persistence(resolved.kernel_config().clone(), persistence);
    kernel.activate_resolved_harness(&resolved).unwrap();
    kernel
        .register_embedded_factory(memory.id, memory_factory)
        .unwrap();
    kernel
        .register_embedded_factory(routing.id, model_routing_factory)
        .unwrap();
    kernel
        .register_embedded_factory(sessions.id, session_factory)
        .unwrap();
    kernel
        .register_embedded_factory(provider.id, || Box::new(FailingProvider))
        .unwrap();
    kernel.activate_all().unwrap();
    kernel
}

fn fixture_provider_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(FAILING_PROVIDER).unwrap(),
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

struct FailingProvider;

impl PluginInstance for FailingProvider {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        _input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &model_inference_service() {
            return Err(format!("unsupported fixture service: {service}"));
        }
        Err("fixture validation failure".into())
    }
}

fn invoke_memory(kernel: &mut Kernel, command: MemoryCommand) -> Result<MemoryResponse, String> {
    let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
    let output = kernel
        .invoke(
            &memory_service(),
            &input,
            &memory_manifest().maximum_authority,
            None,
        )
        .map_err(|error| error.to_string())?;
    let output: PhenixValue = serde_json::from_slice(&output).map_err(|error| error.to_string())?;
    output.project().map_err(|error| error.to_string())
}

fn invoke_sessions(
    kernel: &mut Kernel,
    command: SessionCommand,
) -> Result<SessionResponse, String> {
    let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
    let output = kernel
        .invoke(
            &session_service(),
            &input,
            &session_manifest().maximum_authority,
            None,
        )
        .map_err(|error| error.to_string())?;
    let output: PhenixValue = serde_json::from_slice(&output).map_err(|error| error.to_string())?;
    output.project().map_err(|error| error.to_string())
}

fn invoke_routing(kernel: &mut Kernel, command: RoutingCommand) -> Result<RoutingResponse, String> {
    let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
    let output = kernel
        .invoke(
            &model_routing_service(),
            &input,
            &model_routing_manifest(Authority::default()).maximum_authority,
            None,
        )
        .map_err(|error| error.to_string())?;
    let output: PhenixValue = serde_json::from_slice(&output).map_err(|error| error.to_string())?;
    output.project().map_err(|error| error.to_string())
}

#[test]
fn revalidation_failure_leaves_authoritative_session_history_unchanged() {
    let path = temp_db();
    let mut kernel = kernel_with(&path);
    let session_id = SessionId::parse("root").unwrap();
    invoke_sessions(
        &mut kernel,
        SessionCommand::Create {
            id: session_id.clone(),
        },
    )
    .unwrap();
    let draft = SessionHistoryDraft {
        role: SessionHistoryRole::Assistant,
        content: vec![SessionHistoryContentPart::Text {
            text: "authoritative answer".into(),
        }],
        tool_calls: Vec::new(),
        tool_results: Vec::new(),
        finish_reason: Some(SessionHistoryFinishReason::Complete),
        usage: None,
        context_revision: "ctx-1".into(),
        instruction_revision: "instructions-1".into(),
    };
    invoke_sessions(
        &mut kernel,
        SessionCommand::AppendHistory {
            id: session_id.clone(),
            entry: draft.clone(),
        },
    )
    .unwrap();
    let resource = session_history_resource(&session_id, 1);

    let memory = MemoryRecord {
        id: "derived-answer".into(),
        kind: MemoryKind::Fact,
        scope: MemoryScope::Session {
            session_id: session_id.clone(),
        },
        content: "derived answer".into(),
        source_refs: vec![MemorySourceReference {
            service: session_service(),
            resource: resource.clone(),
            start: None,
            end: None,
        }],
        supersedes: Vec::new(),
        valid_from: None,
        valid_until: None,
        created_at: 10,
    };
    invoke_memory(
        &mut kernel,
        MemoryCommand::Record {
            record: memory.clone(),
        },
    )
    .unwrap();
    invoke_memory(
        &mut kernel,
        MemoryCommand::ObserveRevision {
            service: session_service(),
            resource: resource.clone(),
            revision: "rev-2".into(),
            observed_at: 20,
            limit: 10,
        },
    )
    .unwrap();

    let provider = PluginId::parse(FAILING_PROVIDER).unwrap();
    let profile_id = phenix_core::RoutingProfileId::parse("failure-route").unwrap();
    let target = ModelTarget {
        provider_plugin: provider.clone(),
        model: ModelId::parse("validate-fail").unwrap(),
        options: BTreeMap::new(),
    };
    invoke_routing(
        &mut kernel,
        RoutingCommand::RegisterProfile {
            profile: RoutingProfile {
                id: profile_id.clone(),
                default_target: target.clone(),
                fallback_targets: Vec::new(),
                callable_targets: BTreeMap::from([(memory_validate_callable(), target)]),
            },
        },
    )
    .unwrap();
    invoke_routing(
        &mut kernel,
        RoutingCommand::SetProviderAuthenticated {
            provider_plugin: provider,
            authenticated: true,
        },
    )
    .unwrap();

    let error = invoke_memory(
        &mut kernel,
        MemoryCommand::Revalidate {
            id: memory.id.clone(),
            profile_id,
            at: 30,
        },
    )
    .unwrap_err();
    assert!(error.contains("fixture validation failure"));

    let freshness = invoke_memory(
        &mut kernel,
        MemoryCommand::GetFreshness {
            id: memory.id.clone(),
        },
    )
    .unwrap();
    assert!(matches!(
        freshness,
        MemoryResponse::Freshness { state: Some(state) }
            if state.freshness == MemoryFreshness::NeedsValidation && state.changed_at == 20
    ));

    let source = invoke_sessions(&mut kernel, SessionCommand::ResolveHistory { resource }).unwrap();
    assert!(matches!(
        source,
        SessionResponse::HistoryEntry { entry: Some(entry) }
            if entry.content == draft.content
                && entry.context_revision == draft.context_revision
                && entry.instruction_revision == draft.instruction_revision
    ));

    let _ = fs::remove_file(path);
}
