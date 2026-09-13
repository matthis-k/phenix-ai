use crate::{memory_component_manifest, memory_factory, memory_manifest};
use phenix_core::{
    model_inference_service, Authority, Bytes, Kernel, KernelConfig, LocalPersistence, ModelId,
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
    memory_resolve_callable, memory_service, memory_validate_callable, MemoryCanonicalReference,
    MemoryCommand, MemoryFreshness, MemoryKind, MemoryRecallQuery, MemoryRecord, MemoryResponse,
    MemoryScope, MemorySourceReference,
};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

const FIXTURE_PROVIDER: &str = "fixture.memory-model";

fn temp_db(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "phenix-memory-freshness-{name}-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

fn kernel_with(path: &PathBuf) -> Kernel {
    let manifest = memory_manifest();
    let plugin = manifest.id.clone();
    let persistence = LocalPersistence::open(path).unwrap();
    let mut kernel = Kernel::with_persistence(KernelConfig::new([manifest]).unwrap(), persistence);
    kernel
        .register_embedded_factory(plugin, memory_factory)
        .unwrap();
    kernel.activate_all().unwrap();
    kernel
}

fn routed_kernel_with(path: &PathBuf) -> Kernel {
    let memory = memory_manifest();
    let routing = model_routing_manifest(Authority::default());
    let provider = fixture_provider_manifest();
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
        .register_embedded_factory(provider.id, || Box::new(RevalidationProvider))
        .unwrap();
    kernel.activate_all().unwrap();
    kernel
}

fn fixture_provider_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(FIXTURE_PROVIDER).unwrap(),
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

struct RevalidationProvider;

impl PluginInstance for RevalidationProvider {
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
            "validate-keep" | "resolve-keep" => b"\"keep_current\"".to_vec(),
            "validate-ambiguous" => b"\"needs_validation\"".to_vec(),
            model => return Err(format!("unexpected revalidation model: {model}")),
        };
        serde_json::to_vec(&PhenixValue::from(&ModelInferenceResponse {
            output: Bytes::new(output),
            provider_metadata: BTreeMap::new(),
            tool_calls: Vec::new(),
        }))
        .map_err(|error| error.to_string())
    }
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

fn configure_revalidation_routing(
    kernel: &mut Kernel,
    profile: &str,
    validate_model: &str,
    resolve_model: &str,
) -> phenix_core::RoutingProfileId {
    let provider = PluginId::parse(FIXTURE_PROVIDER).unwrap();
    let profile_id = phenix_core::RoutingProfileId::parse(profile).unwrap();
    let target = |model: &str| ModelTarget {
        provider_plugin: provider.clone(),
        model: ModelId::parse(model).unwrap(),
        options: BTreeMap::new(),
    };
    invoke_routing(
        kernel,
        RoutingCommand::RegisterProfile {
            profile: RoutingProfile {
                id: profile_id.clone(),
                default_target: target("unexpected-default"),
                fallback_targets: Vec::new(),
                callable_targets: BTreeMap::from([
                    (memory_validate_callable(), target(validate_model)),
                    (memory_resolve_callable(), target(resolve_model)),
                ]),
            },
        },
    )
    .unwrap();
    invoke_routing(
        kernel,
        RoutingCommand::SetProviderAuthenticated {
            provider_plugin: provider,
            authenticated: true,
        },
    )
    .unwrap();
    profile_id
}

fn scope() -> MemoryScope {
    MemoryScope::Session {
        session_id: SessionId::parse("root").unwrap(),
    }
}

fn record(id: &str, kind: MemoryKind, content: &str, created_at: u64) -> MemoryRecord {
    MemoryRecord {
        id: id.into(),
        kind,
        scope: scope(),
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

fn mark_needs_validation(kernel: &mut Kernel, id: &str, observed_at: u64) {
    invoke(
        kernel,
        MemoryCommand::ObserveRevision {
            service: ServiceId::parse("fixture.history@1").unwrap(),
            resource: format!("turn/{id}"),
            revision: "rev-2".into(),
            observed_at,
            limit: 10,
        },
    )
    .unwrap();
}

#[test]
fn revision_change_invalidates_only_dependent_current_memory() {
    let path = temp_db("revision");
    let mut kernel = kernel_with(&path);
    let changed = record("changed", MemoryKind::Fact, "shared changed", 10);
    let stable = record("stable", MemoryKind::Fact, "shared stable", 11);
    for record in [changed.clone(), stable.clone()] {
        invoke(&mut kernel, MemoryCommand::Record { record }).unwrap();
    }

    let response = invoke(
        &mut kernel,
        MemoryCommand::ObserveRevision {
            service: ServiceId::parse("fixture.history@1").unwrap(),
            resource: "turn/changed".into(),
            revision: "rev-2".into(),
            observed_at: 20,
            limit: 10,
        },
    )
    .unwrap();
    assert_eq!(
        response,
        MemoryResponse::Affected {
            memory_ids: vec![changed.id.clone()]
        }
    );

    let freshness = invoke(
        &mut kernel,
        MemoryCommand::GetFreshness {
            id: changed.id.clone(),
        },
    )
    .unwrap();
    assert!(matches!(
        freshness,
        MemoryResponse::Freshness { state: Some(state) }
            if state.freshness == MemoryFreshness::NeedsValidation && state.changed_at == 20
    ));

    let current = invoke(
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
    .unwrap();
    assert_eq!(
        current,
        MemoryResponse::Recall {
            records: vec![stable]
        }
    );

    let historical = invoke(
        &mut kernel,
        MemoryCommand::Recall {
            query: MemoryRecallQuery {
                scopes: vec![scope()],
                kinds: vec![MemoryKind::Fact],
                query: "changed".into(),
                at: 15,
                limit: 10,
            },
        },
    )
    .unwrap();
    assert_eq!(
        historical,
        MemoryResponse::Recall {
            records: vec![changed]
        }
    );
    let _ = fs::remove_file(path);
}

#[test]
fn conflicting_new_evidence_invalidates_only_the_bounded_affected_set() {
    let path = temp_db("conflict");
    let mut kernel = kernel_with(&path);
    let changed = record("changed", MemoryKind::Fact, "shared changed", 10);
    let stable = record("stable", MemoryKind::Fact, "shared stable", 11);
    for record in [changed.clone(), stable.clone()] {
        invoke(&mut kernel, MemoryCommand::Record { record }).unwrap();
    }

    let conflict_source = MemorySourceReference {
        service: ServiceId::parse("fixture.history@1").unwrap(),
        resource: "turn/conflicting-evidence".into(),
        start: None,
        end: None,
    };
    let response = invoke(
        &mut kernel,
        MemoryCommand::ObserveConflict {
            source: conflict_source.clone(),
            affected_ids: vec![changed.id.clone()],
            observed_at: 20,
        },
    )
    .unwrap();
    assert_eq!(
        response,
        MemoryResponse::Affected {
            memory_ids: vec![changed.id.clone()]
        }
    );

    let freshness = invoke(
        &mut kernel,
        MemoryCommand::GetFreshness {
            id: changed.id.clone(),
        },
    )
    .unwrap();
    assert!(matches!(
        freshness,
        MemoryResponse::Freshness { state: Some(state) }
            if state.freshness == MemoryFreshness::NeedsValidation
                && state.changed_at == 20
                && state.dependencies.iter().any(|dependency|
                    dependency.service == conflict_source.service
                        && dependency.resource == conflict_source.resource)
    ));

    let current = invoke(
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
    .unwrap();
    assert_eq!(
        current,
        MemoryResponse::Recall {
            records: vec![stable]
        }
    );

    assert_eq!(
        invoke(
            &mut kernel,
            MemoryCommand::ObserveRevision {
                service: conflict_source.service,
                resource: conflict_source.resource,
                revision: "rev-2".into(),
                observed_at: 30,
                limit: 10,
            },
        )
        .unwrap(),
        MemoryResponse::Affected {
            memory_ids: vec![changed.id]
        }
    );
    let _ = fs::remove_file(path);
}

#[test]
fn temporal_expiry_becomes_historical_without_losing_past_recall() {
    let path = temp_db("expiry");
    let mut kernel = kernel_with(&path);
    let mut expiring = record("expiring", MemoryKind::Fact, "temporary fact", 10);
    expiring.valid_until = Some(20);
    invoke(
        &mut kernel,
        MemoryCommand::Record {
            record: expiring.clone(),
        },
    )
    .unwrap();

    let current = invoke(
        &mut kernel,
        MemoryCommand::Recall {
            query: MemoryRecallQuery {
                scopes: vec![scope()],
                kinds: vec![MemoryKind::Fact],
                query: "temporary".into(),
                at: 20,
                limit: 10,
            },
        },
    )
    .unwrap();
    assert_eq!(
        current,
        MemoryResponse::Recall {
            records: Vec::new()
        }
    );

    let freshness = invoke(
        &mut kernel,
        MemoryCommand::GetFreshness {
            id: expiring.id.clone(),
        },
    )
    .unwrap();
    assert!(matches!(
        freshness,
        MemoryResponse::Freshness { state: Some(state) }
            if state.freshness == MemoryFreshness::Historical && state.changed_at == 20
    ));

    let historical = invoke(
        &mut kernel,
        MemoryCommand::Recall {
            query: MemoryRecallQuery {
                scopes: vec![scope()],
                kinds: vec![MemoryKind::Fact],
                query: "temporary".into(),
                at: 15,
                limit: 10,
            },
        },
    )
    .unwrap();
    assert_eq!(
        historical,
        MemoryResponse::Recall {
            records: vec![expiring]
        }
    );
    let _ = fs::remove_file(path);
}

#[test]
fn canonical_decision_revision_is_tracked_as_freshness_dependency() {
    let path = temp_db("canonical-decision");
    let mut kernel = kernel_with(&path);
    let decision = record(
        "decision-memory",
        MemoryKind::Decision,
        "Use canonical decision seven",
        10,
    );
    invoke(
        &mut kernel,
        MemoryCommand::Record {
            record: decision.clone(),
        },
    )
    .unwrap();

    let planning = ServiceId::parse("phenix.planning@1").unwrap();
    invoke(
        &mut kernel,
        MemoryCommand::BindCanonicalReference {
            id: decision.id.clone(),
            reference: MemoryCanonicalReference {
                service: planning.clone(),
                resource: "decision/7".into(),
                revision: Some("rev-1".into()),
            },
            observed_at: 11,
        },
    )
    .unwrap();

    let response = invoke(
        &mut kernel,
        MemoryCommand::ObserveRevision {
            service: planning,
            resource: "decision/7".into(),
            revision: "rev-2".into(),
            observed_at: 20,
            limit: 10,
        },
    )
    .unwrap();
    assert_eq!(
        response,
        MemoryResponse::Affected {
            memory_ids: vec![decision.id.clone()]
        }
    );

    let freshness = invoke(&mut kernel, MemoryCommand::GetFreshness { id: decision.id }).unwrap();
    assert!(matches!(
        freshness,
        MemoryResponse::Freshness { state: Some(state) }
            if state.freshness == MemoryFreshness::NeedsValidation
                && state.canonical_reference.is_some()
    ));
    let _ = fs::remove_file(path);
}

#[test]
fn non_decision_memory_cannot_claim_canonical_decision_authority() {
    let path = temp_db("canonical-guard");
    let mut kernel = kernel_with(&path);
    let fact = record("fact", MemoryKind::Fact, "not a decision", 10);
    invoke(
        &mut kernel,
        MemoryCommand::Record {
            record: fact.clone(),
        },
    )
    .unwrap();

    let error = invoke(
        &mut kernel,
        MemoryCommand::BindCanonicalReference {
            id: fact.id,
            reference: MemoryCanonicalReference {
                service: ServiceId::parse("phenix.planning@1").unwrap(),
                resource: "decision/7".into(),
                revision: Some("rev-1".into()),
            },
            observed_at: 11,
        },
    )
    .unwrap_err();
    assert!(error.contains("canonical references are only valid for decision memory"));
    let _ = fs::remove_file(path);
}

#[test]
fn semantic_revalidation_uses_the_validate_callable_without_resolve_when_decisive() {
    let path = temp_db("validate-route");
    let mut kernel = routed_kernel_with(&path);
    let profile = configure_revalidation_routing(
        &mut kernel,
        "validate-route",
        "validate-keep",
        "unexpected-resolve",
    );
    let fact = record("validate-me", MemoryKind::Fact, "route validation", 10);
    invoke(
        &mut kernel,
        MemoryCommand::Record {
            record: fact.clone(),
        },
    )
    .unwrap();
    mark_needs_validation(&mut kernel, &fact.id, 20);

    let response = invoke(
        &mut kernel,
        MemoryCommand::Revalidate {
            id: fact.id,
            profile_id: profile,
            at: 30,
        },
    )
    .unwrap();
    assert!(matches!(
        response,
        MemoryResponse::Freshness { state: Some(state) }
            if state.freshness == MemoryFreshness::Current && state.changed_at == 30
    ));
    let _ = fs::remove_file(path);
}

#[test]
fn ambiguous_validation_escalates_to_the_resolve_callable() {
    let path = temp_db("resolve-route");
    let mut kernel = routed_kernel_with(&path);
    let profile = configure_revalidation_routing(
        &mut kernel,
        "resolve-route",
        "validate-ambiguous",
        "resolve-keep",
    );
    let fact = record("resolve-me", MemoryKind::Fact, "ambiguous validation", 10);
    invoke(
        &mut kernel,
        MemoryCommand::Record {
            record: fact.clone(),
        },
    )
    .unwrap();
    mark_needs_validation(&mut kernel, &fact.id, 20);

    let response = invoke(
        &mut kernel,
        MemoryCommand::Revalidate {
            id: fact.id,
            profile_id: profile,
            at: 30,
        },
    )
    .unwrap();
    assert!(matches!(
        response,
        MemoryResponse::Freshness { state: Some(state) }
            if state.freshness == MemoryFreshness::Current && state.changed_at == 30
    ));
    let _ = fs::remove_file(path);
}

#[test]
fn deterministic_expiry_revalidation_does_not_invoke_a_model() {
    let path = temp_db("deterministic-revalidate");
    let mut kernel = routed_kernel_with(&path);
    let profile = configure_revalidation_routing(
        &mut kernel,
        "deterministic-route",
        "unexpected-validate",
        "unexpected-resolve",
    );
    let mut fact = record("expired", MemoryKind::Fact, "expired fact", 10);
    fact.valid_until = Some(20);
    invoke(
        &mut kernel,
        MemoryCommand::Record {
            record: fact.clone(),
        },
    )
    .unwrap();

    let response = invoke(
        &mut kernel,
        MemoryCommand::Revalidate {
            id: fact.id,
            profile_id: profile,
            at: 20,
        },
    )
    .unwrap();
    assert!(matches!(
        response,
        MemoryResponse::Freshness { state: Some(state) }
            if state.freshness == MemoryFreshness::Historical && state.changed_at == 20
    ));
    let _ = fs::remove_file(path);
}
