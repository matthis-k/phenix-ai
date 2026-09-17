#![forbid(unsafe_code)]

use phenix_core::{DurableSchemaRegistration, PluginId, PluginManifest};
use phenix_sdk::StaticPluginResources;

pub use phenix_adapter_acp::{adapter_acp_factory, adapter_acp_manifest, ACP_ADAPTER_PLUGIN};
pub use phenix_core::{ContextResourceId, ContextRevisionId, SessionId};
pub use phenix_plugin_api::{
    sdk_component_id, sdk_component_manifest, sdk_config_service, sdk_contribution, sdk_factory,
    sdk_manifest, sdk_session_service, sdk_skills_service, sdk_tools_service, SdkConfigCommand,
    SdkConfigInterface, SdkConfigResponse, SdkSessionCommand, SdkSessionInterface,
    SdkSessionResponse, SdkSkill, SdkSkillCommand, SdkSkillResponse, SdkSkillSummary,
    SdkSkillsInterface, SdkTool, SdkToolCommand, SdkToolResponse, SdkToolsInterface, SDK_COMPONENT,
    SDK_CONFIG_SERVICE, SDK_PLUGIN, SDK_SESSION_SERVICE, SDK_SKILLS_SERVICE, SDK_TOOLS_SERVICE,
};
pub use phenix_plugin_artifacts::{
    artifact_component_id, artifact_component_manifest, artifact_factory, artifact_manifest,
    artifact_service, ArtifactCommand, ArtifactInterface, ArtifactProvenance, ArtifactRecord,
    ArtifactResponse, NormalizedReadRequest, ReadProviderIdentity, ReadResultRecord,
    RevalidationRecord, RevalidationVerdict, ARTIFACT_SERVICE,
};
pub use phenix_plugin_basic_agent::{
    basic_context_component_manifest, basic_context_factory, basic_context_manifest,
    basic_durable_schema_registrations, basic_model_component_manifest, basic_model_factory,
    basic_model_manifest, basic_skills_component_manifest, basic_skills_factory,
    basic_skills_manifest, basic_tools_component_manifest, basic_tools_factory,
    basic_tools_manifest, BasicContextInterface, BasicSkillsInterface, BasicToolsInterface,
    BASIC_CONTEXT_COMPONENT, BASIC_CONTEXT_PLUGIN, BASIC_MODEL_COMPONENT, BASIC_MODEL_PLUGIN,
    BASIC_SKILLS_COMPONENT, BASIC_SKILLS_PLUGIN, BASIC_TOOLS_COMPONENT, BASIC_TOOLS_PLUGIN,
};
pub use phenix_plugin_command_toolbelt::{
    cli_auth_state_service, cli_component_id, cli_component_manifest, cli_discover_service,
    cli_factory, cli_manifest, cli_version_service, CliAuthState, CliAuthStateInterface,
    CliAvailability, CliDescriptor, CliDiscoverInterface, CliName, CliProbeRequest,
    CliVersionInterface, CLI_AUTH_STATE_SERVICE, CLI_DISCOVER_SERVICE, CLI_VERSION_SERVICE,
};
pub use phenix_plugin_context::{
    context_component_id, context_component_manifest, context_factory, context_manifest,
    ContextInjection, ContextInjectionLifetime, ContextInjectionRequester, ContextResourceKind,
    ContextScope, ExactContextReference, ExecutionContextProjection, ProjectedContextEntry,
};
pub use phenix_plugin_debug::{
    debug_component_id, debug_component_manifest, debug_factory, debug_manifest, debug_service,
    DebugCommand, DebugInterface, DebugResponse, DiagnosticEntry, DiagnosticSnapshot,
    DEBUG_SERVICE,
};
pub use phenix_plugin_execution::{
    agent_loop_component_id, agent_loop_component_manifest, agent_loop_service,
    execution_component_id, execution_component_manifest, execution_configuration_service,
    execution_factory, execution_manifest, execution_resource_service, execution_review_service,
    step_attempt_service, AgentDefinition, AgentLoopCommand, AgentLoopInterface, AgentLoopPolicy,
    AgentLoopResponse, AgentLoopUsage, CallablePolicy, ExecutionConfigurationCommand,
    ExecutionConfigurationResponse, ExecutionResourceCommand, ExecutionResourceInterface,
    ExecutionResourceResponse, ExecutionReviewCommand, ExecutionReviewInterface,
    ExecutionReviewResponse, OrchestrationDefinition, OrchestrationNode, PreparedReviewFile,
    StepAttemptCommand, StepAttemptInterface, StepAttemptPhase, StepAttemptRecord,
    StepAttemptResponse, AGENT_LOOP_SERVICE, DEFAULT_MAX_PARALLEL_TOOL_CALLS,
    EXECUTION_CONFIGURATION_SERVICE, EXECUTION_RESOURCE_SERVICE, EXECUTION_REVIEW_SERVICE,
    STEP_ATTEMPT_SERVICE,
};
pub use phenix_plugin_frontend::{
    frontend_component_id, frontend_component_manifest, frontend_factory, frontend_manifest,
    frontend_service, FrontendCommand, FrontendInterface, FrontendProviderDescriptor,
    FrontendResponse, FrontendServiceRequest, FrontendServiceResult, LiveFrontendProvider,
    FRONTEND_SERVICE,
};
pub use phenix_plugin_hooks::{
    hook_component_id, hook_component_manifest, hook_factory, hook_manifest, hook_service,
    HookAction, HookCommand, HookConfiguration, HookDefinition, HookDispatch, HookFailurePolicy,
    HookInterface, HookResponse, HookWarning, LifecycleEvent, HOOK_SERVICE,
};
pub use phenix_plugin_jobs::{
    job_component_id, job_component_manifest, job_factory, job_manifest, job_service,
};
pub use phenix_plugin_language::{
    language_component_id, language_component_manifest, language_factory, language_manifest,
    language_service, DocumentProvenance, LanguageCommand, LanguageDocumentIdentity,
    LanguageInterface, LanguageObservation, LanguageOperationKind, LanguageOperationResult,
    LanguageProviderEpoch, LanguageResponse, LANGUAGE_SERVICE,
};
pub use phenix_plugin_memory::{
    memory_component_id, memory_component_manifest, memory_factory, memory_manifest,
    memory_service, MemoryCommand, MemoryInterface, MemoryKind, MemoryNode, MemoryRecallQuery,
    MemoryRecord, MemoryResponse, MemoryScope, MemorySourceReference, MEMORY_SERVICE,
};
pub use phenix_plugin_models::{
    model_dispatch_service, model_inference_service, model_routing_component_id,
    model_routing_component_manifest, model_routing_factory, model_routing_manifest,
    model_routing_service, ModelCommand, ModelDispatchCommand, ModelDispatchInterface,
    ModelDispatchResponse, ModelInferenceRequest, ModelInferenceResponse, ModelResponse,
    ModelRoutingInterface, ModelTarget, RoutingProfile, RoutingProfileDescriptor,
    MODEL_DISPATCH_SERVICE, MODEL_INFERENCE_SERVICE, MODEL_ROUTING_SERVICE,
};
pub use phenix_plugin_options::{
    default_option_definitions, options_component_id, options_component_manifest, options_factory,
    options_manifest, options_service, OptionAssignment, OptionCommand, OptionContext,
    OptionDefinition, OptionKey, OptionResponse, OptionScope, OptionScopeKind,
    OptionStartupPrecedence, OptionSubjectId, OptionValue, OptionValueLayer, OptionValueSource,
    OptionsInterface, ResolvedOption, OPTIONS_COMPONENT, OPTIONS_PLUGIN, OPTIONS_SERVICE,
};
pub use phenix_plugin_planning::{
    planning_component_id, planning_component_manifest, planning_factory, planning_manifest,
    planning_service,
};
pub use phenix_plugin_providers::{
    common_provider_definitions, providers_manifest, ProviderPreset, COMMON_PROVIDERS,
    PROVIDERS_PLUGIN,
};
pub use phenix_plugin_repository_workers::{
    repository_work_queue_service, repository_worker_component_id,
    repository_worker_component_manifest, repository_worker_factory, repository_worker_manifest,
    ReconstructedPullRequest, RepositoryCheckState, RepositoryChecklistEvidence,
    RepositoryDiscussionEvidence, RepositoryDiscussionKind, RepositoryFinding,
    RepositoryIssueCluster, RepositoryIssueEvidence, RepositoryPullRequestEvidence,
    RepositoryPullRequestState, RepositorySelectionReason, RepositoryValidation,
    RepositoryWorkPriority, RepositoryWorkSelection, RepositoryWorkSnapshot,
    RepositoryWorkerInterface, RepositoryWorkerQueue, REPOSITORY_WORK_QUEUE_SERVICE,
};
pub use phenix_plugin_session_tree::{
    session_tree_component_id, session_tree_component_manifest, session_tree_factory,
    session_tree_manifest, session_tree_service, SessionLineage, SessionTreeCommand,
    SessionTreeInterface, SessionTreeResponse, SESSION_TREE_SERVICE,
};
pub use phenix_plugin_sessions::{
    session_component_manifest, session_factory, session_manifest, session_service, SessionCommand,
    SessionInput, SessionInputKind, SessionInterface, SessionJournalDraft, SessionJournalEntry,
    SessionLifecycle, SessionRecord, SessionResponse, SessionTransition, SESSION_SERVICE,
};
pub use phenix_plugin_step_runner::{
    helper_invocation_component_id, helper_invocation_component_manifest, step_runner_component_id,
    step_runner_component_manifest, step_runner_factory, step_runner_manifest,
    HELPER_INVOCATION_COMPONENT, STEP_RUNNER_COMPONENT, STEP_RUNNER_PLUGIN,
};
pub use phenix_plugin_workspace::{
    workspace_component_id, workspace_component_manifest, workspace_factory, workspace_factory_for,
    workspace_manifest, workspace_service, WorkspaceCommand, WorkspaceFileVersion,
    WorkspaceInterface, WorkspaceResponse, WorkspaceSearchMatch, WORKSPACE_SERVICE,
};
pub use phenix_sdk::{
    context_service, execution_service, step_runner_service, CallableRecord, ContextCommand,
    ContextDescriptor, ContextInterface, ContextResourceRevision, ContextResponse, DecisionRecord,
    ExecutionAuthority, ExecutionCommand, ExecutionInterface, ExecutionRecord, ExecutionResponse,
    ExecutionState, HistoryEntry, HistoryKind, JobCommand, JobInterface, JobResponse,
    ModelInferenceInterface, ObjectiveRecord, PlanRecord, PlanStep, PlannedStepRequest,
    PlanningCommand, PlanningInterface, PlanningResponse, RepositoryContextSource,
    RuntimeResourceKind, RuntimeResourceRecord, RuntimeResourceState, StepRunnerCommand,
    StepRunnerInterface, StepRunnerResponse, StepSettlementBasis, WorkerTaskRecord,
    WorkerTaskState, CONTEXT_SERVICE, EXECUTION_SERVICE, JOB_SERVICE, PLANNING_SERVICE,
    STEP_RUNNER_SERVICE,
};

/// Project generated durable resource metadata for a first-party plugin into the
/// composition plan. Plugins without generated durable resources yield no entries.
#[must_use]
pub fn first_party_durable_schema_registrations(
    manifest: &PluginManifest,
) -> Vec<DurableSchemaRegistration> {
    let owner = &manifest.id;
    if owner == &artifact_manifest().id {
        return registrations::<phenix_plugin_artifacts::Plugin>(owner);
    }
    if owner == &job_manifest().id {
        return registrations::<phenix_plugin_jobs::Plugin>(owner);
    }
    if owner == &options_manifest().id {
        return registrations::<phenix_plugin_options::Plugin>(owner);
    }
    if owner == &planning_manifest().id {
        return registrations::<phenix_plugin_planning::Plugin>(owner);
    }
    let basic = basic_durable_schema_registrations(manifest);
    if !basic.is_empty() {
        return basic;
    }
    Vec::new()
}

fn registrations<T: StaticPluginResources>(owner: &PluginId) -> Vec<DurableSchemaRegistration> {
    T::durable_schema_registrations(owner)
}
