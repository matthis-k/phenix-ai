use crate::{memory_component_manifest, memory_factory, memory_manifest};
use phenix_core::{
    Authority, Bytes, ComponentExport, ComponentId, ComponentInterface, ComponentManifest, Kernel,
    KernelConfig, LocalPersistence, PhenixValue, PluginExecution, PluginHost, PluginId,
    PluginInstance, PluginManifest, ResolvedHarness, ResolvedHarnessActivation, RoutingProfileId,
    ServiceContribution, ServiceId, ServiceRole, SessionId,
};
use phenix_plugin_language::{
    language_component_manifest, language_factory, language_manifest, language_service,
};
use phenix_sdk::{
    helper_invocation_service, memory_resolve_callable, memory_service, memory_validate_callable,
    CodeEntityFacet, CodeEntityFacetRevisions, CodeEntityRevision, DocumentProvenance,
    HelperInvocationCommand, HelperInvocationInterface, HelperInvocationResponse, LanguageCommand,
    LanguageDocumentIdentity, LanguageResponse, LogicalCodeEntity, MemoryCanonicalReference,
    MemoryCommand, MemoryDependencyRevision, MemoryFreshness, MemoryKind, MemoryRecallQuery,
    MemoryRecord, MemoryResponse, MemoryRevisionCursor, MemoryScope, MemorySourceReference,
    ProviderEpoch,
};
use std::{
    collections::BTreeMap,
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
        .register_embedded_factory(helper.id, || Box::new(RevalidationProvider))
        .unwrap();
    kernel.activate_all().unwrap();
    kernel
}

fn code_kernel_with(path: &PathBuf) -> Kernel {
    let memory = memory_manifest();
    let helper = helper_manifest();
    let language = language_manifest();
    let authority = memory.maximum_authority.clone();
    let resolved = ResolvedHarness::resolve(
        [memory.clone(), helper.clone(), language.clone()],
        [
            memory_component_manifest(),
            helper_component_manifest(),
            language_component_manifest(),
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
        .register_embedded_factory(helper.id, || Box::new(RevalidationProvider))
        .unwrap();
    kernel
        .register_embedded_factory(language.id, language_factory)
        .unwrap();
    kernel.activate_all().unwrap();
    kernel
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
        if service != &helper_invocation_service() {
            return Err(format!("unsupported fixture service: {service}"));
        }
        let input: PhenixValue =
            serde_json::from_slice(input).map_err(|error| error.to_string())?;
        let request: HelperInvocationCommand =
            input.project().map_err(|error| error.to_string())?;
        let HelperInvocationCommand::Invoke { request } = request;
        if request.execution_id != EXECUTION || request.parent_attempt_id != PARENT_ATTEMPT {
            return Err("memory revalidation lost execution lineage".into());
        }
        let output = match (request.profile_id.as_str(), request.callable_id.as_str()) {
            ("validate-route", callable) if callable == memory_validate_callable().as_str() => {
                b"\"keep_current\"".to_vec()
            }
            ("resolve-route", callable) if callable == memory_validate_callable().as_str() => {
                b"\"needs_validation\"".to_vec()
            }
            ("resolve-route", callable) if callable == memory_resolve_callable().as_str() => {
                b"\"keep_current\"".to_vec()
            }
            ("deterministic-route", callable) => {
                return Err(format!("deterministic revalidation invoked {callable}"))
            }
            (profile, callable) => {
                return Err(format!(
                    "unexpected revalidation helper: {profile}/{callable}"
                ))
            }
        };
        serde_json::to_vec(&PhenixValue::from(&HelperInvocationResponse {
            attempt_id: "fixture-helper-attempt".into(),
            output: Bytes::new(output),
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

fn invoke_language(
    kernel: &mut Kernel,
    command: LanguageCommand,
) -> Result<LanguageResponse, String> {
    let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
    let output = kernel
        .invoke(
            &language_service(),
            &input,
            &language_manifest().maximum_authority,
            None,
        )
        .map_err(|error| error.to_string())?;
    let output: PhenixValue = serde_json::from_slice(&output).map_err(|error| error.to_string())?;
    output.project().map_err(|error| error.to_string())
}

fn configure_revalidation_routing(
    _kernel: &mut Kernel,
    profile: &str,
    _validate_model: &str,
    _resolve_model: &str,
) -> RoutingProfileId {
    RoutingProfileId::parse(profile).unwrap()
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
        supporting_dependencies: Vec::new(),
        supersedes: Vec::new(),
        valid_from: None,
        valid_until: None,
        created_at,
    }
}

fn code_revision(
    sequence: u64,
    path: &str,
    name: &str,
    name_location_revision: &str,
    body_revision: Option<&str>,
) -> CodeEntityRevision {
    CodeEntityRevision {
        entity: LogicalCodeEntity {
            id: "entity-1".into(),
            repository_id: "repo-1".into(),
        },
        revision: format!("entity-revision-{sequence}"),
        sequence,
        document: LanguageDocumentIdentity {
            path: path.into(),
            file_version: Some(format!("sha256:file-{sequence}")),
            provenance: DocumentProvenance::WorkspaceBacked,
        },
        symbol: Some(format!("crate::{name}")),
        name: name.into(),
        signature_identity: Some("signature-stable".into()),
        body_identity: body_revision.map(str::to_owned),
        provider_id: "fixture-analyzer".into(),
        provider_epoch: ProviderEpoch::new(1).unwrap(),
        facets: CodeEntityFacetRevisions {
            existence: "existence-stable".into(),
            name_location: name_location_revision.into(),
            signature: Some("signature-stable".into()),
            body: body_revision.map(str::to_owned),
            relations: BTreeMap::from([("callers".into(), "callers-stable".into())]),
        },
    }
}

fn code_dependency(
    revision: &CodeEntityRevision,
    facet: CodeEntityFacet,
) -> MemoryDependencyRevision {
    MemoryDependencyRevision::for_code_facet(
        &revision
            .facet_reference(facet)
            .expect("fixture facet must exist"),
    )
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
fn revision_observation_pages_advance_over_the_dependency_index() {
    let path = temp_db("revision-pages");
    let mut kernel = kernel_with(&path);
    let revision = code_revision(1, "src/lib.rs", "run", "name-1", Some("body-1"));
    let dependency = code_dependency(&revision, CodeEntityFacet::Body);
    let service = dependency.service.clone();
    let resource = dependency.resource.clone();

    for id in ["memory-a", "memory-b", "memory-c"] {
        let mut memory = record(id, MemoryKind::Fact, id, 10);
        memory.supporting_dependencies.push(dependency.clone());
        invoke(&mut kernel, MemoryCommand::Record { record: memory }).unwrap();
    }

    let mut cursor: Option<MemoryRevisionCursor> = None;
    let mut affected = Vec::new();
    loop {
        let response = invoke(
            &mut kernel,
            MemoryCommand::ObserveRevisionPage {
                service: service.clone(),
                resource: resource.clone(),
                revision: "body-2".into(),
                observed_at: 20,
                limit: 1,
                cursor,
            },
        )
        .unwrap();
        let MemoryResponse::AffectedPage {
            memory_ids,
            next_cursor,
        } = response
        else {
            panic!("expected affected page");
        };
        affected.extend(memory_ids);
        cursor = next_cursor;
        if cursor.is_none() {
            break;
        }
    }

    assert_eq!(affected, vec!["memory-a", "memory-b", "memory-c"]);
    let _ = fs::remove_file(path);
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
            execution_id: EXECUTION.into(),
            parent_attempt_id: PARENT_ATTEMPT.into(),
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
fn model_revalidation_cannot_make_changed_exact_support_current() {
    let path = temp_db("stale-exact-support");
    let mut kernel = routed_kernel_with(&path);
    let profile = configure_revalidation_routing(
        &mut kernel,
        "stale-support-route",
        "validate-keep",
        "unexpected-resolve",
    );
    let revision = code_revision(1, "src/lib.rs", "run", "name-1", Some("body-1"));
    let dependency = code_dependency(&revision, CodeEntityFacet::Body);
    let service = dependency.service.clone();
    let resource = dependency.resource.clone();
    let mut fact = record("stale-support", MemoryKind::Fact, "code claim", 10);
    fact.supporting_dependencies.push(dependency);
    invoke(
        &mut kernel,
        MemoryCommand::Record {
            record: fact.clone(),
        },
    )
    .unwrap();
    invoke(
        &mut kernel,
        MemoryCommand::ObserveRevision {
            service,
            resource,
            revision: "body-2".into(),
            observed_at: 20,
            limit: 10,
        },
    )
    .unwrap();

    let response = invoke(
        &mut kernel,
        MemoryCommand::Revalidate {
            id: fact.id,
            execution_id: EXECUTION.into(),
            parent_attempt_id: PARENT_ATTEMPT.into(),
            profile_id: profile,
            at: 30,
        },
    )
    .unwrap();
    assert!(matches!(
        response,
        MemoryResponse::Freshness { state: Some(state) }
            if state.freshness == MemoryFreshness::NeedsValidation && state.changed_at == 30
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
            execution_id: EXECUTION.into(),
            parent_attempt_id: PARENT_ATTEMPT.into(),
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
            execution_id: EXECUTION.into(),
            parent_attempt_id: PARENT_ATTEMPT.into(),
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

#[test]
fn verified_move_preserves_body_claim_and_invalidates_name_location_claim() {
    let path = temp_db("code-facet-move");
    let mut kernel = code_kernel_with(&path);
    let first = code_revision(
        1,
        "src/old.rs",
        "old_name",
        "name-location-1",
        Some("body-stable"),
    );
    invoke_language(
        &mut kernel,
        LanguageCommand::RecordEntityRevision {
            revision: first.clone(),
        },
    )
    .unwrap();

    let mut body = record("body-claim", MemoryKind::Fact, "body claim", 10);
    body.supporting_dependencies
        .push(code_dependency(&first, CodeEntityFacet::Body));
    let mut location = record("location-claim", MemoryKind::Fact, "location claim", 10);
    location
        .supporting_dependencies
        .push(code_dependency(&first, CodeEntityFacet::NameLocation));
    for memory in [body.clone(), location.clone()] {
        invoke(&mut kernel, MemoryCommand::Record { record: memory }).unwrap();
    }

    for id in [&body.id, &location.id] {
        assert!(matches!(
            invoke(&mut kernel, MemoryCommand::GetFreshness { id: id.clone() }).unwrap(),
            MemoryResponse::Freshness { state: Some(state) }
                if state.freshness == MemoryFreshness::Current
        ));
    }

    let second = code_revision(
        2,
        "src/new.rs",
        "new_name",
        "name-location-2",
        Some("body-stable"),
    );
    invoke_language(
        &mut kernel,
        LanguageCommand::RecordEntityRevision { revision: second },
    )
    .unwrap();

    let recalled = invoke(
        &mut kernel,
        MemoryCommand::Recall {
            query: MemoryRecallQuery {
                scopes: vec![scope()],
                kinds: vec![MemoryKind::Fact],
                query: "claim".into(),
                at: 20,
                limit: 10,
            },
        },
    )
    .unwrap();
    assert_eq!(
        recalled,
        MemoryResponse::Recall {
            records: vec![body.clone()]
        }
    );
    assert!(matches!(
        invoke(
            &mut kernel,
            MemoryCommand::GetFreshness {
                id: location.id.clone(),
            },
        )
        .unwrap(),
        MemoryResponse::Freshness { state: Some(state) }
            if state.freshness == MemoryFreshness::NeedsValidation
    ));
    assert!(matches!(
        invoke(
            &mut kernel,
            MemoryCommand::GetFreshness { id: body.id },
        )
        .unwrap(),
        MemoryResponse::Freshness { state: Some(state) }
            if state.freshness == MemoryFreshness::Current
    ));
    let _ = fs::remove_file(path);
}

#[test]
fn unavailable_code_owner_never_admits_exact_code_support_as_current() {
    let path = temp_db("code-owner-unavailable");
    let mut kernel = kernel_with(&path);
    let revision = code_revision(1, "src/lib.rs", "run", "name-1", Some("body-1"));
    let mut memory = record(
        "unverified-code",
        MemoryKind::Fact,
        "unverified code claim",
        10,
    );
    memory
        .supporting_dependencies
        .push(code_dependency(&revision, CodeEntityFacet::Body));
    invoke(
        &mut kernel,
        MemoryCommand::Record {
            record: memory.clone(),
        },
    )
    .unwrap();

    assert!(matches!(
        invoke(
            &mut kernel,
            MemoryCommand::GetFreshness { id: memory.id.clone() },
        )
        .unwrap(),
        MemoryResponse::Freshness { state: Some(state) }
            if state.freshness == MemoryFreshness::NeedsValidation
                && !super::freshness::exact_support_is_current(&memory, &state)
    ));
    let _ = fs::remove_file(path);
}

#[test]
fn missing_current_code_facet_cannot_remain_silently_current() {
    let path = temp_db("code-facet-missing");
    let mut kernel = code_kernel_with(&path);
    let first = code_revision(1, "src/lib.rs", "run", "name-1", Some("body-1"));
    invoke_language(
        &mut kernel,
        LanguageCommand::RecordEntityRevision {
            revision: first.clone(),
        },
    )
    .unwrap();
    let mut memory = record("body-dependent", MemoryKind::Fact, "body dependent", 10);
    memory
        .supporting_dependencies
        .push(code_dependency(&first, CodeEntityFacet::Body));
    invoke(
        &mut kernel,
        MemoryCommand::Record {
            record: memory.clone(),
        },
    )
    .unwrap();

    let removed = code_revision(2, "src/lib.rs", "run", "name-1", None);
    invoke_language(
        &mut kernel,
        LanguageCommand::RecordEntityRevision { revision: removed },
    )
    .unwrap();
    let _ = invoke(
        &mut kernel,
        MemoryCommand::Recall {
            query: MemoryRecallQuery {
                scopes: vec![scope()],
                kinds: vec![MemoryKind::Fact],
                query: "body".into(),
                at: 20,
                limit: 10,
            },
        },
    )
    .unwrap();

    assert!(matches!(
        invoke(
            &mut kernel,
            MemoryCommand::GetFreshness { id: memory.id },
        )
        .unwrap(),
        MemoryResponse::Freshness { state: Some(state) }
            if state.freshness == MemoryFreshness::NeedsValidation
    ));
    let _ = fs::remove_file(path);
}

#[test]
fn restart_preserves_code_dependency_provenance_and_rechecks_current_facets() {
    let path = temp_db("code-facet-restart");
    let first = code_revision(1, "src/lib.rs", "run", "name-1", Some("body-1"));
    let memory = {
        let mut kernel = code_kernel_with(&path);
        invoke_language(
            &mut kernel,
            LanguageCommand::RecordEntityRevision {
                revision: first.clone(),
            },
        )
        .unwrap();
        let mut memory = record("restart-code", MemoryKind::Fact, "restart code claim", 10);
        memory
            .supporting_dependencies
            .push(code_dependency(&first, CodeEntityFacet::Body));
        invoke(
            &mut kernel,
            MemoryCommand::Record {
                record: memory.clone(),
            },
        )
        .unwrap();
        memory
    };

    let mut restored = code_kernel_with(&path);
    assert!(matches!(
        invoke(
            &mut restored,
            MemoryCommand::GetFreshness {
                id: memory.id.clone(),
            },
        )
        .unwrap(),
        MemoryResponse::Freshness { state: Some(state) }
            if state.freshness == MemoryFreshness::Current
                && state.dependencies.iter().any(|dependency|
                    dependency == &memory.supporting_dependencies[0])
    ));

    let changed = code_revision(2, "src/lib.rs", "run", "name-1", Some("body-2"));
    invoke_language(
        &mut restored,
        LanguageCommand::RecordEntityRevision { revision: changed },
    )
    .unwrap();
    let _ = invoke(
        &mut restored,
        MemoryCommand::Recall {
            query: MemoryRecallQuery {
                scopes: vec![scope()],
                kinds: vec![MemoryKind::Fact],
                query: "restart".into(),
                at: 20,
                limit: 10,
            },
        },
    )
    .unwrap();
    assert!(matches!(
        invoke(
            &mut restored,
            MemoryCommand::GetFreshness { id: memory.id },
        )
        .unwrap(),
        MemoryResponse::Freshness { state: Some(state) }
            if state.freshness == MemoryFreshness::NeedsValidation
    ));
    let _ = fs::remove_file(path);
}
