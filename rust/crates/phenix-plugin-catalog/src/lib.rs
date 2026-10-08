#![forbid(unsafe_code)]

use phenix_core::{DurableSchemaRegistration, PluginId, PluginManifest};
use phenix_sdk::StaticPluginResources;
use std::collections::BTreeSet;

pub use phenix_adapter_acp::{ACP_ADAPTER_PLUGIN, adapter_acp_factory, adapter_acp_manifest};
pub use phenix_agent_configurations::{
    ADVANCED_AGENT_CONFIGURATION, BASIC_AGENT_CONFIGURATION, BASIC_PRODUCT_CONFIGURATION,
    FULL_PRODUCT_CONFIGURATION, advanced_agent_configuration_manifest,
    basic_agent_configuration_manifest, basic_product_configuration_manifest,
    full_product_configuration_manifest, profile_defaults,
};
/// Expand first-party product defaults, including the optional common-model
/// provider bundle, before validating *actual* manifest dependencies.
#[must_use]
pub fn expand_profile_defaults(
    selected: &BTreeSet<String>,
    excluded: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut expanded = phenix_agent_configurations::expand_profile_defaults(selected, excluded);
    if expanded.contains(PROVIDERS_PLUGIN) {
        for provider in COMMON_PROVIDERS {
            let id = provider.id();
            if !excluded.contains(id) {
                expanded.insert(id.to_owned());
            }
        }
    }
    expanded
}

pub use phenix_core::{ContextResourceId, ContextRevisionId, SessionId};
pub use phenix_plugin_api::{
    SDK_COMPONENT, SDK_CONFIG_SERVICE, SDK_PLUGIN, SDK_SESSION_SERVICE, SDK_SKILLS_SERVICE,
    SDK_TOOLS_SERVICE, SdkConfigCommand, SdkConfigInterface, SdkConfigResponse, SdkSessionCommand,
    SdkSessionInterface, SdkSessionResponse, SdkSkill, SdkSkillCommand, SdkSkillResponse,
    SdkSkillSummary, SdkSkillsInterface, SdkTool, SdkToolCommand, SdkToolResponse,
    SdkToolsInterface, sdk_component_id, sdk_component_manifest, sdk_config_service,
    sdk_contribution, sdk_factory, sdk_manifest, sdk_session_service, sdk_skills_service,
    sdk_tools_service,
};
pub use phenix_plugin_artifacts::{
    ARTIFACT_SERVICE, ArtifactCommand, ArtifactInterface, ArtifactProvenance, ArtifactRecord,
    ArtifactResponse, NormalizedReadRequest, ReadProviderIdentity, ReadResultRecord,
    RevalidationRecord, RevalidationVerdict, artifact_component_id, artifact_component_manifest,
    artifact_factory, artifact_manifest, artifact_service,
};
pub use phenix_plugin_basic_agent::{
    AGENT_LOOP_CONTROL_SERVICE, AGENT_LOOP_PLUGIN, AGENT_LOOP_PROGRESS_SERVICE, AGENT_LOOP_SERVICE,
    AGENT_TOOL_EXECUTION_SERVICE, AgentLoopCommand, AgentLoopControlInterface,
    AgentLoopControlRequest, AgentLoopControlResponse, AgentLoopFailure, AgentLoopInterface,
    AgentLoopPolicy, AgentLoopProgress, AgentLoopProgressInterface, AgentLoopProgressRecord,
    AgentLoopProgressResponse, AgentLoopResponse, AgentLoopUsage, AgentToolExecutionInterface,
    AgentToolExecutionRequest, AgentToolExecutionResponse, agent_loop_component_id,
    agent_loop_component_manifest, agent_loop_control_service, agent_loop_factory,
    agent_loop_factory_with_policy, agent_loop_manifest, agent_loop_progress_authority,
    agent_loop_progress_service, agent_loop_service, agent_tool_execution_service,
};
pub use phenix_plugin_basic_context::{
    BASIC_CONTEXT_COMPONENT, BASIC_CONTEXT_PLUGIN, BasicContextInterface,
    basic_context_component_manifest, basic_context_factory, basic_context_manifest,
};
pub use phenix_plugin_basic_model::{
    BASIC_MODEL_COMPONENT, BASIC_MODEL_PLUGIN, basic_model_component_manifest, basic_model_factory,
    basic_model_manifest,
};
pub use phenix_plugin_basic_skills::{
    BASIC_SKILLS_COMPONENT, BASIC_SKILLS_PLUGIN, BasicSkillsInterface,
    basic_skills_component_manifest, basic_skills_factory, basic_skills_manifest,
};
pub use phenix_plugin_basic_tools::{
    BASIC_TOOLS_COMPONENT, BASIC_TOOLS_PLUGIN, BasicToolsInterface, basic_tools_component_manifest,
    basic_tools_factory, basic_tools_manifest,
};
pub use phenix_plugin_command_toolbelt::{
    CLI_AUTH_STATE_SERVICE, CLI_DISCOVER_SERVICE, CLI_VERSION_SERVICE, CliAuthState,
    CliAuthStateInterface, CliAvailability, CliDescriptor, CliDiscoverInterface, CliName,
    CliProbeRequest, CliVersionInterface, cli_auth_state_service, cli_component_id,
    cli_component_manifest, cli_discover_service, cli_factory, cli_manifest, cli_version_service,
};
pub use phenix_plugin_context::{
    ContextCodeQueryRequest, ContextInjection, ContextInjectionLifetime, ContextInjectionRequester,
    ContextResourceKind, ContextScope, ExactContextReference, ExecutionContextProjection,
    ProjectedContextEntry, context_component_id, context_component_manifest, context_factory,
    context_manifest,
};
pub use phenix_plugin_debug::{
    DEBUG_SERVICE, DebugCommand, DebugInterface, DebugResponse, DiagnosticEntry,
    DiagnosticSnapshot, debug_component_id, debug_component_manifest, debug_factory,
    debug_manifest, debug_runtime_trace_sink, debug_service,
};
pub use phenix_plugin_efficiency_evaluation::{
    BENCHMARK_OUTCOME_COMPONENT, BENCHMARK_OUTCOME_PLUGIN, BENCHMARK_OUTCOME_SERVICE,
    BenchmarkOutcomeCommand, BenchmarkOutcomeInterface, BenchmarkOutcomeRecord,
    BenchmarkOutcomeResponse, EFFICIENCY_EVALUATION_COMPONENT, EFFICIENCY_EVALUATION_PLUGIN,
    EFFICIENCY_EVALUATION_SERVICE, EFFICIENCY_OUTCOME_EVIDENCE_SERVICE,
    EfficiencyCollectionRequest, EfficiencyEvaluationCommand, EfficiencyEvaluationInterface,
    EfficiencyEvaluationResponse, EfficiencyOutcomeEvidence, EfficiencyOutcomeEvidenceCommand,
    EfficiencyOutcomeEvidenceInterface, EfficiencyOutcomeEvidenceRequest,
    EfficiencyOutcomeEvidenceResponse, EfficiencyTaskRecord, benchmark_outcome_component_id,
    benchmark_outcome_component_manifest, benchmark_outcome_factory, benchmark_outcome_manifest,
    benchmark_outcome_service, efficiency_evaluation_component_id,
    efficiency_evaluation_component_manifest, efficiency_evaluation_factory,
    efficiency_evaluation_manifest, efficiency_evaluation_service,
    efficiency_outcome_evidence_service,
};
pub use phenix_plugin_environment_local::{
    LOCAL_ENVIRONMENT_PLUGIN, local_environment_component_id, local_environment_component_manifest,
    local_environment_factory, local_environment_factory_for, local_environment_manifest,
};
pub use phenix_plugin_execution::{
    AgentDefinition, CallablePolicy, EXECUTION_CONFIGURATION_SERVICE, EXECUTION_RESOURCE_SERVICE,
    EXECUTION_REVIEW_SERVICE, ExecutionConfigurationCommand, ExecutionConfigurationResponse,
    ExecutionResourceCommand, ExecutionResourceInterface, ExecutionResourceResponse,
    ExecutionReviewCommand, ExecutionReviewInterface, ExecutionReviewResponse,
    OrchestrationDefinition, OrchestrationNode, PreparedReviewFile, STEP_ATTEMPT_SERVICE,
    StepAttemptCommand, StepAttemptInterface, StepAttemptPhase, StepAttemptRecord,
    StepAttemptResponse, execution_component_id, execution_component_manifest,
    execution_configuration_service, execution_factory, execution_manifest,
    execution_resource_service, execution_review_service, step_attempt_service,
};
pub use phenix_plugin_frontend::{
    FRONTEND_SERVICE, FrontendCommand, FrontendInterface, FrontendProviderDescriptor,
    FrontendResponse, FrontendServiceRequest, FrontendServiceResult, LiveFrontendProvider,
    frontend_component_id, frontend_component_manifest, frontend_factory, frontend_manifest,
    frontend_service,
};
pub use phenix_plugin_hooks::{
    HOOK_SERVICE, HookAction, HookCommand, HookConfiguration, HookDefinition, HookDispatch,
    HookFailurePolicy, HookInterface, HookResponse, HookWarning, LifecycleEvent, hook_component_id,
    hook_component_manifest, hook_factory, hook_manifest, hook_service,
};
pub use phenix_plugin_interactive_ui::{
    INTERACTIVE_UI_PLUGIN, UI_DOCUMENT_SERVICE, UiDocument, UiDocumentCommand, UiDocumentInterface,
    UiDocumentResponse, UiNode, interactive_ui_component_manifest, interactive_ui_factory,
    interactive_ui_manifest, interactive_ui_service, validate_document as validate_ui_document,
};
pub use phenix_plugin_jobs::{
    job_component_id, job_component_manifest, job_factory, job_manifest, job_service,
};
pub use phenix_plugin_language::{
    CodeChangedNeighborhood, CodeEntityEditEvidence, CodeEntityEditResult,
    CodeEntityEditValidation, CodeEntityInsertPosition, CodeEntityRelationKind,
    CodeEntityRelationTarget, CodeEntityRelations, CodeQuery, CodeQueryAnchor, CodeQueryBudget,
    CodeQueryCoverage, CodeQueryDirection, CodeQueryEntity, CodeQueryProjection, CodeQueryRelation,
    CodeQueryResult, CodeQuerySelection, CodeQueryTraversal, CodeRelationKind, DocumentProvenance,
    LANGUAGE_SERVICE, LanguageCommand, LanguageDocumentIdentity, LanguageInterface,
    LanguageObservation, LanguageOperationKind, LanguageOperationResult, LanguageProviderEpoch,
    LanguageResponse, language_component_id, language_component_manifest, language_factory,
    language_manifest, language_service,
};
pub use phenix_plugin_memory::{
    MEMORY_SERVICE, MemoryCommand, MemoryInterface, MemoryKind, MemoryNode, MemoryQueryOrder,
    MemoryRecallQuery, MemoryRecord, MemoryResponse, MemoryScope, MemorySearchQuery,
    MemorySourceReference, MemoryStructuredQuery, MemoryTimeBounds, memory_component_id,
    memory_component_manifest, memory_factory, memory_manifest, memory_service,
};
pub use phenix_plugin_models::{
    MODEL_DISPATCH_SERVICE, MODEL_INFERENCE_SERVICE, MODEL_ROUTING_SERVICE, ModelCommand,
    ModelDispatchCommand, ModelDispatchInterface, ModelDispatchResponse, ModelInferenceRequest,
    ModelInferenceResponse, ModelResponse, ModelRoutingInterface, ModelTarget, RoutingProfile,
    RoutingProfileDescriptor, model_dispatch_service, model_inference_service,
    model_routing_component_id, model_routing_component_manifest, model_routing_factory,
    model_routing_manifest, model_routing_service,
};
pub use phenix_plugin_openai_codex::{
    OPENAI_CODEX_PROVIDER, openai_codex_component_manifest, openai_codex_factory,
    openai_codex_manifest,
};
pub use phenix_plugin_options::{
    OPTIONS_COMPONENT, OPTIONS_PLUGIN, OPTIONS_SERVICE, OptionAssignment, OptionCommand,
    OptionContext, OptionDefinition, OptionKey, OptionResponse, OptionScope, OptionScopeKind,
    OptionStartupPrecedence, OptionSubjectId, OptionValue, OptionValueLayer, OptionValueSource,
    OptionsInterface, ResolvedOption, default_option_definitions, options_component_id,
    options_component_manifest, options_factory, options_manifest, options_service,
};
pub use phenix_plugin_planning::{
    planning_component_id, planning_component_manifest, planning_factory, planning_manifest,
    planning_service,
};
pub use phenix_plugin_providers::{
    COMMON_PROVIDERS, PROVIDERS_PLUGIN, ProviderPreset, common_provider_definitions,
    providers_manifest,
};
pub use phenix_plugin_repository_workers::{
    REPOSITORY_WORK_QUEUE_SERVICE, ReconstructedPullRequest, RepositoryCheckState,
    RepositoryChecklistEvidence, RepositoryDiscussionEvidence, RepositoryDiscussionKind,
    RepositoryFinding, RepositoryIssueCluster, RepositoryIssueEvidence,
    RepositoryPullRequestEvidence, RepositoryPullRequestState, RepositorySelectionReason,
    RepositoryValidation, RepositoryWorkPriority, RepositoryWorkSelection, RepositoryWorkSnapshot,
    RepositoryWorkerInterface, RepositoryWorkerQueue, repository_work_queue_service,
    repository_worker_component_id, repository_worker_component_manifest,
    repository_worker_factory, repository_worker_manifest,
};
pub use phenix_plugin_session_tree::{
    SESSION_TREE_SERVICE, SessionLineage, SessionTreeCommand, SessionTreeInterface,
    SessionTreeResponse, session_tree_component_id, session_tree_component_manifest,
    session_tree_factory, session_tree_manifest, session_tree_service,
};
pub use phenix_plugin_sessions::{
    SESSION_SERVICE, SessionCommand, SessionInput, SessionInputKind, SessionInterface,
    SessionJournalDraft, SessionJournalEntry, SessionLifecycle, SessionRecord, SessionResponse,
    SessionTransition, session_component_manifest, session_factory, session_manifest,
    session_service,
};
pub use phenix_plugin_step_runner::{
    HELPER_INVOCATION_COMPONENT, STEP_RUNNER_COMPONENT, STEP_RUNNER_PLUGIN,
    helper_invocation_component_id, helper_invocation_component_manifest, step_runner_component_id,
    step_runner_component_manifest, step_runner_factory, step_runner_manifest,
};
pub use phenix_plugin_workspace::{
    WORKSPACE_SERVICE, WorkspaceCapabilities, WorkspaceCommand, WorkspaceCommitReceipt,
    WorkspaceCommittedFile, WorkspaceEntry, WorkspaceEntryKind, WorkspaceFileVersion,
    WorkspaceInterface, WorkspaceProjectFile, WorkspaceResponse, WorkspaceSearchMatch,
    WorkspaceVersionConflict, WorkspaceWrite, WorkspaceWriteAtomicity, WorkspaceWrittenFile,
    workspace_component_id, workspace_component_manifest, workspace_factory, workspace_factory_for,
    workspace_manifest, workspace_service,
};
pub use phenix_sdk::{
    CONTEXT_SERVICE, CallableRecord, ContextCommand, ContextDescriptor, ContextInterface,
    ContextResourceRevision, ContextResponse, DecisionRecord, EXECUTION_INSPECTION_SERVICE,
    EXECUTION_SERVICE, ExecutionAuthority, ExecutionCommand, ExecutionInspectionCommand,
    ExecutionInspectionInterface, ExecutionInspectionResponse, ExecutionInterface, ExecutionRecord,
    ExecutionResponse, ExecutionState, HistoryEntry, HistoryKind, JOB_SERVICE, JobCommand,
    JobInterface, JobResponse, ModelInferenceInterface, ObjectiveRecord, PLANNING_SERVICE,
    PlanRecord, PlanStep, PlannedStepRequest, PlanningCommand, PlanningInterface, PlanningResponse,
    RepositoryContextSource, RuntimeResourceKind, RuntimeResourceRecord, RuntimeResourceState,
    STEP_RUNNER_SERVICE, StepRunnerCommand, StepRunnerInterface, StepRunnerResponse,
    StepSettlementBasis, WorkerTaskRecord, WorkerTaskState, context_service,
    execution_inspection_service, execution_service, step_runner_service,
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
    if owner == &basic_context_manifest().id {
        return registrations::<phenix_plugin_basic_context::Plugin>(owner);
    }
    if owner == &basic_skills_manifest().id {
        return registrations::<phenix_plugin_basic_skills::Plugin>(owner);
    }
    if owner == &basic_tools_manifest().id {
        return registrations::<phenix_plugin_basic_tools::Plugin>(owner);
    }
    Vec::new()
}

fn registrations<T: StaticPluginResources>(owner: &PluginId) -> Vec<DurableSchemaRegistration> {
    T::durable_schema_registrations(owner)
}
