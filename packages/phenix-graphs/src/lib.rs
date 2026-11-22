//! Named, bounded flow-based networks with owned cooperative processes.
//! Components declare immutable interfaces; composition, compilation, and runtime
//! preserve explicit type, contract, resource, and provenance boundaries.
#![doc = include_str!("../API-EXAMPLES.md")]

extern crate self as phenix_graph;

pub(crate) fn discard_panic_payload(payload: Box<dyn std::any::Any + Send>) {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(payload))) {
        std::mem::forget(payload);
    }
}

pub mod component;
pub mod compose;
pub mod connection;
pub mod contract;
pub mod correlation;
mod failure;
mod graph_analysis;
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
    ComposeError, Composer, Composition, Edit, Footprint, InvalidEditReason, NetworkPatch,
    Registration,
};
pub use connection::{EdgeAdapter, IdentityEdgeAdapter};
pub use contract::Contract;
pub use failure::{CallbackPhase, Failure, FailureContext, FailureKind, WrongFailureValue};
pub use interface::{Interface, InterfaceError, InterfacePhase, PortDef};
pub use limits::{CompileLimit, CompileLimits, LimitKind, MAX_IDENTIFIER_BYTES};
pub use network::{
    BoundComponent, BoundDeclaration, BoundInput, BoundOutput, CaptureHandle, CompiledNetwork,
    ComponentRef, IngressHandle, InputEndpoint, InputRef, NetworkBuilder, NetworkDef, NetworkError,
    OutputEndpoint, OutputRef, PortPath, SourceHandle,
};
pub use phenix_schema::Schema;
pub use port_keys::{InputError, InputGroup, InputKey, InputRecord, OutputKey};
pub use process::{Process, ProcessContext, ProcessEmission, ProcessFactory, Step};
pub use provenance::GraphOrigin;
pub use resource::{ResourceError, ResourceGrant, ValidatedResourceSet};
pub use runtime::{NetworkSession, SessionOptions};
pub use value::{CodecError, ContractValue, Leased, TypeIdentity, Value, ValueError};

#[cfg(feature = "derive")]
pub use phenix_port_args_derive::{ContractValue, Ports};
