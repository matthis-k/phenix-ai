use crate::{
    debug_component_id, ContextProbeCommand, FrontendProbeCommand, JobProbeCommand,
    ModelProbeCommand, PlanningProbeCommand, SessionProbeCommand,
};
use phenix_core::{
    Authority, ComponentInterface, ComponentInvocationError, EventEnvelope, GraphGenerationId,
    PhenixValue, PluginContext, PluginExecution, PluginHost, PluginInstance, PluginListener,
    PluginManifest, ResolvedListener, RuntimeTraceEvent, SdkClient, ServiceContribution, ServiceId,
};
use phenix_sdk::{
    ContextInterface, FrontendInterface, JobInterface, ModelDiagnosticEvent, ModelRoutingInterface,
    PlanningInterface, SessionInterface,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::BTreeMap,
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    process,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

pub const DEBUG_SERVICE: &str = "phenix.debug@1";
pub const DEBUG_LOG_ENV: &str = "PHENIX_DEBUG_LOG";
const RUNTIME_TRACE_LISTENER_METHOD: &str = "runtime_trace";
const MODEL_DIAGNOSTIC_LISTENER_METHOD: &str = "model_diagnostic";
static TRACE_WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum DebugCommand {
    Snapshot,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DiagnosticEntry {
    Available { value: PhenixValue },
    Unavailable { error: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct DiagnosticSnapshot {
    pub services: BTreeMap<String, DiagnosticEntry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case")]
pub enum DebugResponse {
    Snapshot { snapshot: DiagnosticSnapshot },
}

#[must_use]
pub fn debug_manifest(maximum_authority: Authority) -> PluginManifest {
    PluginManifest {
        id: crate::Plugin::plugin_id(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: phenix_core::ServiceRole::Terminal,
            service: debug_service(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority,
    }
}

#[must_use]
pub fn debug_factory() -> Box<dyn PluginInstance> {
    Box::new(crate::Plugin)
}

#[must_use]
pub fn debug_service() -> ServiceId {
    ServiceId::parse(DEBUG_SERVICE).expect("static debug service id is valid")
}

struct DebugSdk<'host, 'runtime> {
    sessions: SdkClient<'host, 'runtime, SessionInterface>,
    context: SdkClient<'host, 'runtime, ContextInterface>,
    planning: SdkClient<'host, 'runtime, PlanningInterface>,
    jobs: SdkClient<'host, 'runtime, JobInterface>,
    models: SdkClient<'host, 'runtime, ModelRoutingInterface>,
    frontends: SdkClient<'host, 'runtime, FrontendInterface>,
}

type DebugContext<'host, 'runtime> = PluginContext<'host, 'runtime, DebugSdk<'host, 'runtime>>;

fn context<'host, 'runtime>(host: &'host PluginHost<'runtime>) -> DebugContext<'host, 'runtime> {
    let component = debug_component_id();
    PluginContext::new(
        host,
        DebugSdk {
            sessions: SdkClient::new(host, component.clone()),
            context: SdkClient::new(host, component.clone()),
            planning: SdkClient::new(host, component.clone()),
            jobs: SdkClient::new(host, component.clone()),
            models: SdkClient::new(host, component.clone()),
            frontends: SdkClient::new(host, component),
        },
        (),
        (),
    )
}

struct RuntimeTraceLogger;

impl PluginListener for RuntimeTraceLogger {
    fn handle(&self, event: &EventEnvelope, _host: &PluginHost<'_>) -> Result<(), String> {
        match serde_json::from_slice::<RuntimeTraceEvent>(&event.payload) {
            Ok(trace) => record_trace(
                "runtime_trace",
                json!({
                    "emitter": event.emitter.as_str(),
                    "causality_id": event.causality_id,
                    "trace": trace,
                }),
            ),
            Err(error) => record_trace(
                "runtime_trace_decode_failed",
                json!({
                    "emitter": event.emitter.as_str(),
                    "causality_id": event.causality_id,
                    "error": error.to_string(),
                    "payload_bytes": event.payload.len(),
                }),
            ),
        }
        Ok(())
    }
}

struct ModelDiagnosticLogger;

impl PluginListener for ModelDiagnosticLogger {
    fn handle(&self, event: &EventEnvelope, _host: &PluginHost<'_>) -> Result<(), String> {
        match serde_json::from_slice::<ModelDiagnosticEvent>(&event.payload) {
            Ok(diagnostic) => record_trace(
                "model_diagnostic",
                json!({
                    "emitter": event.emitter.as_str(),
                    "causality_id": event.causality_id,
                    "diagnostic": diagnostic,
                }),
            ),
            Err(error) => record_trace(
                "model_diagnostic_decode_failed",
                json!({
                    "emitter": event.emitter.as_str(),
                    "causality_id": event.causality_id,
                    "error": error.to_string(),
                    "payload_bytes": event.payload.len(),
                }),
            ),
        }
        Ok(())
    }
}

impl PluginInstance for crate::Plugin {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        let openai_api_key = env::var_os("OPENAI_API_KEY");
        record_trace(
            "debug_started",
            json!({
                "trace_path": trace_path().to_string_lossy(),
                "openai_api_key": {
                    "defined": openai_api_key.is_some(),
                    "non_empty": openai_api_key
                        .as_ref()
                        .is_some_and(|value| !value.as_os_str().is_empty()),
                },
                "phenix_state_db": env_text("PHENIX_STATE_DB"),
                "phenix_config_dir": env_text("PHENIX_CONFIG_DIR"),
                "phenix_default_config_dir": env_text("PHENIX_DEFAULT_CONFIG_DIR"),
                "phenix_nix_settings": env_text("PHENIX_NIX_SETTINGS"),
                "phenix_settings_precedence": env_text("PHENIX_SETTINGS_PRECEDENCE"),
                "phenix_enabled_plugins": env_text("PHENIX_ENABLED_PLUGINS"),
            }),
        );
        Ok(())
    }

    fn bind_plugin_listener(
        &mut self,
        listener: &ResolvedListener,
        _generation: &GraphGenerationId,
    ) -> Option<Result<Arc<dyn PluginListener>, String>> {
        match listener.declaration.method.as_str() {
            RUNTIME_TRACE_LISTENER_METHOD => Some(Ok(Arc::new(RuntimeTraceLogger))),
            MODEL_DIAGNOSTIC_LISTENER_METHOD => Some(Ok(Arc::new(ModelDiagnosticLogger))),
            _ => None,
        }
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &debug_service() {
            return Err(format!("unsupported debug service: {service}"));
        }
        let context = context(host);
        let interface = crate::DebugInterface::interface_id();
        let command = context
            .kernel
            .decode_projected::<DebugCommand>(&interface, input)
            .map_err(|error| error.to_string())?;
        let response = handle(&context, command);
        context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string())
    }
}

fn handle(context: &DebugContext<'_, '_>, command: DebugCommand) -> DebugResponse {
    match command {
        DebugCommand::Snapshot => DebugResponse::Snapshot {
            snapshot: snapshot(context),
        },
    }
}

fn snapshot(context: &DebugContext<'_, '_>) -> DiagnosticSnapshot {
    let mut services = BTreeMap::new();
    probe(
        &context.sdk.sessions,
        &mut services,
        "sessions",
        &SessionProbeCommand::List,
    );
    probe(
        &context.sdk.context,
        &mut services,
        "context",
        &ContextProbeCommand::List,
    );
    probe(
        &context.sdk.planning,
        &mut services,
        "planning_history",
        &PlanningProbeCommand::SearchHistory {
            objective_id: None,
            query: String::new(),
        },
    );
    probe(
        &context.sdk.jobs,
        &mut services,
        "jobs",
        &JobProbeCommand::List,
    );
    probe(
        &context.sdk.models,
        &mut services,
        "models",
        &ModelProbeCommand::ListProfiles,
    );
    probe(
        &context.sdk.frontends,
        &mut services,
        "frontends",
        &FrontendProbeCommand::Catalog,
    );
    DiagnosticSnapshot { services }
}

fn probe<I, Request>(
    client: &SdkClient<'_, '_, I>,
    entries: &mut BTreeMap<String, DiagnosticEntry>,
    name: &str,
    command: &Request,
) where
    I: ComponentInterface,
    for<'value> PhenixValue: From<&'value Request>,
{
    let request = PhenixValue::from(command);
    let entry = match client.invoke_value(&request) {
        Ok(response) => response_entry(response),
        Err(error) => error_entry(error),
    };
    entries.insert(name.into(), entry);
}

fn response_entry(response: PhenixValue) -> DiagnosticEntry {
    DiagnosticEntry::Available { value: response }
}

fn error_entry(error: ComponentInvocationError) -> DiagnosticEntry {
    DiagnosticEntry::Unavailable {
        error: error.to_string(),
    }
}

fn env_text(name: &str) -> Option<String> {
    env::var_os(name).map(|value| value.to_string_lossy().into_owned())
}

fn trace_path() -> PathBuf {
    if let Some(path) = env::var_os(DEBUG_LOG_ENV).filter(|path| !path.as_os_str().is_empty()) {
        return PathBuf::from(path);
    }
    if let Some(state_db) = env::var_os("PHENIX_STATE_DB") {
        let state_db = PathBuf::from(state_db);
        if let Some(parent) = state_db.parent() {
            return parent.join("debug.jsonl");
        }
    }
    if let Some(state_home) = env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(state_home).join("phenix/debug.jsonl");
    }
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home).join(".local/state/phenix/debug.jsonl");
    }
    env::temp_dir().join("phenix-debug.jsonl")
}

fn record_trace(kind: &str, payload: serde_json::Value) {
    if let Err(error) = append_trace(kind, payload) {
        eprintln!("phenix.debug: failed to write diagnostic trace: {error}");
    }
}

fn append_trace(kind: &str, payload: serde_json::Value) -> Result<(), String> {
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_millis();
    let record = json!({
        "timestamp_ms": timestamp_ms,
        "pid": process::id(),
        "kind": kind,
        "payload": payload,
    });
    let mut line = serde_json::to_vec(&record).map_err(|error| error.to_string())?;
    line.push(b'\n');

    let _guard = TRACE_WRITE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let path = trace_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    file.write_all(&line)
        .map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug_component_manifest;
    use phenix_core::{Kernel, KernelConfig, Project, ResolvedHarness, ResolvedHarnessActivation};
    use phenix_plugin_sessions::{session_component_manifest, session_factory, session_manifest};
    use phenix_sdk::SessionResponse;

    #[test]
    fn diagnostic_service_uses_resolved_optional_imports_without_kernel_fallbacks() {
        let session_manifest = session_manifest();
        let authority = session_manifest.maximum_authority.clone();
        let session_id = session_manifest.id.clone();
        let debug_manifest = debug_manifest(authority.clone());
        let debug_id = debug_manifest.id.clone();
        let manifests = vec![session_manifest, debug_manifest];
        let resolved = ResolvedHarness::resolve(
            manifests.clone(),
            [
                session_component_manifest(),
                debug_component_manifest(authority.clone()),
            ],
            [],
            &authority,
        )
        .unwrap();
        let mut kernel = Kernel::new(KernelConfig::new(manifests).unwrap());
        kernel
            .register_embedded_factory(session_id, session_factory)
            .unwrap();
        kernel
            .register_embedded_factory(debug_id, debug_factory)
            .unwrap();
        kernel.activate_resolved_harness(&resolved).unwrap();
        kernel.activate_all().unwrap();

        let input = serde_json::to_vec(&PhenixValue::from(&DebugCommand::Snapshot)).unwrap();
        let output = kernel
            .invoke(&debug_service(), &input, &authority, None)
            .unwrap();
        let output: PhenixValue = serde_json::from_slice(&output).unwrap();
        let DebugResponse::Snapshot { snapshot } = output.project().unwrap();
        let DiagnosticEntry::Available { value } = &snapshot.services["sessions"] else {
            panic!("sessions probe should be available");
        };
        assert!(matches!(
            SessionResponse::try_from(Project(value)).unwrap(),
            SessionResponse::Sessions { sessions } if sessions.is_empty()
        ));
        assert!(matches!(
            snapshot.services["context"],
            DiagnosticEntry::Unavailable { .. }
        ));
        assert!(matches!(
            snapshot.services["models"],
            DiagnosticEntry::Unavailable { .. }
        ));
    }
}
