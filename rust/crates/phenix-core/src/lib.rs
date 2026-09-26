//! Generic Phenix kernel mechanisms.
//!
//! This crate owns fundamental Phenix primitives and simple host mechanisms. Rich
//! behavior, policy, discovery, management, and product semantics belong to plugins.

mod agent;
mod artifact;
mod authority;
mod capability;
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
mod provider_availability_regression;
#[cfg(test)]
mod provider_fallback_regression;
#[cfg(test)]
mod provider_rebind_generation_regression;
#[cfg(test)]
mod runtime_component_parity_regression;
#[cfg(test)]
mod runtime_provider_host_regression;
#[cfg(test)]
mod runtime_provider_regression;
#[cfg(test)]
mod runtime_topology_generation_regression;
#[cfg(test)]
mod service_layer_dispatch_regression;
#[cfg(test)]
mod third_party_component_regression;

pub use agent::{
    context_service, model_inference_service, skill_service, tool_service, ContextCommand,
    ContextDescriptor, ContextResourceKind, ContextResourceRevision, ContextResponse, ContextScope,
    ModelCacheControl, ModelCacheRetention, ModelCacheWritePolicy, ModelInferenceFailure,
    ModelInferenceInterface, ModelInferenceRequest, ModelInferenceResponse, ModelToolCall,
    ModelToolDescriptor, ModelToolResult, ModelToolTurn, ModelTurnUsage, SkillCommand,
    SkillDefinition, SkillResponse, ToolCatalogCursor, ToolCatalogDescriptor, ToolCommand,
    ToolDefinition, ToolResponse, UsageQuantity, CONTEXT_SERVICE, MODEL_INFERENCE_SERVICE,
    SKILL_SERVICE, TOOL_SERVICE,
};
pub use artifact::{ArtifactRevision, ArtifactRevisionParseError};
pub use authority::Authority;
pub use capability::{
    CapabilityError, CapabilityHandler, CapabilityInvokeInput, CapabilityInvokeResult,
    CapabilityRegistry, SharedCapabilityRegistry,
};
pub use composition::activation::{
    ActiveResolvedGraph, ResolvedHarnessActivation, ResolvedHarnessActivationError,
};
pub use composition::component::{
    ComponentGraphError, ResolvedComponent, ResolvedComponentGraph, ResolvedImport,
    ResolvedImportHandle, ResolvedListener, ResolvedProviderPlan,
};
pub use composition::component_invocation::ComponentInvocationError;
pub use composition::inspection::{ResolvedHarnessInspection, ResolvedListenerInspection};
pub use composition::manifest::{
    ComponentExport, ComponentImport, ComponentListener, ComponentManifest, ListenerProjection,
    PluginArtifact, PluginExecution, PluginManifest, ServiceContribution, ServiceRole,
};
pub use composition::provider_resolution::{
    InterfaceProviderPolicy, ProviderCompositionPolicy, ProviderFallbackReason,
    ProviderSelectionReason,
};
pub use composition::registry::{
    runtime_provider_runtime, runtime_provider_service, KernelConfig, KernelError,
    KernelPolicyIdentity, LayerPolicy, ProviderBinding, ResolvedComponentDispatchPlan,
    ResolvedDispatchTopology, ResolvedLayerPlan, ResolvedServiceChain, ResolvedServicePlan,
    ResolvedTerminalPlan, RuntimeBinding, EMBEDDED_RUNTIME, RUNTIME_PROVIDER_SERVICE_PREFIX,
};
pub use composition::resolver::{ResolvedHarness, ResolvedHarnessError, RuntimeGeneration};
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
    LogDetailMode, LogSink, StructuredLogger, PHENIX_LOG_DEPTH_ENV, PHENIX_LOG_ENV,
    PHENIX_LOG_STORE_ENV,
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
    CommitId, InitialObservation, ObservableError, ObservableMetadata, ObservableRef,
    ObservableRegistration, ObservableSnapshot, ObservableStore, ObservableTransaction,
    ObservationDelivery, ObservationGeneration, ObservationHandler, ObservationId, ObservationMode,
    ObservationScope, ObservationSpec, ObservationSubscription, SnapshotPolicy, ValueAddress,
    ValueChange, ValueId, ValuePath, ValuePathSegment, ValueVersion, OBSERVABLE_CONTRACT,
};
pub use persistence::backend::{
    BackendFeature, DurableSchema, LocalPersistence, NamespaceTransaction, PersistenceBackend,
    PersistenceError, SchemaMigration, TransactionOp,
};
pub use persistence::bootstrap::{
    resolve_persistence_bootstrap, DurableSchemaRegistration, PersistenceBootstrapDependency,
    PersistenceBootstrapError, PersistenceProviderDescriptor, PersistenceProviderTransition,
    ResolvedPersistenceBootstrap, StoreBinding, StoreBindingId, StoreBindingIdParseError,
};
pub use persistence::provider::{
    prepare_persistence_candidate, PersistenceCandidateError, PersistenceProvider,
    PersistenceProviderError, PreparedPersistence,
};
pub use phenix_contract::{
    Bytes, CallableId, CallableRef, CapabilityGenerationId, CapabilityId, CapabilityOwnerId,
    ClientConnectionId, ComponentId, ComponentInterface, ConfigurationFrontendId,
    ContextResourceId, ContextRevisionId, Contract, ContractId, ContractValue, EventTypeId, Exact,
    GraphGenerationId, HasPhenixSchema, InterfaceCompatibility, InterfaceId, InterfaceSchema,
    InterfaceSchemaMismatch, Key, ModelId, ObjectRef, PhenixContract, PhenixSchema, PhenixValue,
    PluginId, Project, ReferenceId, ResourceNamespace, RoutingProfileId, RuntimeId,
    SchemaCompatibility, SchemaMismatch, SdkNamespace, SdkResourceId, ServiceId, SessionId,
    SkillId, SubscriptionId, Type, TypeKind, ValueCodec, ValueError, ValueMatch,
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
    PluginUnloadRequest,
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
    ComponentProviderProvenance, Kernel, LayerResult, PluginHost, PluginInstance, PluginListener,
    PluginRuntimeProvider, PluginState, ProvenanceBuffer, ProviderEndpointProvenance,
    RuntimePluginCandidate, RuntimeTraceBuffer, RuntimeTraceEvent, RuntimeTraceParticipant,
    RuntimeTraceSink, ServiceInvocationProvenance, ServiceParticipantOutcome,
    ServiceParticipantProvenance, SharedPluginInvocation, DEFAULT_PROVENANCE_CAPACITY,
    DEFAULT_RUNTIME_TRACE_CAPACITY,
};
pub use sdk::{
    observable_delivery_schema, ResolvedSdkContributions, SdkContribution, SdkObservableResource,
    SdkResolutionError, SdkValue,
};
pub use tasks::{CallCancellationToken, CancellationToken, TaskHandle, TaskRuntime, TaskScope};
