use crate::{memory_component_manifest, memory_factory, memory_manifest};
use phenix_core::{
    Authority, ComponentExport, ComponentId, ComponentInterface, ComponentManifest, Kernel,
    LocalPersistence, PhenixValue, PluginExecution, PluginHost, PluginId, PluginInstance,
    PluginManifest, ResolvedHarness, ResolvedHarnessActivation, ServiceContribution, ServiceId,
    ServiceRole, SessionId,
};
use phenix_plugin_models::{
    model_routing_component_manifest, model_routing_factory, model_routing_manifest,
};
use phenix_sdk::{
    helper_invocation_service, memory_embedding_service, memory_service, HelperInvocationInterface,
    MemoryCommand, MemoryEmbeddingInterface, MemoryEmbeddingRequest, MemoryEmbeddingResponse,
    MemoryKind, MemoryRecallQuery, MemoryRecord, MemoryResponse, MemoryScope,
    MemorySourceReference,
};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

const EMBED_PLUGIN: &str = "fixture.memory-embed";

fn temp_db(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "phenix-memory-embed-{name}-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

fn embed_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(EMBED_PLUGIN).unwrap(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![
            ServiceContribution {
                role: ServiceRole::Terminal,
                service: memory_embedding_service(),
                priority: 200,
                required_authority: Authority::default(),
            },
            ServiceContribution {
                role: ServiceRole::Terminal,
                service: helper_invocation_service(),
                priority: 200,
                required_authority: Authority::default(),
            },
        ],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn embed_component_manifest() -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: ComponentId::parse(EMBED_PLUGIN).unwrap(),
        owner: PluginId::parse(EMBED_PLUGIN).unwrap(),
        imports: Vec::new(),
        exports: vec![
            ComponentExport {
                interface: MemoryEmbeddingInterface::interface_id(),
                schema: MemoryEmbeddingInterface::schema(),
                priority: 200,
                required_authority: Authority::default(),
            },
            ComponentExport {
                interface: HelperInvocationInterface::interface_id(),
                schema: HelperInvocationInterface::schema(),
                priority: 200,
                required_authority: Authority::default(),
            },
        ],
        maximum_authority: Authority::default(),
    }
}

struct EmbedProvider;

impl PluginInstance for EmbedProvider {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &memory_embedding_service() {
            return Err(format!("unsupported fixture service: {service}"));
        }
        let value: PhenixValue =
            serde_json::from_slice(input).map_err(|error| error.to_string())?;
        let request: MemoryEmbeddingRequest = value.project().map_err(|error| error.to_string())?;
        if request
            .inputs
            .first()
            .is_some_and(|input| input == "provider-error")
        {
            return Err("fixture embedding failed".into());
        }
        let embeddings = request
            .inputs
            .into_iter()
            .map(|input| {
                if input.contains("canine") || input.contains("dog") {
                    vec![1.0, 0.0]
                } else {
                    vec![0.0, 1.0]
                }
            })
            .collect();
        serde_json::to_vec(&PhenixValue::from(&MemoryEmbeddingResponse { embeddings }))
            .map_err(|error| error.to_string())
    }
}

fn kernel_with_embed(path: &PathBuf) -> Kernel {
    let memory = memory_manifest();
    let routing = model_routing_manifest(Authority::default());
    let embed = embed_manifest();
    let authority = memory.maximum_authority.clone();
    let resolved = ResolvedHarness::resolve(
        [memory.clone(), routing.clone(), embed.clone()],
        [
            memory_component_manifest(),
            model_routing_component_manifest(Authority::default()),
            embed_component_manifest(),
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
        .register_embedded_factory(embed.id, || Box::new(EmbedProvider))
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

fn record(id: &str, content: &str, created_at: u64) -> MemoryRecord {
    MemoryRecord {
        id: id.into(),
        kind: MemoryKind::Fact,
        scope: MemoryScope::Session {
            session_id: SessionId::parse("root").unwrap(),
        },
        content: content.into(),
        source_refs: vec![MemorySourceReference {
            service: ServiceId::parse("fixture.history@1").unwrap(),
            resource: format!("turn/{id}"),
            start: None,
            end: None,
        }],
        supersedes: Vec::new(),
        valid_from: None,
        valid_until: None,
        created_at,
    }
}

fn recall(kernel: &mut Kernel, query: &str) -> Vec<String> {
    let response = invoke(
        kernel,
        MemoryCommand::Recall {
            query: MemoryRecallQuery {
                scopes: vec![MemoryScope::Session {
                    session_id: SessionId::parse("root").unwrap(),
                }],
                kinds: vec![MemoryKind::Fact],
                query: query.into(),
                at: 30,
                limit: 10,
            },
        },
    )
    .unwrap();
    let MemoryResponse::Recall { records } = response else {
        panic!("recall must return records");
    };
    records.into_iter().map(|record| record.id).collect()
}

#[test]
fn optional_embedding_provider_adds_semantic_candidates_without_lexical_overlap() {
    let path = temp_db("semantic");
    let mut kernel = kernel_with_embed(&path);
    for memory in [
        record("database", "sqlite durable storage", 20),
        record("pet", "friendly dog companion", 10),
    ] {
        invoke(&mut kernel, MemoryCommand::Record { record: memory }).unwrap();
    }

    assert_eq!(recall(&mut kernel, "canine"), vec!["pet", "database"]);
    let _ = fs::remove_file(path);
}

#[test]
fn embedding_failure_keeps_exact_lexical_recall_available() {
    let path = temp_db("fallback");
    let mut kernel = kernel_with_embed(&path);
    invoke(
        &mut kernel,
        MemoryCommand::Record {
            record: record("fallback", "provider-error remains searchable", 10),
        },
    )
    .unwrap();

    assert_eq!(recall(&mut kernel, "provider-error"), vec!["fallback"]);
    let _ = fs::remove_file(path);
}
