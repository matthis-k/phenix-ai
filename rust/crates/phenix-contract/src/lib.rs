//! Wire-stable Phenix contract, value, identity, and interface primitives.

mod contract;
mod contract_wire;
mod identity;
mod infallible_value;
mod interface;
mod std_value;
mod structural_value;

pub use contract::{
    Bytes, CallableRef, CapabilityOwnerId, Contract, ContractId, ContractValue, Exact,
    HasPhenixSchema, Key, ObjectRef, PhenixContract, PhenixSchema, PhenixValue, Project,
    ReferenceId, SchemaCompatibility, SchemaMismatch, Type, TypeKind, ValueCodec, ValueError,
    ValueMatch,
};
pub use identity::{
    CallableId, CapabilityGenerationId, CapabilityId, ClientConnectionId, ComponentId,
    ConfigurationFrontendId, ContextResourceId, ContextRevisionId, EventTypeId, GraphGenerationId,
    InterfaceId, ModelId, PluginId, ResourceNamespace, RoutingProfileId, RuntimeId, SdkNamespace,
    SdkResourceId, ServiceId, SessionId, SkillId, SubscriptionId,
};
pub use interface::{
    ComponentInterface, InterfaceCompatibility, InterfaceSchema, InterfaceSchemaMismatch,
};
