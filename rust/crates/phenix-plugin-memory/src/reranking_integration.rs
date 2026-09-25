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
    helper_invocation_service, memory_rank_service, memory_service, HelperInvocationInterface,
    MemoryCommand, MemoryKind, MemoryRankInterface, MemoryRankRequest, MemoryRankResponse,
    MemoryRecallQuery, MemoryRecord, MemoryResponse, MemoryScope, MemorySourceReference,
};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

const RANK_PLUGIN: &str = "fixture.memory-rank";

fn temp_db(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "phenix-memory-rank-{name}-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

fn rank_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(RANK_PLUGIN).unwrap(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![
            ServiceContribution {
                role: ServiceRole::Terminal,
                service: memory_rank_service(),
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

fn rank_component_manifest() -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: ComponentId::parse(RANK_PLUGIN).unwrap(),
        owner: PluginId::parse(RANK_PLUGIN).unwrap(),
        imports: Vec::new(),
        exports: vec![
            ComponentExport {
                interface: MemoryRankInterface::interface_id(),
                schema: MemoryRankInterface::schema(),
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

struct RankProvider;

impl PluginInstance for RankProvider {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        _host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &memory_rank_service() {
            return Err(format!("unsupported fixture service: {service}"));
        }
        let value: PhenixValue =
            serde_json::from_slice(input).map_err(|error| error.to_string())?;
        let request: MemoryRankRequest = value.project().map_err(|error| error.to_string())?;
        if request.query == "provider-error" {
            return Err("fixture reranker failed".into());
        }

        let mut ids = request
            .candidates
            .into_iter()
            .map(|candidate| candidate.id)
            .collect::<Vec<_>>();
        ids.reverse();
        ids.truncate(request.limit as usize);
        serde_json::to_vec(&PhenixValue::from(&MemoryRankResponse { ids }))
            .map_err(|error| error.to_string())
    }
}

fn kernel_with_rank(path: &PathBuf) -> Kernel {
    let memory = memory_manifest();
    let routing = model_routing_manifest(Authority::default());
    let rank = rank_manifest();
    let authority = memory.maximum_authority.clone();
    let resolved = ResolvedHarness::resolve(
        [memory.clone(), routing.clone(), rank.clone()],
        [
            memory_component_manifest(),
            model_routing_component_manifest(Authority::default()),
            rank_component_manifest(),
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
        .register_embedded_factory(rank.id, || Box::new(RankProvider))
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
        supporting_dependencies: Vec::new(),
        supersedes: Vec::new(),
        valid_from: None,
        valid_until: None,
        created_at,
    }
}

fn record_pair(kernel: &mut Kernel, query: &str) {
    for record in [
        record("older", &format!("{query} shared"), 10),
        record("newer", &format!("{query} shared"), 20),
    ] {
        invoke(kernel, MemoryCommand::Record { record }).unwrap();
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
fn optional_rank_provider_can_reorder_lexical_candidates() {
    let path = temp_db("reorder");
    let mut kernel = kernel_with_rank(&path);
    record_pair(&mut kernel, "transport");

    assert_eq!(recall(&mut kernel, "transport"), vec!["older", "newer"]);
    let _ = fs::remove_file(path);
}

#[test]
fn rank_provider_failure_falls_back_to_deterministic_lexical_order() {
    let path = temp_db("fallback");
    let mut kernel = kernel_with_rank(&path);
    record_pair(&mut kernel, "provider-error");

    assert_eq!(
        recall(&mut kernel, "provider-error"),
        vec!["newer", "older"]
    );
    let _ = fs::remove_file(path);
}
