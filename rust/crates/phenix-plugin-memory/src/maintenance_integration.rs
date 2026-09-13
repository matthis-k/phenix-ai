use crate::{memory_component_manifest, memory_factory, memory_manifest};
use phenix_core::{
    model_inference_service, Authority, Bytes, Kernel, LocalPersistence, ModelId,
    ModelInferenceRequest, ModelInferenceResponse, PhenixValue, PluginExecution, PluginHost,
    PluginId, PluginInstance, PluginManifest, ResolvedHarness, ResolvedHarnessActivation,
    ServiceContribution, ServiceId, ServiceRole, SessionId,
};
use phenix_plugin_models::{
    model_routing_component_manifest, model_routing_factory, model_routing_manifest,
    model_routing_service, ModelCommand as RoutingCommand, ModelResponse as RoutingResponse,
    ModelTarget, RoutingProfile,
};
use phenix_sdk::{
    memory_consolidate_callable, memory_extract_callable, memory_service, MemoryCommand,
    MemoryConsolidationRequest, MemoryExtractionObservation, MemoryExtractionRequest, MemoryKind,
    MemoryRecallQuery, MemoryRecord, MemoryResponse, MemoryScope, MemorySourceReference,
};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

const PROVIDER: &str = "fixture.memory-maintenance";

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

fn provider_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(PROVIDER).unwrap(),
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

struct Provider;

impl PluginInstance for Provider {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &model_inference_service() {
            return Err(format!("unsupported fixture service: {service}"));
        }
        let input: PhenixValue =
            serde_json::from_slice(input).map_err(|error| error.to_string())?;
        let request: ModelInferenceRequest = input.project().map_err(|error| error.to_string())?;
        let output = match request.model.as_str() {
            "extract" => "extracted durable fact",
            "consolidate" => "consolidated durable fact",
            "fail" => return Err("fixture maintenance failure".into()),
            model => return Err(format!("unexpected maintenance model: {model}")),
        };
        serde_json::to_vec(&PhenixValue::from(&ModelInferenceResponse {
            output: Bytes::new(output.as_bytes().to_vec()),
            provider_metadata: BTreeMap::new(),
            tool_calls: Vec::new(),
        }))
        .map_err(|error| error.to_string())
    }
}

fn kernel_with(path: &PathBuf) -> Kernel {
    let memory = memory_manifest();
    let routing = model_routing_manifest(Authority::default());
    let provider = provider_manifest();
    let authority = memory.maximum_authority.clone();
    let resolved = ResolvedHarness::resolve(
        [memory.clone(), routing.clone(), provider.clone()],
        [
            memory_component_manifest(),
            model_routing_component_manifest(Authority::default()),
        ],
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
        .register_embedded_factory(routing.id, model_routing_factory)
        .unwrap();
    kernel
        .register_embedded_factory(provider.id, || Box::new(Provider))
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

fn routing(
    kernel: &mut Kernel,
    name: &str,
    extract: &str,
    consolidate: &str,
) -> phenix_core::RoutingProfileId {
    let profile_id = phenix_core::RoutingProfileId::parse(name).unwrap();
    let provider = PluginId::parse(PROVIDER).unwrap();
    let target = |model: &str| ModelTarget {
        provider_plugin: provider.clone(),
        model: ModelId::parse(model).unwrap(),
        options: BTreeMap::new(),
    };
    let profile = RoutingProfile {
        id: profile_id.clone(),
        default_target: target("fail"),
        fallback_targets: Vec::new(),
        callable_targets: BTreeMap::from([
            (memory_extract_callable(), target(extract)),
            (memory_consolidate_callable(), target(consolidate)),
        ]),
    };
    let input = serde_json::to_vec(&PhenixValue::from(&RoutingCommand::RegisterProfile {
        profile,
    }))
    .unwrap();
    let output = kernel
        .invoke(
            &model_routing_service(),
            &input,
            &model_routing_manifest(Authority::default()).maximum_authority,
            None,
        )
        .unwrap();
    let value: PhenixValue = serde_json::from_slice(&output).unwrap();
    let _: RoutingResponse = value.project().unwrap();
    let command = RoutingCommand::SetProviderAuthenticated {
        provider_plugin: provider,
        authenticated: true,
    };
    let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
    kernel
        .invoke(
            &model_routing_service(),
            &input,
            &model_routing_manifest(Authority::default()).maximum_authority,
            None,
        )
        .unwrap();
    profile_id
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
        supersedes: Vec::new(),
        valid_from: None,
        valid_until: None,
        created_at,
    }
}

#[test]
fn extraction_uses_routed_model_but_keeps_caller_owned_exact_provenance() {
    let path = temp_db("extract");
    let mut kernel = kernel_with(&path);
    let profile_id = routing(&mut kernel, "extract-profile", "extract", "consolidate");
    let expected_source = source("history/42");
    let response = invoke(
        &mut kernel,
        MemoryCommand::Extract {
            request: MemoryExtractionRequest {
                profile_id,
                id: "extracted".into(),
                kind: MemoryKind::Fact,
                scope: scope(),
                observations: vec![MemoryExtractionObservation {
                    content: "raw retained observation".into(),
                    source_refs: vec![expected_source.clone()],
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
    let profile_id = routing(&mut kernel, "consolidate-profile", "extract", "consolidate");
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
                profile_id,
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
    let profile_id = routing(&mut kernel, "failure-profile", "extract", "fail");
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
                profile_id,
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
