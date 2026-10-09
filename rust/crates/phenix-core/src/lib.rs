#![forbid(unsafe_code)]

//! Generic Phenix kernel mechanisms.
//!
//! This crate owns fundamental Phenix primitives and simple host mechanisms. Rich
//! behavior, policy, discovery, management, and product semantics belong to plugins.

mod agent;
mod artifact;
mod authority;
mod callable;
mod composition;
mod configuration;
#[cfg(test)]
mod configuration_regression;
mod content_reference;
mod events;
mod invocation;
mod logging;
mod metadata;
mod observable;
mod persistence;
mod plugin;
mod reconciliation;
mod runtime;
mod sdk;
mod tasks;
mod workflow;
mod workflow_frame;
mod workflow_join;
mod workflow_projection;
mod workflow_tasks;

extern crate self as phenix_core;
#[cfg(test)]
mod component_endpoint_regression;
#[cfg(test)]
mod component_reentrancy_regression;
#[cfg(test)]
mod host_authority_regression;
#[cfg(test)]
mod invalid_candidate_activation_regression;
#[cfg(test)]
mod layer_regression;
#[cfg(test)]
mod metadata_semantic_identity_regression;
#[cfg(test)]
#[path = "../tests/persistence_backend_conformance.rs"]
mod persistence_backend_conformance;
#[cfg(test)]
mod plugin_build_loading_regression;
#[cfg(test)]
mod plugin_management_regression;
#[cfg(test)]
mod plugin_runtime_adapter_host_regression;
#[cfg(test)]
mod plugin_runtime_adapter_regression;
#[cfg(test)]
mod provider_availability_regression;
#[cfg(test)]
mod provider_fallback_regression;
#[cfg(test)]
mod provider_rebind_generation_regression;
#[cfg(test)]
mod runtime_component_parity_regression;
#[cfg(test)]
mod runtime_topology_generation_regression;
#[cfg(test)]
mod service_layer_dispatch_regression;
#[cfg(test)]
mod third_party_component_regression;
#[cfg(test)]
mod workflow_execution_regression;

pub use agent::{
    CONTEXT_SERVICE, ContextCommand, ContextDescriptor, ContextResourceKind,
    ContextResourceRevision, ContextResponse, ContextScope, MODEL_INFERENCE_SERVICE,
    ModelCacheControl, ModelCacheRetention, ModelCacheWritePolicy, ModelInferenceFailure,
    ModelInferenceInterface, ModelInferenceRequest, ModelInferenceResponse, ModelToolCall,
    ModelToolDescriptor, ModelToolResult, ModelToolTurn, ModelTurnUsage, SKILL_SERVICE,
    SkillCommand, SkillDefinition, SkillResponse, TOOL_SERVICE, ToolCatalogCursor,
    ToolCatalogDescriptor, ToolCommand, ToolDefinition, ToolResponse, UsageQuantity,
    context_service, model_inference_service, skill_service, tool_service,
};
pub use artifact::{ArtifactRevision, ArtifactRevisionParseError};
pub use authority::Authority;
pub use callable::{
    CallableError, CallableHandler, CallableInvocation, CallableInvocationResult, CallableRegistry,
    SharedCallableRegistry,
};
pub use composition::activation::{
    ActiveGenerationGraph, ResolvedGenerationActivation, ResolvedGenerationActivationError,
};
pub use composition::component::{
    ComponentGraphError, ResolvedComponent, ResolvedComponentGraph, ResolvedImport,
    ResolvedImportHandle, ResolvedListener, ResolvedProviderPlan,
};
pub use composition::component_invocation::ComponentInvocationError;
pub use composition::inspection::{ResolvedGenerationInspection, ResolvedListenerInspection};
pub use composition::manifest::{
    ComponentEntryTrigger, ComponentExport, ComponentImport, ComponentListener, ComponentManifest,
    ComponentProcessArgument, EntryTriggerKind, ListenerProjection, PluginArtifact,
    PluginExecution, PluginManifest, ServiceContribution, ServiceRole,
};
pub use composition::provider_resolution::{
    InterfaceProviderPolicy, ProviderCompositionPolicy, ProviderFallbackReason,
    ProviderSelectionReason,
};
pub use composition::registry::{
    EMBEDDED_RUNTIME, KernelConfig, KernelError, KernelPolicyIdentity, LayerPolicy,
    PLUGIN_RUNTIME_ADAPTER_SERVICE_PREFIX, PluginRuntimeBinding, ProviderBinding,
    ResolvedComponentDispatchPlan, ResolvedDispatchTopology, ResolvedLayerPlan,
    ResolvedServiceChain, ResolvedServicePlan, ResolvedTerminalPlan,
    plugin_runtime_adapter_id_from_service, plugin_runtime_adapter_service,
};
pub use composition::resolver::{
    GenerationResolutionError, GenerationTopology, ResolvedGeneration,
};
pub use configuration::{
    ConfigContribution, ConfigContributionSource, ConfigMergeError, ConfigNamespace,
    ConfigSourceClass, ConfigurationFrontendMetadata, FrontendConfigContribution,
    FrontendConfigError, ResolvedConfigContribution, ResolvedConfigContributions,
};
pub use content_reference::{
    ContentLocator, ContentReference, ContentReferenceStore, FileContentReferenceStore,
};
pub use events::{
    EventAdmissionReceipt, EventBus, EventDeliveryCancellation, EventDeliveryStatus,
    EventDispatchReport, EventEnvelope, EventError, EventFailurePolicy, EventHandler,
    EventSubscription, KernelEvent, SubscriptionSpec,
};
pub use invocation::{
    CallError, InvocationFailure, InvocationFailureClass, InvocationOutcome, InvocationResult,
};
pub use logging::{
    LogDetailMode, LogSink, PHENIX_LOG_DEPTH_ENV, PHENIX_LOG_ENV, PHENIX_LOG_STORE_ENV,
    StructuredLogPage, StructuredLogReader, StructuredLogger,
};
pub use metadata::composition::{
    CompatibilityMetadata, ComponentHostKind, ComponentRuntimeMetadata, ComponentStateClass,
    CompositionMetadataError, DurableMigrationMetadata, PluginPackageMetadata, ReloadPolicy,
    SkillResourceMetadata,
};
pub use metadata::frontend::FrontendMetadataResolutionError;
pub use metadata::input::{CompositionMetadataInput, MetadataResolutionError};
pub use metadata::inspection::ResolvedCompositionMetadata;
pub use metadata::reconciliation::{
    ComponentMetadataChange, CompositionMetadataDiff, FrontendMetadataChange, MetadataChangeKind,
    MetadataReconciliationError, MetadataReconciliationPreview, PackageMetadataChange,
    ResourceMetadataChange,
};
pub use observable::{
    CommitId, InitialObservation, OBSERVABLE_CONTRACT, ObservableError, ObservableMetadata,
    ObservableRef, ObservableRegistration, ObservableSnapshot, ObservableStore,
    ObservableTransaction, ObservationDelivery, ObservationGeneration, ObservationHandler,
    ObservationId, ObservationMode, ObservationScope, ObservationSpec, ObservationSubscription,
    SnapshotPolicy, ValueAddress, ValueChange, ValueId, ValuePath, ValuePathSegment, ValueVersion,
};
pub use persistence::backend::{
    DurableSchema, LocalPersistence, NamespaceTransaction, PersistenceBackend,
    PersistenceBackendFeature, PersistenceError, SchemaMigration, TransactionOp,
};
pub use persistence::bootstrap::{
    DurableSchemaRegistration, PersistenceBootstrapDependency, PersistenceBootstrapError,
    PersistenceProviderDescriptor, PersistenceProviderTransition, ResolvedPersistenceBootstrap,
    StoreBinding, StoreBindingId, StoreBindingIdParseError, resolve_persistence_bootstrap,
};
pub use persistence::provider::{
    PersistenceCandidateError, PersistenceProvider, PersistenceProviderError, PreparedPersistence,
    prepare_persistence_candidate,
};
pub use phenix_contract::{
    Bytes, CallableId, CallableRef, ClientConnectionId, ComponentId, ComponentInterface,
    ConfigurationFrontendId, ContextResourceId, ContextRevisionId, Contract, ContractId,
    ContractValue, EventTypeId, Exact, GenerationId, HasPhenixSchema, InterfaceCompatibility,
    InterfaceId, InterfaceSchema, InterfaceSchemaMismatch, Key, ModelFeatureGenerationId, ModelId,
    ObjectRef, PermissionId, PhenixContract, PhenixSchema, PhenixValue, PluginId, PluginRuntimeId,
    Project, ReferenceGenerationId, ReferenceId, ReferenceOwnerId, ResourceNamespace,
    RoutingProfileId, SchemaCompatibility, SchemaMismatch, SdkNamespace, SdkResourceId, ServiceId,
    SessionId, SkillId, SubscriptionId, Type, TypeKind, ValueCodec, ValueError, ValueMatch,
};
pub use plugin::build::{
    BuildArgument, BuildArtifactOutput, BuildEnvironment, BuildEnvironmentName, BuildExecutable,
    BuildSourceIdentity, BuildSourceRevision, BuildWorkingDirectory, PluginArtifactInput,
    PluginBuildPlan, PluginBuildPlanError, PluginBuildSource, PluginBuildStep,
};
pub use plugin::build_execution::{
    PluginArtifactStore, PluginArtifactStoreError, PluginBuildEvidence, PluginBuildExecution,
    PluginBuildExecutor, PluginBuildFailure, PluginBuildOutput,
};
pub use plugin::context::{
    CallContext, CurrentPlugin, KernelAccess, PluginContext, SdkClient, SdkContract, SdkObject,
};
pub use plugin::management::{
    PluginBuildReport, PluginLoadRequest, PluginManagementContext, PluginManagementError,
    PluginManagementPolicy, PluginManagementRequest, PluginManagementResult, PluginSetRequest,
    PluginTrialResult, PluginUnloadRequest, PreparedPluginManagement,
};
pub use plugin::prepared_mutation::PreparedMutationHandle;
pub use reconciliation::graph::{
    BindingChange, ComponentChange, ComponentChangeKind, GraphDiff, GraphReconciler,
    ReconciliationAction, ReconciliationPreview, ReconciliationResult, ResourceChange,
    ResourceChangeKind,
};
pub use reconciliation::inspection::CandidateResolutionInspection;
pub use reconciliation::live::LiveReconciliationError;
pub use runtime::{
    ComponentProviderProvenance, DEFAULT_PROVENANCE_CAPACITY, DEFAULT_RUNTIME_TRACE_CAPACITY,
    Kernel, LayerResult, PluginHost, PluginInstance, PluginListener, PluginRuntimeAdapter,
    PluginRuntimeCandidate, PluginState, ProvenanceBuffer, ProviderEndpointProvenance,
    RootExecutionConstraints, RootExecutionHandle, RuntimeTraceBuffer, RuntimeTraceEvent,
    RuntimeTraceParticipant, RuntimeTraceSink, ServiceInvocationProvenance,
    ServiceParticipantOutcome, ServiceParticipantProvenance, SharedPluginInvocation,
};
pub use sdk::{
    ResolvedSdkContributions, SdkContribution, SdkObservableResource, SdkResolutionError, SdkValue,
    observable_delivery_schema,
};
pub use tasks::{CallCancellationToken, CancellationToken, TaskHandle, TaskRuntime, TaskScope};
pub use workflow::{
    CompiledWorkflow, WorkflowBoundCallError, WorkflowCompileError, WorkflowDeclaration,
    WorkflowEdge, WorkflowNode, WorkflowNodeDispatchError, WorkflowRunError, WorkflowRunReport,
    WorkflowTopology,
};
pub use workflow_frame::{
    WorkflowFrame, WorkflowFrameDeclaration, WorkflowFrameError, WorkflowFrameSchema,
};

pub use workflow_projection::{
    WORKFLOW_PROJECTION_REVISION, WorkflowOutcomeProjection, WorkflowProjectionError,
    WorkflowProjectionSelector,
};
pub use workflow_tasks::{
    WorkflowPendingTasks, WorkflowTaskError, WorkflowTaskId, WorkflowTaskState,
};
pub use workflow_join::{
    WorkflowChildSettlement, WorkflowJoinAllPolicy, WorkflowJoinDecision, WorkflowJoinError,
    WorkflowJoinObservation, WorkflowJoinPolicy,
};
