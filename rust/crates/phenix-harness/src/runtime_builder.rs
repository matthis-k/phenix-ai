//! First-party runtime composition and kernel construction.

use crate::{PhenixRuntime, application, default_suite_authority};
use phenix_core::{
    Authority, ComponentEntryTrigger, ComponentId, ComponentInterface, ComponentManifest,
    ComponentProcessArgument, ConfigContribution, DurableSchemaRegistration,
    GenerationResolutionError, GraphReconciler, InterfaceCompatibility, InterfaceId, Kernel,
    KernelError, LayerPolicy, PersistenceBackend, PluginExecution, PluginId, PluginInstance,
    PluginManifest, ProviderCompositionPolicy, ResolvedGeneration, ResolvedGenerationActivation,
    ResolvedGenerationActivationError, ServiceId,
};
use phenix_plugin_catalog::{
    AGENT_TOPOLOGY_PLUGIN, adapter_acp_factory, adapter_acp_manifest,
    advanced_agent_configuration_manifest, agent_loop_component_manifest, agent_loop_factory,
    agent_loop_manifest, agent_topology_component_manifest, agent_topology_declaration,
    agent_topology_manifest, agent_topology_projections, artifact_component_manifest,
    artifact_factory, artifact_manifest, basic_agent_configuration_manifest,
    basic_agent_nodes_component_manifest, basic_agent_nodes_factory, basic_agent_nodes_manifest,
    basic_context_component_manifest, basic_context_factory, basic_context_manifest,
    basic_model_component_manifest, basic_model_factory, basic_model_manifest,
    basic_product_configuration_manifest, basic_skills_component_manifest, basic_skills_factory,
    basic_skills_manifest, basic_tools_component_manifest, basic_tools_factory,
    basic_tools_manifest, benchmark_outcome_component_manifest, benchmark_outcome_factory,
    benchmark_outcome_manifest, cli_component_manifest, cli_factory, cli_manifest,
    common_provider_definitions, context_component_manifest, context_factory, context_manifest,
    debug_component_manifest, debug_factory, debug_manifest, debug_runtime_trace_sink,
    efficiency_evaluation_component_manifest, efficiency_evaluation_factory,
    efficiency_evaluation_manifest, execution_component_manifest, execution_factory,
    execution_manifest, expand_profile_defaults, first_party_durable_schema_registrations,
    frontend_component_manifest, frontend_factory, frontend_manifest,
    full_product_configuration_manifest, helper_invocation_component_manifest,
    hook_component_manifest, hook_factory, hook_manifest, interactive_ui_component_manifest,
    interactive_ui_factory, interactive_ui_manifest, job_component_manifest, job_factory,
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
    workspace_manifest,
};
use phenix_plugin_invocation_defaults as invocation_defaults;
use phenix_sdk::{LanguageInterface, MemoryInterface, WorkspaceInterface};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt::{self, Display, Formatter},
    sync::Arc,
};

type EmbeddedFactory = Arc<dyn Fn() -> Box<dyn PluginInstance> + Send + Sync>;

#[derive(Debug)]
pub enum PhenixRuntimeBuildError {
    Kernel(KernelError),
    Resolution(GenerationResolutionError),
    Activation(ResolvedGenerationActivationError),
    Persistence(phenix_core::PersistenceCandidateError),
}

impl Display for PhenixRuntimeBuildError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Kernel(error) => Display::fmt(error, f),
            Self::Resolution(error) => Display::fmt(error, f),
            Self::Activation(error) => {
                write!(f, "resolved generation activation failed: {error:?}")
            }
            Self::Persistence(error) => Display::fmt(error, f),
        }
    }
}

impl Error for PhenixRuntimeBuildError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Kernel(error) => Some(error),
            Self::Resolution(error) => Some(error),
            Self::Activation(_) => None,
            Self::Persistence(error) => Some(error),
        }
    }
}

impl From<KernelError> for PhenixRuntimeBuildError {
    fn from(error: KernelError) -> Self {
        Self::Kernel(error)
    }
}

impl From<GenerationResolutionError> for PhenixRuntimeBuildError {
    fn from(error: GenerationResolutionError) -> Self {
        Self::Resolution(error)
    }
}

impl From<ResolvedGenerationActivationError> for PhenixRuntimeBuildError {
    fn from(error: ResolvedGenerationActivationError) -> Self {
        Self::Activation(error)
    }
}

impl From<phenix_core::PersistenceCandidateError> for PhenixRuntimeBuildError {
    fn from(error: phenix_core::PersistenceCandidateError) -> Self {
        Self::Persistence(error)
    }
}

#[derive(Default)]
pub struct PhenixRuntimeBuilder {
    pub(crate) manifests: Vec<PluginManifest>,
    durable_schemas: Vec<DurableSchemaRegistration>,
    embedded_factories: BTreeMap<PluginId, EmbeddedFactory>,
    layer_policies: BTreeMap<ServiceId, Vec<LayerPolicy>>,
    provider_policy: ProviderCompositionPolicy,
    pub(crate) components: Vec<ComponentManifest>,
    pub(crate) entry_triggers: Vec<ComponentEntryTrigger>,
    pub(crate) workflows: Vec<phenix_core::WorkflowDeclaration>,
    workflow_frame_schemas: Vec<phenix_core::WorkflowFrameDeclaration>,
    workflow_projections: Vec<phenix_core::WorkflowProjectionDeclaration>,
    process_arguments: Vec<ComponentProcessArgument>,
    contributions: Vec<ConfigContribution>,
    component_authority: Authority,
    application_agent_tools: application::ApplicationAgentToolRegistry,
}

impl PhenixRuntimeBuilder {
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
        builder.add_embedded(interactive_ui_manifest(), interactive_ui_factory)?;
        builder.add_embedded(debug_manifest(authority.clone()), debug_factory)?;
        builder.add_embedded(options_manifest(), options_factory)?;
        builder.add_embedded(
            invocation_defaults::invocation_defaults_manifest(authority.clone()),
            invocation_defaults::invocation_defaults_factory,
        )?;
        builder.add_embedded(sdk_manifest(authority.clone()), sdk_factory)?;
        builder.add_embedded(basic_skills_manifest(), basic_skills_factory)?;
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
            interactive_ui_component_manifest(),
            debug_component_manifest(authority.clone()),
            options_component_manifest(),
            invocation_defaults::invocation_defaults_component_manifest(authority.clone()),
            sdk_component_manifest(authority),
            basic_skills_component_manifest(),
        ] {
            builder.add_component(component);
        }
        for provider in provider_definitions {
            builder.add_component(provider.component_manifest());
        }
        Ok(builder)
    }

    pub fn with_selected_suite(enabled: &BTreeSet<String>) -> Result<Self, String> {
        Self::with_selected_suite_excluding(enabled, &BTreeSet::new())
    }

    /// Expand named Phenix product defaults before enforcing actual hard plugin
    /// dependencies. Explicit exclusions affect defaults, not contract resolution:
    /// a concrete implementation that truly requires a disabled plugin still fails.
    pub fn with_selected_suite_excluding(
        enabled: &BTreeSet<String>,
        excluded: &BTreeSet<String>,
    ) -> Result<Self, String> {
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
            basic_agent_nodes_manifest(authority.clone()),
            agent_topology_manifest(authority.clone()),
            application::application_agent_tool_manifest(authority.clone()),
            language_manifest(),
            memory_manifest(),
            planning_manifest(),
            local_environment_manifest(),
            workspace_manifest(),
            model_routing_manifest(authority.clone()),
            step_runner_manifest(authority.clone()),
            job_manifest(),
            frontend_manifest(authority.clone()),
            interactive_ui_manifest(),
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
        let mut enabled = expand_profile_defaults(enabled, excluded);
        let unknown = enabled
            .iter()
            .chain(excluded.iter())
            .filter(|id| !available.contains_key(*id))
            .cloned()
            .collect::<Vec<_>>();
        if !unknown.is_empty() {
            return Err(format!(
                "unknown first-party plugin id(s): {}",
                unknown.join(", ")
            ));
        }

        // Default profiles have already been expanded; only real manifest
        // dependencies can enlarge this selection now.
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
                    if excluded.contains(&dependency) {
                        return Err(format!(
                            "first-party plugin {plugin} requires disabled first-party plugin {dependency}"
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
        if enabled.contains(AGENT_TOPOLOGY_PLUGIN) {
            builder.add_manifest(agent_topology_manifest(authority.clone()));
            builder.add_workflow(agent_topology_declaration());
            for projection in agent_topology_projections() {
                builder.add_workflow_projection(projection);
            }
        }
        if enabled.contains(application::APPLICATION_AGENT_TOOL_PLUGIN) {
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
        builder.add_selected(&enabled, interactive_ui_manifest(), interactive_ui_factory)?;
        builder.add_selected(&enabled, hook_manifest(authority.clone()), hook_factory)?;
        builder.add_selected(&enabled, debug_manifest(authority.clone()), debug_factory)?;
        builder.add_selected(&enabled, options_manifest(), options_factory)?;
        builder.add_selected(
            &enabled,
            invocation_defaults::invocation_defaults_manifest(authority.clone()),
            invocation_defaults::invocation_defaults_factory,
        )?;
        builder.add_selected(&enabled, sdk_manifest(authority.clone()), sdk_factory)?;
        builder.add_selected(
            &enabled,
            basic_agent_nodes_manifest(authority.clone()),
            basic_agent_nodes_factory,
        )?;
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
            basic_agent_nodes_component_manifest(authority.clone()),
            agent_topology_component_manifest(authority.clone()),
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
            interactive_ui_component_manifest(),
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
        if enabled.contains(application::APPLICATION_AGENT_TOOL_PLUGIN) {
            builder.add_component(application::application_agent_tool_component_manifest(
                authority,
            ));
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

    /// Register a declarative workflow owned by a component in this selection.
    pub fn add_workflow(&mut self, declaration: phenix_core::WorkflowDeclaration) {
        self.workflows.push(declaration);
    }

    /// Register the typed data contract for a selected workflow.
    /// The resolver validates all frame slots and transitions before activation.
    pub fn add_workflow_frame_schema(
        &mut self,
        declaration: phenix_core::WorkflowFrameDeclaration,
    ) {
        self.workflow_frame_schemas.push(declaration);
    }

    pub fn add_workflow_projection(
        &mut self,
        declaration: phenix_core::WorkflowProjectionDeclaration,
    ) {
        self.workflow_projections.push(declaration);
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

    /// Set the provider-selection policy for this Phenix runtime.
    ///
    /// This is independent of whether configuration came from Nix, a file, a
    /// frontend or direct embedding. Resolution remains kernel-owned.
    pub fn set_provider_policy(&mut self, policy: ProviderCompositionPolicy) {
        self.provider_policy = policy;
    }

    /// Select a provider implementation for a contract in this runtime.
    ///
    /// The provider must still be available, compatible and authorized when
    /// the graph generation is resolved. An explicit binding re-enables a provider
    /// excluded by an earlier configuration layer.
    pub fn bind_provider(&mut self, interface: InterfaceId, provider: ComponentId) {
        self.provider_policy = std::mem::take(&mut self.provider_policy)
            .with_enabled_provider(interface.clone(), provider.clone())
            .with_explicit_binding(interface, provider);
    }

    /// Exclude one provider from selection for an interface.
    pub fn disable_provider(&mut self, interface: InterfaceId, provider: ComponentId) {
        self.provider_policy =
            std::mem::take(&mut self.provider_policy).with_disabled_provider(interface, provider);
    }

    /// Add application tools for providers contributed after profile selection.
    ///
    /// The final assembled component set, rather than an implementation plugin
    /// ID, determines whether a tool contract is available to the application.
    fn add_late_application_tool_triggers(&mut self) {
        if !self
            .components
            .iter()
            .any(|component| component.owner.as_str() == application::APPLICATION_AGENT_TOOL_PLUGIN)
        {
            return;
        }
        if self.has_compatible_export::<WorkspaceInterface>() {
            self.add_missing_triggers(application::application_workspace_tool_triggers());
        }
        if self.has_compatible_export::<LanguageInterface>() {
            self.add_missing_triggers(application::application_code_tool_triggers());
        }
        if self.has_compatible_export::<MemoryInterface>() {
            self.add_missing_triggers(application::application_memory_tool_triggers());
        }
    }

    fn has_compatible_export<I: ComponentInterface>(&self) -> bool {
        let interface = I::interface_id();
        let schema = I::schema();
        self.components.iter().any(|component| {
            self.provider_policy
                .provider_enabled(&interface, &component.id)
                && component.exports.iter().any(|export| {
                    export.interface == interface
                        && !matches!(
                            schema.accepts_provider(&export.schema),
                            InterfaceCompatibility::Incompatible(_)
                        )
                        && component
                            .maximum_authority
                            .permits_all(&export.required_authority)
                })
        })
    }

    fn add_missing_triggers(&mut self, triggers: Vec<ComponentEntryTrigger>) {
        for trigger in triggers {
            if !self.entry_triggers.iter().any(|existing| {
                existing.component == trigger.component && existing.interface == trigger.interface
            }) {
                self.entry_triggers.push(trigger);
            }
        }
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

    #[cfg(test)]
    pub(crate) fn replace_embedded_factory<F>(&mut self, plugin: PluginId, factory: F) -> bool
    where
        F: Fn() -> Box<dyn PluginInstance> + Send + Sync + 'static,
    {
        self.embedded_factories
            .insert(plugin, Arc::new(factory))
            .is_some()
    }

    pub fn build(self) -> Result<PhenixRuntime, PhenixRuntimeBuildError> {
        self.build_using(|resolved| Ok(Kernel::new(resolved.kernel_config().clone())))
    }

    pub fn build_with_persistence(
        self,
        persistence: impl PersistenceBackend + 'static,
    ) -> Result<PhenixRuntime, PhenixRuntimeBuildError> {
        self.build_using(|resolved| {
            Ok(Kernel::with_persistence(
                resolved.kernel_config().clone(),
                persistence,
            ))
        })
    }

    pub(crate) fn build_using(
        mut self,
        create_kernel: impl FnOnce(&ResolvedGeneration) -> Result<Kernel, PhenixRuntimeBuildError>,
    ) -> Result<PhenixRuntime, PhenixRuntimeBuildError> {
        self.add_late_application_tool_triggers();
        let application_agent_tools = self.application_agent_tools.clone();
        let debug_id = debug_manifest(self.component_authority.clone()).id;
        let debug_enabled = self
            .manifests
            .iter()
            .any(|manifest| manifest.id == debug_id);
        let resolved = ResolvedGeneration::resolve_with_composition_policies(
            self.manifests.clone(),
            self.components,
            self.durable_schemas,
            self.entry_triggers,
            self.process_arguments,
            self.contributions,
            self.layer_policies,
            self.provider_policy,
            &self.component_authority,
        )?
        .with_workflows(self.workflows)?
        .with_workflow_frame_schemas(self.workflow_frame_schemas)?
        .with_workflow_projections(self.workflow_projections)?;
        let mut kernel = create_kernel(&resolved)?;
        if debug_enabled {
            kernel.set_runtime_trace_sink(debug_runtime_trace_sink());
        }
        kernel.activate_resolved_generation(&resolved)?;
        for (plugin, factory) in self.embedded_factories {
            kernel.register_embedded_factory(plugin, move || factory())?;
        }
        let reconciler = GraphReconciler::new(resolved);
        Ok(PhenixRuntime::from_parts(
            kernel,
            reconciler,
            application_agent_tools,
        ))
    }
}

#[cfg(test)]
mod workflow_frame_selection_tests {
    use super::*;
    use phenix_core::{
        ComponentExport, ComponentImport, InterfaceSchema, Key, PluginHost, Type,
        WorkflowEdge, WorkflowFrameDeclaration, WorkflowFrameSchema, WorkflowNode,
        WorkflowTopology, WorkflowDeclaration, WorkflowOutcomeProjection,
        WorkflowProjectionDeclaration, WorkflowProjectionSelector,
        WORKFLOW_PROJECTION_REVISION,
    };

    struct NoopProvider;

    impl PluginInstance for NoopProvider {
        fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
            Ok(())
        }
    }

    fn builder_with_typed_workflow() -> PhenixRuntimeBuilder {
        let owner = ComponentId::parse("fixture.builder-topology").unwrap();
        let provider_id = PluginId::parse("fixture.builder-provider").unwrap();
        let service = InterfaceId::parse("fixture.builder-service@1").unwrap();
        let contract = InterfaceSchema::new(Type::Unit, Type::String);
        let mut builder = PhenixRuntimeBuilder::new();
        builder.add_manifest(PluginManifest {
            id: PluginId::parse("fixture.builder-topology").unwrap(),
            version: 1,
            execution: PluginExecution::ResourceOnly,
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        });
        builder
            .add_embedded(
                PluginManifest {
                    id: provider_id.clone(),
                    version: 1,
                    execution: PluginExecution::Embedded,
                    dependencies: Vec::new(),
                    services: Vec::new(),
                    resource_namespaces: Vec::new(),
                    maximum_authority: Authority::default(),
                },
                || Box::new(NoopProvider),
            )
            .unwrap();
        builder.add_component(ComponentManifest {
            id: owner.clone(),
            owner: PluginId::parse("fixture.builder-topology").unwrap(),
            imports: vec![ComponentImport {
                interface: service.clone(),
                schema: contract.clone(),
                required: true,
                authority: Authority::default(),
            }],
            exports: Vec::new(),
            listeners: Vec::new(),
            maximum_authority: Authority::default(),
        });
        builder.add_component(ComponentManifest {
            id: ComponentId::parse("fixture.builder-provider").unwrap(),
            owner: provider_id,
            imports: Vec::new(),
            exports: vec![ComponentExport {
                interface: service.clone(),
                schema: contract,
                priority: 1,
                required_authority: Authority::default(),
            }],
            listeners: Vec::new(),
            maximum_authority: Authority::default(),
        });
        builder.add_workflow(WorkflowDeclaration {
            owner: owner.clone(),
            name: "selected".into(),
            topology: WorkflowTopology {
                entry: "node".into(),
                nodes: BTreeMap::from([(
                    "node".into(),
                    WorkflowNode {
                        import: service,
                        branches: BTreeMap::from([(
                            "done".into(),
                            WorkflowEdge::Finish,
                        )]),
                    },
                )]),
            },
        });
        builder.add_workflow_projection(WorkflowProjectionDeclaration {
            owner,
            workflow: "selected".into(),
            node: "node".into(),
            projection: WorkflowOutcomeProjection {
                revision: WORKFLOW_PROJECTION_REVISION,
                selector: WorkflowProjectionSelector::DirectString,
                cases: BTreeMap::from([("done".into(), "done".into())]),
            },
        });
        builder
    }

    #[test]
    fn builder_selects_typed_workflow_frame_before_generation_activation() {
        let mut builder = builder_with_typed_workflow();
        let owner = ComponentId::parse("fixture.builder-topology").unwrap();
        let counter = Key::parse("counter").unwrap();
        let schema = WorkflowFrameSchema {
            revision: 1,
            slots: BTreeMap::from([(counter, Type::U64)]),
        };
        builder.add_workflow_frame_schema(WorkflowFrameDeclaration {
            owner: owner.clone(),
            name: "selected".into(),
            schema: schema.clone(),
        });
        let runtime = builder.build().unwrap();
        let compiled = runtime.resolved_generation()
            .generation_topology()
            .workflow(&owner, "selected")
            .unwrap();
        assert_eq!(compiled.frame_schema(), Some(&schema));
        assert!(compiled.outcome_projection("node").is_some());
    }

    #[test]
    fn builder_rejects_undeclared_frame_workflow_during_resolution() {
        let mut builder = builder_with_typed_workflow();
        builder.add_workflow_frame_schema(WorkflowFrameDeclaration {
            owner: ComponentId::parse("fixture.builder-topology").unwrap(),
            name: "unselected".into(),
            schema: WorkflowFrameSchema {
                revision: 1,
                slots: BTreeMap::new(),
            },
        });
        assert!(matches!(
            builder.build(),
            Err(PhenixRuntimeBuildError::Resolution(
                GenerationResolutionError::MissingFrameWorkflow { .. }
            ))
        ));
    }
}
