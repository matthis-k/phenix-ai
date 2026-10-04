use phenix_core::{
    Authority, CapabilityId, ComponentEntryTrigger, ComponentId, ComponentManifest,
    ComponentProcessArgument, ConfigContribution, DurableSchemaRegistration, GraphGenerationId,
    GraphReconciler, InterfaceId, Kernel, KernelError, LayerPolicy, LiveReconciliationError,
    PersistenceBackend, PluginBuildPlan, PluginBuildReport, PluginExecution, PluginId,
    PluginInstance, PluginManagementContext, PluginManagementError, PluginManagementRequest,
    PluginManifest, PluginTrialResult, ReconciliationResult, ResolvedHarness,
    ResolvedHarnessActivation, ResolvedHarnessActivationError, ResolvedHarnessError,
    RootExecutionConstraints, RootExecutionHandle, ServiceId,
};
use phenix_plugin_catalog::{
    adapter_acp_factory, adapter_acp_manifest, advanced_agent_configuration_manifest,
    agent_loop_component_manifest, agent_loop_factory, agent_loop_manifest,
    artifact_component_manifest, artifact_factory, artifact_manifest,
    basic_agent_configuration_manifest, basic_context_component_manifest, basic_context_factory,
    basic_context_manifest, basic_model_component_manifest, basic_model_factory,
    basic_model_manifest, basic_product_configuration_manifest, basic_skills_component_manifest,
    basic_skills_factory, basic_skills_manifest, basic_tools_component_manifest,
    basic_tools_factory, basic_tools_manifest, benchmark_outcome_component_manifest,
    benchmark_outcome_factory, benchmark_outcome_manifest, cli_component_manifest, cli_factory,
    cli_manifest, common_provider_definitions, context_component_manifest, context_factory,
    context_manifest, debug_component_manifest, debug_factory, debug_manifest,
    debug_runtime_trace_sink, efficiency_evaluation_component_manifest,
    efficiency_evaluation_factory, efficiency_evaluation_manifest, execution_component_manifest,
    execution_factory, execution_manifest, first_party_durable_schema_registrations,
    frontend_component_manifest, frontend_factory, frontend_manifest,
    full_product_configuration_manifest, helper_invocation_component_manifest,
    hook_component_manifest, hook_factory, hook_manifest, job_component_manifest, job_factory,
    job_manifest, language_component_manifest, language_factory, language_manifest,
    local_environment_component_manifest, local_environment_factory, local_environment_manifest,
    memory_component_manifest, memory_factory, memory_manifest, model_routing_component_manifest,
    model_routing_factory, model_routing_manifest, openai_codex_component_manifest,
    openai_codex_factory, openai_codex_manifest, options_component_manifest, options_factory,
    options_manifest, planning_component_manifest, planning_factory, planning_manifest,
    providers_manifest, repository_worker_component_manifest, repository_worker_factory,
    repository_worker_manifest, sdk_component_manifest, sdk_factory, sdk_manifest,
    session_component_manifest, session_factory, session_manifest, session_tree_component_manifest,
    session_tree_factory, session_tree_manifest, step_runner_component_manifest,
    step_runner_factory, step_runner_manifest, workspace_component_manifest, workspace_factory,
    workspace_manifest, AGENT_LOOP_PLUGIN,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt::{self, Display, Formatter},
    sync::Arc,
};

pub mod application;
mod basic_suite;
pub use invocation_defaults::{invocation_defaults_manifest, INVOCATION_DEFAULTS_PLUGIN};
use phenix_plugin_invocation_defaults as invocation_defaults;
pub mod model_surface_fixture;
mod persistence;
pub mod runtime_config;

type EmbeddedFactory = Arc<dyn Fn() -> Box<dyn PluginInstance> + Send + Sync>;

#[derive(Debug)]
pub enum HarnessBuildError {
    Kernel(KernelError),
    Resolution(ResolvedHarnessError),
    Activation(ResolvedHarnessActivationError),
    Persistence(phenix_core::PersistenceCandidateError),
}

impl Display for HarnessBuildError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Kernel(error) => Display::fmt(error, f),
            Self::Resolution(error) => Display::fmt(error, f),
            Self::Activation(error) => write!(f, "resolved Harness activation failed: {error:?}"),
            Self::Persistence(error) => Display::fmt(error, f),
        }
    }
}

impl Error for HarnessBuildError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Kernel(error) => Some(error),
            Self::Resolution(error) => Some(error),
            Self::Activation(_) => None,
            Self::Persistence(error) => Some(error),
        }
    }
}

impl From<KernelError> for HarnessBuildError {
    fn from(error: KernelError) -> Self {
        Self::Kernel(error)
    }
}

impl From<ResolvedHarnessError> for HarnessBuildError {
    fn from(error: ResolvedHarnessError) -> Self {
        Self::Resolution(error)
    }
}

impl From<ResolvedHarnessActivationError> for HarnessBuildError {
    fn from(error: ResolvedHarnessActivationError) -> Self {
        Self::Activation(error)
    }
}

impl From<phenix_core::PersistenceCandidateError> for HarnessBuildError {
    fn from(error: phenix_core::PersistenceCandidateError) -> Self {
        Self::Persistence(error)
    }
}

pub fn default_application_root_authority() -> Authority {
    Authority::new([
        CapabilityId::parse("kernel.persistence.schema").expect("static capability"),
        CapabilityId::parse("kernel.persistence.read").expect("static capability"),
        CapabilityId::parse("kernel.persistence.write").expect("static capability"),
        CapabilityId::parse("network.http").expect("static capability"),
        CapabilityId::parse("secrets.manage").expect("static capability"),
        CapabilityId::parse("workspace.read").expect("static capability"),
        CapabilityId::parse("workspace.write").expect("static capability"),
        CapabilityId::parse("workspace.shell").expect("static capability"),
        CapabilityId::parse("workspace.git").expect("static capability"),
    ])
}

pub fn runtime_orchestration_authority() -> Authority {
    Authority::new([
        CapabilityId::parse("application.session.control").expect("static capability"),
        CapabilityId::parse("runtime.generation.select").expect("static capability"),
        CapabilityId::parse("runtime.plugin.inspect").expect("static capability"),
        CapabilityId::parse("runtime.plugin.build").expect("static capability"),
        CapabilityId::parse("runtime.plugin.trial").expect("static capability"),
        CapabilityId::parse("runtime.plugin.promote").expect("static capability"),
        CapabilityId::parse("runtime.plugin.retire").expect("static capability"),
    ])
}

pub fn default_suite_authority() -> Authority {
    let application = default_application_root_authority();
    let orchestration = runtime_orchestration_authority();
    Authority::new(
        application
            .capabilities()
            .cloned()
            .chain(orchestration.capabilities().cloned()),
    )
}

#[derive(Default)]
pub struct HarnessBuilder {
    manifests: Vec<PluginManifest>,
    durable_schemas: Vec<DurableSchemaRegistration>,
    embedded_factories: BTreeMap<PluginId, EmbeddedFactory>,
    layer_policies: BTreeMap<ServiceId, Vec<LayerPolicy>>,
    components: Vec<ComponentManifest>,
    entry_triggers: Vec<ComponentEntryTrigger>,
    process_arguments: Vec<ComponentProcessArgument>,
    contributions: Vec<ConfigContribution>,
    component_authority: Authority,
    application_agent_tools: application::ApplicationAgentToolRegistry,
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
        builder.add_embedded(
            efficiency_evaluation_manifest(),
            efficiency_evaluation_factory,
        )?;
        builder.add_embedded(agent_loop_manifest(authority.clone()), agent_loop_factory)?;
        let application_agent_tools = builder.application_agent_tools.clone();
        builder.add_embedded(
            application::application_agent_tool_manifest(authority.clone()),
            move || application::application_agent_tool_factory(application_agent_tools.clone()),
        )?;
        builder.add_embedded(language_manifest(), language_factory)?;
        builder.add_embedded(memory_manifest(), memory_factory)?;
        builder.add_embedded(planning_manifest(), planning_factory)?;
        builder.add_embedded(local_environment_manifest(), local_environment_factory)?;
        builder.add_embedded(workspace_manifest(), workspace_factory)?;
        builder.add_embedded(
            model_routing_manifest(authority.clone()),
            model_routing_factory,
        )?;
        let provider_definitions = common_provider_definitions();
        for provider in &provider_definitions {
            builder.add_embedded(provider.manifest(), provider.factory())?;
        }
        builder.add_embedded(openai_codex_manifest(), openai_codex_factory)?;
        builder.add_embedded(step_runner_manifest(authority.clone()), step_runner_factory)?;
        builder.add_embedded(job_manifest(), job_factory)?;
        builder.add_embedded(frontend_manifest(authority.clone()), frontend_factory)?;
        builder.add_embedded(debug_manifest(authority.clone()), debug_factory)?;
        builder.add_embedded(options_manifest(), options_factory)?;
        builder.add_embedded(
            invocation_defaults::invocation_defaults_manifest(authority.clone()),
            invocation_defaults::invocation_defaults_factory,
        )?;
        builder.add_embedded(sdk_manifest(authority.clone()), sdk_factory)?;
        for component in [
            repository_worker_component_manifest(),
            session_component_manifest(),
            session_tree_component_manifest(),
            artifact_component_manifest(),
            cli_component_manifest(authority.clone()),
            context_component_manifest(),
            execution_component_manifest(authority.clone()),
            efficiency_evaluation_component_manifest(),
            agent_loop_component_manifest(authority.clone()),
            application::application_agent_tool_component_manifest(authority.clone()),
            language_component_manifest(),
            memory_component_manifest(),
            planning_component_manifest(),
            local_environment_component_manifest(),
            workspace_component_manifest(),
            model_routing_component_manifest(authority.clone()),
            openai_codex_component_manifest(),
            step_runner_component_manifest(authority.clone()),
            helper_invocation_component_manifest(authority.clone()),
            job_component_manifest(),
            frontend_component_manifest(authority.clone()),
            debug_component_manifest(authority.clone()),
            options_component_manifest(),
            invocation_defaults::invocation_defaults_component_manifest(authority.clone()),
            sdk_component_manifest(authority),
        ] {
            builder.add_component(component);
        }
        for provider in provider_definitions {
            builder.add_component(provider.component_manifest());
        }
        for trigger in application::application_workspace_tool_triggers() {
            builder.add_entry_trigger(trigger);
        }
        Ok(builder)
    }

    pub fn with_selected_suite(enabled: &BTreeSet<String>) -> Result<Self, String> {
        let authority = default_suite_authority();
        let provider_definitions = common_provider_definitions();
        let mut available = [
            advanced_agent_configuration_manifest(),
            basic_agent_configuration_manifest(),
            basic_product_configuration_manifest(),
            full_product_configuration_manifest(),
            providers_manifest(),
            openai_codex_manifest(),
            adapter_acp_manifest(),
            repository_worker_manifest(),
            session_manifest(),
            session_tree_manifest(),
            artifact_manifest(),
            cli_manifest(authority.clone()),
            context_manifest(),
            execution_manifest(authority.clone()),
            efficiency_evaluation_manifest(),
            benchmark_outcome_manifest(),
            agent_loop_manifest(authority.clone()),
            language_manifest(),
            memory_manifest(),
            planning_manifest(),
            local_environment_manifest(),
            workspace_manifest(),
            model_routing_manifest(authority.clone()),
            step_runner_manifest(authority.clone()),
            job_manifest(),
            frontend_manifest(authority.clone()),
            hook_manifest(authority.clone()),
            debug_manifest(authority.clone()),
            options_manifest(),
            invocation_defaults::invocation_defaults_manifest(authority.clone()),
            sdk_manifest(authority.clone()),
            basic_model_manifest(),
            basic_tools_manifest(),
            basic_skills_manifest(),
            basic_context_manifest(),
        ]
        .into_iter()
        .map(|manifest| (manifest.id.as_str().to_owned(), manifest))
        .collect::<BTreeMap<_, _>>();
        for provider in &provider_definitions {
            let manifest = provider.manifest();
            available.insert(manifest.id.as_str().to_owned(), manifest);
        }
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

        // Explicit selection is exact. Only declared manifest dependencies may expand it.
        let mut enabled = enabled.clone();
        let mut pending = enabled.iter().cloned().collect::<Vec<_>>();
        let expand_dependencies = |enabled: &mut BTreeSet<String>,
                                   pending: &mut Vec<String>|
         -> Result<(), String> {
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
            Ok(())
        };
        expand_dependencies(&mut enabled, &mut pending)?;

        if enabled.contains(AGENT_LOOP_PLUGIN) {
            let adapter = application::application_agent_tool_manifest(authority.clone());
            for dependency in &adapter.dependencies {
                let dependency = dependency.as_str().to_owned();
                if !available.contains_key(&dependency) {
                    return Err(format!(
                        "first-party plugin {} depends on unavailable first-party plugin {dependency}",
                        adapter.id
                    ));
                }
                if enabled.insert(dependency.clone()) {
                    pending.push(dependency);
                }
            }
            expand_dependencies(&mut enabled, &mut pending)?;
        }

        let mut builder = Self::new();
        builder.component_authority = authority.clone();
        for manifest in [
            basic_agent_configuration_manifest(),
            advanced_agent_configuration_manifest(),
            basic_product_configuration_manifest(),
            full_product_configuration_manifest(),
            providers_manifest(),
        ] {
            if enabled.contains(manifest.id.as_str()) {
                builder.add_manifest(manifest);
            }
        }
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
        builder.add_selected(
            &enabled,
            efficiency_evaluation_manifest(),
            efficiency_evaluation_factory,
        )?;
        builder.add_selected(
            &enabled,
            benchmark_outcome_manifest(),
            benchmark_outcome_factory,
        )?;
        builder.add_selected(
            &enabled,
            agent_loop_manifest(authority.clone()),
            agent_loop_factory,
        )?;
        if enabled.contains(AGENT_LOOP_PLUGIN) {
            let application_agent_tools = builder.application_agent_tools.clone();
            builder
                .add_embedded(
                    application::application_agent_tool_manifest(authority.clone()),
                    move || {
                        application::application_agent_tool_factory(application_agent_tools.clone())
                    },
                )
                .map_err(|error| error.to_string())?;
        }
        builder.add_selected(&enabled, language_manifest(), language_factory)?;
        builder.add_selected(&enabled, memory_manifest(), memory_factory)?;
        builder.add_selected(&enabled, planning_manifest(), planning_factory)?;
        builder.add_selected(
            &enabled,
            local_environment_manifest(),
            local_environment_factory,
        )?;
        builder.add_selected(&enabled, workspace_manifest(), workspace_factory)?;
        builder.add_selected(
            &enabled,
            model_routing_manifest(authority.clone()),
            model_routing_factory,
        )?;
        for provider in &provider_definitions {
            let manifest = provider.manifest();
            if enabled.contains(manifest.id.as_str()) {
                builder
                    .add_embedded(manifest, provider.factory())
                    .map_err(|error| error.to_string())?;
            }
        }
        builder.add_selected(&enabled, openai_codex_manifest(), openai_codex_factory)?;
        builder.add_selected(
            &enabled,
            step_runner_manifest(authority.clone()),
            step_runner_factory,
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
        builder.add_selected(
            &enabled,
            invocation_defaults::invocation_defaults_manifest(authority.clone()),
            invocation_defaults::invocation_defaults_factory,
        )?;
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
            efficiency_evaluation_component_manifest(),
            benchmark_outcome_component_manifest(),
            agent_loop_component_manifest(authority.clone()),
            language_component_manifest(),
            memory_component_manifest(),
            planning_component_manifest(),
            local_environment_component_manifest(),
            workspace_component_manifest(),
            model_routing_component_manifest(authority.clone()),
            openai_codex_component_manifest(),
            step_runner_component_manifest(authority.clone()),
            helper_invocation_component_manifest(authority.clone()),
            job_component_manifest(),
            frontend_component_manifest(authority.clone()),
            hook_component_manifest(authority.clone()),
            debug_component_manifest(authority.clone()),
            options_component_manifest(),
            invocation_defaults::invocation_defaults_component_manifest(authority.clone()),
            sdk_component_manifest(authority.clone()),
            basic_model_component_manifest(),
            basic_tools_component_manifest(),
            basic_skills_component_manifest(),
            basic_context_component_manifest(),
        ] {
            if enabled.contains(component.owner.as_str()) {
                builder.add_component(component);
            }
        }
        for provider in provider_definitions {
            let component = provider.component_manifest();
            if enabled.contains(component.owner.as_str()) {
                builder.add_component(component);
            }
        }
        if enabled.contains(AGENT_LOOP_PLUGIN) {
            builder.add_component(application::application_agent_tool_component_manifest(
                authority,
            ));
            if enabled.contains("phenix.workspace") {
                for trigger in application::application_workspace_tool_triggers() {
                    builder.add_entry_trigger(trigger);
                }
            }
            if enabled.contains("phenix.language") {
                for trigger in application::application_code_tool_triggers() {
                    builder.add_entry_trigger(trigger);
                }
            }
            if enabled.contains("phenix.memory") {
                for trigger in application::application_memory_tool_triggers() {
                    builder.add_entry_trigger(trigger);
                }
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

    pub fn add_entry_trigger(&mut self, trigger: ComponentEntryTrigger) {
        self.entry_triggers.push(trigger);
    }

    pub fn add_process_argument(&mut self, argument: ComponentProcessArgument) {
        self.process_arguments.push(argument);
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
        let application_agent_tools = self.application_agent_tools.clone();
        let debug_id = debug_manifest(self.component_authority.clone()).id;
        let debug_enabled = self
            .manifests
            .iter()
            .any(|manifest| manifest.id == debug_id);
        let resolved =
            ResolvedHarness::resolve_with_durable_schemas_layer_policies_entry_triggers_and_process_arguments(
                self.manifests.clone(),
                self.components,
                self.durable_schemas,
                self.entry_triggers,
                self.process_arguments,
                self.contributions,
                self.layer_policies,
                &self.component_authority,
            )?;
        let mut kernel = create_kernel(&resolved)?;
        if debug_enabled {
            kernel.set_runtime_trace_sink(debug_runtime_trace_sink());
        }
        kernel.activate_resolved_harness(&resolved)?;
        for (plugin, factory) in self.embedded_factories {
            kernel.register_embedded_factory(plugin, move || factory())?;
        }
        let reconciler = GraphReconciler::new(resolved);
        Ok(PhenixHarness {
            kernel,
            reconciler,
            application_agent_tools,
        })
    }
}

pub struct PhenixHarness {
    kernel: Kernel,
    reconciler: GraphReconciler,
    application_agent_tools: application::ApplicationAgentToolRegistry,
}

impl PhenixHarness {
    pub fn kernel(&self) -> &Kernel {
        &self.kernel
    }

    pub fn kernel_mut(&mut self) -> &mut Kernel {
        &mut self.kernel
    }

    pub fn component_graph(&self) -> &phenix_core::ResolvedComponentGraph {
        self.reconciler.active().component_graph()
    }

    pub fn resolved_harness(&self) -> &ResolvedHarness {
        self.reconciler.active()
    }

    pub fn resolved_harness_in_generation(
        &self,
        generation: &GraphGenerationId,
    ) -> Result<&ResolvedHarness, KernelError> {
        if self.reconciler.active().generation() == generation {
            return Ok(self.reconciler.active());
        }
        self.reconciler
            .resident(generation)
            .ok_or_else(|| KernelError::UnknownGeneration(generation.clone()))
    }

    pub fn generation(&self) -> &GraphGenerationId {
        self.reconciler.active().generation()
    }

    pub fn selectable_generations(&self) -> Vec<GraphGenerationId> {
        self.kernel.resident_generation_ids()
    }

    pub fn capture_root_execution_constraints(
        &self,
        caller_authority: &Authority,
        pinned_bindings: impl IntoIterator<Item = (ComponentId, InterfaceId)>,
    ) -> Result<RootExecutionConstraints, KernelError> {
        self.kernel
            .capture_root_execution_constraints(caller_authority, pinned_bindings)
    }

    pub fn root_execution_handle(&self, caller_authority: &Authority) -> RootExecutionHandle {
        self.kernel.root_execution_handle(caller_authority)
    }

    pub fn root_execution_handle_in_generation(
        &self,
        generation: &GraphGenerationId,
        constraints: &RootExecutionConstraints,
    ) -> Result<RootExecutionHandle, KernelError> {
        self.kernel
            .root_execution_handle_in_generation(generation, constraints)
    }

    pub fn build_plugin_artifact(
        &self,
        plan: PluginBuildPlan,
        context: &mut PluginManagementContext<'_>,
    ) -> Result<PluginBuildReport, PluginManagementError> {
        GraphReconciler::build_artifact(plan, context)
    }

    pub fn trial_plugin_management(
        &mut self,
        request: PluginManagementRequest,
        authority_ceiling: &Authority,
        constraints: &RootExecutionConstraints,
        context: &mut PluginManagementContext<'_>,
    ) -> Result<PluginTrialResult, PluginManagementError> {
        self.reconciler.trial_management(
            &mut self.kernel,
            request,
            authority_ceiling,
            constraints,
            context,
        )
    }

    pub fn make_candidate_resident(
        &mut self,
        candidate: ResolvedHarness,
        constraints: &RootExecutionConstraints,
    ) -> Result<GraphGenerationId, LiveReconciliationError> {
        self.reconciler
            .make_candidate_resident_on_kernel(&mut self.kernel, candidate, constraints)
    }

    pub fn promote_resident(
        &mut self,
        generation: &GraphGenerationId,
        constraints: &RootExecutionConstraints,
    ) -> Result<ReconciliationResult, LiveReconciliationError> {
        self.reconciler
            .promote_resident_on_kernel(&mut self.kernel, generation, constraints)
    }

    pub fn retire_resident(
        &mut self,
        generation: &GraphGenerationId,
        constraints: &RootExecutionConstraints,
    ) -> Result<(), LiveReconciliationError> {
        self.reconciler
            .retire_resident_on_kernel(&mut self.kernel, generation, constraints)
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
        Self {
            kernel,
            reconciler: GraphReconciler::new(resolved),
            application_agent_tools: application::ApplicationAgentToolRegistry::default(),
        }
    }

    pub(crate) fn application_agent_tools(&self) -> &application::ApplicationAgentToolRegistry {
        &self.application_agent_tools
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

    pub fn invoke_in_generation(
        &mut self,
        generation: &GraphGenerationId,
        service: &phenix_core::ServiceId,
        input: &[u8],
        constraints: &RootExecutionConstraints,
        binding: Option<&PluginId>,
    ) -> Result<Vec<u8>, KernelError> {
        self.kernel
            .invoke_in_generation(generation, service, input, constraints, binding)
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
        artifact_manifest, artifact_service, context_manifest, context_service,
        efficiency_evaluation_service, memory_service, planning_manifest, planning_service,
        repository_work_queue_service, sdk_contribution, session_manifest, session_service,
        ArtifactCommand, ArtifactProvenance, ArtifactResponse, ContextCommand, ContextDescriptor,
        ContextResourceKind, ContextResponse, ContextScope, EfficiencyCollectionRequest,
        EfficiencyEvaluationCommand, PlanningCommand, PlanningResponse, RepositoryWorkSnapshot,
        SessionCommand, SessionResponse, ADVANCED_AGENT_CONFIGURATION, BASIC_AGENT_CONFIGURATION,
        BASIC_PRODUCT_CONFIGURATION, FULL_PRODUCT_CONFIGURATION,
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
    fn advanced_agent_configuration_extends_basic_through_dependency_resolution() {
        let basic = HarnessBuilder::with_selected_suite(&BTreeSet::from([
            BASIC_AGENT_CONFIGURATION.to_owned(),
        ]))
        .unwrap();
        let basic_ids = basic
            .manifests
            .iter()
            .map(|manifest| manifest.id.as_str())
            .collect::<BTreeSet<_>>();
        for required in [
            BASIC_AGENT_CONFIGURATION,
            "phenix.agent-loop",
            "phenix.context",
            "phenix.execution",
            "phenix.harness.invocation-defaults",
            "phenix.models",
            "phenix.sessions",
            "phenix.step-runner",
        ] {
            assert!(
                basic_ids.contains(required),
                "basic configuration missed {required}"
            );
        }
        for optional in ["phenix.options", "phenix.memory", "phenix.planning"] {
            assert!(
                !basic_ids.contains(optional),
                "basic configuration unexpectedly included {optional}"
            );
        }
        basic.build().unwrap();

        let advanced = HarnessBuilder::with_selected_suite(&BTreeSet::from([
            ADVANCED_AGENT_CONFIGURATION.to_owned(),
        ]))
        .unwrap();
        let advanced_ids = advanced
            .manifests
            .iter()
            .map(|manifest| manifest.id.as_str())
            .collect::<BTreeSet<_>>();
        for required in [
            ADVANCED_AGENT_CONFIGURATION,
            BASIC_AGENT_CONFIGURATION,
            "phenix.agent-loop",
            "phenix.options",
            "phenix.memory",
            "phenix.planning",
            "phenix.repository-workers",
            "phenix.session-tree",
            "phenix.language",
            "phenix.hooks",
            "phenix.debug",
        ] {
            assert!(
                advanced_ids.contains(required),
                "advanced configuration missed {required}"
            );
        }
        advanced.build().unwrap();
    }

    #[test]
    fn product_configurations_resolve_providers_and_frontend_sdk() {
        for root in [BASIC_PRODUCT_CONFIGURATION, FULL_PRODUCT_CONFIGURATION] {
            let builder =
                HarnessBuilder::with_selected_suite(&BTreeSet::from([root.to_owned()])).unwrap();
            let ids = builder
                .manifests
                .iter()
                .map(|manifest| manifest.id.as_str())
                .collect::<BTreeSet<_>>();

            for required in [
                root,
                "phenix.agent.basic",
                "phenix.api",
                "phenix.options",
                "phenix.providers",
                "openai-api",
                "openai-codex",
                "phenix.sessions",
            ] {
                assert!(ids.contains(required), "{root} missed {required}");
            }

            let harness = builder.build().unwrap();
            harness
                .resolved_harness()
                .resolve_sdk_contributions([sdk_contribution()])
                .unwrap();
        }
    }

    #[test]
    fn full_product_exposes_model_entry_triggers_from_its_resolved_composition() {
        let builder = HarnessBuilder::with_selected_suite(&BTreeSet::from([
            FULL_PRODUCT_CONFIGURATION.to_owned(),
        ]))
        .unwrap();

        let callables = builder
            .entry_triggers
            .iter()
            .filter_map(|trigger| match &trigger.trigger {
                phenix_core::EntryTriggerKind::ToolCall { callable_id, .. } => {
                    Some(callable_id.as_str())
                }
            })
            .collect::<BTreeSet<_>>();

        for required in [
            "bash",
            "workspace.read",
            "workspace.search",
            "workspace.write",
            "workspace.git",
            "code.query",
            "memory.record",
            "memory.recall",
        ] {
            assert!(
                callables.contains(required),
                "full product missed {required}"
            );
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
    fn harness_can_exercise_resident_generation_before_promotion() {
        let service = service();
        let active_plugin = plugin("fixture.active");
        let trial_plugin = plugin("fixture.trial");
        let mut builder = HarnessBuilder::new();
        builder
            .add_embedded(
                service_manifest(
                    active_plugin.as_str(),
                    service.clone(),
                    100,
                    Authority::default(),
                ),
                || Box::new(Echo(b"active")),
            )
            .unwrap();
        let mut harness = builder.build().unwrap();
        harness.activate().unwrap();

        let active_generation = harness.generation().clone();
        harness
            .kernel_mut()
            .preload_embedded_factory(trial_plugin.clone(), || Box::new(Echo(b"trial")));
        let candidate = ResolvedHarness::resolve(
            [service_manifest(
                trial_plugin.as_str(),
                service.clone(),
                100,
                Authority::default(),
            )],
            [],
            [],
            &Authority::default(),
        )
        .unwrap();
        let trial_generation = candidate.generation().clone();
        let constraints = harness
            .capture_root_execution_constraints(&Authority::default(), [])
            .unwrap();

        harness
            .make_candidate_resident(candidate, &constraints)
            .unwrap();

        assert_eq!(harness.generation(), &active_generation);
        assert_eq!(
            harness
                .invoke(&service, b"input", &Authority::default(), None)
                .unwrap(),
            b"active"
        );
        assert_eq!(
            harness
                .invoke_in_generation(&trial_generation, &service, b"input", &constraints, None,)
                .unwrap(),
            b"trial"
        );

        harness
            .promote_resident(&trial_generation, &constraints)
            .unwrap();

        assert_eq!(harness.generation(), &trial_generation);
        assert_eq!(
            harness
                .invoke(&service, b"input", &Authority::default(), None)
                .unwrap(),
            b"trial"
        );
        assert!(harness
            .selectable_generations()
            .contains(&active_generation));
        assert_eq!(
            harness
                .invoke_in_generation(&active_generation, &service, b"input", &constraints, None,)
                .unwrap(),
            b"active"
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
    fn efficiency_collection_requires_terminal_outcome_provider() {
        let mut harness = HarnessBuilder::with_default_suite()
            .unwrap()
            .build()
            .unwrap();
        harness.activate().unwrap();

        let command = EfficiencyEvaluationCommand::CollectTask {
            request: EfficiencyCollectionRequest {
                task_fixture_revision: "fixture-1".into(),
                root_execution_id: "root-without-outcome-provider".into(),
                policy_revision: "policy-1".into(),
                outcome_evaluator_identity: "fixture.tests".into(),
                price_revision: "prices-1".into(),
            },
        };
        let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
        let error = harness
            .invoke(
                &efficiency_evaluation_service(),
                &input,
                &default_suite_authority(),
                None,
            )
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("terminal outcome evidence unavailable")
                || error.contains("unresolved")
                || error.contains("provider"),
            "unexpected evaluation error: {error}"
        );
    }

    #[test]
    fn benchmark_outcomes_are_opt_in_but_selectable() {
        let benchmark = benchmark_outcome_manifest().id.as_str().to_owned();
        let default = HarnessBuilder::with_default_suite().unwrap();
        assert!(!default
            .manifests
            .iter()
            .any(|manifest| manifest.id.as_str() == benchmark));

        let selected =
            HarnessBuilder::with_selected_suite(&BTreeSet::from([benchmark.clone()])).unwrap();
        assert!(selected
            .manifests
            .iter()
            .any(|manifest| manifest.id.as_str() == benchmark));
        assert!(selected
            .components
            .iter()
            .any(|component| component.owner.as_str() == benchmark));
    }

    #[test]
    fn selected_efficiency_evaluation_pulls_in_execution_source() {
        let evaluation = efficiency_evaluation_manifest().id.as_str().to_owned();
        let execution = execution_manifest(default_suite_authority())
            .id
            .as_str()
            .to_owned();
        let builder = HarnessBuilder::with_selected_suite(&BTreeSet::from([evaluation])).unwrap();

        assert!(builder
            .manifests
            .iter()
            .any(|manifest| manifest.id.as_str() == execution));
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
            session: phenix_plugin_catalog::SessionRecord::new(
                SessionId::parse("session-1").unwrap(),
            ),
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
        builder.set_component_authority(session_authority());
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
                .invoke(&session_service(), &input, &session_authority(), None)
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
        builder.set_component_authority(default_suite_authority());
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

#[cfg(test)]
mod exact_selected_suite_tests {
    use super::*;

    #[test]
    fn explicit_workspace_selection_does_not_inject_local_environment() {
        let enabled = BTreeSet::from(["phenix.workspace".to_owned()]);
        let builder = HarnessBuilder::with_selected_suite(&enabled)
            .expect("workspace is a known first-party plugin");
        let selected = builder
            .manifests
            .iter()
            .map(|manifest| manifest.id.as_str())
            .collect::<BTreeSet<_>>();

        assert!(selected.contains("phenix.workspace"));
        assert!(
            !selected.contains("phenix.environment.local"),
            "explicit composition must not choose an Environment implementation"
        );
    }
}
