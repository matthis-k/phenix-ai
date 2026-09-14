use crate::{memory_component_manifest, memory_factory, memory_manifest};
use phenix_core::{
    Authority, ComponentExport, ComponentId, ComponentInterface, ComponentManifest, Kernel,
    LocalPersistence, PhenixValue, PluginExecution, PluginHost, PluginId, PluginInstance,
    PluginManifest, ResolvedHarness, ResolvedHarnessActivation, RoutingProfileId,
    ServiceContribution, ServiceId, ServiceRole, SessionId,
};
use phenix_plugin_sessions::{session_component_manifest, session_factory, session_manifest};
use phenix_sdk::{
    helper_invocation_service, memory_service, session_history_resource, session_service,
    HelperInvocationCommand, HelperInvocationInterface, MemoryCommand, MemoryFreshness, MemoryKind,
    MemoryRecord, MemoryResponse, MemoryScope, MemorySourceReference, SessionCommand,
    SessionHistoryContentPart, SessionHistoryDraft, SessionHistoryFinishReason, SessionHistoryRole,
    SessionResponse,
};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

const FAILING_PROVIDER: &str = "fixture.memory-validation-failure";
const HELPER_COMPONENT: &str = "fixture.memory-validation-failure";
const EXECUTION: &str = "execution-1";
const PARENT_ATTEMPT: &str = "attempt-1";

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
    let sessions = session_manifest();
    let provider = fixture_provider_manifest();
    let resolved = ResolvedHarness::resolve(
        [memory.clone(), sessions.clone(), provider.clone()],
        [
            memory_component_manifest(),
            session_component_manifest(),
            helper_component_manifest(),
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
            service: helper_invocation_service(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn helper_component_manifest() -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: ComponentId::parse(HELPER_COMPONENT).unwrap(),
        owner: PluginId::parse(FAILING_PROVIDER).unwrap(),
        imports: Vec::new(),
        exports: vec![ComponentExport {
            interface: HelperInvocationInterface::interface_id(),
            schema: HelperInvocationInterface::schema(),
            priority: 100,
            required_authority: Authority::default(),
        }],
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
        input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &helper_invocation_service() {
            return Err(format!("unsupported fixture service: {service}"));
        }
        let input: PhenixValue =
            serde_json::from_slice(input).map_err(|error| error.to_string())?;
        let command: HelperInvocationCommand = input.project().map_err(|error| error.to_string())?;
        let HelperInvocationCommand::Invoke { request } = command;
        if request.execution_id != EXECUTION || request.parent_attempt_id != PARENT_ATTEMPT {
            return Err("memory revalidation lost execution lineage".into());
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

    let error = invoke_memory(
        &mut kernel,
        MemoryCommand::Revalidate {
            id: memory.id.clone(),
            execution_id: EXECUTION.into(),
            parent_attempt_id: PARENT_ATTEMPT.into(),
            profile_id: RoutingProfileId::parse("failure-route").unwrap(),
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
