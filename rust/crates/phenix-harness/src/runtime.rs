use crate::{PhenixRuntimeBuildError, PhenixRuntimeBuilder, application};
use phenix_core::{
    Authority, ComponentId, GenerationId, GraphReconciler, InterfaceId, Kernel, KernelError,
    LiveReconciliationError, PersistenceBackend, PluginBuildPlan, PluginBuildReport, PluginId,
    PluginManagementContext, PluginManagementError, PluginManagementRequest, PluginTrialResult,
    ReconciliationResult, ResolvedGeneration, ResolvedGenerationActivation,
    RootExecutionConstraints, RootExecutionHandle,
};

pub struct PhenixRuntime {
    kernel: Kernel,
    reconciler: GraphReconciler,
    application_agent_tools: application::ApplicationAgentToolRegistry,
}

impl PhenixRuntime {
    pub(crate) fn from_parts(
        kernel: Kernel,
        reconciler: GraphReconciler,
        application_agent_tools: application::ApplicationAgentToolRegistry,
    ) -> Self {
        Self {
            kernel,
            reconciler,
            application_agent_tools,
        }
    }

    pub fn kernel(&self) -> &Kernel {
        &self.kernel
    }

    pub fn kernel_mut(&mut self) -> &mut Kernel {
        &mut self.kernel
    }

    pub fn component_graph(&self) -> &phenix_core::ResolvedComponentGraph {
        self.reconciler.active().component_graph()
    }

    pub fn resolved_generation(&self) -> &ResolvedGeneration {
        self.reconciler.active()
    }

    pub fn resolved_generation_by_id(
        &self,
        generation: &GenerationId,
    ) -> Result<&ResolvedGeneration, KernelError> {
        if self.reconciler.active().generation() == generation {
            return Ok(self.reconciler.active());
        }
        self.reconciler
            .resident(generation)
            .ok_or_else(|| KernelError::UnknownGeneration(generation.clone()))
    }

    pub fn generation(&self) -> &GenerationId {
        self.reconciler.active().generation()
    }

    pub fn selectable_generations(&self) -> Vec<GenerationId> {
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

    /// Opt in to the resolved declarative agent workflow in the active
    /// generation. This does not replace the legacy agent-loop service.
    ///
    /// Both Basic and Advanced use the same topology. The selected generation
    /// supplies the node providers and the kernel enforces pinned imports,
    /// authority and cancellation across every node invocation.
    pub fn run_declared_agent_workflow(
        &self,
        command: phenix_sdk::AgentLoopCommand,
        caller_authority: &Authority,
        cancelled: impl FnMut() -> bool,
        step_limit: Option<std::num::NonZeroU64>,
    ) -> Result<phenix_sdk::AgentLoopResponse, String> {
        let root = self.root_execution_handle(caller_authority);
        phenix_plugin_catalog::run_agent_workflow(&root, command, cancelled, step_limit)
    }

    /// Execute the unchanged selected agent topology over the native pending
    /// Core Invoke scheduler. This is opt-in until Basic/Full parity tests
    /// cover streaming, cancellation and all legacy side effects.
    pub fn run_declared_agent_workflow_pending(
        &self,
        command: phenix_sdk::AgentLoopCommand,
        caller_authority: &Authority,
        cancelled: impl FnMut() -> bool,
        step_limit: Option<std::num::NonZeroU64>,
    ) -> Result<phenix_sdk::AgentLoopResponse, String> {
        let root = self.root_execution_handle(caller_authority);
        phenix_plugin_catalog::run_agent_workflow_pending(
            &root,
            command,
            cancelled,
            step_limit,
        )
    }

    pub fn root_execution_handle_in_generation(
        &self,
        generation: &GenerationId,
        constraints: &RootExecutionConstraints,
    ) -> Result<RootExecutionHandle, KernelError> {
        self.kernel
            .root_execution_handle_in_generation(generation, constraints)
    }

    /// Execute the selected declarative agent workflow in an explicitly
    /// retained generation. The supplied root constraints are captured before
    /// starting the execution; they cannot gain permissions or rebind node
    /// providers through a later promotion of a different graph.
    ///
    /// In particular, an unavailable resident generation must fail instead
    /// of silently switching to the active generation or legacy agent loop.
    pub fn run_declared_agent_workflow_in_generation(
        &self,
        generation: &GenerationId,
        constraints: &RootExecutionConstraints,
        command: phenix_sdk::AgentLoopCommand,
        cancelled: impl FnMut() -> bool,
        step_limit: Option<std::num::NonZeroU64>,
    ) -> Result<phenix_sdk::AgentLoopResponse, String> {
        let root = self
            .root_execution_handle_in_generation(generation, constraints)
            .map_err(|error| error.to_string())?;
        phenix_plugin_catalog::run_agent_workflow(&root, command, cancelled, step_limit)
    }

    /// Pending agent execution stays inside the explicitly pinned resident
    /// generation, including after default-generation promotion.
    pub fn run_declared_agent_workflow_pending_in_generation(
        &self,
        generation: &GenerationId,
        constraints: &RootExecutionConstraints,
        command: phenix_sdk::AgentLoopCommand,
        cancelled: impl FnMut() -> bool,
        step_limit: Option<std::num::NonZeroU64>,
    ) -> Result<phenix_sdk::AgentLoopResponse, String> {
        let root = self
            .root_execution_handle_in_generation(generation, constraints)
            .map_err(|error| error.to_string())?;
        phenix_plugin_catalog::run_agent_workflow_pending(
            &root,
            command,
            cancelled,
            step_limit,
        )
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
        candidate: ResolvedGeneration,
        constraints: &RootExecutionConstraints,
    ) -> Result<GenerationId, LiveReconciliationError> {
        self.reconciler
            .make_candidate_resident_on_kernel(&mut self.kernel, candidate, constraints)
    }

    pub fn promote_resident(
        &mut self,
        generation: &GenerationId,
        constraints: &RootExecutionConstraints,
    ) -> Result<ReconciliationResult, LiveReconciliationError> {
        self.reconciler
            .promote_resident_on_kernel(&mut self.kernel, generation, constraints)
    }

    pub fn retire_resident(
        &mut self,
        generation: &GenerationId,
        constraints: &RootExecutionConstraints,
    ) -> Result<(), LiveReconciliationError> {
        self.reconciler
            .retire_resident_on_kernel(&mut self.kernel, generation, constraints)
    }

    pub fn activate(&mut self) -> Result<(), KernelError> {
        self.kernel.activate_all()
    }

    pub fn kernel_only() -> Self {
        let resolved = ResolvedGeneration::resolve([], [], [], &Authority::default())
            .expect("empty kernel-only composition is valid");
        let mut kernel = Kernel::kernel_only();
        kernel
            .activate_resolved_generation(&resolved)
            .expect("kernel-only resolved generation activates");
        Self {
            kernel,
            reconciler: GraphReconciler::new(resolved),
            application_agent_tools: application::ApplicationAgentToolRegistry::default(),
        }
    }

    pub(crate) fn application_agent_tools(&self) -> &application::ApplicationAgentToolRegistry {
        &self.application_agent_tools
    }

    pub fn default_suite() -> Result<Self, PhenixRuntimeBuildError> {
        PhenixRuntimeBuilder::with_default_suite()?.build()
    }

    pub fn default_suite_with_persistence(
        persistence: impl PersistenceBackend + 'static,
    ) -> Result<Self, PhenixRuntimeBuildError> {
        PhenixRuntimeBuilder::with_default_suite()?.build_with_persistence(persistence)
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
        generation: &GenerationId,
        service: &phenix_core::ServiceId,
        input: &[u8],
        constraints: &RootExecutionConstraints,
        binding: Option<&PluginId>,
    ) -> Result<Vec<u8>, KernelError> {
        self.kernel
            .invoke_in_generation(generation, service, input, constraints, binding)
    }
}
