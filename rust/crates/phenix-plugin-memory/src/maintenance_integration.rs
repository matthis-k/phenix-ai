use crate::{memory_component_manifest, memory_factory, memory_manifest};
use phenix_core::{
    Authority, Bytes, ComponentExport, ComponentId, ComponentInterface, ComponentManifest, Kernel,
    LocalPersistence, PhenixValue, PluginExecution, PluginHost, PluginId, PluginInstance,
    PluginManifest, ResolvedHarness, ResolvedHarnessActivation, RoutingProfileId,
    ServiceContribution, ServiceId, ServiceRole, SessionId,
};
use phenix_sdk::{
    helper_invocation_service, memory_service, HelperInvocationCommand, HelperInvocationInterface,
    HelperInvocationResponse, MemoryCommand, MemoryConsolidationRequest,
    MemoryExtractionObservation, MemoryExtractionRequest, MemoryKind, MemoryRecallQuery,
    MemoryRecord, MemoryResponse, MemoryScope, MemorySourceReference,
};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

const HELPER_PROVIDER: &str = "fixture.memory-helper";
const HELPER_COMPONENT: &str = "fixture.memory-helper";
const EXECUTION: &str = "execution-1";
const PARENT_ATTEMPT: &str = "attempt-1";

fn temp_db(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "phenix-memory-maintenance-{name}-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

fn helper_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(HELPER_PROVIDER).unwrap(),
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
        owner: PluginId::parse(HELPER_PROVIDER).unwrap(),
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

struct HelperProvider;

impl PluginInstance for HelperProvider {
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
        let command: HelperInvocationCommand =
            input.project().map_err(|error| error.to_string())?;
        let HelperInvocationCommand::Invoke { request } = command;
        if request.execution_id != EXECUTION || request.parent_attempt_id != PARENT_ATTEMPT {
            return Err("memory helper lost execution lineage".into());
        }
        let output = match (request.profile_id.as_str(), request.callable_id.as_str()) {
            ("extract-profile", "memory.extract") => "extracted durable fact",
            ("consolidate-profile", "memory.consolidate") => "consolidated durable fact",
            ("failure-profile", "memory.consolidate") => {
                return Err("fixture maintenance failure".into())
            }
            (profile, callable) => {
                return Err(format!(
                    "unexpected memory helper route: {profile}/{callable}"
                ))
            }
        };
        serde_json::to_vec(&PhenixValue::from(&HelperInvocationResponse {
            attempt_id: "fixture-helper-attempt".into(),
            output: Bytes::new(output.as_bytes().to_vec()),
            tool_calls: Vec::new(),
        }))
        .map_err(|error| error.to_string())
    }
}

fn kernel_with(path: &PathBuf) -> Kernel {
    let memory = memory_manifest();
    let helper = helper_manifest();
    let authority = memory.maximum_authority.clone();
    let resolved = ResolvedHarness::resolve(
        [memory.clone(), helper.clone()],
        [memory_component_manifest(), helper_component_manifest()],
        [],
        &authority,
    )
    .unwrap();
    let persistence = LocalPersistence::open(path).unwrap();
    let mut kernel = Kernel::with_persistence(resolved.kernel_config().clone(), persistence);
    kernel.activate_resolved_harness(&resolved).unwrap();
    kernel
        .register_embedded_factory(memory.id, memory_factory)
        .unwrap();
    kernel
        .register_embedded_factory(helper.id, || Box::new(HelperProvider))
        .unwrap();
    kernel.activate_all().unwrap();
    kernel
}

fn invoke(kernel: &mut Kernel, command: MemoryCommand) -> Result<MemoryResponse, String> {
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

fn profile(name: &str) -> RoutingProfileId {
    RoutingProfileId::parse(name).unwrap()
}

fn scope() -> MemoryScope {
    MemoryScope::Session {
        session_id: SessionId::parse("root").unwrap(),
    }
}

fn source(resource: &str) -> MemorySourceReference {
    MemorySourceReference {
        service: ServiceId::parse("fixture.history@1").unwrap(),
        resource: resource.into(),
        start: None,
        end: None,
    }
}

fn fact(id: &str, content: &str, resource: &str, created_at: u64) -> MemoryRecord {
    MemoryRecord {
        id: id.into(),
        kind: MemoryKind::Fact,
        scope: scope(),
        content: content.into(),
        source_refs: vec![source(resource)],
        supporting_dependencies: Vec::new(),
        supersedes: Vec::new(),
        valid_from: None,
        valid_until: None,
        created_at,
    }
}

#[test]
fn extraction_uses_helper_profile_and_keeps_caller_owned_exact_provenance() {
    let path = temp_db("extract");
    let mut kernel = kernel_with(&path);
    let expected_source = source("history/42");
    let response = invoke(
        &mut kernel,
        MemoryCommand::Extract {
            request: MemoryExtractionRequest {
                execution_id: EXECUTION.into(),
                parent_attempt_id: PARENT_ATTEMPT.into(),
                profile_id: profile("extract-profile"),
                id: "extracted".into(),
                kind: MemoryKind::Fact,
                scope: scope(),
                observations: vec![MemoryExtractionObservation {
                    content: "raw retained observation".into(),
                    source_refs: vec![expected_source.clone()],
                    supporting_dependencies: Vec::new(),
                }],
                created_at: 10,
            },
        },
    )
    .unwrap();
    let MemoryResponse::Record { record } = response else {
        panic!("extraction must create a durable record");
    };
    assert_eq!(record.content, "extracted durable fact");
    assert_eq!(record.source_refs, vec![expected_source]);
    let _ = fs::remove_file(path);
}

#[test]
fn consolidation_unions_provenance_and_supersedes_inputs() {
    let path = temp_db("consolidate");
    let mut kernel = kernel_with(&path);
    for record in [
        fact("a", "fact A", "history/a", 10),
        fact("b", "fact B", "history/b", 11),
    ] {
        invoke(&mut kernel, MemoryCommand::Record { record }).unwrap();
    }
    let response = invoke(
        &mut kernel,
        MemoryCommand::Consolidate {
            request: MemoryConsolidationRequest {
                execution_id: EXECUTION.into(),
                parent_attempt_id: PARENT_ATTEMPT.into(),
                profile_id: profile("consolidate-profile"),
                ids: vec!["a".into(), "b".into()],
                consolidated_id: "ab".into(),
                created_at: 20,
            },
        },
    )
    .unwrap();
    let MemoryResponse::Record { record } = response else {
        panic!("consolidation must create a durable record");
    };
    assert_eq!(record.content, "consolidated durable fact");
    assert_eq!(record.supersedes, vec!["a", "b"]);
    assert_eq!(
        record.source_refs,
        vec![source("history/a"), source("history/b")]
    );

    let current = invoke(
        &mut kernel,
        MemoryCommand::Recall {
            query: MemoryRecallQuery {
                scopes: vec![scope()],
                kinds: vec![MemoryKind::Fact],
                query: "durable".into(),
                at: 25,
                limit: 10,
            },
        },
    )
    .unwrap();
    assert_eq!(
        current,
        MemoryResponse::Recall {
            records: vec![record]
        }
    );
    let _ = fs::remove_file(path);
}

#[test]
fn failed_consolidation_does_not_mutate_existing_memory() {
    let path = temp_db("failure");
    let mut kernel = kernel_with(&path);
    for record in [
        fact("a", "shared fact A", "history/a", 10),
        fact("b", "shared fact B", "history/b", 11),
    ] {
        invoke(&mut kernel, MemoryCommand::Record { record }).unwrap();
    }
    assert!(invoke(
        &mut kernel,
        MemoryCommand::Consolidate {
            request: MemoryConsolidationRequest {
                execution_id: EXECUTION.into(),
                parent_attempt_id: PARENT_ATTEMPT.into(),
                profile_id: profile("failure-profile"),
                ids: vec!["a".into(), "b".into()],
                consolidated_id: "ab".into(),
                created_at: 20,
            },
        },
    )
    .is_err());
    assert_eq!(
        invoke(
            &mut kernel,
            MemoryCommand::Recall {
                query: MemoryRecallQuery {
                    scopes: vec![scope()],
                    kinds: vec![MemoryKind::Fact],
                    query: "shared".into(),
                    at: 25,
                    limit: 10,
                },
            },
        )
        .unwrap(),
        MemoryResponse::Recall {
            records: vec![
                fact("b", "shared fact B", "history/b", 11),
                fact("a", "shared fact A", "history/a", 10),
            ],
        }
    );
    let _ = fs::remove_file(path);
}
