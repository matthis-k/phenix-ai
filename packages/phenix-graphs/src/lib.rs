//! Named, bounded flow-based networks with owned cooperative processes.
//! Components declare immutable interfaces; composition, compilation, and runtime
//! preserve explicit type, contract, resource, and provenance boundaries.
//!
//! Components expose named typed streams. Primitive and composite definitions use
//! the same `ComponentRef`; compilation flattens modules, and a persistent session
//! owns bounded FIFO connections and one mutable process per physical instance.
//!
//! ## Closure Components
//!
//! The closure conveniences use the same cooperative process and publication path
//! as custom processes. A sink has no dummy output. Sources start explicitly.
//!
//! ```rust
//! use phenix_graph::{ComponentDef, NetworkBuilder};
//! use std::{num::NonZeroUsize, sync::{Arc, Mutex}};
//! let capacity = NonZeroUsize::new(1).unwrap();
//! let observed = Arc::new(Mutex::new(Vec::new()));
//! let sink_values = observed.clone();
//! let mut network = NetworkBuilder::new();
//! let source = network.add("source", Arc::new(ComponentDef::source(|| 1_i64..=3)))?;
//! let map = network.add("double", Arc::new(ComponentDef::map(|n: i64| n * 2)))?;
//! let sink = network.add("sink", Arc::new(ComponentDef::sink(move |n: &i64| {
//!     sink_values.lock().unwrap().push(*n);
//! })))?;
//! network.connect(source.output::<i64>("output")?, map.input::<i64>("input")?, capacity)?;
//! network.connect(map.output::<i64>("output")?, sink.input::<i64>("input")?, capacity)?;
//! network.start(&source)?;
//! let mut session = network.build()?.compile()?.start();
//! let report = session.run_until_idle(128);
//! assert_eq!(report.state, phenix_graph::runtime::SessionState::Completed);
//! assert_eq!(*observed.lock().unwrap(), [2, 4, 6]);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//!
//! See [network] for replacement and authoring, [resource] for capabilities,
//! [runtime] for request queues, and [compose] for composition limits.
//!
//! ## Runnable examples
//!
//! - `cargo run -p phenix-graph --example basic_fbp --locked`: source, map, sink.
//! - `cargo run -p phenix-graph --example streaming_fbp --locked`: capacity-one
//!   split, stateful join, finite feedback, repeated injection and shutdown.
//! - `cargo run -p phenix-graph --example plugin_composition --locked`: owned edits,
//!   primitive/composite replacement, dependency-ordered splices, conflicts and revocation.
//! - `cargo run -p phenix-graph --example owned_dispatch --locked`: bounded worker
//!   handoff, delayed completion and publication gating.
//! - `cargo run -p phenix-graph --example resource_capabilities --locked`: private
//!   application read/write facades and exact-type capability grants.
//!

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
