//! Named, bounded flow-based networks with owned cooperative processes.
//! Components declare immutable interfaces; composition, compilation, and runtime
//! preserve explicit type, contract, resource, and provenance boundaries.
//! # Phenix FBP
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
//! ## Interchangeable Bodies
//!
//! Replacement checks the whole public interface and semantic obligations. Adjacent
//! symbolic connections do not change, but old authoring handles are revoked.
//!
//! ```rust
//! use phenix_graph::{ComponentDef, NetworkBuilder};
//! use std::{num::NonZeroUsize, sync::Arc};
//! let primitive = Arc::new(ComponentDef::map(|n: i64| n + 1));
//! let mut body = NetworkBuilder::new();
//! let child = body.add("child", primitive.clone())?;
//! body.expose_input("input", child.input::<i64>("input")?, NonZeroUsize::new(1).unwrap())?;
//! body.expose_output("output", child.output::<i64>("output")?)?;
//! let composite = Arc::new(ComponentDef::composite(body.build()?));
//! let mut network = NetworkBuilder::new();
//! let a = network.add("a", primitive.clone())?;
//! let b = network.module("b", match composite.body() {
//!     phenix_graph::Body::Composite(body) => (**body).clone(),
//!     _ => unreachable!(),
//! })?;
//! network.connect(a.output::<i64>("output")?, b.input::<i64>("input")?, NonZeroUsize::new(1).unwrap())?;
//! let a = network.replace(&a, composite)?;
//! let _b = network.replace(&b, primitive)?;
//! network.expose_input("request", a.input::<i64>("input")?, NonZeroUsize::new(1).unwrap())?;
//! let compiled = network.build()?.compile()?;
//! assert_eq!(compiled.processes().len(), 2);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Streams And Ownership
//!
//! `inject` returns the original value on `Full`, type mismatch or closure. A retained
//! emission waits for all selected destinations without rerunning its action.
//! `run_until_idle(budget)` yields at quiescence, delayed work or backpressure; it
//! does not imply completion. Close public inputs and drain captures to complete.
//! Feedback is legal, but full queue cycles can deadlock and are diagnostic.
//!
//! Custom `Process::step` implementations return one `Step::Emit` at a time.
//! `Step::Complete` settles an activation; `Step::Finished` terminates the process.
//! `WaitInput`, `WaitInputs` and `WaitWake` preserve its continuation. Typed `Ports`
//! keys and input-record reconstruction are optional; manual interfaces use the
//! same representation and do not require derive support.
//!
//! Local values use `TypeId`; encoded values use the complete versioned schema and
//! codec identity. Structural compatibility never establishes a semantic `Contract`
//! claim. Values carrying plugin/resource leases reject bare owned extraction;
//! `borrow`, `leased_shared` and `decoded` preserve the ownership boundary.
//!
//! ## Resource Capabilities
//!
//! Graph resolves opaque capabilities supplied by the host. It provides no store,
//! resource locks, read/write callbacks, or permission engine. A capability's public
//! API defines its authority: an application-private read facade must not expose its
//! inner mutable handle or a conversion to a writer. A write facade may expose both
//! read and write operations. Real Storage handles and their authorization remain
//! outside Graph; no Graph-specific storage dependency is required.
//!
//! ```rust
//! use phenix_graph::{ProcessContext, ResourceError, ResourceGrant, ValidatedResourceSet};
//! use std::sync::Arc;
//! let capability = Arc::new(String::from("application-owned capability"));
//! let resources = ValidatedResourceSet::try_new(vec![
//!     ResourceGrant::new("service", capability.clone()),
//! ])?;
//! let context = ProcessContext::default().with_resources(resources);
//! assert!(Arc::ptr_eq(context.resource::<Arc<String>>("service")?, &capability));
//! assert!(matches!(context.resource::<String>("service"), Err(ResourceError::WrongType("service"))));
//! assert!(matches!(context.resource::<String>("absent"), Err(ResourceError::NotGranted("absent"))));
//! # Ok::<(), ResourceError>(())
//! ```
//!
//! `ResourceGrant::new(key, capability)` preserves the exact Rust `Any` type, including
//! an explicit `Arc<C>` wrapper. Lookup returns `&C` and performs no capability
//! operation. `ValidatedResourceSet::try_new` rejects duplicate keys regardless of
//! type or order, before a set can be used in a context or session. Rejection consumes
//! the input and may execute application destructors. Empty/default sets are valid;
//! clones preserve validation and share the granted capabilities. The same key is
//! valid in different processes, without implying shared state. Configure each
//! physical path through `SessionOptions::resources`; auto-started sources, explicit
//! request sources, and ordinary reservations receive clones of that process's set.
//! Recovery replaces process-private state, not these external capabilities or effects.
//!
//! `with_lease` appends a capability-specific code/resource lease. Each grant keeps
//! all its leases alive through capability destruction and destruction of a caught
//! panic payload, then attempts each lease drop separately. Direct grant/set drops,
//! rejected sets, and retained session sets use the same cleanup. Managed context
//! cleanup reports caught destructor panics through the existing handoff failure
//! path; ordinary `Drop` cannot report them. This is not a sandbox or a guarantee
//! against all in-process aborts, non-yielding destructors, or double panics inside
//! one application's own destructor. Resources do not schedule work or roll back
//! committed external effects.
//!
//! The executable `resource_capabilities` example keeps the facade's lock private.
//! Its exact source is compile-checked from outside the facade module: a reader
//! cannot call `write`, access the inner lock, or convert into a writer. The writer
//! can both read and write the same external state.
//!
//! ## Executable Examples
//!
//! ### Independent Request Queues
//!
//! Shared FIFO is the default. Opt-in bounded lanes keep FIFO within each request and
//! separate ingress, internal edges and captures, without duplicating process state.
//! Use request-specific capture reads in this mode; `take_output` reads unscoped traffic.
//! A process holding a staged continuation or blocked publication still serializes
//! other requests. Queue capacities bound packet counts, not arbitrary payload bytes.
//!
//! ```rust
//! use phenix_graph::{ComponentDef, InputKey, NetworkBuilder, SessionOptions};
//! use phenix_graph::runtime::{RequestQueuePolicy, RequestStatus};
//! use std::{num::NonZeroUsize, sync::Arc, time::Duration};
//! let mut network = NetworkBuilder::new();
//! let map = network.add("map", Arc::new(ComponentDef::map(|n: i64| n + 1)))?;
//! network.expose_input("input", map.input::<i64>("input")?, NonZeroUsize::new(1).unwrap())?;
//! network.expose_output("output", map.output::<i64>("output")?)?;
//! let mut session = network.build()?.compile()?.start_with_options(SessionOptions {
//!     max_requests: NonZeroUsize::new(2).unwrap(),
//!     capture_capacity: NonZeroUsize::new(1).unwrap(),
//!     request_queue_policy: RequestQueuePolicy::BoundedLanes,
//!     deferred_timeout: Some(Duration::from_secs(30)),
//!     ..Default::default()
//! });
//! let slow = session.begin_request(Some("output"))?;
//! let fast = session.begin_request(Some("output"))?;
//! session.inject_request_key(slow, InputKey::<i64>::new("input"), 1)?;
//! session.inject_request_key(fast, InputKey::<i64>::new("input"), 2)?;
//! session.close_request(slow)?;
//! session.close_request(fast)?;
//! session.run_until_idle(16);
//! let packet = session.take_request_output(fast, "output")?.unwrap();
//! assert_eq!(packet.value().borrow::<i64>(), Some(&3));
//! assert_eq!(session.request_status(fast)?, RequestStatus::Settled { captures: 1 });
//! session.release_request(fast)?;
//! assert!(session.take_request_output(slow, "output")?.is_some());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Deferred timeouts are cooperative ticket invalidation, not worker preemption.
//! The built-in driver times only jobs whose dispatcher returns `Deferred`; manual
//! `next_job` handoff starts its timer immediately. Status queries are snapshots until
//! the session is supervised/driven via `supervise_deferred(Instant::now())` or a driver
//! operation. `overdue_jobs(now)` is passive; `RunReport::next_deferred_timeout` bounds
//! the driver's next wait. Execution guards are an ordered
//! `SessionOptions::execution_guards` list; `ActivationJob::with_guard` appends rather
//! than replacing installed guards, and guard teardown happens in reverse order.
//!
//! `inject_key` and `inject_request_key` pair an existing typed input key with its
//! payload. `InputKey::new` constructs a local Value; `encoded` uses the canonical
//! wire codec/schema. Keys name public exports and grant no authority or topology
//! identity. Conversion runs before admission checks, and failures retain the
//! constructed Value. Runtime-discovered inputs continue using raw `inject`.
//!
//! For a pending request, `request_blocked_queues(id)` passively reports its exact
//! full publication destinations, including occupancy/capacity. Attribution follows
//! the pending producer request, not the request owning older buffered packets.
//! Foreign/released request IDs reject; the query neither supervises deadlines nor
//! proves deadlock or progress. `blocked_queues()` remains the session-wide view.
//!
//! ### Bounded Composition Selection
//!
//! `Composer` requires exactly one consistent dependency/priority selection before
//! replay and compilation. A lower-priority registration is eligible again when its
//! only blocker is itself ineligible; provisional defeat is not permanent exclusion.
//! Zero or multiple models reject as `NoConsistentSelection` or `AmbiguousSelection`.
//!
//! Use `composer.set_selection_limits(SelectionLimits { max_work: ... })` from
//! `phenix_graph::compose` to change the finite allowance for future updates. The
//! default is 1,000,000 charged selector operations; exhausted construction/search or
//! uniqueness proof returns `SelectionBudgetExceeded`, even if one model was found.
//! Changing limits alone leaves current bindings/topology intact. This allowance
//! does not bound dependency/footprint planning, replay, compilation or total build
//! time. Kernel hosts configure both composer paths through `BuildOptions::selection_limits`.
//!
//! Composition planning has a separate `PlanningLimits { max_units: 1_000_000 }`
//! default. Configure future `register`/`revoke` updates with
//! `composer.set_planning_limits(...)`. This is a deliberate new host admission
//! policy, not compatibility for all formerly unbounded valid inputs: 2,048
//! independent 512-byte registration IDs reject at the default and can be admitted
//! with a larger finite allowance. The pinned default success envelope includes
//! real-write chains/diamonds/skew through 128 registrations, dense priorities and
//! ordered splice/hierarchical cases through 32, plus Kernel prefixes/self-previews
//! through 32 contributions. These are tested examples, not universal capacity promises.
//!
//! Units reserve logical planning operations/storage and symbolic query/copy/scan
//! bytes before work. Blockers reserve two index cells, ancestor rows their full
//! logical Boolean length, and generated paths their bytes before allocation.
//! Explicit predicate operands and possible diagnostic copies are preflighted;
//! internal BTree comparison multiplicity is excluded. Reservations are conservative
//! and never refunded. First denied reservation returns `PlanningBudgetExceeded`
//! before any later semantic error; with sufficient allowance semantic order remains
//! unchanged. Selection is still independently budgeted per attempt. Incoming owned
//! input construction, opaque payloads, replay/network copying/compiler work,
//! arbitrary callbacks/destructors and allocation failure are outside this logical
//! allowance. It is not a wall-clock/allocator-byte/total-work guarantee.
//!
//! For a synchronous host scope, `register_with_planning_usage(registrations,
//! &mut used_units)` consumes an ordinary caller-owned counter, including on failed
//! attempts. Use the same counter and configured limit on all participating
//! Composers; the borrow is not retained. Ordinary `register`/`revoke` start fresh
//! per-update counters. There is no public meter, cross-thread sharing, or cumulative
//! revoke API. Kernel configures `BuildOptions::planning_limits` and shares one
//! counter across the complete build's main prefixes and static self-import previews.
//!
//! Updates prepare before committing. `into_iter`, `next`, iterator Drop, and
//! rejected candidate cleanup are caught before publication; cleanup cannot replace
//! an established semantic/budget error. Only nondropping field replacements commit
//! registry and composition. A catchable retired-state Drop panic after commit is
//! suppressed and the published update remains successful. Failed attempts keep
//! old registry/topology/generation; successful unchanged topology still refreshes
//! generation. Abort, double panic and nontermination are outside the native boundary.
//!
//! ### Programs
//!
//! - `cargo run -p phenix-graph --example basic_fbp --locked`: source, map, sink.
//! - `cargo run -p phenix-graph --example streaming_fbp --locked`: capacity-one
//!   split, stateful join, finite feedback, repeated injection and shutdown.
//! - `cargo run -p phenix-graph --example plugin_composition --locked`: owned edits,
//!   Node/Module replacement, dependency-ordered splices, conflicts and revocation.
//! - `cargo run -p phenix-graph --example owned_dispatch --locked`: bounded worker
//!   handoff, delayed completion and publication gating.
//! - `cargo run -p phenix-graph --example resource_capabilities --locked`: private
//!   application read/write facades and exact-type capability grants.
//!
//! Kernel owns producer lifecycle and selection policy. StorageGraph observes
//! authorized subscriptions and uses native request scopes; neither owns another
//! scheduler. Native behavior and migration coverage are recorded in
//! `docs/fbp-regression-ledger.md` at the workspace root.

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
