//! Named, bounded flow-based networks with owned cooperative processes.
//! Components declare immutable interfaces; composition, compilation, and runtime
//! preserve explicit type, contract, resource, and provenance boundaries.
#![doc = include_str!("../API-EXAMPLES.md")]

extern crate self as phenix_graph;

pub mod component;
pub mod compose;
pub mod connection;
pub mod contract;
pub mod interface;
mod limits;
pub mod network;
mod port_keys;
pub mod process;
mod provenance;
pub mod resource;
pub mod runtime;
pub mod testing;
mod value;

pub use component::{Body, ComponentDef};
pub use compose::{
    ComposeError, Composer, Composition, Edit, Footprint, NetworkPatch, Registration,
};
pub use connection::{EdgeAdapter, IdentityEdgeAdapter};
pub use contract::Contract;
pub use interface::{Interface, InterfaceError, InterfacePhase, PortDef};
pub use limits::{CompileLimits, MAX_IDENTIFIER_BYTES};
pub use network::{
    CompiledNetwork, ComponentRef, InputEndpoint, InputRef, NetworkBuilder, NetworkDef,
    NetworkError, OutputEndpoint, OutputRef, PortPath,
};
pub use phenix_schema::Schema;
pub use port_keys::{InputGroup, InputKey, InputRecord, OutputKey};
pub use process::{Process, ProcessContext, ProcessEmission, ProcessFactory, ResourceGrant, Step};
pub use provenance::GraphOrigin;
pub use resource::Store;
pub use runtime::{NetworkSession, SessionOptions};
pub use value::{ContractValue, Leased, TypeIdentity, Value};

#[cfg(feature = "derive")]
pub use phenix_port_args_derive::{ContractValue, Ports};
