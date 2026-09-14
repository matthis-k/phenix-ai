use phenix_core::{
    Authority, CapabilityId, ComponentManifest, ConfigContribution, DurableSchemaRegistration,
    GraphGenerationId, Kernel, KernelError, LayerPolicy, PersistenceBackend, PluginExecution,
    PluginId, PluginInstance, PluginManifest, ResolvedHarness, ResolvedHarnessActivation,
    ResolvedHarnessActivationError, ResolvedHarnessError, ServiceId,
};
use phenix_plugin_catalog::{
    adapter_acp_factory, adapter_acp_manifest, artifact_component_manifest, artifact_factory,
    artifact_manifest, basic_context_component_manifest, basic_context_factory,
    basic_context_manifest, basic_model_component_manifest, basic_model_factory,
    basic_model_manifest, basic_skills_component_manifest, basic_skills_factory,
    basic_skills_manifest, basic_tools_component_manifest, basic_tools_factory,
    basic_tools_manifest, cli_component_manifest, cli_factory, cli_manifest,
    context_component_manifest, context_factory, context_manifest, debug_component_manifest,
    debug_factory, debug_manifest, execution_component_manifest, execution_factory,
    execution_manifest, first_party_durable_schema_registrations, frontend_component_manifest,
    frontend_factory, frontend_manifest, hook_component_manifest, hook_factory, hook_manifest,
    job_component_manifest, job_factory, job_manifest, language_component_manifest,
    language_factory, language_manifest, memory_component_manifest, memory_factory,
    memory_manifest, model_routing_component_manifest, model_routing_factory,
    model_routing_manifest, options_component_manifest, options_factory, options_manifest,
    planning_component_manifest, planning_factory, planning_manifest,
    repository_worker_component_manifest, repository_worker_factory, repository_worker_manifest,
    sdk_component_manifest, sdk_factory, sdk_manifest, session_component_manifest, session_factory,
    session_manifest, session_tree_component_manifest, session_tree_factory, session_tree_manifest,
    workspace_component_manifest, workspace_factory, workspace_manifest,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

mod basic_suite;
mod persistence;

type EmbeddedFactory = Arc<dyn Fn() -> Box<dyn PluginInstance> + Send + Sync>;

#[derive(Debug, thiserror::Error)]
pub enum HarnessBuildError {
    #[error(transparent)]
    Kernel(#[from] KernelError),
    #[error(transparent)]
    Resolution(#[from] ResolvedHarnessError),
    #[error("resolved Harness activation failed: {0:?}")]
    Activation(ResolvedHarnessActivationError),
    #[error(transparent)]
    Persistence(#[from] phenix_core::PersistenceCandidateError),
}

impl From<ResolvedHarnessActivationError> for HarnessBuildError {
    fn from(error: ResolvedHarnessActivationError) -> Self {
        Self::Activation(error)
    }
}

pub fn default_suite_authority() -> Authority {
    Authority::new([
        CapabilityId::parse("kernel.persistence.schema").expect("static capability"),
        CapabilityId::parse("kernel.persistence.read").expect("static capability"),
        CapabilityId::parse("kernel.persistence.write").expect("static capability"),
        CapabilityId::parse("workspace.read").expect("static capability"),
        CapabilityId::parse("workspace.write").expect("static capability"),
        CapabilityId::parse("workspace.shell").expect("static capability"),
        CapabilityId::parse("workspace.git").expect("static capability"),
    ])
}

#[derive(Default)]
pub struct HarnessBuilder {
    manifests: Vec<PluginManifest>,
    durable_schemas: Vec<DurableSchemaRegistration>,
    embedded_factories: BTreeMap<PluginId, EmbeddedFactory>,
    layer_policies: BTreeMap<ServiceId, Vec<LayerPolicy>>,
    components: Vec<ComponentManifest>,
    contributions: Vec<ConfigContribution>,
    component_authority: Authority,
}

impl HarnessBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_default_suite() -> Result<Self, KernelError> {
        let mut builder = Self::new();
        let authority = default_suite_authority();
        builder.component_authority = authority.clone();
        builder.add_embedded(repository_worker_manifest(), repository_worker_factory)?;
        builder.add_embedded(session_manifest(), session_factory)?;
        builder.add_embedded(session_tree_manifest(), session_tree_factory)?;
        builder.add_embedded(artifact_manifest(), artifact_factory)?;
        builder.add_embedded(cli_manifest(authority.clone()), cli_factory)?;
        builder.add_embedded(context_manifest(), context_factory)?;
        builder.add_embedded(execution_manifest(authority.clone()), execution_factory)?;
        builder.add_embedded(language_manifest(), language_factory)?;
        builder.add_embedded(memory_manifest(), memory_factory)?;
        builder.add_embedded(planning_manifest(), planning_factory)?;
        builder.add_embedded(workspace_manifest(), workspace_factory)?;
        builder.add_embedded(
            model_routing_manifest(authority.clone()),
            model_routing_factory,
        )?;
        builder.add_embedded(job_manifest(), job_factory)?;
        builder.add_embedded(frontend_manifest(authority.clone()), frontend_factory)?;
        builder.add_embedded(hook_manifest(authority.clone()), hook_factory)?;
        builder.add_embedded(debug_manifest(authority.clone()), debug_factory)?;
        builder.add_embedded(options_manifest(), options_factory)?;
        builder.add_embedded(sdk_manifest(authority.clone()), sdk_factory)?;
        for component in [
            repository_worker_component_manifest(),
            session_component_manifest(),
            session_tree_component_manifest(),
            artifact_component_manifest(),
            cli_component_manifest(authority.clone()),
            context_component_manifest(),
            execution_component_manifest(authority.clone()),
            language_component_manifest(),
            memory_component_manifest(),
            planning_component_manifest(),
            workspace_component_manifest(),
            model_routing_component_manifest(authority.clone()),
            job_component_manifest(),
            frontend_component_manifest(authority.clone()),
            hook_component_manifest(authority.clone()),
            debug_component_manifest(authority.clone()),
            options_component_manifest(),
            sdk_component_manifest(authority),
        ] {
            builder.add_component(component);
        }
        Ok(builder)
    }

    pub fn with_selected_suite(enabled: &BTreeSet<String>) -> Result<Self, String> {
        let authority = default_suite_authority();
        let available = [
            adapter_acp_manifest(),
            repository_worker_manifest(),
            session_manifest(),
            session_tree_manifest(),
            artifact_manifest(),
            cli_manifest(authority.clone()),
            context_manifest(),
            execution_manifest(authority.clone()),
            language_manifest(),
            memory_manifest(),
            planning_manifest(),
            workspace_manifest(),
            model_routing_manifest(authority.clone()),
            job_manifest(),
            frontend_manifest(authority.clone()),
            hook_manifest(authority.clone()),
            debug_manifest(authority.clone()),
            options_manifest(),
            sdk_manifest(authority.clone()),
            basic_model_manifest(),
            basic_tools_manifest(),
            basic_skills_manifest(),
            basic_context_manifest(),
        ]
        .into_iter()
        .map(|manifest| (manifest.id.as_str().to_owned(), manifest))
        .collect::<BTreeMap<_, _>>();
        let unknown = enabled
            .iter()
            .filter(|id| !available.contains_key(*id))
            .cloned()
            .collect::<Vec<_>>();
        if !unknown.is_empty() {
            return Err(format!(
                "unknown first-party plugin id(s): {}",
                unknown.join(", ")
            ));
        }

        let mut enabled = enabled.clone();
        let mut pending = enabled.iter().cloned().collect::<Vec<_>>();
        while let Some(plugin) = pending.pop() {
            let manifest = available
                .get(&plugin)
                .expect("validated selected plugin exists in first-party catalog");
            for dependency in &manifest.dependencies {
                let dependency = dependency.as_str().to_owned();
                if !available.contains_key(&dependency) {
                    return Err(format!(
                        "first-party plugin {plugin} depends on unavailable first-party plugin {dependency}"
                    ));
                }
                if enabled.insert(dependency.clone()) {
                    pending.push(dependency);
                }
            }
        }

        let mut builder = Self::new();
        builder.component_authority = authority.clone();
        builder.add_selected(&enabled, adapter_acp_manifest(), adapter_acp_factory)?;
        builder.add_selected(
            &enabled,
            repository_worker_manifest(),
            repository_worker_factory,
        )?;
        builder.add_selected(&enabled, session_manifest(), session_factory)?;
        builder.add_selected(&enabled, session_tree_manifest(), session_tree_factory)?;
        builder.add_selected(&enabled, artifact_manifest(), artifact_factory)?;
        builder.add_selected(&enabled, cli_manifest(authority.clone()), cli_factory)?;
        builder.add_selected(&enabled, context_manifest(), context_factory)?;
        builder.add_selected(
            &enabled,
            execution_manifest(authority.clone()),
            execution_factory,
        )?;
        builder.add_selected(&enabled, language_manifest(), language_factory)?;
        builder.add_selected(&enabled, memory_manifest(), memory_factory)?;
        builder.add_selected(&enabled, planning_manifest(), planning_factory)?;
        builder.add_selected(&enabled, workspace_manifest(), workspace_factory)?;
        builder.add_selected(
            &enabled,
            model_routing_manifest(authority.clone()),
            model_routing_factory,
        )?;
        builder.add_selected(&enabled, job_manifest(), job_factory)?;
        builder.add_selected(
            &enabled,
            frontend_manifest(authority.clone()),
            frontend_factory,
        )?;
        builder.add_selected(&enabled, hook_manifest(authority.clone()), hook_factory)?;
        builder.add_selected(&enabled, debug_manifest(authority.clone()), debug_factory)?;
        builder.add_selected(&enabled, options_manifest(), options_factory)?;
        builder.add_selected(&enabled, sdk_manifest(authority.clone()), sdk_factory)?;
        builder.add_selected(&enabled, basic_model_manifest(), basic_model_factory)?;
        builder.add_selected(&enabled, basic_tools_manifest(), basic_tools_factory)?;
        builder.add_selected(&enabled, basic_skills_manifest(), basic_skills_factory)?;
        builder.add_selected(&enabled, basic_context_manifest(), basic_context_factory)?;
        for component in [
            repository_worker_component_manifest(),
            session_component_manifest(),
            session_tree_component_manifest(),
            artifact_component_manifest(),
            cli_component_manifest(authority.clone()),
            context_component_manifest(),
            execution_component_manifest(authority.clone()),
            language_component_manifest(),
            memory_component_manifest(),
            planning_component_manifest(),
            workspace_component_manifest(),
            model_routing_component_manifest(authority.clone()),
            job_component_manifest(),
            frontend_component_manifest(authority.clone()),
            hook_component_manifest(authority.clone()),
            debug_component_manifest(authority.clone()),
            options_component_manifest(),
            sdk_component_manifest(authority),
            basic_model_component_manifest(),
            basic_tools_component_manifest(),
            basic_skills_component_manifest(),
            basic_context_component_manifest(),
        ] {
            if enabled.contains(component.owner.as_str()) {
                builder.add_component(component);
            }
        }
        Ok(builder)
    }

    fn add_selected<F>(
        &mut self,
        enabled: &BTreeSet<String>,
        manifest: PluginManifest,
        factory: F,
    ) -> Result<(), String>
    where
        F: Fn() -> Box<dyn PluginInstance> + Send + Sync + 'static,
    {
        if enabled.contains(manifest.id.as_str()) {
            self.add_embedded(manifest, factory)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    pub fn add_manifest(&mut self, manifest: PluginManifest) {
        self.manifests.push(manifest);
    }

    pub fn add_durable_schema(&mut self, registration: DurableSchemaRegistration) {
        self.durable_schemas.push(registration);
    }

    pub fn add_component(&mut self, manifest: ComponentManifest) {
        self.components.push(manifest);
    }

    pub fn add_config_contribution(&mut self, contribution: ConfigContribution) {
        self.contributions.push(contribution);
    }

    pub fn set_component_authority(&mut self, authority: Authority) {
        self.component_authority = authority;
    }

    pub fn set_layer_policy(&mut self, service: ServiceId, layers: Vec<LayerPolicy>) {
        self.layer_policies.insert(service, layers);
    }

    pub fn add_embedded<F>(
        &mut self,
        manifest: PluginManifest,
        factory: F,
    ) -> Result<(), KernelError>
    where
        F: Fn() -> Box<dyn PluginInstance> + Send + Sync + 'static,
    {
        if !matches!(manifest.execution, PluginExecution::Embedded) {
            return Err(KernelError::WrongExecutionKind(manifest.id));
        }
        let id = manifest.id.clone();
        self.durable_schemas
            .extend(first_party_durable_schema_registrations(&manifest));
        self.manifests.push(manifest);
        self.embedded_factories.insert(id, Arc::new(factory));
        Ok(())
    }

    pub fn build(self) -> Result<PhenixHarness, HarnessBuildError> {
        self.build_using(|resolved| Ok(Kernel::new(resolved.kernel_config().clone())))
    }

    pub fn build_with_persistence(
        self,
        persistence: impl PersistenceBackend + 'static,
    ) -> Result<PhenixHarness, HarnessBuildError> {
        self.build_using(|resolved| {
            Ok(Kernel::with_persistence(
                resolved.kernel_config().clone(),
                persistence,
            ))
        })
    }

    fn build_using(
        self,
        create_kernel: impl FnOnce(&ResolvedHarness) -> Result<Kernel, HarnessBuildError>,
    ) -> Result<PhenixHarness, HarnessBuildError> {
        let resolved = ResolvedHarness::resolve_with_durable_schemas_and_layer_policies(
            self.manifests.clone(),
            self.components,
            self.durable_schemas,
            self.contributions,
            self.layer_policies,
            &self.component_authority,
        )?;
        let mut kernel = create_kernel(&resolved)?;
        kernel.activate_resolved_harness(&resolved)?;
        for (plugin, factory) in self.embedded_factories {
            kernel.register_embedded_factory(plugin, move || factory())?;
        }
        Ok(PhenixHarness { kernel, resolved })
    }
}

pub struct PhenixHarness {
    kernel: Kernel,
    resolved: ResolvedHarness,
}

impl PhenixHarness {
    pub fn kernel(&self) -> &Kernel {
        &self.kernel
    }

    pub fn kernel_mut(&mut self) -> &mut Kernel {
        &mut self.kernel
    }

    pub fn component_graph(&self) -> &phenix_core::ResolvedComponentGraph {
        self.resolved.component_graph()
    }

    pub fn resolved_harness(&self) -> &ResolvedHarness {
        &self.resolved
    }

    pub fn generation(&self) -> &GraphGenerationId {
        self.resolved.generation()
    }

    pub fn activate(&mut self) -> Result<(), KernelError> {
        self.kernel.activate_all()
    }

    pub fn kernel_only() -> Self {
        let resolved = ResolvedHarness::resolve([], [], [], &Authority::default())
            .expect("empty kernel-only composition is valid");
        let mut kernel = Kernel::kernel_only();
        kernel
            .activate_resolved_harness(&resolved)
            .expect("kernel-only resolved Harness activates");
        Self { kernel, resolved }
    }

    pub fn default_suite() -> Result<Self, HarnessBuildError> {
        HarnessBuilder::with_default_suite()?.build()
    }

    pub fn default_suite_with_persistence(
        persistence: impl PersistenceBackend + 'static,
    ) -> Result<Self, HarnessBuildError> {
        HarnessBuilder::with_default_suite()?.build_with_persistence(persistence)
    }

    pub fn invoke(
        &mut self,
        service: &phenix_core::ServiceId,
        input: &[u8],
        authority: &Authority,
        binding: Option<&PluginId>,
    ) -> Result<Vec<u8>, KernelError> {
        self.kernel.invoke(service, input, authority, binding)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{
        CapabilityId, ContextResourceId, ContextRevisionId, LayerResult, PhenixValue, Project,
        ServiceContribution, ServiceId, ServiceRole, SessionId,
    };
    use phenix_plugin_catalog::{
        artifact_manifest, artifact_service, context_manifest, context_service, memory_service,
        planning_manifest, planning_service, repository_work_queue_service, session_manifest,
        session_service, ArtifactCommand, ArtifactProvenance, ArtifactResponse, ContextCommand,
        ContextDescriptor, ContextResourceKind, ContextResponse, ContextScope, PlanningCommand,
        PlanningResponse, RepositoryWorkSnapshot, SessionCommand, SessionResponse,
    };

    fn plugin(value: &str) -> PluginId {
        PluginId::parse(value).unwrap()
    }

    fn capability(value: &str) -> CapabilityId {
        CapabilityId::parse(value).unwrap()
    }

    fn service() -> ServiceId {
        ServiceId::parse("fixture.echo@1").unwrap()
    }

    fn session_authority() -> Authority {
        session_manifest().maximum_authority
    }

    fn artifact_authority() -> Authority {
        artifact_manifest().maximum_authority
    }

    fn context_authority() -> Authority {
        context_manifest().maximum_authority
    }

    fn planning_authority() -> Authority {
        planning_manifest().maximum_authority
    }

    fn manifest(id: &str, priority: i32) -> PluginManifest {
        service_manifest(id, service(), priority, Authority::default())
    }

    fn service_manifest(
        id: &str,
        service: ServiceId,
        priority: i32,
        maximum_authority: Authority,
    ) -> PluginManifest {
        PluginManifest {
            id: plugin(id),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: vec![ServiceContribution {
                role: ServiceRole::Terminal,
                service,
                priority,
                required_authority: Authority::default(),
            }],
            resource_namespaces: Vec::new(),
            maximum_authority,
        }
    }

    fn layer_manifest(id: &str, service: ServiceId, priority: i32) -> PluginManifest {
        PluginManifest {
            id: plugin(id),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: vec![ServiceContribution {
                role: ServiceRole::Layer,
                service,
                priority,
                required_authority: Authority::default(),
            }],
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        }
    }

    struct Echo(&'static [u8]);

    impl PluginInstance for Echo {
        fn start(&mut self, _host: &phenix_core::PluginHost<'_>) -> Result<(), String> {
            Ok(())
        }

        fn invoke(
            &mut self,
            _service: &ServiceId,
            _input: &[u8],
            _host: &phenix_core::PluginHost<'_>,
        ) -> Result<Vec<u8>, String> {
            Ok(self.0.to_vec())
        }
    }

    struct LayerEcho;

    impl PluginInstance for LayerEcho {
        fn start(&mut self, _host: &phenix_core::PluginHost<'_>) -> Result<(), String> {
            Ok(())
        }

        fn invoke_layer(
            &mut self,
            _service: &ServiceId,
            input: &[u8],
            host: &phenix_core::PluginHost<'_>,
        ) -> Result<LayerResult, String> {
            let lower = host
                .continue_service(input, host.authority())
                .map_err(|error| error.to_string())?;
            let mut output = b"layer:".to_vec();
            output.extend_from_slice(&lower);
            Ok(LayerResult::Handled(output))
        }
    }

    struct FixedResponse(Vec<u8>);

    impl PluginInstance for FixedResponse {
        fn start(&mut self, _host: &phenix_core::PluginHost<'_>) -> Result<(), String> {
            Ok(())
        }

        fn invoke(
            &mut self,
            _service: &ServiceId,
            _input: &[u8],
            _host: &phenix_core::PluginHost<'_>,
        ) -> Result<Vec<u8>, String> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn alternate_memory_provider_replaces_default_without_core_changes() {
        let mut builder = HarnessBuilder::with_default_suite().unwrap();
        builder
            .add_embedded(
                service_manifest(
                    "fixture.alternate-memory",
                    memory_service(),
                    200,
                    Authority::default(),
                ),
                || Box::new(Echo(b"alternate-memory")),
            )
            .unwrap();
        let mut harness = builder.build().unwrap();
        harness.activate().unwrap();

        assert_eq!(
            harness
                .invoke(&memory_service(), b"ignored", &Authority::default(), None)
                .unwrap(),
            b"alternate-memory"
        );
    }

    #[test]
    fn harness_builder_applies_layer_policy() {
        let service = service();
        let layer_id = plugin("fixture-layer");
        let mut builder = HarnessBuilder::new();
        builder
            .add_embedded(manifest("terminal", 1), || Box::new(Echo(b"terminal")))
            .unwrap();
        builder
            .add_embedded(
                layer_manifest("fixture-layer", service.clone(), 100),
                || Box::new(LayerEcho),
            )
            .unwrap();
        builder.set_layer_policy(
            service.clone(),
            vec![LayerPolicy {
                plugin: layer_id,
                priority: 100,
                required: true,
                enabled: true,
            }],
        );
        let mut harness = builder.build().unwrap();
        harness.activate().unwrap();

        assert_eq!(
            harness
                .invoke(&service, b"input", &Authority::default(), None)
                .unwrap(),
            b"layer:terminal"
        );
    }

    #[test]
    fn layer_policy_is_part_of_resolved_generation_identity() {
        fn generation(required: bool) -> GraphGenerationId {
            let service = service();
            let mut builder = HarnessBuilder::new();
            builder
                .add_embedded(manifest("terminal", 1), || Box::new(Echo(b"terminal")))
                .unwrap();
            builder
                .add_embedded(
                    layer_manifest("fixture-layer", service.clone(), 100),
                    || Box::new(LayerEcho),
                )
                .unwrap();
            builder.set_layer_policy(
                service,
                vec![LayerPolicy {
                    plugin: plugin("fixture-layer"),
                    priority: 100,
                    required,
                    enabled: true,
                }],
            );
            builder.build().unwrap().generation().clone()
        }

        assert_eq!(generation(true), generation(true));
        assert_ne!(generation(true), generation(false));
    }

    #[test]
    fn kernel_only_harness_has_no_userspace_plugins() {
        let mut harness = PhenixHarness::kernel_only();
        harness.activate().unwrap();
        assert_eq!(harness.kernel().config().manifests().count(), 0);
        let input = serde_json::to_vec(&SessionCommand::Get {
            id: SessionId::parse("missing").unwrap(),
        })
        .unwrap();
        assert!(harness
            .invoke(&session_service(), &input, &session_authority(), None)
            .is_err());
        let context = serde_json::to_vec(&ContextCommand::List).unwrap();
        assert!(harness
            .invoke(&context_service(), &context, &context_authority(), None)
            .is_err());
        let planning = serde_json::to_vec(&PlanningCommand::GetObjective {
            id: "missing".into(),
        })
        .unwrap();
        assert!(harness
            .invoke(&planning_service(), &planning, &planning_authority(), None)
            .is_err());
    }

    #[test]
    fn default_harness_routes_first_party_services_through_kernel_contracts() {
        let mut harness = PhenixHarness::default_suite().unwrap();
        harness.activate().unwrap();

        let snapshot = RepositoryWorkSnapshot {
            pull_requests: Vec::new(),
            issues: Vec::new(),
        };
        let input = serde_json::to_vec(&PhenixValue::from(&snapshot)).unwrap();
        let output = harness
            .invoke(
                &repository_work_queue_service(),
                &input,
                &Authority::default(),
                None,
            )
            .unwrap();
        serde_json::from_slice::<PhenixValue>(&output).unwrap();

        let create = serde_json::to_vec(&PhenixValue::from(&SessionCommand::Create {
            id: SessionId::parse("session-1").unwrap(),
        }))
        .unwrap();
        let response = harness
            .invoke(&session_service(), &create, &session_authority(), None)
            .unwrap();
        let response: PhenixValue = serde_json::from_slice(&response).unwrap();
        assert!(matches!(
            SessionResponse::try_from(Project(&response)).unwrap(),
            SessionResponse::Created { .. }
        ));

        let store = serde_json::to_vec(&PhenixValue::from(&ArtifactCommand::Store {
            content: b"readme".to_vec(),
            provenance: ArtifactProvenance {
                producer: "harness-smoke".into(),
                provider_identity: None,
                configuration_identity: None,
                source_observations: BTreeMap::new(),
            },
        }))
        .unwrap();
        let artifact = artifact_component_manifest();
        let response = harness
            .kernel_mut()
            .invoke_component(
                &artifact.id,
                &artifact_service(),
                &store,
                &artifact_authority(),
                &artifact.owner,
            )
            .unwrap();
        let response: PhenixValue = serde_json::from_slice(&response).unwrap();
        assert!(matches!(
            ArtifactResponse::try_from(Project(&response)).unwrap(),
            ArtifactResponse::Stored { reused: false, .. }
        ));

        let register = serde_json::to_vec(&PhenixValue::from(&ContextCommand::Register {
            resource_id: ContextResourceId::parse("skill:review").unwrap(),
            kind: ContextResourceKind::Skill,
            source: "skills/review/SKILL.md".into(),
            scope: ContextScope::Workspace,
            content: b"review".to_vec().into(),
        }))
        .unwrap();
        let response = harness
            .invoke(&context_service(), &register, &context_authority(), None)
            .unwrap();
        let response: PhenixValue = serde_json::from_slice(&response).unwrap();
        assert!(matches!(
            ContextResponse::try_from(Project(&response)).unwrap(),
            ContextResponse::Registered { .. }
        ));

        let objective = serde_json::to_vec(&PhenixValue::from(&PlanningCommand::CreateObjective {
            id: "objective-1".into(),
            title: "Use plugin-owned planning".into(),
            parent: None,
        }))
        .unwrap();
        let planning = planning_component_manifest();
        let response = harness
            .kernel_mut()
            .invoke_component(
                &planning.id,
                &planning_service(),
                &objective,
                &planning_authority(),
                &planning.owner,
            )
            .unwrap();
        let response: PhenixValue = serde_json::from_slice(&response).unwrap();
        assert!(matches!(
            PlanningResponse::try_from(Project(&response)).unwrap(),
            PlanningResponse::Objective { objective: Some(_) }
        ));
    }

    #[test]
    fn first_party_session_provider_is_replaceable_through_normal_resolution() {
        let alternate = serde_json::to_vec(&SessionResponse::Session { session: None }).unwrap();
        let alternate_factory = alternate.clone();
        let mut builder = HarnessBuilder::new();
        builder
            .add_embedded(session_manifest(), session_factory)
            .unwrap();
        builder
            .add_embedded(
                service_manifest(
                    "alternate-sessions",
                    session_service(),
                    200,
                    Authority::default(),
                ),
                move || Box::new(FixedResponse(alternate_factory.clone())),
            )
            .unwrap();
        let mut harness = builder.build().unwrap();
        harness.activate().unwrap();
        let input = serde_json::to_vec(&SessionCommand::Get {
            id: SessionId::parse("x").unwrap(),
        })
        .unwrap();
        assert_eq!(
            harness
                .invoke(&session_service(), &input, &Authority::default(), None)
                .unwrap(),
            alternate
        );
    }

    #[test]
    fn first_party_context_provider_is_replaceable_through_normal_resolution() {
        let alternate = serde_json::to_vec(&ContextResponse::Resources {
            descriptors: Vec::new(),
        })
        .unwrap();
        let alternate_factory = alternate.clone();
        let mut builder = HarnessBuilder::new();
        builder
            .add_embedded(
                execution_manifest(default_suite_authority()),
                execution_factory,
            )
            .unwrap();
        builder
            .add_embedded(context_manifest(), context_factory)
            .unwrap();
        builder
            .add_embedded(
                service_manifest(
                    "alternate-context",
                    context_service(),
                    200,
                    Authority::default(),
                ),
                move || Box::new(FixedResponse(alternate_factory.clone())),
            )
            .unwrap();
        let mut harness = builder.build().unwrap();
        harness.activate().unwrap();
        let input = serde_json::to_vec(&ContextCommand::List).unwrap();
        assert_eq!(
            harness
                .invoke(&context_service(), &input, &Authority::default(), None)
                .unwrap(),
            alternate
        );
    }

    #[test]
    fn mock_qml_context_provider_contributes_through_the_same_service_contract() {
        let qml_descriptor = ContextDescriptor {
            resource_id: ContextResourceId::parse("qml:Main.qml").unwrap(),
            revision: ContextRevisionId::parse("qml-revision").unwrap(),
            kind: ContextResourceKind::External,
            source: "Main.qml".into(),
            scope: ContextScope::Workspace,
            content_identity: "qml-revision".into(),
            estimated_bytes: 128,
        };
        let response = serde_json::to_vec(&ContextResponse::Resources {
            descriptors: vec![qml_descriptor.clone()],
        })
        .unwrap();
        let response_factory = response.clone();
        let mut builder = HarnessBuilder::new();
        builder
            .add_embedded(
                service_manifest(
                    "mock-qml-context",
                    context_service(),
                    200,
                    Authority::default(),
                ),
                move || Box::new(FixedResponse(response_factory.clone())),
            )
            .unwrap();
        let mut harness = builder.build().unwrap();
        harness.activate().unwrap();
        let input = serde_json::to_vec(&ContextCommand::List).unwrap();
        let output = harness
            .invoke(&context_service(), &input, &Authority::default(), None)
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<ContextResponse>(&output).unwrap(),
            ContextResponse::Resources {
                descriptors: vec![qml_descriptor],
            }
        );
    }

    #[test]
    fn product_policy_can_replace_provider_without_kernel_changes() {
        let mut builder = HarnessBuilder::new();
        builder
            .add_embedded(manifest("first-party", 10), || Box::new(Echo(b"first")))
            .unwrap();
        builder
            .add_embedded(manifest("alternate", 20), || Box::new(Echo(b"alternate")))
            .unwrap();
        let mut harness = builder.build().unwrap();
        harness.activate().unwrap();

        assert_eq!(
            harness
                .invoke(&service(), b"", &Authority::default(), None)
                .unwrap(),
            b"alternate"
        );
        assert_eq!(
            harness
                .invoke(
                    &service(),
                    b"",
                    &Authority::default(),
                    Some(&plugin("first-party")),
                )
                .unwrap(),
            b"first"
        );
    }

    #[test]
    fn omitting_provider_removes_it_from_product_composition() {
        let mut builder = HarnessBuilder::new();
        builder
            .add_embedded(manifest("first-party", 10), || Box::new(Echo(b"first")))
            .unwrap();
        let mut harness = builder.build().unwrap();
        harness.activate().unwrap();

        assert_eq!(
            harness
                .invoke(&service(), b"", &Authority::default(), None)
                .unwrap(),
            b"first"
        );
        assert_eq!(
            harness.kernel().config().manifests().count(),
            1,
            "omitted plugins do not exist as kernel fallbacks"
        );
    }

    #[test]
    fn first_party_state_plugins_require_only_persistence_authority() {
        for authority in [
            session_authority(),
            artifact_authority(),
            context_authority(),
            planning_authority(),
        ] {
            assert!(authority.permits(&capability("kernel.persistence.schema")));
            assert!(authority.permits(&capability("kernel.persistence.read")));
            assert!(authority.permits(&capability("kernel.persistence.write")));
            assert!(!authority.permits(&capability("fs.write")));
        }
    }
}
