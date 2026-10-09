#![forbid(unsafe_code)]

//! Wire-stable Phenix contract, value, identity, and interface primitives.

mod contract;
mod contract_wire;
mod identity;
mod infallible_value;
mod interface;
mod std_value;
mod structural_value;

pub use contract::{
    Bytes, CallableRef, Contract, ContractId, ContractValue, Exact, HasPhenixSchema, Key,
    ObjectRef, PhenixContract, PhenixSchema, PhenixValue, Project, ReferenceId, ReferenceOwnerId,
    SchemaCompatibility, SchemaMismatch, Type, TypeKind, ValueCodec, ValueError, ValueMatch,
};
pub use identity::{
    CallableId, ClientConnectionId, ComponentId, ConfigurationFrontendId, ContextResourceId,
    ContextRevisionId, EventTypeId, GenerationId, InterfaceId, ModelFeatureGenerationId, ModelId,
    PermissionId, PluginId, PluginRuntimeId, ReferenceGenerationId, ResourceNamespace,
    RoutingProfileId, SdkNamespace, SdkResourceId, ServiceId, SessionId, SkillId, SubscriptionId,
};
pub use interface::{
    ComponentInterface, InterfaceCompatibility, InterfaceSchema, InterfaceSchemaMismatch,
};

// Temporary probe: shared contract changes should select reverse-dependency checks.
#[doc(hidden)]
pub const CI_SHARED_CONTRACT_IMPACT_PROBE: u8 = 1;
