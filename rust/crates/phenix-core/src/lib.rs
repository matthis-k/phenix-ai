//! Generic Phenix kernel mechanisms.
//!
//! This crate owns fundamental Phenix primitives and simple host mechanisms. Rich
//! behavior, policy, discovery, management, and product semantics belong to plugins.

mod activation;
mod agent;
mod artifact;
mod authority;
mod capability;
mod component;
mod composition_metadata;
mod configuration;
#[cfg(test)]
mod configuration_regression;
mod contract;
mod contract_wire;
mod events;
mod frontend_metadata;
mod identity;
mod infallible_value;
mod inspection;
mod invocation;
mod live_reconciliation;
mod management;
mod manifest;
mod metadata_input;
mod metadata_inspection;
mod metadata_reconciliation;
mod observable;
mod persistence;
mod persistence_bootstrap;
mod persistence_provider;
mod persistence_value;
mod plugin_build;
mod plugin_build_execution;
mod plugin_context;
mod prepared_mutation;
mod provider_resolution;
mod reconciliation;
mod reconciliation_inspection;
mod registry;
mod resolver;
mod runtime;
mod sdk;
mod std_value;
mod structural_value;
mod tasks;
mod typed_component;

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

pub use activation::{
    ActiveResolvedGraph, ResolvedHarnessActivation, ResolvedHarnessActivationError,
};
pub use agent::{
    context_service, model_inference_service, skill_service, tool_service, ContextCommand,
    ContextDescriptor, ContextResourceKind, ContextResourceRevision, ContextResponse, ContextScope,
    ModelInferenceInterface, ModelInferenceRequest, ModelInferenceResponse, ModelToolCall,
    ModelToolDescriptor, SkillCommand, SkillDefinition, SkillResponse, ToolCommand, ToolDefinition,
    ToolResponse, CONTEXT_SERVICE, MODEL_INFERENCE_SERVICE, SKILL_SERVICE, TOOL_SERVICE,
};
pub use artifact::{ArtifactRevision, ArtifactRevisionParseError};
pub use authority::Authority;
pub use capability::{
    CapabilityError, CapabilityHandler, CapabilityInvokeInput, CapabilityInvokeResult,
    CapabilityRegistry, SharedCapabilityRegistry,
};
pub use component::{
    ComponentGraphError, ResolvedComponent, ResolvedComponentGraph, ResolvedImport,
    ResolvedImportHandle, ResolvedListener, ResolvedProviderPlan,
};
pub use composition_metadata::{
    CompatibilityMetadata, ComponentHostKind, ComponentRuntimeMetadata, ComponentStateClass,
    CompositionMetadataError, DurableMigrationMetadata, PluginPackageMetadata, ReloadPolicy,
    SkillResourceMetadata,
};
pub use configuration::{
    ConfigContribution, ConfigContributionSource, ConfigMergeError, ConfigNamespace,
    ConfigSourceClass, ConfigurationFrontendMetadata, FrontendConfigContribution,
    FrontendConfigError, ResolvedConfigContribution, ResolvedConfigContributions,
};
pub use contract::{
    Bytes, CallableRef, CapabilityOwnerId, Contract, ContractId, ContractValue, Exact,
    HasPhenixSchema, Key, ObjectRef, PhenixContract, PhenixSchema, PhenixValue, Project,
    ReferenceId, SchemaCompatibility, SchemaMismatch, Type, TypeKind, ValueCodec, ValueError,
    ValueMatch,
};
pub use events::{
    EventAdmissionReceipt, EventBus, EventDeliveryCancellation, EventDeliveryStatus,
    EventDispatchReport, EventEnvelope, EventError, EventFailurePolicy, EventHandler,
    EventSubscription, KernelEvent, SubscriptionSpec,
};
pub use frontend_metadata::FrontendMetadataResolutionError;
pub use identity::{
    CallableId, CapabilityGenerationId, CapabilityId, ClientConnectionId, ComponentId,
    ConfigurationFrontendId, ContextResourceId, ContextRevisionId, EventTypeId, InterfaceId,
    ModelId, PluginId, ResourceNamespace, RoutingProfileId, RuntimeId, SdkNamespace, SdkResourceId,
    ServiceId, SessionId, SkillId, SubscriptionId,
};
pub use inspection::{ResolvedHarnessInspection, ResolvedListenerInspection};
pub use invocation::{
    CallError, InvocationFailure, InvocationFailureClass, InvocationOutcome, InvocationResult,
};
pub use live_reconciliation::LiveReconciliationError;
pub use management::{
    PluginBuildReport, PluginLoadRequest, PluginManagementContext, PluginManagementError,
    PluginManagementPolicy, PluginManagementRequest, PluginManagementResult, PluginSetRequest,
    PluginUnloadRequest,
};
pub use manifest::{
    ComponentExport, ComponentImport, ComponentListener, ComponentManifest, ListenerProjection,
    PluginArtifact, PluginExecution, PluginManifest, ServiceContribution, ServiceRole,
};
pub use metadata_input::{CompositionMetadataInput, MetadataResolutionError};
pub use metadata_inspection::ResolvedCompositionMetadata;
pub use metadata_reconciliation::{
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
pub use persistence::{
    BackendFeature, DurableSchema, LocalPersistence, NamespaceTransaction, PersistenceBackend,
    PersistenceError, SchemaMigration, TransactionOp,
};
pub use persistence_bootstrap::{
    resolve_persistence_bootstrap, DurableSchemaRegistration, PersistenceBootstrapDependency,
    PersistenceBootstrapError, PersistenceProviderDescriptor, PersistenceProviderTransition,
    ResolvedPersistenceBootstrap, StoreBinding, StoreBindingId, StoreBindingIdParseError,
};
pub use persistence_provider::{
    prepare_persistence_candidate, PersistenceCandidateError, PersistenceProvider,
    PersistenceProviderError, PreparedPersistence,
};
pub use plugin_build::{
    BuildArgument, BuildArtifactOutput, BuildEnvironment, BuildEnvironmentName, BuildExecutable,
    BuildSourceIdentity, BuildSourceRevision, BuildWorkingDirectory, PluginArtifactInput,
    PluginBuildPlan, PluginBuildPlanError, PluginBuildSource, PluginBuildStep,
};
pub use plugin_build_execution::{
    PluginArtifactStore, PluginArtifactStoreError, PluginBuildEvidence, PluginBuildExecution,
    PluginBuildExecutor, PluginBuildFailure, PluginBuildOutput,
};
pub use plugin_context::{
    CallContext, CurrentPlugin, KernelAccess, PluginContext, SdkClient, SdkContract, SdkObject,
};
pub use prepared_mutation::PreparedMutationHandle;
pub use provider_resolution::{
    InterfaceProviderPolicy, ProviderCompositionPolicy, ProviderFallbackReason,
    ProviderSelectionReason,
};
pub use reconciliation::{
    BindingChange, ComponentChange, ComponentChangeKind, GraphDiff, GraphReconciler,
    ReconciliationAction, ReconciliationPreview, ReconciliationResult, ResourceChange,
    ResourceChangeKind,
};
pub use reconciliation_inspection::CandidateResolutionInspection;
pub use registry::{
    runtime_provider_runtime, runtime_provider_service, KernelConfig, KernelError,
    KernelPolicyIdentity, LayerPolicy, ProviderBinding, ResolvedServiceChain, RuntimeBinding,
    EMBEDDED_RUNTIME, RUNTIME_PROVIDER_SERVICE_PREFIX,
};
pub use resolver::{GraphGenerationId, ResolvedHarness, ResolvedHarnessError};
pub use runtime::{
    ComponentProviderProvenance, Kernel, LayerResult, PluginHost, PluginInstance, PluginListener,
    PluginRuntimeProvider, PluginState, ProviderEndpointProvenance, RuntimePluginCandidate,
    ServiceInvocationProvenance, ServiceParticipantOutcome, ServiceParticipantProvenance,
    SharedPluginInvocation,
};
pub use sdk::{
    observable_delivery_schema, ResolvedSdkContributions, SdkContribution, SdkObservableResource,
    SdkResolutionError, SdkValue,
};
pub use tasks::{CallCancellationToken, CancellationToken, TaskHandle, TaskRuntime, TaskScope};
pub use typed_component::{
    ComponentInterface, ComponentInvocationError, InterfaceCompatibility, InterfaceSchema,
    InterfaceSchemaMismatch,
};
