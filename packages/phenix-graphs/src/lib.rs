//! Phenix standalone dataflow engine (Rust 2024).
//! Graph definitions, composition, compilation and execution use one engine;
//! optional derives and test fixtures are clients of that same implementation.
//! Author graphs with `GraphBuilder` and typed node/module operations.
//! Zero-argument nodes remain portless and run only on an explicit source trigger:
//!
//! ```
//! use phenix_graph::{Deployment, GraphBuilder};
//! let mut graph = GraphBuilder::new("source-example");
//! let source = graph.node("source", || 42_i64).map_err(|e| e.to_string())?;
//! let sink = graph.node("sink", |x: i64| x + 1).map_err(|e| e.to_string())?;
//! graph.connect(source.output(), sink.inlet())?;
//! graph.trigger("start", [source.operation().start()])?;
//! let engine = Deployment::new(graph.compile().map_err(|e| e.to_string())?).build()?;
//! let report = engine.fire("start", vec![])?;
//! assert!(report.success());
//! # Ok::<(), String>(())
//! ```
//!
//! `ModuleDefBuilder` declares physical compiler structures, not typed endpoints.
//! Expected `WaveError` values are successful negative-path test outcomes.
//! Outside scope: preemptive task cancellation, replay, remote operations,
//! transaction rollback, production ABI, automatic cycle recovery.
//! Run: cargo test --manifest-path Cargo.toml

//! Phenix trigger/port/slot/store dataflow prototype.
//!
//! Single-threaded, standard library only.
//!
//! Includes original regression tests: expression DAGs, repeated invocations,
//! typed two-of-three selection, pair-then-third phase obligations,
//! upstream coloring, skipped branches, independent cohorts, and guards.
//!
//! A color is NOT an authorization capability: it is derived from admitted,
//! queued/running invocations and reachability to the target port. Ports may
//! wait for color exhaustion before choosing among available deliveries.
//!
//! This is a semantic executable model, NOT the production ABI. In particular,
//! policy authorship is dynamic here; production can generate erased adapters
//! from typed generic policies. Cooperative wave cancellation is modeled;
//! external task termination/replay/crash recovery are not.

use std::any::{Any, TypeId, type_name};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::marker::PhantomData;

// Allows derive expansion to use the same paths in unit tests and downstream crates.
extern crate self as phenix_graph;

pub mod interface;
pub use interface::{
    BoundInput, BoundOutput, BoundUnit, GraphContract, GraphUnit, InterfaceError, InterfacePhase,
    InterfaceShape, UnitProvenance, UnitSnapshot,
};
pub mod authoring;
pub use authoring::{AuthoringUnit, BoundAuthoringUnit};
pub mod deployment;
pub mod output;
pub use output::{Destination, Output};
pub mod testing;
pub use authoring::{
    AssembledGraph, AssemblyChange, AssemblyRevision, ConnectionRoute, DeclaredValue,
    DiscoveredOperation, EncodedValue, FieldSelector, GraphAssembly, GraphBuilder, InletLayout,
    InletPort, InletView, Inlets, Input, InputInterface, Layout, LayoutMode, LocalValue, Module,
    ModuleHandle, ModulePackage, NoPorts, Node, NodeDefinition, NodeHandle, NodeInputs,
    NodeSignature, Operation, OutputRef, PatchAnchor, PatchBuilder, PatchDestination, PatchError,
    PatchPlanner, PatchResolution, PatchSource, PatchSpec, PatchTarget, PlannedSplice,
    PortInterface, PortSelector, Ports, PublicInlets, PublicInput, PublicView, Replacement,
    ReplacementKind, SinglePort, SlotField, SlotPath, SourceStart, SplicePosition, SupersededPatch,
    TypedModulePackage, ValueMode,
};
pub use deployment::{Deployment, ExecutionLimits};
#[cfg(feature = "derive")]
pub use phenix_port_args_derive::{
    ContractValue, InputFields, LocalPortLayout, PortArgs, PortLayout, patch,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

/// Safety limits for flattening graph definitions. These constrain compilation,
/// not runtime recursion or the number of graph waves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompileLimits {
    pub max_depth: usize,
    pub max_instances: u64,
    pub max_work: u64,
}
impl Default for CompileLimits {
    fn default() -> Self {
        Self {
            max_depth: 128,
            max_instances: 4096,
            max_work: 1_000_000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(u16);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PortId(u16);
impl NodeId {
    pub const fn new(id: u16) -> Self {
        Self(id)
    }
    pub const fn get(self) -> u16 {
        self.0
    }
}
impl PortId {
    pub const fn new(id: u16) -> Self {
        Self(id)
    }
    pub const fn get(self) -> u16 {
        self.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SlotId(u16);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EdgeId(u16);
impl EdgeId {
    pub const fn new(id: u16) -> Self {
        Self(id)
    }
    pub const fn get(self) -> u16 {
        self.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TriggerId(u16);
impl TriggerId {
    pub const fn new(id: u16) -> Self {
        Self(id)
    }
    pub const fn get(self) -> u16 {
        self.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct WaveId(u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct InvocationId(u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct DeliveryId(u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CohortId(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InputAddr {
    node: NodeId,
    port: PortId,
    slot: SlotId,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OutputAddr {
    node: NodeId,
    slot: SlotId,
}
impl InputAddr {
    pub fn node_id(self) -> u16 {
        self.node.0
    }
    pub fn port_id(self) -> u16 {
        self.port.0
    }
    pub fn slot_id(self) -> u16 {
        self.slot.0
    }
}
impl OutputAddr {
    pub fn node_id(self) -> u16 {
        self.node.0
    }
    pub fn slot_id(self) -> u16 {
        self.slot.0
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct InputHandle<T: ContractValue> {
    address: InputAddr,
    _type: PhantomData<fn() -> T>,
}
#[derive(Debug, PartialEq, Eq)]
pub struct OutputHandle<T: ContractValue> {
    address: OutputAddr,
    _type: PhantomData<fn() -> T>,
}
impl<T: ContractValue> Copy for InputHandle<T> {}
impl<T: ContractValue> Clone for InputHandle<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: ContractValue> Copy for OutputHandle<T> {}
impl<T: ContractValue> Clone for OutputHandle<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: ContractValue> InputHandle<T> {
    pub fn address(self) -> InputAddr {
        self.address
    }
}
impl<T: ContractValue> OutputHandle<T> {
    pub fn address(self) -> OutputAddr {
        self.address
    }
}
pub fn input(node: u16, port: u16, slot: u16) -> InputAddr {
    InputAddr {
        node: NodeId(node),
        port: PortId(port),
        slot: SlotId(slot),
    }
}
pub fn output(node: u16, slot: u16) -> OutputAddr {
    OutputAddr {
        node: NodeId(node),
        slot: SlotId(slot),
    }
}
impl SlotId {
    pub const fn new(id: u16) -> Self {
        Self(id)
    }
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Local Rust identities never depend on `type_name` strings. Wire types
/// retain explicit versioned schema names and are never silently compatible
/// with local Rust values, even if the textual name happens to match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeIdentity {
    Local(TypeId),
    Wire(Arc<str>),
}
fn identity(schema: &Arc<str>, local_type: Option<TypeId>) -> TypeIdentity {
    match local_type {
        Some(id) => TypeIdentity::Local(id),
        None => TypeIdentity::Wire(schema.clone()),
    }
}

/// The wire format is schema identity plus canonical bytes for that schema.
/// Only a few codecs are included to keep this experiment self-contained.
/// Values are locally typed payloads or explicitly encoded wire envelopes.
/// A local payload is never encoded; fan-out clones only its Arc.
#[derive(Clone)]
pub struct Value {
    schema: Arc<str>,
    bytes: Vec<u8>,
    local: Option<Arc<dyn Any + Send + Sync>>,
}
impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Value")
            .field("schema", &self.schema)
            .field("wire_bytes", &self.bytes.len())
            .field("is_local", &self.local.is_some())
            .finish()
    }
}
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        self.schema == other.schema
            && match (&self.local, &other.local) {
                (None, None) => self.bytes == other.bytes,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
    }
}
impl Eq for Value {}
pub use phenix_schema::Schema;

pub trait ContractValue: Sized + Send + Sync + 'static {
    /// Complete wire contract, independent of the implementing Rust type.
    fn schema() -> Schema;
    /// Owned canonical identity. Consumers can cheaply clone the returned Arc.
    fn wire_schema() -> Arc<str> {
        Self::schema().identity()
    }
    fn encode(&self) -> Vec<u8>;
    fn decode(bytes: &[u8]) -> Result<Self, String>;
}
impl ContractValue for i64 {
    fn schema() -> Schema {
        Schema {
            name: "example.i64".into(),
            revision: 1,
            structure: "i64".into(),
            codec: "i64-le-v1".into(),
        }
    }
    fn encode(&self) -> Vec<u8> {
        self.to_le_bytes().to_vec()
    }
    fn decode(bytes: &[u8]) -> Result<Self, String> {
        let arr: [u8; 8] = bytes.try_into().map_err(|_| "bad i64 length".to_string())?;
        Ok(i64::from_le_bytes(arr))
    }
}
impl ContractValue for String {
    fn schema() -> Schema {
        Schema {
            name: "example.utf8".into(),
            revision: 1,
            structure: "string".into(),
            codec: "utf8-v1".into(),
        }
    }
    fn encode(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
    fn decode(bytes: &[u8]) -> Result<Self, String> {
        String::from_utf8(bytes.to_vec()).map_err(|e| e.to_string())
    }
}
macro_rules! integer_contract {
    ($($ty:ty => $schema:literal),* $(,)?) => {$ (
        impl ContractValue for $ty {
            fn schema() -> Schema {
                Schema { name: $schema.into(), revision: 1, structure: stringify!($ty).into(), codec: concat!(stringify!($ty), "-le-v1").into() }
            }
            fn encode(&self) -> Vec<u8> { self.to_le_bytes().to_vec() }
            fn decode(bytes: &[u8]) -> Result<Self, String> {
                let array: [u8; std::mem::size_of::<Self>()] = bytes.try_into()
                    .map_err(|_| format!("bad {} length", stringify!($ty)))?;
                Ok(<$ty>::from_le_bytes(array))
            }
        }
    )*};
}
integer_contract!(
    i8 => "example.i8", i16 => "example.i16", i32 => "example.i32",
    i128 => "example.i128", u8 => "example.u8", u16 => "example.u16",
    u32 => "example.u32", u64 => "example.u64", u128 => "example.u128",
    f32 => "example.f32", f64 => "example.f64",
);
impl ContractValue for bool {
    fn schema() -> Schema {
        Schema {
            name: "example.bool".into(),
            revision: 1,
            structure: "bool".into(),
            codec: "bool-u8-v1".into(),
        }
    }
    fn encode(&self) -> Vec<u8> {
        vec![u8::from(*self)]
    }
    fn decode(bytes: &[u8]) -> Result<Self, String> {
        match bytes {
            [0] => Ok(false),
            [1] => Ok(true),
            _ => Err("bad bool encoding".into()),
        }
    }
}
impl ContractValue for () {
    fn schema() -> Schema {
        Schema {
            name: "example.unit".into(),
            revision: 1,
            structure: "unit".into(),
            codec: "empty-v1".into(),
        }
    }
    fn encode(&self) -> Vec<u8> {
        Vec::new()
    }
    fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.is_empty() {
            Ok(())
        } else {
            Err("unit value must be empty".into())
        }
    }
}
fn enc<T: ContractValue>(v: T) -> Value {
    Value {
        schema: T::wire_schema(),
        bytes: v.encode(),
        local: None,
    }
}
impl Value {
    pub fn of<T: ContractValue>(value: T) -> Self {
        enc(value)
    }
    /// Only a prototype wire envelope. No Rust layout or trait object crosses it.
    pub fn schema(&self) -> &Arc<str> {
        &self.schema
    }
    pub fn identity(&self) -> TypeIdentity {
        match &self.local {
            Some(value) => TypeIdentity::Local(value.as_ref().type_id()),
            None => TypeIdentity::Wire(self.schema.clone()),
        }
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn from_wire_parts(schema: Arc<str>, bytes: Vec<u8>) -> Self {
        Self {
            schema,
            bytes,
            local: None,
        }
    }
    /// Construct an in-process, non-serializing value. No codec trait required.
    pub fn local<T: Send + Sync + 'static>(value: T) -> Self {
        Self {
            schema: type_name::<T>().into(),
            bytes: Vec::new(),
            local: Some(Arc::new(value)),
        }
    }
    /// A typed view of local data; never copies or decodes the payload.
    pub fn borrow<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.local.as_ref()?.downcast_ref::<T>()
    }
    /// Shared ownership of locally stored data, including non-Clone types.
    pub fn shared<T: Send + Sync + 'static>(&self) -> Result<Arc<T>, String> {
        let data = self.local.as_ref().ok_or("value is encoded, not local")?;
        Arc::downcast::<T>(Arc::clone(data)).map_err(|_| {
            format!(
                "local value type mismatch: expected {}, found {}",
                type_name::<T>(),
                self.schema
            )
        })
    }
    /// Consume a local payload without cloning when it is uniquely owned.
    /// A shared payload is returned intact so the caller can choose its policy.
    pub fn into_shared<T: Send + Sync + 'static>(self) -> Result<Arc<T>, String> {
        let data = self.local.ok_or("value is encoded, not local")?;
        Arc::downcast::<T>(data)
            .map_err(|_| format!("local value type mismatch: expected {}", type_name::<T>()))
    }
    pub fn into_owned<T: Clone + Send + Sync + 'static>(self) -> Result<T, String> {
        let shared = self.into_shared::<T>()?;
        Ok(Arc::try_unwrap(shared).unwrap_or_else(|value| (*value).clone()))
    }
    pub fn is_local(&self) -> bool {
        self.local.is_some()
    }
    pub fn get<T: ContractValue>(&self) -> Result<T, String> {
        if self.schema != T::wire_schema() {
            return Err(format!(
                "expected {}, got {}",
                T::wire_schema(),
                self.schema
            ));
        }
        if self.is_local() {
            return Err("local values are not encoded; use borrow/shared".into());
        }
        T::decode(&self.bytes)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferMode {
    Immediate,
    Deferred,
}
#[derive(Clone, Debug)]
pub struct InputSlot {
    pub id: SlotId,
    pub name: Option<String>,
    /// For human-readable diagnostics, not local identity checks.
    pub schema: Arc<str>,
    pub local_type: Option<TypeId>,
    pub buffer: BufferMode,
}
/// Select zero or more of an output's existing edges. The selector never
/// creates edges, grants authority, or performs delivery; the engine validates
/// its entire selection before any value enters downstream input buffers.
/// Selectors must be deterministic and side-effect-free for a given value and
/// candidate edge list.
#[derive(Clone, Debug)]
pub struct OutputSlot {
    pub id: SlotId,
    pub name: Option<String>,
    pub schema: Arc<str>,
    pub local_type: Option<TypeId>,
}
impl InputSlot {
    pub fn identity(&self) -> TypeIdentity {
        identity(&self.schema, self.local_type)
    }
    pub fn new(id: u16, schema: Arc<str>, buffer: BufferMode) -> Self {
        slot(id, schema, buffer)
    }
    pub fn immediate<T: ContractValue>(id: u16) -> Self {
        Self::new(id, T::wire_schema(), BufferMode::Immediate)
    }
    pub fn deferred<T: ContractValue>(id: u16) -> Self {
        Self::new(id, T::wire_schema(), BufferMode::Deferred)
    }
    pub fn local<T: Send + Sync + 'static>(id: u16) -> Self {
        let mut slot = Self::new(id, type_name::<T>().into(), BufferMode::Deferred);
        slot.local_type = Some(TypeId::of::<T>());
        slot
    }
    /// Declarative name: validated during graph compilation.
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}
impl OutputSlot {
    pub fn identity(&self) -> TypeIdentity {
        identity(&self.schema, self.local_type)
    }
    pub fn new(id: u16, schema: Arc<str>) -> Self {
        out(id, schema)
    }
    pub fn typed<T: ContractValue>(id: u16) -> Self {
        Self::new(id, T::wire_schema())
    }
    pub fn local<T: Send + Sync + 'static>(id: u16) -> Self {
        let mut slot = Self::new(id, type_name::<T>().into());
        slot.local_type = Some(TypeId::of::<T>());
        slot
    }
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}

#[derive(Clone, Debug)]
pub struct Candidate {
    slot: SlotId,
    delivery: DeliveryId,
    cohort: CohortId,
}
impl Candidate {
    pub fn slot_id(&self) -> u16 {
        self.slot.0
    }
    pub fn delivery_id(&self) -> u64 {
        self.delivery.0
    }
    pub fn cohort_id(&self) -> u64 {
        self.cohort.0
    }
}
#[derive(Clone, Debug)]
pub struct SlotSnapshot {
    slot: SlotId,
    name: Option<String>,
    candidates: Vec<Candidate>,
}
impl SlotSnapshot {
    pub fn slot_id(&self) -> u16 {
        self.slot.0
    }
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
    pub fn candidates(&self) -> &[Candidate] {
        &self.candidates
    }
}
#[derive(Clone, Debug)]
pub struct Pick {
    slot: SlotId,
    delivery: DeliveryId,
}
impl Pick {
    pub fn new(slot: u16, delivery_id: u64) -> Self {
        Self {
            slot: SlotId(slot),
            delivery: DeliveryId(delivery_id),
        }
    }
    pub fn slot_id(&self) -> u16 {
        self.slot.0
    }
    pub fn delivery_id(&self) -> u64 {
        self.delivery.0
    }
}
#[derive(Clone, Debug)]
pub struct Selection {
    cohort: CohortId,
    picks: Vec<Pick>,
}
impl Selection {
    pub fn new(cohort_id: u64, picks: Vec<Pick>) -> Self {
        Self {
            cohort: CohortId(cohort_id),
            picks,
        }
    }
    pub fn cohort_id(&self) -> u64 {
        self.cohort.0
    }
    pub fn picks(&self) -> &[Pick] {
        &self.picks
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncompleteGroup {
    cohort: CohortId,
    present: Vec<SlotId>,
    missing: Vec<SlotId>,
    requirement: &'static str,
}
impl IncompleteGroup {
    pub fn new(
        cohort_id: u64,
        present: Vec<u16>,
        missing: Vec<u16>,
        requirement: &'static str,
    ) -> Self {
        Self {
            cohort: CohortId(cohort_id),
            present: present.into_iter().map(SlotId).collect(),
            missing: missing.into_iter().map(SlotId).collect(),
            requirement,
        }
    }
    pub fn cohort_id(&self) -> u64 {
        self.cohort.0
    }
    pub fn present(&self) -> impl Iterator<Item = u16> + '_ {
        self.present.iter().map(|slot| slot.0)
    }
    pub fn missing(&self) -> impl Iterator<Item = u16> + '_ {
        self.missing.iter().map(|slot| slot.0)
    }
    pub fn requirement(&self) -> &'static str {
        self.requirement
    }
}

/// History records successful, atomic reservations. It does not retain values:
/// consumed values have become invocation arguments (and are not selectable again).
#[derive(Clone, Debug)]
pub struct ReservationRecord {
    invocation: InvocationId,
    cohort: CohortId,
    slots: BTreeSet<SlotId>,
    completed: bool,
}
impl ReservationRecord {
    pub fn invocation_id(&self) -> u64 {
        self.invocation.0
    }
    pub fn cohort_id(&self) -> u64 {
        self.cohort.0
    }
    pub fn slots(&self) -> impl Iterator<Item = u16> + '_ {
        self.slots.iter().map(|slot| slot.0)
    }
    pub fn completed(&self) -> bool {
        self.completed
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColorState {
    Open,
    Terminal(ColorOutcome),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColorOutcome {
    Delivered,
    EmittedTowardReachablePort,
    Closed,
    Failed(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColorFact {
    invocation: InvocationId,
    node: NodeId,
    state: ColorState,
}
impl ColorFact {
    pub fn invocation_id(&self) -> u64 {
        self.invocation.0
    }
    pub fn node_id(&self) -> u16 {
        self.node.0
    }
    pub fn state(&self) -> &ColorState {
        &self.state
    }
}

/// Colors describe admitted upstream work that can reach this port, including
/// its terminal outcome. They are scheduling facts, not plugin permissions.
#[derive(Clone, Debug)]
pub struct PortSnapshot {
    slots: Vec<SlotSnapshot>,
    reservations: Vec<ReservationRecord>,
    colors: BTreeMap<CohortId, Vec<ColorFact>>,
}
impl PortSnapshot {
    pub fn slots(&self) -> &[SlotSnapshot] {
        &self.slots
    }
    pub fn reservations(&self) -> &[ReservationRecord] {
        &self.reservations
    }
    pub fn colors(&self) -> impl Iterator<Item = (u64, &[ColorFact])> {
        self.colors
            .iter()
            .map(|(cohort, facts)| (cohort.0, facts.as_slice()))
    }
    pub fn candidates(&self, slot: u16, cohort_id: u64) -> Option<&Candidate> {
        self.slots
            .iter()
            .find(|snapshot| snapshot.slot == SlotId(slot))?
            .candidates
            .iter()
            .find(|candidate| candidate.cohort.0 == cohort_id)
    }
    pub fn candidates_named(&self, name: &str, cohort_id: u64) -> Option<&Candidate> {
        self.slots
            .iter()
            .find(|slot| slot.name.as_deref() == Some(name))?
            .candidates
            .iter()
            .find(|candidate| candidate.cohort.0 == cohort_id)
    }
    pub fn slot_named(&self, name: &str) -> Option<&SlotSnapshot> {
        self.slots
            .iter()
            .find(|slot| slot.name.as_deref() == Some(name))
    }
    pub fn has_reserved(&self, cohort_id: u64, slots: &[u16]) -> bool {
        let expected: BTreeSet<_> = slots.iter().map(|slot| SlotId(*slot)).collect();
        self.reservations
            .iter()
            .any(|record| record.cohort.0 == cohort_id && record.slots == expected)
    }
    pub fn has_completed(&self, cohort_id: u64, slots: &[u16]) -> bool {
        let expected: BTreeSet<_> = slots.iter().map(|slot| SlotId(*slot)).collect();
        self.reservations.iter().any(|record| {
            record.cohort.0 == cohort_id && record.slots == expected && record.completed
        })
    }
    #[cfg(test)]
    fn cohorts(&self) -> BTreeSet<CohortId> {
        self.slots
            .iter()
            .flat_map(|s| s.candidates.iter().map(|c| c.cohort))
            .chain(self.reservations.iter().map(|r| r.cohort))
            .collect()
    }
    fn open_upstream(&self) -> BTreeMap<CohortId, BTreeSet<NodeId>> {
        self.colors
            .iter()
            .filter_map(|(&cohort, facts)| {
                let nodes: BTreeSet<_> = facts
                    .iter()
                    .filter(|fact| fact.state == ColorState::Open)
                    .map(|fact| fact.node)
                    .collect();
                (!nodes.is_empty()).then_some((cohort, nodes))
            })
            .collect()
    }
    fn excluding_open(&self) -> Self {
        let mut result = self.clone();
        for s in &mut result.slots {
            s.candidates.retain(|c| {
                !self
                    .colors
                    .get(&c.cohort)
                    .is_some_and(|facts| facts.iter().any(|fact| fact.state == ColorState::Open))
            });
        }
        result
    }
}

/// A schema-checked, named view over exactly the atomically chosen deliveries.
/// This is local to the graph runtime; individual Values are schema-tagged bytes.
pub struct SelectedValues<'a> {
    selected: &'a BTreeMap<SlotId, &'a Value>,
    slots: &'a [InputSlot],
}
impl SelectedValues<'_> {
    pub fn get<T: ContractValue>(&self, name: &str) -> Result<T, String> {
        self.optional::<T>(name)?
            .ok_or_else(|| format!("selected input {name:?} is missing"))
    }
    pub fn optional<T: ContractValue>(&self, name: &str) -> Result<Option<T>, String> {
        let slot = self
            .slots
            .iter()
            .find(|slot| slot.name.as_deref() == Some(name))
            .ok_or_else(|| format!("unknown input name {name:?}"))?;
        if slot.schema != T::wire_schema() {
            return Err(format!(
                "input {name:?} has schema {}, expected {}",
                slot.schema,
                T::wire_schema()
            ));
        }
        self.selected
            .get(&slot.id)
            .map(|value| value.get::<T>())
            .transpose()
    }
    pub fn get_id<T: ContractValue>(&self, id: SlotId) -> Result<T, String> {
        self.selected
            .get(&id)
            .ok_or_else(|| format!("selected slot {} is missing", id.0))?
            .get::<T>()
    }
    /// Local typed field access. The field is cloned only when an owned struct
    /// must be assembled from borrowed, atomically selected slot values.
    /// Use Arc<T> as the field type for non-Clone domain payloads.
    pub fn get_local_path<T: Clone + Send + Sync + 'static>(
        &self,
        path: &str,
    ) -> Result<T, String> {
        let slot = self
            .slots
            .iter()
            .find(|slot| slot.name.as_deref() == Some(path))
            .ok_or_else(|| format!("unknown input path {path:?}"))?;
        if slot.identity() != TypeIdentity::Local(TypeId::of::<T>()) {
            return Err(format!(
                "local input {path:?} has type {}, expected {}",
                slot.schema,
                type_name::<T>()
            ));
        }
        self.selected
            .get(&slot.id)
            .ok_or_else(|| format!("selected local input {path:?} is missing"))?
            .borrow::<T>()
            .ok_or_else(|| format!("selected local input {path:?} type mismatch"))
            .cloned()
    }
    pub fn get_path<T: ContractValue>(&self, path: &str) -> Result<T, String> {
        let slot = self
            .slots
            .iter()
            .find(|slot| slot.name.as_deref() == Some(path))
            .ok_or_else(|| format!("unknown input path {path:?}"))?;
        if slot.schema != T::wire_schema() {
            return Err(format!(
                "input {path:?} has schema {}, expected {}",
                slot.schema,
                T::wire_schema()
            ));
        }
        self.selected
            .get(&slot.id)
            .ok_or_else(|| format!("selected input path {path:?} is missing"))?
            .get::<T>()
    }
    pub fn present(&self, name: &str) -> Result<bool, String> {
        let slot = self
            .slots
            .iter()
            .find(|slot| slot.name.as_deref() == Some(name))
            .ok_or_else(|| format!("unknown input name {name:?}"))?;
        Ok(self.selected.contains_key(&slot.id))
    }
}

/// Selected local values owned by one invocation, after atomic port admission.
/// Each named leaf can be taken exactly once. The value is moved when the Arc
/// has a single owner; a still-shared non-Clone leaf is an explicit error.
pub struct OwnedSelectedValues {
    selected: BTreeMap<SlotId, Value>,
    slots: Vec<InputSlot>,
}
impl OwnedSelectedValues {
    fn new(selected: BTreeMap<SlotId, Value>, slots: Vec<InputSlot>) -> Self {
        Self { selected, slots }
    }
    pub fn take_local_path<T: Send + Sync + 'static>(&mut self, path: &str) -> Result<T, String> {
        let slot = self
            .slots
            .iter()
            .find(|slot| slot.name.as_deref() == Some(path))
            .ok_or_else(|| format!("unknown local input path {path:?}"))?;
        if slot.identity() != TypeIdentity::Local(TypeId::of::<T>()) {
            return Err(format!(
                "local input path {path:?} has incompatible declared Rust type"
            ));
        }
        let id = slot.id;
        let value = self
            .selected
            .get(&id)
            .ok_or_else(|| format!("missing or already taken local input {path:?}"))?;
        if value.borrow::<T>().is_none() {
            return Err(format!("local input {path:?} is not {}", type_name::<T>()));
        }
        let owned = self
            .selected
            .remove(&id)
            .expect("validated local selection")
            .into_shared::<T>()?;
        Arc::try_unwrap(owned).map_err(|_| {
            format!("local field {path:?} still has other owners; use Arc<T> fields or a shared adapter for fan-out")
        })
    }
    /// An explicit `Arc<T>` leaf may be shared across independent selected
    /// deliveries. Clones only the Arc handle, never the underlying T payload.
    pub fn take_shared_local_path<T: Send + Sync + 'static>(
        &mut self,
        path: &str,
    ) -> Result<Arc<T>, String> {
        let slot = self
            .slots
            .iter()
            .find(|slot| slot.name.as_deref() == Some(path))
            .ok_or_else(|| format!("unknown shared local input path {path:?}"))?;
        if slot.identity() != TypeIdentity::Local(TypeId::of::<Arc<T>>()) {
            return Err(format!(
                "local input {path:?} is not Arc<{}>",
                type_name::<T>()
            ));
        }
        let value = self
            .selected
            .remove(&slot.id)
            .ok_or_else(|| format!("missing or already taken shared local input {path:?}"))?;
        let payload = value.into_shared::<Arc<T>>()?;
        Ok(Arc::try_unwrap(payload).unwrap_or_else(|shared| Arc::clone(shared.as_ref())))
    }
    /// Borrow a leaf while preparing another value, without moving it.
    pub fn borrow_local_path<T: Send + Sync + 'static>(&self, path: &str) -> Result<&T, String> {
        let slot = self
            .slots
            .iter()
            .find(|slot| slot.name.as_deref() == Some(path))
            .ok_or_else(|| format!("unknown local input path {path:?}"))?;
        self.selected
            .get(&slot.id)
            .and_then(Value::borrow::<T>)
            .ok_or_else(|| format!("missing or mistyped local input {path:?}"))
    }
}

/// Declares the inputs admitted by one typed port.
pub trait InputSchema {
    fn slots() -> Result<Vec<InputSlot>, LayoutError>;
}
/// Converts the policy-selected values into a typed handler argument.
pub trait FromSelected: Sized {
    fn from_values(values: &SelectedValues<'_>) -> Result<Self, String>;
}
/// Composite capability used by typed ports; implement the two component
/// traits independently or use `#[derive(PortArgs)]`.
pub trait PortArgs: InputSchema + FromSelected + Send + Sync + 'static {}
impl<T> PortArgs for T where T: InputSchema + FromSelected + Send + Sync + 'static {}

/// A recoverable layout declaration error. Layout expansion is transactional:
/// a failure never produces a partially accepted port or graph node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutError {
    TooManySlots { path: String, capacity: usize },
}
impl std::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManySlots { path, capacity } => {
                write!(f, "input layout exceeds {capacity} slots at {path:?}")
            }
        }
    }
}
impl std::error::Error for LayoutError {}

/// Allocate a unique input slot ID, rejecting overflow instead of truncating
/// or panicking. Derive expansions use this same function as built-in layouts.
#[doc(hidden)]
pub fn push_layout_slot(
    slots: &mut Vec<InputSlot>,
    path: String,
    create: impl FnOnce(u16) -> InputSlot,
) -> Result<(), LayoutError> {
    let id = u16::try_from(slots.len()).map_err(|_| LayoutError::TooManySlots {
        path: path.clone(),
        capacity: usize::from(u16::MAX) + 1,
    })?;
    slots.push(create(id).named(path));
    Ok(())
}

/// Structural description of a user value. Layout code declares and assembles
/// values; the policy still owns selection and the graph still owns scheduling.
///
/// Recursive expansion is rejected when a derive can see a direct self-reference:
///
/// ```compile_fail
/// use phenix_graph::PortLayout;
/// #[derive(PortLayout)]
/// struct Recursive { #[port(flatten)] next: Box<Recursive> }
/// ```
///
/// Conflicting field directives are also rejected by the derive:
///
/// ```compile_fail
/// use phenix_graph::PortLayout;
/// #[derive(PortLayout)]
/// struct Invalid { #[port(atomic, flatten)] value: String }
/// ```
pub trait PortLayout: Sized + Send + Sync + 'static {
    fn append_slots(prefix: &str, slots: &mut Vec<InputSlot>) -> Result<(), LayoutError>;
    fn from_selected(values: &SelectedValues<'_>, prefix: &str) -> Result<Self, String>;
}

/// A statically bounded recursive layout. Depth zero is the whole value;
/// depth one exposes its immediate fields, and each further level expands one
/// additional struct/tuple boundary.
pub trait PortLayoutAt<const DEPTH: usize>: Sized + Send + Sync + 'static {
    fn append_slots_at(prefix: &str, slots: &mut Vec<InputSlot>) -> Result<(), LayoutError>;
    fn from_selected_at(values: &SelectedValues<'_>, prefix: &str) -> Result<Self, String>;
}
impl<T: ContractValue> PortLayoutAt<0> for T {
    fn append_slots_at(prefix: &str, slots: &mut Vec<InputSlot>) -> Result<(), LayoutError> {
        let name = if prefix.is_empty() {
            "value".to_owned()
        } else {
            prefix.to_owned()
        };
        push_layout_slot(slots, name, InputSlot::deferred::<T>)?;
        Ok(())
    }
    fn from_selected_at(values: &SelectedValues<'_>, prefix: &str) -> Result<T, String> {
        let name = if prefix.is_empty() { "value" } else { prefix };
        values.get_path(name)
    }
}

/// Local structural decoding consumes the policy-selected leaves. Neither
/// the domain type nor its leaf fields need Clone or a wire codec.
pub trait LocalPortLayout: Sized + Send + Sync + 'static {
    fn append_local_slots(prefix: &str, slots: &mut Vec<InputSlot>) -> Result<(), LayoutError>;
    fn from_local_owned(values: &mut OwnedSelectedValues, prefix: &str) -> Result<Self, String>;
}
/// Depth-bounded local structural decoding, consuming each selected leaf once.
pub trait LocalPortLayoutAt<const DEPTH: usize>: Sized + Send + Sync + 'static {
    fn append_local_slots_at(prefix: &str, slots: &mut Vec<InputSlot>) -> Result<(), LayoutError>;
    fn from_local_owned_at(values: &mut OwnedSelectedValues, prefix: &str) -> Result<Self, String>;
}
impl<T: Send + Sync + 'static> LocalPortLayoutAt<0> for T {
    fn append_local_slots_at(prefix: &str, slots: &mut Vec<InputSlot>) -> Result<(), LayoutError> {
        let path = if prefix.is_empty() {
            "value".to_owned()
        } else {
            prefix.to_owned()
        };
        push_layout_slot(slots, path, InputSlot::local::<T>)?;
        Ok(())
    }
    fn from_local_owned_at(values: &mut OwnedSelectedValues, prefix: &str) -> Result<Self, String> {
        values.take_local_path(if prefix.is_empty() { "value" } else { prefix })
    }
}

/// A typed layout passed to a port before the port erases it into ArgsBuilder.
/// Port shape is independent of both wire decoding and local move semantics.
pub trait InputLayout: Send + Sync + 'static {
    type Value: Send + 'static;
    fn slots() -> Result<Vec<InputSlot>, LayoutError>;
    fn atomic_identity() -> Option<TypeIdentity> {
        None
    }
}
/// Decode a selected wire/legacy input without consuming the port's buffered
/// values. This is not implemented for local move-only layouts.
pub trait DecodedInputLayout: InputLayout {
    fn prepare(values: &SelectedValues<'_>) -> Result<Self::Value, String>;
}
/// Consume the selected local payloads after admission, once per invocation.
pub trait LocalInputLayout: InputLayout {
    fn prepare_owned(values: &mut OwnedSelectedValues) -> Result<Self::Value, String>;
}

pub struct Atomic<T>(PhantomData<fn() -> T>);
pub struct Flatten<T>(PhantomData<fn() -> T>);
pub struct FlattenTo<T, const DEPTH: usize>(PhantomData<fn() -> T>);
pub struct DeepFlatten<T>(PhantomData<fn() -> T>);
impl<T: ContractValue> InputLayout for Atomic<T> {
    type Value = T;
    fn atomic_identity() -> Option<TypeIdentity> {
        Some(InputSlot::deferred::<T>(0).identity())
    }
    fn slots() -> Result<Vec<InputSlot>, LayoutError> {
        Ok(vec![InputSlot::deferred::<T>(0).named("value")])
    }
}
impl<T: ContractValue> DecodedInputLayout for Atomic<T> {
    fn prepare(values: &SelectedValues<'_>) -> Result<Self::Value, String> {
        values.get_path("value")
    }
}
impl<T: PortLayoutAt<1>> InputLayout for Flatten<T> {
    type Value = T;
    fn slots() -> Result<Vec<InputSlot>, LayoutError> {
        let mut slots = Vec::new();
        T::append_slots_at("", &mut slots)?;
        Ok(slots)
    }
}
impl<T: PortLayoutAt<1>> DecodedInputLayout for Flatten<T> {
    fn prepare(values: &SelectedValues<'_>) -> Result<Self::Value, String> {
        T::from_selected_at(values, "")
    }
}
impl<T: PortLayoutAt<DEPTH>, const DEPTH: usize> InputLayout for FlattenTo<T, DEPTH> {
    type Value = T;
    fn slots() -> Result<Vec<InputSlot>, LayoutError> {
        let mut slots = Vec::new();
        T::append_slots_at("", &mut slots)?;
        Ok(slots)
    }
}
impl<T: PortLayoutAt<DEPTH>, const DEPTH: usize> DecodedInputLayout for FlattenTo<T, DEPTH> {
    fn prepare(values: &SelectedValues<'_>) -> Result<Self::Value, String> {
        T::from_selected_at(values, "")
    }
}
impl<T: PortLayout> InputLayout for DeepFlatten<T> {
    type Value = T;
    fn slots() -> Result<Vec<InputSlot>, LayoutError> {
        let mut slots = Vec::new();
        T::append_slots("", &mut slots)?;
        Ok(slots)
    }
}
impl<T: PortLayout> DecodedInputLayout for DeepFlatten<T> {
    fn prepare(values: &SelectedValues<'_>) -> Result<Self::Value, String> {
        T::from_selected(values, "")
    }
}
/// Local layouts are independent of the legacy encoded PortLayout family.
pub struct LocalAtomic<T>(PhantomData<fn() -> T>);
pub struct LocalFlatten<T>(PhantomData<fn() -> T>);
pub struct LocalFlattenTo<T, const DEPTH: usize>(PhantomData<fn() -> T>);
pub struct LocalDeepFlatten<T>(PhantomData<fn() -> T>);
impl<T: Send + Sync + 'static> InputLayout for LocalAtomic<T> {
    type Value = T;
    fn atomic_identity() -> Option<TypeIdentity> {
        Some(TypeIdentity::Local(TypeId::of::<T>()))
    }
    fn slots() -> Result<Vec<InputSlot>, LayoutError> {
        Ok(vec![InputSlot::local::<T>(0).named("value")])
    }
}
impl<T: Send + Sync + 'static> LocalInputLayout for LocalAtomic<T> {
    fn prepare_owned(values: &mut OwnedSelectedValues) -> Result<T, String> {
        values.take_local_path("value")
    }
}
impl<T: LocalPortLayoutAt<1>> InputLayout for LocalFlatten<T> {
    type Value = T;
    fn slots() -> Result<Vec<InputSlot>, LayoutError> {
        let mut slots = Vec::new();
        T::append_local_slots_at("", &mut slots)?;
        Ok(slots)
    }
}
impl<T: LocalPortLayoutAt<1>> LocalInputLayout for LocalFlatten<T> {
    fn prepare_owned(values: &mut OwnedSelectedValues) -> Result<T, String> {
        T::from_local_owned_at(values, "")
    }
}
impl<T: LocalPortLayoutAt<DEPTH>, const DEPTH: usize> InputLayout for LocalFlattenTo<T, DEPTH> {
    type Value = T;
    fn slots() -> Result<Vec<InputSlot>, LayoutError> {
        let mut slots = Vec::new();
        T::append_local_slots_at("", &mut slots)?;
        Ok(slots)
    }
}
impl<T: LocalPortLayoutAt<DEPTH>, const DEPTH: usize> LocalInputLayout
    for LocalFlattenTo<T, DEPTH>
{
    fn prepare_owned(values: &mut OwnedSelectedValues) -> Result<T, String> {
        T::from_local_owned_at(values, "")
    }
}
impl<T: LocalPortLayout> InputLayout for LocalDeepFlatten<T> {
    type Value = T;
    fn slots() -> Result<Vec<InputSlot>, LayoutError> {
        let mut slots = Vec::new();
        T::append_local_slots("", &mut slots)?;
        Ok(slots)
    }
}
impl<T: LocalPortLayout> LocalInputLayout for LocalDeepFlatten<T> {
    fn prepare_owned(values: &mut OwnedSelectedValues) -> Result<T, String> {
        T::from_local_owned(values, "")
    }
}

// Tuple layouts are fully local; each direct element is a named slot. The
// tuple boundary counts as one expansion level in bounded layouts.
macro_rules! local_tuple_layout {
    ($depth:literal; $($name:ident : $index:tt),+ $(,)?) => {
        impl<$($name: Send + Sync + 'static),+> LocalPortLayoutAt<$depth> for ($($name,)+) {
            fn append_local_slots_at(prefix: &str, slots: &mut Vec<InputSlot>) -> Result<(), LayoutError> {
                $(
                    let path = if prefix.is_empty() { stringify!($index).to_owned() }
                        else { format!("{prefix}.{}", stringify!($index)) };
                    push_layout_slot(slots, path, InputSlot::local::<$name>)?;
                )+
                Ok(())
            }
            fn from_local_owned_at(values: &mut OwnedSelectedValues, prefix: &str) -> Result<Self, String> {
                Ok(($(
                    {
                        let path = if prefix.is_empty() { stringify!($index).to_owned() }
                            else { format!("{prefix}.{}", stringify!($index)) };
                        values.take_local_path::<$name>(&path)?
                    },
                )+))
            }
        }
    };
}
macro_rules! local_tuple_depths {
    ($($name:ident : $index:tt),+ $(,)?) => {
        local_tuple_layout!(1; $($name:$index),+);
        local_tuple_layout!(2; $($name:$index),+);
        local_tuple_layout!(3; $($name:$index),+);
        local_tuple_layout!(4; $($name:$index),+);
        local_tuple_layout!(5; $($name:$index),+);
        local_tuple_layout!(6; $($name:$index),+);
        local_tuple_layout!(7; $($name:$index),+);
        local_tuple_layout!(8; $($name:$index),+);
    };
}
local_tuple_depths!(A:0);
local_tuple_depths!(A:0, B:1);
local_tuple_depths!(A:0, B:1, C:2);
local_tuple_depths!(A:0, B:1, C:2, D:3);

// A derived/manual PortArgs is itself an InputLayout. The old extra
// PortArgsLayout wrapper contributed no identity or behavior: the schema and
// decoder already belong to the argument type.
impl<A: PortArgs> InputLayout for A {
    type Value = A;
    fn slots() -> Result<Vec<InputSlot>, LayoutError> {
        <A as InputSchema>::slots()
    }
}
impl<A: PortArgs> DecodedInputLayout for A {
    fn prepare(values: &SelectedValues<'_>) -> Result<Self::Value, String> {
        <A as FromSelected>::from_values(values)
    }
}

macro_rules! tuple_port_layout {
    ($($name:ident:$index:tt),+ $(,)?) => {
        impl<$($name: ContractValue + Send + Sync),+> PortLayout for ($($name,)+) {
            fn append_slots(prefix: &str, slots: &mut Vec<InputSlot>) -> Result<(), LayoutError> {
                $(
                    let path = if prefix.is_empty() { stringify!($index).to_owned() }
                        else { format!("{prefix}.{}", stringify!($index)) };
                    push_layout_slot(slots, path, InputSlot::deferred::<$name>)?;
                )+
                Ok(())
            }
            fn from_selected(values: &SelectedValues<'_>, prefix: &str) -> Result<Self, String> {
                Ok(($(
                    {
                        let path = if prefix.is_empty() { stringify!($index).to_owned() }
                            else { format!("{prefix}.{}", stringify!($index)) };
                        values.get_path::<$name>(&path)?
                    },
                )+))
            }
        }
        impl<$($name: ContractValue + Send + Sync),+> PortLayoutAt<1> for ($($name,)+) {
            fn append_slots_at(prefix: &str, slots: &mut Vec<InputSlot>) -> Result<(), LayoutError> {
                $(
                    let path = if prefix.is_empty() { stringify!($index).to_owned() }
                        else { format!("{prefix}.{}", stringify!($index)) };
                    push_layout_slot(slots, path, InputSlot::deferred::<$name>)?;
                )+
                Ok(())
            }
            fn from_selected_at(values: &SelectedValues<'_>, prefix: &str) -> Result<Self, String> {
                Ok(($(
                    {
                        let path = if prefix.is_empty() { stringify!($index).to_owned() }
                            else { format!("{prefix}.{}", stringify!($index)) };
                        values.get_path::<$name>(&path)?
                    },
                )+))
            }
        }
    };
}
tuple_port_layout!(A:0);
tuple_port_layout!(A:0, B:1);
tuple_port_layout!(A:0, B:1, C:2);
tuple_port_layout!(A:0, B:1, C:2, D:3);

/// Deep local tuple layout; the existing depth-specific tuple impls above
/// are retained for bounded expansion, without duplicate trait impls.
macro_rules! local_tuple_deep {
    ($($ty:ident:$index:tt),+ $(,)?) => {
        impl<$($ty: Send + Sync + 'static),+> LocalPortLayout for ($($ty,)+) {
            fn append_local_slots(prefix: &str, slots: &mut Vec<InputSlot>) -> Result<(), LayoutError> {
                $(
                    let path = if prefix.is_empty() { stringify!($index).to_owned() }
                        else { format!("{prefix}.{}", stringify!($index)) };
                    push_layout_slot(slots, path, InputSlot::local::<$ty>)?;
                )+
                Ok(())
            }
            fn from_local_owned(values: &mut OwnedSelectedValues, prefix: &str) -> Result<Self, String> {
                Ok(($(
                    {
                        let path = if prefix.is_empty() { stringify!($index).to_owned() }
                            else { format!("{prefix}.{}", stringify!($index)) };
                        values.take_local_path::<$ty>(&path)?
                    },
                )+))
            }
        }
    }
}
local_tuple_deep!(A:0);
local_tuple_deep!(A:0, B:1);
local_tuple_deep!(A:0, B:1, C:2);
local_tuple_deep!(A:0, B:1, C:2, D:3);

/// Every node declares exactly one output slot, including unit-valued actions.
/// An invocation may produce zero or one values. Producing no value is distinct
/// from producing a value routed to zero recipients; plain `Option<T>` is data,
/// not an optional output declaration. The output always uses slot zero.
///
/// An absent output schema is deliberately not representable:
///
/// ```compile_fail
/// use phenix_graph::{OutputSchema, OutputSlot};
/// struct OldOutputless;
/// impl OutputSchema for OldOutputless {
///     fn output_slot() -> Option<OutputSlot> { None }
/// }
/// ```
pub trait OutputSchema {
    fn output_slot() -> OutputSlot;
}
pub trait IntoEmission: Sized {
    fn into_emission(self) -> Emission;
}
pub trait NodeOutput: OutputSchema + IntoEmission + Send + 'static {}
impl<T> NodeOutput for T where T: OutputSchema + IntoEmission + Send + 'static {}
impl OutputSchema for () {
    fn output_slot() -> OutputSlot {
        OutputSlot::typed::<()>(0).named("value")
    }
}
impl IntoEmission for () {
    fn into_emission(self) -> Emission {
        emit(0, ())
    }
}

/// Object-safe part of argument decoding, independent of port readiness.
pub trait ArgsBuilder: Send + Sync {
    fn prepare(&self, values: &SelectedValues<'_>) -> Result<Option<PreparedArgs>, String>;
}
struct LayoutArgsBuilder<L: DecodedInputLayout>(PhantomData<L>);
impl<L: DecodedInputLayout> ArgsBuilder for LayoutArgsBuilder<L> {
    fn prepare(&self, values: &SelectedValues<'_>) -> Result<Option<PreparedArgs>, String> {
        Ok(Some(Box::new(L::prepare(values)?)))
    }
}
struct ClosureArgsBuilder<A, F>(F, PhantomData<A>);
impl<A, F> ArgsBuilder for ClosureArgsBuilder<A, F>
where
    A: Send + Sync + 'static,
    F: for<'a> Fn(&SelectedValues<'a>) -> Result<A, String> + Send + Sync,
{
    fn prepare(&self, values: &SelectedValues<'_>) -> Result<Option<PreparedArgs>, String> {
        Ok(Some(Box::new((self.0)(values)?)))
    }
}

/// One preparation path owned by the port. Validation can return `None` when a
/// low-level handler should keep receiving its selected raw values.
struct ValidatorArgsBuilder<F>(F);
impl<F> ArgsBuilder for ValidatorArgsBuilder<F>
where
    F: for<'a> Fn(&SelectedValues<'a>) -> Result<(), String> + Send + Sync,
{
    fn prepare(&self, values: &SelectedValues<'_>) -> Result<Option<PreparedArgs>, String> {
        (self.0)(values)?;
        Ok(None)
    }
}

pub type PreparedArgs = Box<dyn Any + Send>;
// A typed port owns decoded arguments; a raw port owns only the original values.
enum InvocationArgs {
    Raw(BTreeMap<SlotId, Value>),
    Typed(PreparedArgs),
    Taken,
}
#[cfg(test)]
impl InvocationArgs {
    fn raw(&self) -> &BTreeMap<SlotId, Value> {
        match self {
            Self::Raw(values) => values,
            Self::Typed(_) | Self::Taken => panic!("expected raw arguments"),
        }
    }
}
pub trait PortPolicy: Send + Sync {
    fn select(&self, snapshot: &PortSnapshot) -> Option<Selection>;
    fn incomplete(&self, snapshot: &PortSnapshot) -> Vec<IncompleteGroup>;
    /// Human-readable policy type for diagnostics and structural inspection.
    /// This is deliberately NOT a behavior or executable identity: policy
    /// configuration and captured values are not represented by a Rust type.
    fn diagnostic_type(&self) -> &'static str {
        type_name::<Self>()
    }
    /// Explicitly discard unselected alternatives as part of this reservation.
    /// Policies that do not opt in leave buffered deliveries available.
    fn retire(&self, _snapshot: &PortSnapshot, _selection: &Selection) -> Vec<Pick> {
        vec![]
    }
}

// An already-erased policy can still be wrapped by ordinary, generic decorators.
// The dynamic dispatch boundary stays at Port, rather than infecting graph types.
impl<P: PortPolicy + ?Sized> PortPolicy for Arc<P> {
    fn select(&self, snapshot: &PortSnapshot) -> Option<Selection> {
        (**self).select(snapshot)
    }
    fn incomplete(&self, snapshot: &PortSnapshot) -> Vec<IncompleteGroup> {
        (**self).incomplete(snapshot)
    }
    fn diagnostic_type(&self) -> &'static str {
        (**self).diagnostic_type()
    }
    fn retire(&self, snapshot: &PortSnapshot, selection: &Selection) -> Vec<Pick> {
        (**self).retire(snapshot, selection)
    }
}

/// Default join: one delivery from EVERY slot with identical cohort.
pub struct AllSameCohort;
impl PortPolicy for AllSameCohort {
    fn select(&self, snapshot: &PortSnapshot) -> Option<Selection> {
        let slots = &snapshot.slots;
        let first = slots.first()?;
        for candidate in &first.candidates {
            let mut picks = vec![Pick {
                slot: first.slot,
                delivery: candidate.delivery,
            }];
            let mut complete = true;
            for s in slots.iter().skip(1) {
                if let Some(c) = s.candidates.iter().find(|c| c.cohort == candidate.cohort) {
                    picks.push(Pick {
                        slot: s.slot,
                        delivery: c.delivery,
                    });
                } else {
                    complete = false;
                    break;
                }
            }
            if complete {
                return Some(Selection {
                    cohort: candidate.cohort,
                    picks,
                });
            }
        }
        None
    }
    fn incomplete(&self, snapshot: &PortSnapshot) -> Vec<IncompleteGroup> {
        let slots = &snapshot.slots;
        let cohorts: BTreeSet<CohortId> = slots
            .iter()
            .flat_map(|s| s.candidates.iter().map(|c| c.cohort))
            .collect();
        cohorts
            .into_iter()
            .filter_map(|cohort| {
                let present: Vec<_> = slots
                    .iter()
                    .filter(|s| s.candidates.iter().any(|c| c.cohort == cohort))
                    .map(|s| s.slot)
                    .collect();
                let missing: Vec<_> = slots
                    .iter()
                    .filter(|s| !present.contains(&s.slot))
                    .map(|s| s.slot)
                    .collect();
                if missing.is_empty() {
                    None
                } else {
                    Some(IncompleteGroup {
                        cohort,
                        present,
                        missing,
                        requirement: "all slots",
                    })
                }
            })
            .collect()
    }
}

/// Alternative policy: one of several candidate producers is sufficient.
pub struct AnyOne;
impl PortPolicy for AnyOne {
    fn select(&self, snapshot: &PortSnapshot) -> Option<Selection> {
        snapshot
            .slots
            .iter()
            .flat_map(|s| s.candidates.iter())
            .next()
            .map(|c| Selection {
                cohort: c.cohort,
                picks: vec![Pick {
                    slot: c.slot,
                    delivery: c.delivery,
                }],
            })
    }
    fn incomplete(&self, _snapshot: &PortSnapshot) -> Vec<IncompleteGroup> {
        vec![]
    }
}

/// An arbitrary policy can normalize different candidate combinations into
/// a single strongly typed node argument shape. No special scheduler support.
#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
struct ChosenArgs {
    parts: Vec<String>,
}
#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
struct AnyTwoArgs {
    first: (SlotId, String),
    second: (SlotId, String),
}
#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
struct OptionalPairArgs {
    first: Option<String>,
    second: Option<String>,
}
#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
enum PairStageArgs {
    Pair { left: String, right: String },
    Third { value: String },
}

#[cfg(test)]
fn decode_string<V: SelectedValue>(
    selected: &BTreeMap<SlotId, V>,
    slot: u16,
) -> Result<String, String> {
    selected
        .get(&SlotId(slot))
        .ok_or_else(|| format!("missing slot {slot}"))?
        .value()
        .get::<String>()
}

#[cfg(test)]
trait SelectedValue {
    fn value(&self) -> &Value;
}
#[cfg(test)]
impl SelectedValue for Value {
    fn value(&self) -> &Value {
        self
    }
}
#[cfg(test)]
impl SelectedValue for &Value {
    fn value(&self) -> &Value {
        self
    }
}

/// Accept any two of slots 0/1/2; preference is 0+1, then 0+2, then 1+2.
/// This defines a policy rather than adding a new scheduler node kind.
#[cfg(test)]
struct AnyTwoOfThree;
#[cfg(test)]
impl PortPolicy for AnyTwoOfThree {
    fn select(&self, snapshot: &PortSnapshot) -> Option<Selection> {
        for cohort in snapshot.cohorts() {
            for (a, b) in [(0, 1), (0, 2), (1, 2)] {
                if let (Some(a), Some(b)) = (
                    snapshot.candidates(a, cohort.0),
                    snapshot.candidates(b, cohort.0),
                ) {
                    return Some(Selection {
                        cohort,
                        picks: vec![
                            Pick {
                                slot: a.slot,
                                delivery: a.delivery,
                            },
                            Pick {
                                slot: b.slot,
                                delivery: b.delivery,
                            },
                        ],
                    });
                }
            }
        }
        None
    }
    fn incomplete(&self, snapshot: &PortSnapshot) -> Vec<IncompleteGroup> {
        snapshot
            .cohorts()
            .into_iter()
            .filter_map(|cohort| {
                if snapshot
                    .reservations
                    .iter()
                    .any(|r| r.cohort == cohort && r.slots.len() == 2)
                {
                    return None; // remaining unused alternative does not invalidate the pair
                }
                let present: Vec<_> = snapshot
                    .slots
                    .iter()
                    .filter(|s| s.candidates.iter().any(|c| c.cohort == cohort))
                    .map(|s| s.slot)
                    .collect();
                if present.is_empty() {
                    return None;
                }
                let missing = snapshot
                    .slots
                    .iter()
                    .filter(|s| !present.contains(&s.slot))
                    .map(|s| s.slot)
                    .collect();
                Some(IncompleteGroup {
                    cohort,
                    present,
                    missing,
                    requirement: "at least two of three slots",
                })
            })
            .collect()
    }
}

#[cfg(test)]
impl AnyTwoOfThree {
    fn build_args<V: SelectedValue>(selected: &BTreeMap<SlotId, V>) -> Result<AnyTwoArgs, String> {
        if selected.len() != 2 {
            return Err("AnyTwo requires exactly two values".into());
        }
        let mut pairs = selected
            .iter()
            .map(|(id, value)| -> Result<(SlotId, String), String> {
                Ok((*id, value.value().get::<String>()?))
            });
        Ok(AnyTwoArgs {
            first: pairs.next().unwrap()?,
            second: pairs.next().unwrap()?,
        })
    }
}

/// At most one invocation per cohort. Prefer a correlated pair if both
/// are present, otherwise use whichever arrived first; the coloring decorator
/// can delay this decision until relevant upstream producers are exhausted.
#[cfg(test)]
struct PreferPairOnce;
#[cfg(test)]
impl PortPolicy for PreferPairOnce {
    fn select(&self, snapshot: &PortSnapshot) -> Option<Selection> {
        for cohort in snapshot.cohorts() {
            if snapshot.reservations.iter().any(|r| r.cohort == cohort) {
                continue;
            }
            let candidates: Vec<_> = snapshot
                .slots
                .iter()
                .filter_map(|s| s.candidates.iter().find(|c| c.cohort == cohort))
                .collect();
            if !candidates.is_empty() {
                return Some(Selection {
                    cohort,
                    picks: candidates
                        .iter()
                        .map(|c| Pick {
                            slot: c.slot,
                            delivery: c.delivery,
                        })
                        .collect(),
                });
            }
        }
        None
    }
    fn incomplete(&self, _snapshot: &PortSnapshot) -> Vec<IncompleteGroup> {
        vec![]
    }
}

/// Select the available members of a pair once its live upstream producers settle.
#[cfg(test)]
struct OptionalPair;
#[cfg(test)]
impl PortPolicy for OptionalPair {
    fn select(&self, snapshot: &PortSnapshot) -> Option<Selection> {
        for cohort in snapshot.cohorts() {
            if snapshot.reservations.iter().any(|r| r.cohort == cohort) {
                continue;
            }
            let picks: Vec<_> = [0, 1]
                .into_iter()
                .filter_map(|slot| {
                    snapshot.candidates(slot, cohort.0).map(|candidate| Pick {
                        slot: candidate.slot,
                        delivery: candidate.delivery,
                    })
                })
                .collect();
            if !picks.is_empty() {
                return Some(Selection { cohort, picks });
            }
        }
        None
    }
    fn incomplete(&self, _snapshot: &PortSnapshot) -> Vec<IncompleteGroup> {
        vec![]
    }
}

/// Sequential requirements on ONE port and THREE slots:
///  (1) reserve slots 0 and 1 => invoke once, publish a result;
///  (2) retain the obligation for slot 2 under the same cohort;
///  (3) reserve slot 2 => invoke a second time;
///  (4) missing slot 2 at wave exhaustion => exactly one structured error.
#[cfg(test)]
struct PairThenThird;
#[cfg(test)]
impl PortPolicy for PairThenThird {
    fn select(&self, snapshot: &PortSnapshot) -> Option<Selection> {
        for cohort in snapshot.cohorts() {
            if !snapshot.has_reserved(cohort.0, &[0, 1]) {
                if let (Some(a), Some(b)) = (
                    snapshot.candidates(0, cohort.0),
                    snapshot.candidates(1, cohort.0),
                ) {
                    return Some(Selection {
                        cohort,
                        picks: vec![
                            Pick {
                                slot: a.slot,
                                delivery: a.delivery,
                            },
                            Pick {
                                slot: b.slot,
                                delivery: b.delivery,
                            },
                        ],
                    });
                }
            } else if snapshot.has_completed(cohort.0, &[0, 1])
                && !snapshot.has_reserved(cohort.0, &[2])
                && let Some(third) = snapshot.candidates(2, cohort.0)
            {
                return Some(Selection {
                    cohort,
                    picks: vec![Pick {
                        slot: third.slot,
                        delivery: third.delivery,
                    }],
                });
            }
        }
        None
    }
    fn incomplete(&self, snapshot: &PortSnapshot) -> Vec<IncompleteGroup> {
        snapshot
            .cohorts()
            .into_iter()
            .filter_map(|cohort| {
                if snapshot.has_reserved(cohort.0, &[0, 1]) {
                    if !snapshot.has_completed(cohort.0, &[0, 1]) {
                        return None;
                    } // failed phase already diagnosed
                    if snapshot.has_reserved(cohort.0, &[2]) {
                        None
                    } else {
                        Some(IncompleteGroup {
                            cohort,
                            present: vec![SlotId(0), SlotId(1)],
                            missing: vec![SlotId(2)],
                            requirement: "third slot after pair",
                        })
                    }
                } else {
                    let present: Vec<_> = snapshot
                        .slots
                        .iter()
                        .filter(|s| s.candidates.iter().any(|c| c.cohort == cohort))
                        .map(|s| s.slot)
                        .collect();
                    let missing: Vec<_> = [SlotId(0), SlotId(1)]
                        .into_iter()
                        .filter(|s| !present.contains(s))
                        .collect();
                    Some(IncompleteGroup {
                        cohort,
                        present,
                        missing,
                        requirement: "first pair before third",
                    })
                }
            })
            .collect()
    }
}

/// Selected-value decoding belongs to the port, not the admission policy.
#[cfg(test)]
fn decode_chosen(values: &SelectedValues<'_>) -> Result<ChosenArgs, String> {
    let selected = values.selected;
    if !(1..=2).contains(&selected.len()) {
        return Err("Choose requires one or two inputs".into());
    }
    let parts = selected
        .values()
        .map(|v| v.get::<String>())
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ChosenArgs { parts })
}
#[cfg(test)]
fn decode_optional_pair(values: &SelectedValues<'_>) -> Result<OptionalPairArgs, String> {
    let selected = values.selected;
    if selected.is_empty() || selected.len() > 2 || selected.keys().any(|slot| slot.0 > 1) {
        return Err("OptionalPair requires one or two values from slots 0 and 1".into());
    }
    Ok(OptionalPairArgs {
        first: selected
            .get(&SlotId(0))
            .map(|value| value.get::<String>())
            .transpose()?,
        second: selected
            .get(&SlotId(1))
            .map(|value| value.get::<String>())
            .transpose()?,
    })
}
#[cfg(test)]
fn decode_pair_stage(values: &SelectedValues<'_>) -> Result<PairStageArgs, String> {
    let selected = values.selected;
    match selected.keys().copied().collect::<Vec<_>>().as_slice() {
        [SlotId(0), SlotId(1)] => Ok(PairStageArgs::Pair {
            left: decode_string(selected, 0)?,
            right: decode_string(selected, 1)?,
        }),
        [SlotId(2)] => Ok(PairStageArgs::Third {
            value: decode_string(selected, 2)?,
        }),
        _ => Err("PairThenThird selected an invalid stage".into()),
    }
}
#[cfg(test)]
fn validate_cohort(values: &SelectedValues<'_>) -> Result<(), String> {
    if values
        .selected
        .values()
        .any(|value| value.get::<String>().is_ok_and(|s| s == "bad"))
    {
        Err("invalid arguments for bad cohort".into())
    } else {
        Ok(())
    }
}

/// Readiness decorator that delays any eligible cohort while an admitted
/// upstream invocation could still reach this port. It does not wait for
/// imaginary graph branches; colors come only from live invocations.
/// A configurable policy decorator. Supports both `WhenUpstreamSettles { policy }`
/// and `WhenUpstreamSettles::new(policy)` for future declarative macros.
pub struct WhenUpstreamSettles<P> {
    pub policy: P,
}
impl<P> WhenUpstreamSettles<P> {
    pub fn new(policy: P) -> Self {
        Self { policy }
    }
}
impl<P: PortPolicy> PortPolicy for WhenUpstreamSettles<P> {
    fn select(&self, snapshot: &PortSnapshot) -> Option<Selection> {
        // Terminal colors remain visible to the wrapped policy; only live work
        // delays the selection decision.
        self.policy.select(&snapshot.excluding_open())
    }
    fn incomplete(&self, snapshot: &PortSnapshot) -> Vec<IncompleteGroup> {
        self.policy.incomplete(snapshot)
    }
    fn retire(&self, snapshot: &PortSnapshot, selection: &Selection) -> Vec<Pick> {
        self.policy.retire(snapshot, selection)
    }
}

#[derive(Clone)]
pub struct Port {
    pub id: PortId,
    pub name: Option<String>,
    pub slots: Vec<InputSlot>,
    pub policy: Arc<dyn PortPolicy>,
    pub args: Option<Arc<dyn ArgsBuilder>>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Access {
    Read,
    Write,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ResourceNeed {
    key: &'static str,
    access: Access,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CohortMode {
    Inherit,
    FreshPerInvocation,
}
#[derive(Clone)]
pub struct Emission {
    slot: SlotId,
    value: Value,
    selection: Option<output::OutputSelection>,
}
pub fn emit<T: ContractValue>(slot: u16, v: T) -> Emission {
    Emission {
        slot: SlotId(slot),
        value: enc(v),
        selection: None,
    }
}
/// Emit an in-process value without invoking a codec.
pub fn emit_local<T: Send + Sync + 'static>(slot: u16, v: T) -> Emission {
    Emission {
        slot: SlotId(slot),
        value: Value::local(v),
        selection: None,
    }
}

/// Shared resources are separate from the wave. Access never triggers work.
#[derive(Default)]
pub struct Store {
    entries: BTreeMap<&'static str, Arc<dyn Any + Send + Sync>>,
}
impl Store {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn insert<T: Send + Sync + 'static>(&mut self, key: &'static str, value: T) {
        self.entries.insert(key, Arc::new(RwLock::new(value)));
    }
    fn get<T: Send + Sync + 'static>(&self, key: &'static str) -> Result<&RwLock<T>, String> {
        self.entries
            .get(key)
            .ok_or_else(|| format!("resource {key} absent"))?
            .as_ref()
            .downcast_ref::<RwLock<T>>()
            .ok_or_else(|| format!("resource {key} has wrong Rust type"))
    }
}

pub struct InvocationCtx {
    wave: WaveId,
    invocation: InvocationId,
    cohort: CohortId,
    from_port: Option<PortId>,
    args: InvocationArgs,
    store: Arc<Store>,
    grants: Vec<ResourceNeed>,
}
impl InvocationCtx {
    pub fn wave_id(&self) -> u64 {
        self.wave.0
    }
    pub fn invocation_id(&self) -> u64 {
        self.invocation.0
    }
    pub fn cohort_id(&self) -> u64 {
        self.cohort.0
    }
    /// Physical input port for input-driven invocations; `None` for explicit
    /// zero-input starts. Sources have empty raw arguments, not a unit slot.
    pub fn source_port_id(&self) -> Option<u16> {
        self.from_port.map(|port| port.0)
    }

    /// Take a prepared value once, without cloning or replacing it with fake raw
    /// arguments. A failed type check does not consume the pending value.
    pub fn take_prepared<T: Send + 'static>(&mut self) -> Result<T, String> {
        if !matches!(&self.args, InvocationArgs::Typed(value) if value.is::<T>()) {
            return Err(format!(
                "port did not prepare {}",
                std::any::type_name::<T>()
            ));
        }
        let InvocationArgs::Typed(value) = std::mem::replace(&mut self.args, InvocationArgs::Taken)
        else {
            unreachable!("type-checked prepared argument")
        };
        Ok(*value
            .downcast::<T>()
            .expect("type-checked prepared argument"))
    }

    /// Borrow a prepared argument from a port-owned decoder.
    pub fn prepared<T: Send + 'static>(&self) -> Result<&T, String> {
        match &self.args {
            InvocationArgs::Typed(args) => args.downcast_ref::<T>(),
            InvocationArgs::Raw(_) | InvocationArgs::Taken => None,
        }
        .ok_or_else(|| {
            format!(
                "port did not provide typed {} args",
                std::any::type_name::<T>()
            )
        })
    }
    pub fn arg<T: ContractValue>(&self, slot: u16) -> Result<T, String> {
        match &self.args {
            InvocationArgs::Raw(values) => values
                .get(&SlotId(slot))
                .ok_or_else(|| format!("missing argument {slot}"))?
                .get(),
            InvocationArgs::Typed(_) | InvocationArgs::Taken => {
                Err("typed port arguments must be accessed through prepared<T>()".into())
            }
        }
    }
    pub fn has_arg(&self, slot: u16) -> bool {
        matches!(&self.args, InvocationArgs::Raw(values) if values.contains_key(&SlotId(slot)))
    }
    /// Consume one local argument, moving it if unique and cloning only when
    /// a different delivery or publication still shares the allocation.
    /// Invalid type checks leave the argument available for diagnostics.
    pub fn take_local<T: Clone + Send + Sync + 'static>(&mut self, slot: u16) -> Result<T, String> {
        let InvocationArgs::Raw(values) = &mut self.args else {
            return Err("owned input requires a raw local port".into());
        };
        let value = values
            .get(&SlotId(slot))
            .ok_or_else(|| format!("missing local argument {slot}"))?;
        if value.borrow::<T>().is_none() {
            return Err(format!("local argument {slot} is not {}", type_name::<T>()));
        }
        values
            .remove(&SlotId(slot))
            .expect("checked local argument")
            .into_owned::<T>()
    }
    /// Access a local input by shared ownership; no Clone or codec bound on T.
    pub fn shared<T: Send + Sync + 'static>(&self, slot: u16) -> Result<Arc<T>, String> {
        match &self.args {
            InvocationArgs::Raw(values) => values
                .get(&SlotId(slot))
                .ok_or_else(|| format!("missing local argument {slot}"))?
                .shared::<T>(),
            _ => Err("shared input requires a raw local port".into()),
        }
    }
    pub fn read<T: Send + Sync + 'static, R>(
        &self,
        key: &'static str,
        f: impl FnOnce(&T) -> R,
    ) -> Result<R, String> {
        if !self.grants.iter().any(|g| g.key == key) {
            return Err(format!("read of {key} not granted"));
        }
        let guard = self
            .store
            .get::<T>(key)?
            .read()
            .map_err(|e| e.to_string())?;
        Ok(f(&guard))
    }
    pub fn write<T: Send + Sync + 'static, R>(
        &self,
        key: &'static str,
        f: impl FnOnce(&mut T) -> R,
    ) -> Result<R, String> {
        if !self
            .grants
            .iter()
            .any(|g| g.key == key && g.access == Access::Write)
        {
            return Err(format!("write of {key} not granted"));
        }
        let mut guard = self
            .store
            .get::<T>(key)?
            .write()
            .map_err(|e| e.to_string())?;
        Ok(f(&mut guard))
    }
}

/// User-defined execution behavior. Implement this on a normal `struct { ... }`,
/// or pass a closure to the existing `NodeTemplate::new` constructor.
/// Each invocation is passed an owned context; mutable instance state should use
/// interior mutability or a factory-created action per physical node instance.
pub trait NodeAction: Send + Sync {
    fn execute(&self, ctx: InvocationCtx) -> Result<Vec<Emission>, String>;
}

// Typed argument decoding, handler dispatch and output contract are bound by
// one constructor, so the erased executor cannot be paired with the wrong A.
struct TypedNodeAction<A, O, F>(F, PhantomData<fn(A) -> O>);
impl<A, O, F> NodeAction for TypedNodeAction<A, O, F>
where
    A: PortArgs,
    O: NodeOutput,
    F: Fn(A, &InvocationCtx) -> Result<O, String> + Send + Sync,
{
    fn execute(&self, mut ctx: InvocationCtx) -> Result<Vec<Emission>, String> {
        let args = ctx.take_prepared::<A>()?;
        (self.0)(args, &ctx).map(|output| vec![output.into_emission()])
    }
}

struct FunctionNodeAction<A, O, F>(F, PhantomData<fn(A) -> O>);
impl<A, O, F> NodeAction for FunctionNodeAction<A, O, F>
where
    A: Send + 'static,
    O: ContractValue,
    F: Fn(A) -> O + Send + Sync,
{
    fn execute(&self, mut ctx: InvocationCtx) -> Result<Vec<Emission>, String> {
        let args = ctx.take_prepared::<A>()?;
        Ok(vec![emit(0, (self.0)(args))])
    }
}

/// Restrict return lowering to the two deliberately supported cases:
/// ordinary values and the explicit `Output<T>` routing wrapper. In
/// particular, downstream crates cannot reinterpret Option/Result as control.
mod local_return_private {
    use super::*;
    pub trait Sealed<T: Send + Sync + 'static> {}
    impl<T: Send + Sync + 'static> Sealed<T> for T {}
    impl<T: Send + Sync + 'static> Sealed<T> for output::Output<T> {}
}

/// One local return contract: a plain value broadcasts, while `Output<T>`
/// can withhold production entirely or carry a value and its destinations.
/// The type parameter avoids competing blanket impls and runtime node kinds.
#[doc(hidden)]
pub trait LocalReturn<T: Send + Sync + 'static>: local_return_private::Sealed<T> + 'static {
    fn into_local_emission(self) -> Option<Emission>;
}
impl<T: Send + Sync + 'static> LocalReturn<T> for T {
    fn into_local_emission(self) -> Option<Emission> {
        Some(emit_local(0, self))
    }
}
impl<T: Send + Sync + 'static> LocalReturn<T> for output::Output<T> {
    fn into_local_emission(self) -> Option<Emission> {
        self.into_emission()
    }
}

/// The *same* physical node action handles ordinary and directed local
/// returns. Neither creates a second scheduler, output interpreter, or
/// special node category; input ownership is shared as well.
struct LocalFunctionNodeAction<A, O, R, F>(F, PhantomData<fn(A) -> (O, R)>);
impl<A, O, R, F> NodeAction for LocalFunctionNodeAction<A, O, R, F>
where
    A: Clone + Send + Sync + 'static,
    O: Send + Sync + 'static,
    R: LocalReturn<O>,
    F: Fn(A) -> R + Send + Sync,
{
    fn execute(&self, mut ctx: InvocationCtx) -> Result<Vec<Emission>, String> {
        let owned = ctx.take_local::<A>(0)?;
        Ok((self.0)(owned).into_local_emission().into_iter().collect())
    }
}

/// Local structural layouts consume selected values only after the port has
/// committed a reservation. This permits move-only fields and avoids redundant
/// Arc cloning by the pre-admission borrowed decoder.
struct LocalPreparedFunctionNodeAction<L, O, R, F> {
    handler: F,
    slots: Vec<InputSlot>,
    _types: PhantomData<fn(L) -> (O, R)>,
}
impl<L, O, R, F> NodeAction for LocalPreparedFunctionNodeAction<L, O, R, F>
where
    L: LocalInputLayout,
    O: Send + Sync + 'static,
    R: LocalReturn<O>,
    F: Fn(L::Value) -> R + Send + Sync,
{
    fn execute(&self, mut ctx: InvocationCtx) -> Result<Vec<Emission>, String> {
        let InvocationArgs::Raw(raw) = std::mem::replace(&mut ctx.args, InvocationArgs::Taken)
        else {
            return Err("local layout requires atomically selected raw values".into());
        };
        // The schema is established once at authoring, never recomputed from
        // arbitrary user layout code during an admitted invocation.
        let mut owned = OwnedSelectedValues::new(raw, self.slots.clone());
        let args = L::prepare_owned(&mut owned)?;
        Ok((self.handler)(args)
            .into_local_emission()
            .into_iter()
            .collect())
    }
}

struct ContextFunctionNodeAction<A, O, F>(F, PhantomData<fn(A) -> O>);
impl<A, O, F> NodeAction for ContextFunctionNodeAction<A, O, F>
where
    A: Send + 'static,
    O: NodeOutput,
    F: Fn(A, &InvocationCtx) -> Result<O, String> + Send + Sync,
{
    fn execute(&self, mut ctx: InvocationCtx) -> Result<Vec<Emission>, String> {
        let args = ctx.take_prepared::<A>()?;
        (self.0)(args, &ctx).map(|output| vec![output.into_emission()])
    }
}

type PortDispatch<E> = dyn Fn(&mut InvocationCtx) -> Result<E, String> + Send + Sync;
pub struct TypedPort<E> {
    port: Port,
    dispatch: Arc<PortDispatch<E>>,
}
impl<E: Send + 'static> TypedPort<E> {
    /// Build a port and its checked adapter into a shared event type.
    ///
    /// ```compile_fail
    /// use phenix_graph::{AllSameCohort, Atomic, TypedPort};
    /// let _port = TypedPort::new::<Atomic<i32>, _>(0, "number", AllSameCohort,
    ///     |text: String| text.len());
    /// ```
    pub fn new<L, F>(
        id: u16,
        name: impl Into<String>,
        policy: impl PortPolicy + 'static,
        adapt: F,
    ) -> Result<Self, LayoutError>
    where
        L: DecodedInputLayout,
        F: Fn(L::Value) -> E + Send + Sync + 'static,
    {
        let port = Port::for_layout::<L>(id, name, policy)?;
        let dispatch = Arc::new(move |ctx: &mut InvocationCtx| {
            let prepared = ctx.take_prepared::<L::Value>()?;
            Ok(adapt(prepared))
        });
        Ok(Self { port, dispatch })
    }
}

struct MultiPortNodeAction<E, O, F> {
    dispatch: BTreeMap<u16, Arc<PortDispatch<E>>>,
    handler: F,
    _types: PhantomData<fn(E) -> O>,
}
impl<E, O, F> NodeAction for MultiPortNodeAction<E, O, F>
where
    E: Send + 'static,
    O: NodeOutput,
    F: Fn(E, &InvocationCtx) -> Result<O, String> + Send + Sync,
{
    fn execute(&self, mut ctx: InvocationCtx) -> Result<Vec<Emission>, String> {
        let port = ctx
            .source_port_id()
            .ok_or("typed input dispatch requires an input port")?;
        let dispatch = self
            .dispatch
            .get(&port)
            .ok_or_else(|| format!("no typed dispatch registered for input port {port}"))?;
        let input = dispatch(&mut ctx)?;
        (self.handler)(input, &ctx).map(|output| vec![output.into_emission()])
    }
}

// Keep the convenient, inferred `|ctx| { ... }` API without special runtime cases.
impl<F> NodeAction for F
where
    F: Fn(InvocationCtx) -> Result<Vec<Emission>, String> + Send + Sync,
{
    fn execute(&self, ctx: InvocationCtx) -> Result<Vec<Emission>, String> {
        (self)(ctx)
    }
}

mod provenance;
pub use provenance::GraphOrigin;

type NodeLogic = dyn NodeAction;
type NodeLogicFactory = dyn Fn() -> Arc<NodeLogic> + Send + Sync;
#[derive(Clone)]
pub struct NodeTemplate {
    composition_origin: Option<GraphOrigin>,
    pub claims: Vec<GraphContract>,
    id: NodeId,
    name: Option<String>,
    declaring_scope: Option<u64>,
    declaration_id: u64,
    ports: Vec<Port>,
    output: OutputSlot,
    grants: Vec<ResourceNeed>,
    cohort_mode: CohortMode,
    logic_factory: Arc<NodeLogicFactory>,
}
fn node_action<A: NodeAction + 'static>(
    id: u16,
    ports: Vec<Port>,
    output: OutputSlot,
    cohort_mode: CohortMode,
    grants: Vec<ResourceNeed>,
    action: A,
) -> NodeTemplate {
    // Stateless actions can be shared. For private state use node_action_with_factory.
    let action: Arc<NodeLogic> = Arc::new(action);
    let logic_factory: Arc<NodeLogicFactory> = Arc::new(move || Arc::clone(&action));
    NodeTemplate {
        composition_origin: None,
        claims: Vec::new(),
        id: NodeId(id),
        name: None,
        declaring_scope: None,
        declaration_id: 0,
        ports,
        output,
        grants,
        cohort_mode,
        logic_factory,
    }
}
fn node_action_with_factory<A, Factory>(
    id: u16,
    ports: Vec<Port>,
    output: OutputSlot,
    cohort_mode: CohortMode,
    grants: Vec<ResourceNeed>,
    factory: Factory,
) -> NodeTemplate
where
    A: NodeAction + 'static,
    Factory: Fn() -> A + Send + Sync + 'static,
{
    let logic_factory: Arc<NodeLogicFactory> = Arc::new(move || Arc::new(factory()));
    NodeTemplate {
        composition_origin: None,
        claims: Vec::new(),
        id: NodeId(id),
        name: None,
        declaring_scope: None,
        declaration_id: 0,
        ports,
        output,
        grants,
        cohort_mode,
        logic_factory,
    }
}
fn node<F>(
    id: u16,
    ports: Vec<Port>,
    output: OutputSlot,
    cohort_mode: CohortMode,
    grants: Vec<ResourceNeed>,
    logic: F,
) -> NodeTemplate
where
    F: Fn(InvocationCtx) -> Result<Vec<Emission>, String> + Send + Sync + 'static,
{
    node_action(id, ports, output, cohort_mode, grants, logic)
}
/// Construct a fresh logic closure for each physical instance produced by module expansion.
fn node_with_factory<F, Factory>(
    id: u16,
    ports: Vec<Port>,
    output: OutputSlot,
    cohort_mode: CohortMode,
    grants: Vec<ResourceNeed>,
    factory: Factory,
) -> NodeTemplate
where
    F: Fn(InvocationCtx) -> Result<Vec<Emission>, String> + Send + Sync + 'static,
    Factory: Fn() -> F + Send + Sync + 'static,
{
    node_action_with_factory(id, ports, output, cohort_mode, grants, factory)
}
struct PhysicalNode {
    id: NodeId,
    name: Option<String>,
    declaring_scope: Option<u64>,
    declaration_id: u64,
    ports: Vec<Port>,
    output: OutputSlot,
    grants: Vec<ResourceNeed>,
    cohort_mode: CohortMode,
    logic_factory: Arc<NodeLogicFactory>,
}
impl NodeTemplate {
    /// Compose heterogeneous input layouts into one explicitly typed event
    /// argument. Each adapter is checked against the handler's event type.
    pub fn multi_port<E, O, F>(ports: Vec<TypedPort<E>>, handler: F) -> Self
    where
        E: Send + 'static,
        O: NodeOutput,
        F: Fn(E, &InvocationCtx) -> Result<O, String> + Send + Sync + 'static,
    {
        let mut dispatch = BTreeMap::new();
        let mut declarations = Vec::with_capacity(ports.len());
        for typed in ports {
            dispatch.insert(typed.port.id.get(), typed.dispatch);
            declarations.push(typed.port);
        }
        Self::with_action(
            0,
            declarations,
            O::output_slot(),
            MultiPortNodeAction {
                dispatch,
                handler,
                _types: PhantomData,
            },
        )
    }

    /// Register a conventional one-input/one-output function using an atomic
    /// input contract and inferred Rust function argument/output types.
    pub fn function<A, O, F>(handler: F) -> Self
    where
        A: ContractValue,
        O: ContractValue,
        F: Fn(A) -> O + Send + Sync + 'static,
    {
        Self::with_action(
            0,
            vec![
                Port::all(0, vec![InputSlot::deferred::<A>(0).named("value")])
                    .named("run")
                    .with_args_builder(LayoutArgsBuilder::<Atomic<A>>(PhantomData)),
            ],
            OutputSlot::typed::<O>(0).named("value"),
            FunctionNodeAction::<A, O, F>(handler, PhantomData),
        )
    }

    /// The canonical local function adapter for both plain and explicitly
    /// routed values. The return `R` supplies one `O` payload, optionally
    /// wrapped in `Output<O>` to select existing outgoing edges. Plain
    /// Option/Result values are transported unchanged.
    pub fn local_function<A, O, R, F>(handler: F) -> Self
    where
        A: Clone + Send + Sync + 'static,
        O: Send + Sync + 'static,
        R: LocalReturn<O>,
        F: Fn(A) -> R + Send + Sync + 'static,
    {
        Self::with_action(
            0,
            vec![Port::all(0, vec![InputSlot::local::<A>(0).named("value")]).named("run")],
            OutputSlot::local::<O>(0).named("value"),
            LocalFunctionNodeAction::<A, O, R, F>(handler, PhantomData),
        )
    }

    /// Codec-free structured input and codec-free output.
    pub fn local_with_layout<L, O, R, F>(handler: F) -> Result<Self, LayoutError>
    where
        L: LocalInputLayout,
        O: Send + Sync + 'static,
        R: LocalReturn<O>,
        F: Fn(L::Value) -> R + Send + Sync + 'static,
    {
        let slots = L::slots()?;
        Ok(Self::with_action(
            0,
            vec![Port::all(0, slots.clone()).named("run")],
            OutputSlot::local::<O>(0).named("value"),
            LocalPreparedFunctionNodeAction::<L, O, R, F> {
                handler,
                slots,
                _types: PhantomData,
            },
        ))
    }
    /// Structural counterpart to `function`; the handler still receives the
    /// original domain value reconstructed from independent leaf slots.
    pub fn flattened<A, O, F>(handler: F) -> Result<Self, LayoutError>
    where
        A: PortLayout + PortLayoutAt<1>,
        O: ContractValue,
        F: Fn(A) -> O + Send + Sync + 'static,
    {
        Ok(Self::with_action(
            0,
            vec![Port::for_layout::<Flatten<A>>(0, "run", AllSameCohort)?],
            OutputSlot::typed::<O>(0).named("value"),
            FunctionNodeAction::<A, O, F>(handler, PhantomData),
        ))
    }

    /// Choose any explicit input layout while retaining ordinary inferred
    /// single-value output emission.
    ///
    /// ```compile_fail
    /// use phenix_graph::{AllSameCohort, Atomic, NodeTemplate};
    /// let _node = NodeTemplate::with_input_layout::<Atomic<i32>, i32, _>(
    ///     "run", AllSameCohort, |text: String| text.len() as i32);
    /// ```
    pub fn with_input_layout<L, O, F>(
        port_name: impl Into<String>,
        policy: impl PortPolicy + 'static,
        handler: F,
    ) -> Result<Self, LayoutError>
    where
        L: DecodedInputLayout,
        O: ContractValue,
        F: Fn(L::Value) -> O + Send + Sync + 'static,
    {
        let port = Port::for_layout::<L>(0, port_name, policy)?;
        Ok(Self::with_action(
            0,
            vec![port],
            OutputSlot::typed::<O>(0).named("value"),
            FunctionNodeAction::<L::Value, O, F>(handler, PhantomData),
        ))
    }

    /// General typed adapter for contextual single-output handlers and layouts.
    pub fn with_layout<L, O, F>(
        port_name: impl Into<String>,
        policy: impl PortPolicy + 'static,
        handler: F,
    ) -> Result<Self, LayoutError>
    where
        L: DecodedInputLayout,
        O: NodeOutput,
        F: Fn(L::Value, &InvocationCtx) -> Result<O, String> + Send + Sync + 'static,
    {
        let port = Port::for_layout::<L>(0, port_name, policy)?;
        Ok(Self::with_action(
            0,
            vec![port],
            O::output_slot(),
            ContextFunctionNodeAction::<L::Value, O, F>(handler, PhantomData),
        ))
    }

    /// Derive input schema and decoding from A, output metadata from O,
    /// and A/O from an annotated closure whenever Rust can infer them.
    /// `add_named` assigns the local node ID; no node/port IDs are required here.
    pub fn typed<A, O, F>(
        port_name: impl Into<String>,
        policy: impl PortPolicy + 'static,
        handler: F,
    ) -> Result<Self, LayoutError>
    where
        A: PortArgs,
        O: NodeOutput,
        F: Fn(A, &InvocationCtx) -> Result<O, String> + Send + Sync + 'static,
    {
        let port = Port::for_layout::<A>(0, port_name, policy)?;
        Ok(Self::with_action(
            0,
            vec![port],
            O::output_slot(),
            TypedNodeAction::<A, O, F>(handler, PhantomData),
        ))
    }

    /// Fresh typed handler and private state for each physical engine instance.
    pub fn typed_factory<A, O, F, Factory>(
        port_name: impl Into<String>,
        policy: impl PortPolicy + 'static,
        factory: Factory,
    ) -> Result<Self, LayoutError>
    where
        A: PortArgs,
        O: NodeOutput,
        F: Fn(A, &InvocationCtx) -> Result<O, String> + Send + Sync + 'static,
        Factory: Fn() -> F + Send + Sync + 'static,
    {
        let port = Port::for_layout::<A>(0, port_name, policy)?;
        Ok(Self::with_action_factory(
            0,
            vec![port],
            O::output_slot(),
            move || TypedNodeAction::<A, O, F>(factory(), PhantomData),
        ))
    }

    fn instantiate(&self, id: NodeId) -> PhysicalNode {
        PhysicalNode {
            id,
            name: self.name.clone(),
            declaring_scope: self.declaring_scope,
            declaration_id: self.declaration_id,
            ports: self.ports.clone(),
            output: self.output.clone(),
            grants: self.grants.clone(),
            cohort_mode: self.cohort_mode,
            logic_factory: Arc::clone(&self.logic_factory),
        }
    }
    pub fn new<F>(id: u16, ports: Vec<Port>, output: OutputSlot, logic: F) -> Self
    where
        F: Fn(InvocationCtx) -> Result<Vec<Emission>, String> + Send + Sync + 'static,
    {
        node(id, ports, output, CohortMode::Inherit, vec![], logic)
    }
    pub fn with_factory<F, Factory>(
        id: u16,
        ports: Vec<Port>,
        output: OutputSlot,
        factory: Factory,
    ) -> Self
    where
        F: Fn(InvocationCtx) -> Result<Vec<Emission>, String> + Send + Sync + 'static,
        Factory: Fn() -> F + Send + Sync + 'static,
    {
        node_with_factory(id, ports, output, CohortMode::Inherit, vec![], factory)
    }
    /// Declarative struct-literal form: `NodeTemplate::with_action(..., MyAction { ... })`.
    pub fn with_action<A: NodeAction + 'static>(
        id: u16,
        ports: Vec<Port>,
        output: OutputSlot,
        action: A,
    ) -> Self {
        node_action(id, ports, output, CohortMode::Inherit, vec![], action)
    }
    /// Accept an already-erased action selected dynamically by a plugin or registry.
    /// The Arc is intentionally shared by every physical instance of this template.
    pub fn with_dyn_action(
        id: u16,
        ports: Vec<Port>,
        output: OutputSlot,
        action: Arc<dyn NodeAction>,
    ) -> Self {
        let logic_factory: Arc<NodeLogicFactory> = Arc::new(move || Arc::clone(&action));
        Self {
            composition_origin: None,
            claims: Vec::new(),
            id: NodeId(id),
            name: None,
            declaring_scope: None,
            declaration_id: 0,
            ports,
            output,
            grants: vec![],
            cohort_mode: CohortMode::Inherit,
            logic_factory,
        }
    }
    /// Fresh action instance for each physical node; never share mutable closure state
    /// accidentally across distinct module instances.
    pub fn with_action_factory<A, Factory>(
        id: u16,
        ports: Vec<Port>,
        output: OutputSlot,
        factory: Factory,
    ) -> Self
    where
        A: NodeAction + 'static,
        Factory: Fn() -> A + Send + Sync + 'static,
    {
        node_action_with_factory(id, ports, output, CohortMode::Inherit, vec![], factory)
    }
    /// Named input addressing is resolved at definition time, not by a macro.
    pub fn input(&self, port: &str, slot: &str) -> Result<InputAddr, String> {
        let port = self
            .ports
            .iter()
            .find(|p| p.name.as_deref() == Some(port))
            .ok_or_else(|| format!("unknown input port {port:?}"))?;
        let slot = port
            .slots
            .iter()
            .find(|s| s.name.as_deref() == Some(slot))
            .ok_or_else(|| format!("unknown input slot {slot:?}"))?;
        Ok(InputAddr {
            node: self.id,
            port: port.id,
            slot: slot.id,
        })
    }
    pub fn output(&self, slot: &str) -> Result<OutputAddr, String> {
        if self.output.name.as_deref() != Some(slot) {
            return Err(format!("unknown output {slot:?}"));
        }
        Ok(OutputAddr {
            node: self.id,
            slot: self.output.id,
        })
    }
    pub fn input_handle<T: ContractValue>(
        &self,
        port: &str,
        slot: &str,
    ) -> Result<InputHandle<T>, String> {
        let address = self.input(port, slot)?;
        let declared = self
            .ports
            .iter()
            .find(|p| p.id == address.port)
            .unwrap()
            .slots
            .iter()
            .find(|s| s.id == address.slot)
            .unwrap();
        if declared.schema != T::wire_schema() {
            return Err(format!(
                "input {port}.{slot} schema {} differs from {}",
                declared.schema,
                T::wire_schema()
            ));
        }
        Ok(InputHandle {
            address,
            _type: PhantomData,
        })
    }
    pub fn output_handle<T: ContractValue>(&self, slot: &str) -> Result<OutputHandle<T>, String> {
        let address = self.output(slot)?;
        let declared = &self.output;
        if declared.schema != T::wire_schema() {
            return Err(format!(
                "output {slot} schema {} differs from {}",
                declared.schema,
                T::wire_schema()
            ));
        }
        Ok(OutputHandle {
            address,
            _type: PhantomData,
        })
    }
    /// Explicit resource grants, independent of dataflow connectivity.
    pub fn reads(mut self, key: &'static str) -> Self {
        if !self.grants.iter().any(|g| g.key == key) {
            self.grants.push(ResourceNeed {
                key,
                access: Access::Read,
            });
        }
        self
    }
    pub fn writes(mut self, key: &'static str) -> Self {
        if let Some(grant) = self.grants.iter_mut().find(|g| g.key == key) {
            grant.access = Access::Write;
        } else {
            self.grants.push(ResourceNeed {
                key,
                access: Access::Write,
            });
        }
        self
    }

    pub fn named(mut self, name: impl Into<String>) -> Result<Self, ModuleNameError> {
        let name = name.into();
        validate_name_segment(&name)?;
        self.name = Some(name);
        Ok(self)
    }
    pub fn with_fresh_cohort(mut self) -> Self {
        self.cohort_mode = CohortMode::FreshPerInvocation;
        self
    }
}
fn slot(id: u16, schema: Arc<str>, buffer: BufferMode) -> InputSlot {
    InputSlot {
        id: SlotId(id),
        name: None,
        schema,
        local_type: None,
        buffer,
    }
}
fn out(id: u16, schema: Arc<str>) -> OutputSlot {
    OutputSlot {
        id: SlotId(id),
        name: None,
        schema,
        local_type: None,
    }
}
fn all(id: u16, slots: Vec<InputSlot>) -> Port {
    Port {
        id: PortId(id),
        name: None,
        slots,
        policy: Arc::new(AllSameCohort),
        args: None,
    }
}
fn any(id: u16, slots: Vec<InputSlot>) -> Port {
    Port {
        id: PortId(id),
        name: None,
        slots,
        policy: Arc::new(AnyOne),
        args: None,
    }
}
fn custom_policy(id: u16, slots: Vec<InputSlot>, policy: impl PortPolicy + 'static) -> Port {
    Port {
        id: PortId(id),
        name: None,
        slots,
        policy: Arc::new(policy),
        args: None,
    }
}
impl Port {
    /// Canonical policy constructor: accepts presets or user-defined structs.
    pub fn new(id: u16, slots: Vec<InputSlot>, policy: impl PortPolicy + 'static) -> Self {
        Self::with_policy(id, slots, Arc::new(policy))
    }
    pub fn all(id: u16, slots: Vec<InputSlot>) -> Self {
        all(id, slots)
    }
    pub fn any(id: u16, slots: Vec<InputSlot>) -> Self {
        any(id, slots)
    }
    pub fn with_policy(id: u16, slots: Vec<InputSlot>, policy: Arc<dyn PortPolicy>) -> Self {
        Port {
            id: PortId(id),
            name: None,
            slots,
            policy,
            args: None,
        }
    }
    /// All configuration remains available without macros or derive expansion.
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
    pub fn with_buffer(mut self, mode: BufferMode) -> Self {
        for slot in &mut self.slots {
            slot.buffer = mode;
        }
        self
    }
    pub fn slot_buffer(mut self, path: &str, mode: BufferMode) -> Result<Self, String> {
        let slot = self
            .slots
            .iter_mut()
            .find(|slot| slot.name.as_deref() == Some(path))
            .ok_or_else(|| format!("unknown input path {path:?}"))?;
        slot.buffer = mode;
        Ok(self)
    }
    pub fn for_layout<L: DecodedInputLayout>(
        id: u16,
        name: impl Into<String>,
        policy: impl PortPolicy + 'static,
    ) -> Result<Self, LayoutError> {
        Ok(Self::new(id, L::slots()?, policy)
            .named(name)
            .with_args_builder(LayoutArgsBuilder::<L>(PhantomData)))
    }
    pub fn with_args_builder(mut self, builder: impl ArgsBuilder + 'static) -> Self {
        self.args = Some(Arc::new(builder));
        self
    }
    pub fn with_erased_args_builder(mut self, builder: Arc<dyn ArgsBuilder>) -> Self {
        self.args = Some(builder);
        self
    }
    /// Zero-macro typed decoding with an arbitrary closure.
    pub fn args<A, F>(self, builder: F) -> Self
    where
        A: Send + Sync + 'static,
        F: for<'a> Fn(&SelectedValues<'a>) -> Result<A, String> + Send + Sync + 'static,
    {
        self.with_args_builder(ClosureArgsBuilder::<A, F>(builder, PhantomData))
    }
    /// Validate selected values atomically, retaining the raw arguments on success.
    pub fn validate<F>(self, validator: F) -> Self
    where
        F: for<'a> Fn(&SelectedValues<'a>) -> Result<(), String> + Send + Sync + 'static,
    {
        self.with_args_builder(ValidatorArgsBuilder(validator))
    }
    pub fn all_after_upstream(id: u16, slots: Vec<InputSlot>) -> Self {
        custom_policy(id, slots, WhenUpstreamSettles::new(AllSameCohort))
    }
    pub fn any_after_upstream(id: u16, slots: Vec<InputSlot>) -> Self {
        custom_policy(id, slots, WhenUpstreamSettles::new(AnyOne))
    }
}

#[derive(Clone, Copy, Debug)]
struct Edge {
    id: EdgeId,
    from: OutputAddr,
    to: InputAddr,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModuleNameError {
    InvalidSegment(String),
    DuplicateName(String),
    DuplicateNodeId(u16),
    ExhaustedNodeIds,
}
impl fmt::Display for ModuleNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSegment(segment) => write!(f, "invalid name path segment {segment:?}"),
            Self::DuplicateName(name) => write!(f, "duplicate name {name:?} in scope"),
            Self::DuplicateNodeId(id) => write!(f, "duplicate node ID {id}"),
            Self::ExhaustedNodeIds => write!(f, "declaration node ID space exhausted"),
        }
    }
}
impl std::error::Error for ModuleNameError {}
/// The two independently owned failure stages in a graph declaration.
/// Layout expansion is checked before mutating the module builder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GraphBuildError {
    Layout(LayoutError),
    Name(ModuleNameError),
    Configuration(String),
}
impl From<LayoutError> for GraphBuildError {
    fn from(error: LayoutError) -> Self {
        Self::Layout(error)
    }
}
impl From<ModuleNameError> for GraphBuildError {
    fn from(error: ModuleNameError) -> Self {
        Self::Name(error)
    }
}
impl fmt::Display for GraphBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Layout(error) => write!(f, "{error}"),
            Self::Name(error) => write!(f, "{error}"),
            Self::Configuration(error) => write!(f, "{error}"),
        }
    }
}
impl std::error::Error for GraphBuildError {}

fn validate_name_segment(name: &str) -> Result<(), ModuleNameError> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.chars().any(char::is_control)
    {
        return Err(ModuleNameError::InvalidSegment(name.to_owned()));
    }
    Ok(())
}
fn parse_name_path(path: &str) -> Option<Vec<&str>> {
    let segments: Vec<_> = path.split('/').collect();
    if segments.is_empty()
        || segments
            .iter()
            .any(|segment| validate_name_segment(segment).is_err())
    {
        None
    } else {
        Some(segments)
    }
}
fn qualified_name(path: &[String], name: &str) -> String {
    path.iter()
        .map(String::as_str)
        .chain(std::iter::once(name))
        .collect::<Vec<_>>()
        .join("/")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModuleInputTarget {
    Node(InputAddr),
    /// Resolved against the current declaration when the graph is compiled.
    NodeNamed {
        node: String,
        port: String,
        slot: String,
    },
    Instance {
        name: String,
        port: u16,
    },
    InstanceNamed {
        name: String,
        port: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModuleOutputSource {
    Node(OutputAddr),
    /// Resolved against the current declaration when the graph is compiled.
    NodeNamed {
        node: String,
        output: String,
    },
    Instance {
        name: String,
        port: u16,
    },
    InstanceNamed {
        name: String,
        port: String,
    },
}
// A node endpoint is usable directly in module topology; an instance boundary
// uses the explicit Instance variant. Neither form embeds destinations in a node.
impl From<OutputAddr> for ModuleOutputSource {
    fn from(addr: OutputAddr) -> Self {
        Self::Node(addr)
    }
}
impl From<InputAddr> for ModuleInputTarget {
    fn from(addr: InputAddr) -> Self {
        Self::Node(addr)
    }
}
#[derive(Clone, Debug)]
pub struct ModuleEdge {
    /// Stable within this editable declaration; never assigned by node authors.
    /// Distinct from the flattened runtime `EdgeId`.
    pub declaration_id: u32,
    pub from: ModuleOutputSource,
    pub to: ModuleInputTarget,
}
#[derive(Clone, Debug)]
pub struct ModuleInput {
    pub id: u16,
    /// Unique flat routing key, independent of public port/slot metadata.
    pub name: String,
    pub port: String,
    pub slot: String,
    pub schema: Arc<str>,
    pub local_type: Option<TypeId>,
    pub targets: Vec<ModuleInputTarget>,
}
impl ModuleInput {
    pub fn identity(&self) -> TypeIdentity {
        identity(&self.schema, self.local_type)
    }
}
fn module_input_slot_name(port: &str, slot: &str) -> String {
    // Literal defaults outside this namespace keep their historical routing key.
    // Escape defaults inside it too, so no literal can alias an encoded pair.
    if slot == "value" && !port.starts_with("__slot_") {
        port.to_owned()
    } else {
        format!("__slot_{}:{}:{port}{slot}", port.len(), slot.len())
    }
}
#[derive(Clone, Debug)]
pub struct ModuleOutput {
    pub id: u16,
    pub name: String,
    pub schema: Arc<str>,
    pub local_type: Option<TypeId>,
    pub source: ModuleOutputSource,
}
impl ModuleOutput {
    pub fn identity(&self) -> TypeIdentity {
        identity(&self.schema, self.local_type)
    }
}
#[derive(Clone, Debug)]
pub struct ModuleInstance {
    pub name: String,
    pub definition: String,
}
impl ModuleInstance {
    pub fn new(
        name: impl Into<String>,
        definition: impl Into<String>,
    ) -> Result<Self, ModuleNameError> {
        let name = name.into();
        validate_name_segment(&name)?;
        Ok(Self {
            name,
            definition: definition.into(),
        })
    }
}
pub enum NodeDef {
    Node(NodeTemplate),
    Module(ModuleInstance),
}
// Let a graph consumer write `module.add(template)` or
// `module.add(ModuleInstance::new(...)? )` without wrapping structural enums.
impl From<NodeTemplate> for NodeDef {
    fn from(template: NodeTemplate) -> Self {
        Self::Node(template)
    }
}
impl From<ModuleInstance> for NodeDef {
    fn from(instance: ModuleInstance) -> Self {
        Self::Module(instance)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModuleTriggerTarget {
    PublicInput(u16),
    Internal(InputAddr),
    /// A direct zero-input node or a no-public-input module operation.
    Source(String),
}
#[derive(Clone, Debug)]
pub struct ModuleTrigger {
    pub id: TriggerId,
    pub name: String,
    pub targets: Vec<ModuleTriggerTarget>,
}
#[derive(Clone)]
pub struct ModuleDef {
    /// Instance-name to authoring declaration identity, retained for typed destinations.
    /// Low-level definitions without authoring handles may leave this empty.
    pub declaration_ids: BTreeMap<String, u64>,
    pub claims: Vec<GraphContract>,
    pub contract_requests: Vec<(String, GraphContract)>,
    pub id: String,
    pub nodes: Vec<NodeTemplate>,
    pub instances: Vec<ModuleInstance>,
    pub edges: Vec<ModuleEdge>,
    pub inputs: Vec<ModuleInput>,
    /// Explicitly chosen source. Optional only for an uninstanced root container.
    pub output: Option<ModuleOutput>,
}
/// A composition entry binds external triggers without modifying the module definition.
#[derive(Clone)]
pub struct ModuleEntry {
    pub definition: ModuleDef,
    pub triggers: Vec<ModuleTrigger>,
}
static NEXT_DECLARATION_SCOPE: AtomicU64 = AtomicU64::new(1);

/// An owned declaration handle. Unlike compiled NodeId, this identifies a
/// named node in one specific authoring scope, including after local rewiring.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedNodeHandle {
    scope: u64,
    declaration_id: u64,
    name: String,
}

#[derive(Clone)]
pub struct ModuleDefBuilder {
    declaration_ids: BTreeMap<String, u64>,
    claims: Vec<GraphContract>,
    contract_requests: Vec<(String, GraphContract)>,
    id: String,
    scope: u64,
    nodes: Vec<NodeTemplate>,
    // Registration owns these indexes; replacements preserve names/IDs and
    // append-only transaction rollback removes entries and restores the cursor.
    node_names: BTreeMap<String, usize>,
    node_ids: BTreeSet<u16>,
    next_node_id: u32,
    instances: Vec<ModuleInstance>,
    edges: Vec<ModuleEdge>,
    next_connection: u32,
    inputs: Vec<ModuleInput>,
    output: Option<ModuleOutput>,
    triggers: Vec<ModuleTrigger>,
}
impl Default for ModuleDefBuilder {
    fn default() -> Self {
        Self {
            declaration_ids: BTreeMap::new(),
            claims: Vec::new(),
            contract_requests: Vec::new(),
            id: String::new(),
            scope: NEXT_DECLARATION_SCOPE.fetch_add(1, Ordering::Relaxed),
            nodes: vec![],
            node_names: BTreeMap::new(),
            node_ids: BTreeSet::new(),
            next_node_id: 1,
            instances: vec![],
            edges: vec![],
            next_connection: 0,
            inputs: vec![],
            output: None,
            triggers: vec![],
        }
    }
}

impl ModuleDefBuilder {
    fn named_node(&self, name: &str) -> Option<&NodeTemplate> {
        self.node_names.get(name).map(|&index| &self.nodes[index])
    }

    fn check_registration_name(&self, name: &str) -> Result<(), ModuleNameError> {
        validate_name_segment(name)?;
        if self.node_names.contains_key(name) || self.declaration_ids.contains_key(name) {
            return Err(ModuleNameError::DuplicateName(name.to_owned()));
        }
        Ok(())
    }

    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            ..Self::default()
        }
    }
    #[cfg(test)]
    fn node(&mut self, node: NodeTemplate) {
        self.add(NodeDef::Node(node))
            .expect("invalid node registration");
    }
    pub fn add(&mut self, node_def: impl Into<NodeDef>) -> Result<(), ModuleNameError> {
        match node_def.into() {
            NodeDef::Node(mut node) => {
                node.declaring_scope.get_or_insert(self.scope);
                node.declaration_id = NEXT_DECLARATION_SCOPE.fetch_add(1, Ordering::Relaxed);
                if node.id == NodeId(0) {
                    // 0 is the unassigned sentinel used by inference-first NodeTemplate::typed.
                    let next = u16::try_from(self.next_node_id)
                        .map_err(|_| ModuleNameError::ExhaustedNodeIds)?;
                    node.id = NodeId(next);
                }
                if let Some(name) = &node.name {
                    self.check_registration_name(name)?;
                }
                if self.node_ids.contains(&node.id.0) {
                    return Err(ModuleNameError::DuplicateNodeId(node.id.0));
                }
                self.node_ids.insert(node.id.0);
                while self.next_node_id <= u16::MAX as u32
                    && self.node_ids.contains(&(self.next_node_id as u16))
                {
                    self.next_node_id += 1;
                }
                if let Some(name) = &node.name {
                    self.node_names.insert(name.clone(), self.nodes.len());
                }
                self.nodes.push(node);
            }
            NodeDef::Module(instance) => {
                self.check_registration_name(&instance.name)?;
                self.declaration_ids.insert(
                    instance.name.clone(),
                    NEXT_DECLARATION_SCOPE.fetch_add(1, Ordering::Relaxed),
                );
                self.instances.push(instance);
            }
        }
        Ok(())
    }
    pub fn add_named(
        &mut self,
        name: impl Into<String>,
        mut node: NodeTemplate,
    ) -> Result<(), ModuleNameError> {
        let name = name.into();
        validate_name_segment(&name)?;
        node.name = Some(name);
        self.add(NodeDef::Node(node))
    }
    /// Register a physical node template and retain its declaration identity.
    pub fn register(
        &mut self,
        name: impl Into<String>,
        node: NodeTemplate,
    ) -> Result<NamedNodeHandle, ModuleNameError> {
        let name = name.into();
        self.add_named(name.clone(), node)?;
        Ok(NamedNodeHandle {
            scope: self.scope,
            declaration_id: self.nodes.last().expect("registered node").declaration_id,
            name,
        })
    }

    pub fn try_instance(
        &mut self,
        name: impl Into<String>,
        definition: impl Into<String>,
    ) -> Result<(), ModuleNameError> {
        self.add(NodeDef::Module(ModuleInstance {
            name: name.into(),
            definition: definition.into(),
        }))
    }
    pub fn input(
        &mut self,
        id: u16,
        name: impl Into<String>,
        schema: Arc<str>,
        targets: Vec<ModuleInputTarget>,
    ) {
        self.input_with_identity(id, name, schema, None, targets);
    }
    pub(crate) fn input_with_identity(
        &mut self,
        id: u16,
        name: impl Into<String>,
        schema: Arc<str>,
        local_type: Option<TypeId>,
        targets: Vec<ModuleInputTarget>,
    ) {
        self.input_slot_with_identity(id, name.into(), "value".into(), schema, local_type, targets);
    }
    pub(crate) fn input_slot_with_identity(
        &mut self,
        id: u16,
        port: String,
        slot: String,
        schema: Arc<str>,
        local_type: Option<TypeId>,
        targets: Vec<ModuleInputTarget>,
    ) {
        self.inputs.push(ModuleInput {
            id,
            name: module_input_slot_name(&port, &slot),
            port,
            slot,
            schema,
            local_type,
            targets,
        });
    }
    pub fn output(
        &mut self,
        id: u16,
        name: impl Into<String>,
        schema: Arc<str>,
        source: ModuleOutputSource,
    ) {
        self.output_with_identity(id, name, schema, None, source);
    }
    pub(crate) fn output_with_identity(
        &mut self,
        id: u16,
        name: impl Into<String>,
        schema: Arc<str>,
        local_type: Option<TypeId>,
        source: ModuleOutputSource,
    ) {
        assert!(self.output.is_none(), "module already has an output");
        self.output = Some(ModuleOutput {
            id,
            name: name.into(),
            schema,
            local_type,
            source,
        });
    }
    /// Connect one output to one input; the compiler assigns edge identities.
    pub fn connect(
        &mut self,
        from: impl Into<ModuleOutputSource>,
        to: impl Into<ModuleInputTarget>,
    ) -> &mut Self {
        self.next_connection = self
            .next_connection
            .checked_add(1)
            .expect("declaration connection identity exhausted");
        self.edges.push(ModuleEdge {
            declaration_id: self.next_connection,
            from: from.into(),
            to: to.into(),
        });
        self
    }
    /// Fan-out is just multiple ordinary connections (no special runtime node).
    pub fn fanout<I, T>(&mut self, from: impl Into<ModuleOutputSource>, targets: I) -> &mut Self
    where
        I: IntoIterator<Item = T>,
        T: Into<ModuleInputTarget>,
    {
        let source = from.into();
        for target in targets {
            self.connect(source.clone(), target);
        }
        self
    }
    /// Typed structural wiring: compiler still validates the actual graph.
    /// It is impossible to connect OutputHandle<i64> to InputHandle<String>.
    ///
    /// ```compile_fail
    /// use phenix_graph::{InputSlot, ModuleDefBuilder, NodeTemplate,
    ///     OutputSlot, Port, emit};
    /// let source = NodeTemplate::with_action(1, vec![],
    ///     OutputSlot::typed::<String>(0).named("text"), |_ctx| Ok(vec![emit(0, String::new())]));
    /// let target = NodeTemplate::with_action(2,
    ///     vec![Port::all(0, vec![InputSlot::immediate::<i32>(0).named("number")]).named("run")],
    ///     OutputSlot::typed::<()>(0).named("value"), |_ctx| Ok(vec![emit(0, ())]));
    /// let mut module = ModuleDefBuilder::new("wrong-types");
    /// module.connect_typed(source.output_handle::<String>("text").unwrap(),
    ///     target.input_handle::<i32>("run", "number").unwrap());
    /// ```
    pub fn connect_typed<T: ContractValue>(
        &mut self,
        from: OutputHandle<T>,
        to: InputHandle<T>,
    ) -> &mut Self {
        self.connect(from.address(), to.address())
    }
    /// Resolve a named source node or child-module public output.
    fn named_source(&self, node: &str, output: &str) -> Result<ModuleOutputSource, String> {
        if let Some(def) = self.nodes.iter().find(|n| n.name.as_deref() == Some(node)) {
            def.output(output)?; // fail early, but retain the symbolic declaration reference
            return Ok(ModuleOutputSource::NodeNamed {
                node: node.into(),
                output: output.into(),
            });
        }
        if self.instances.iter().any(|instance| instance.name == node) {
            return Ok(ModuleOutputSource::InstanceNamed {
                name: node.to_owned(),
                port: output.to_owned(),
            });
        }
        Err(format!("unknown source node/instance {node:?}"))
    }
    /// Named wiring of local nodes, including child-module output sources.
    pub fn connect_named(
        &mut self,
        from_node: &str,
        from_output: &str,
        to_node: &str,
        to_port: &str,
        to_slot: &str,
    ) -> Result<&mut Self, String> {
        let from = self.named_source(from_node, from_output)?;
        self.nodes
            .iter()
            .find(|n| n.name.as_deref() == Some(to_node))
            .ok_or_else(|| format!("unknown destination node {to_node:?}"))?
            .input(to_port, to_slot)?; // validate now, resolve at compile time
        Ok(self.connect(
            from,
            ModuleInputTarget::NodeNamed {
                node: to_node.into(),
                port: to_port.into(),
                slot: to_slot.into(),
            },
        ))
    }
    /// A child module's public input has no nested slot; its definition
    /// resolves to one or more internal slots during graph compilation.
    pub fn connect_public_named(
        &mut self,
        from_node: &str,
        from_output: &str,
        to_instance: &str,
        public_input: &str,
    ) -> Result<&mut Self, String> {
        let from = self.named_source(from_node, from_output)?;
        if !self
            .instances
            .iter()
            .any(|instance| instance.name == to_instance)
        {
            return Err(format!("unknown destination instance {to_instance:?}"));
        }
        Ok(self.connect(
            from,
            ModuleInputTarget::InstanceNamed {
                name: to_instance.to_owned(),
                port: public_input.to_owned(),
            },
        ))
    }
    /// Entry bindings are packaged separately from the reusable ModuleDef.
    pub fn trigger(&mut self, id: u16, targets: Vec<ModuleTriggerTarget>) {
        self.trigger_named(id, id.to_string(), targets);
    }
    /// Define a stable external name independently of the generated trigger ID.
    pub fn trigger_named(
        &mut self,
        id: u16,
        name: impl Into<String>,
        targets: Vec<ModuleTriggerTarget>,
    ) {
        self.triggers.push(ModuleTrigger {
            id: TriggerId(id),
            name: name.into(),
            targets,
        });
    }
    pub fn build(self) -> ModuleEntry {
        ModuleEntry {
            definition: ModuleDef {
                declaration_ids: self.declaration_ids,
                claims: self.claims,
                contract_requests: self.contract_requests,
                id: self.id,
                nodes: self.nodes,
                instances: self.instances,
                edges: self.edges,
                inputs: self.inputs,
                output: self.output,
            },
            triggers: self.triggers,
        }
    }
}

/// Small flat-graph authoring facade. It accepts ordinary functions and typed
/// symbolic endpoints while compiling through the same module/compiler path.

#[derive(Clone, Debug, PartialEq, Eq)]
struct NodeProvenance {
    definition: String,
    instance_path: Vec<String>,
    local_node: NodeId,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct EdgeProvenance {
    definition: String,
    instance_path: Vec<String>,
    local_edge: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct ModuleInstanceProvenance {
    declaration_id: Option<u64>,
    definition: String,
    instance_path: Vec<String>,
    inputs: BTreeMap<u16, Vec<InputAddr>>,
    output: Option<OutputAddr>,
}
#[derive(Clone)]
enum NamedUnit {
    Node(NodeId),
    Module(ModuleLookup),
}
#[derive(Clone)]
struct ModuleLookup {
    definition: String,
    instance_path: Vec<String>,
    instance_metadata: usize,
}
#[derive(Default)]
struct CompiledMetadata {
    root_definition: Option<String>,
    definitions: BTreeMap<String, Arc<ModuleDef>>,
    nodes: BTreeMap<NodeId, NodeProvenance>,
    edges: BTreeMap<EdgeId, EdgeProvenance>,
    instances: Vec<ModuleInstanceProvenance>,
    inputs: BTreeMap<u16, Vec<InputAddr>>,
    output: Option<OutputAddr>,
    name_index: BTreeMap<String, NamedUnit>,
    trigger_names: BTreeMap<String, TriggerId>,
}

#[derive(Default)]
struct FlatGraphBuilder {
    nodes: BTreeMap<NodeId, Arc<PhysicalNode>>,
    edges: Vec<Edge>,
    triggers: BTreeMap<TriggerId, Vec<Vec<InputAddr>>>,
    source_triggers: BTreeMap<TriggerId, Vec<NodeId>>,
    metadata: CompiledMetadata,
}
impl FlatGraphBuilder {
    // Both registration paths must share name and ID validation. In particular,
    // a pre-named template cannot bypass the compiled lookup index.
    #[cfg(test)]
    fn try_add_node(&mut self, template: NodeTemplate) -> Result<(), ModuleNameError> {
        if self.nodes.contains_key(&template.id) {
            return Err(ModuleNameError::DuplicateNodeId(template.id.0));
        }
        if let Some(name) = template.name.as_deref() {
            validate_name_segment(name)?;
            if self.metadata.name_index.contains_key(name) {
                return Err(ModuleNameError::DuplicateName(name.to_owned()));
            }
        }
        let node = template.instantiate(template.id);
        let id = node.id;
        if let Some(name) = node.name.as_deref() {
            self.metadata
                .name_index
                .insert(name.to_owned(), NamedUnit::Node(id));
        }
        self.nodes.insert(id, Arc::new(node));
        Ok(())
    }
    #[cfg(test)]
    fn add_node(&mut self, template: NodeTemplate) {
        self.try_add_node(template)
            .expect("invalid graph node registration");
    }
    #[cfg(test)]
    fn add_named(
        &mut self,
        name: impl Into<String>,
        mut template: NodeTemplate,
    ) -> Result<(), ModuleNameError> {
        template.name = Some(name.into());
        self.try_add_node(template)
    }
    /// Low-level flat-graph authoring; IDs exist only on the compiled edges.
    #[cfg(test)]
    fn connect(&mut self, from: OutputAddr, to: InputAddr) -> &mut Self {
        let id = EdgeId(u16::try_from(self.edges.len() + 1).expect("too many graph connections"));
        self.edges.push(Edge { id, from, to });
        self
    }
    #[cfg(test)]
    fn fanout(
        &mut self,
        from: OutputAddr,
        targets: impl IntoIterator<Item = InputAddr>,
    ) -> &mut Self {
        for target in targets {
            self.connect(from, target);
        }
        self
    }
    #[cfg(test)]
    fn trigger(&mut self, id: u16, targets: Vec<InputAddr>) {
        let id = TriggerId(id);
        assert!(!self.triggers.contains_key(&id), "duplicate trigger ID");
        self.triggers
            .insert(id, targets.into_iter().map(|target| vec![target]).collect());
    }
    fn build(self) -> Result<Arc<Graph>, String> {
        let mut graph = Graph {
            nodes: self.nodes,
            by_output: BTreeMap::new(),
            by_node: BTreeMap::new(),
            triggers: self.triggers,
            source_triggers: self.source_triggers,
            order: BTreeMap::new(),
            metadata: Arc::new(self.metadata),
            composition_origins: BTreeMap::new(),
            structural: OnceLock::new(),
        };
        for n in graph.nodes.values() {
            let mut port_ids = BTreeSet::new();
            let mut port_names = BTreeSet::new();
            for p in &n.ports {
                if !port_ids.insert(p.id) || p.slots.is_empty() {
                    return Err(format!("duplicate/empty port on {:?}", n.id));
                }
                if let Some(name) = &p.name {
                    validate_name_segment(name).map_err(|e| e.to_string())?;
                    if !port_names.insert(name.as_str()) {
                        return Err(format!("duplicate input port name {name:?}"));
                    }
                }
                let mut slot_ids = BTreeSet::new();
                let mut slot_names = BTreeSet::new();
                for s in &p.slots {
                    if !slot_ids.insert(s.id) {
                        return Err("duplicate input slot".into());
                    }
                    if let Some(name) = &s.name {
                        validate_name_segment(name).map_err(|e| e.to_string())?;
                        if !slot_names.insert(name.as_str()) {
                            return Err(format!("duplicate input slot name {name:?}"));
                        }
                    }
                }
            }
            if n.output.id != SlotId(0) {
                return Err("node output must use canonical slot 0".into());
            }
            if let Some(name) = &n.output.name {
                validate_name_segment(name).map_err(|e| e.to_string())?;
            }
        }
        let mut edge_ids = BTreeSet::new();
        let mut connections = BTreeSet::new();
        for e in self.edges {
            if !edge_ids.insert(e.id) {
                return Err("duplicate generated edge ID".into());
            }
            if !connections.insert((e.from, e.to)) {
                return Err("duplicate connection".into());
            }
            let src = graph.output_slot(e.from)?;
            let dst = graph.input_slot(e.to)?;
            if src.identity() != dst.identity() {
                return Err(format!(
                    "type identity mismatch on edge {:?}: {:?} != {:?}",
                    e.id,
                    src.identity(),
                    dst.identity()
                ));
            }
            graph.by_output.entry(e.from).or_default().push(e);
            graph.by_node.entry(e.from.node).or_default().push(e);
        }
        graph.order = graph.topological_order();
        for groups in graph.triggers.values() {
            for addresses in groups {
                for &addr in addresses {
                    graph.input_slot(addr)?;
                }
            }
        }
        assert!(
            graph
                .structural
                .set(StructuralIndex::compile(&graph))
                .is_ok(),
            "structural index initialized exactly once"
        );
        Ok(Arc::new(graph))
    }
}

/// Compilation remains a one-time transformation from editable declarations
/// to immutable physical graph. These phases are diagnostics, not executors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompilePhase {
    Declaration,
    Expansion,
    Triggers,
    Index,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileDiagnostic {
    pub phase: CompilePhase,
    pub definition: String,
    pub message: String,
}
impl CompileDiagnostic {
    fn at(phase: CompilePhase, definition: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            phase,
            definition: definition.into(),
            message: message.into(),
        }
    }
}
impl fmt::Display for CompileDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} [{}]: {}",
            self.phase, self.definition, self.message
        )
    }
}
impl std::error::Error for CompileDiagnostic {}

#[derive(Default)]
pub struct ModuleCatalog {
    definitions: BTreeMap<String, Arc<ModuleDef>>,
    entry_triggers: BTreeMap<String, Vec<ModuleTrigger>>,
    limits: CompileLimits,
}
impl ModuleCatalog {
    pub fn new() -> Self {
        Self::default()
    }
    /// Change only the compiler's resource guardrails, not graph semantics.
    pub fn limits(mut self, limits: CompileLimits) -> Self {
        self.limits = limits;
        self
    }
    pub fn add(&mut self, entry: ModuleEntry) -> Result<(), String> {
        let ModuleEntry {
            definition,
            triggers,
        } = entry;
        if self.definitions.contains_key(&definition.id) {
            return Err(format!("duplicate module definition {}", definition.id));
        }
        self.entry_triggers.insert(definition.id.clone(), triggers);
        self.definitions
            .insert(definition.id.clone(), Arc::new(definition));
        Ok(())
    }
    /// Validate reachable definitions independently of module expansion.
    /// All subsequent compilation uses exactly this immutable catalog.
    pub fn validate(&self, root: &str) -> Result<(), CompileDiagnostic> {
        fn visit(
            catalog: &ModuleCatalog,
            id: &str,
            active: &mut Vec<String>,
            visited: &mut BTreeSet<String>,
        ) -> Result<(), CompileDiagnostic> {
            if active.iter().any(|entry| entry == id) {
                return Err(CompileDiagnostic::at(
                    CompilePhase::Declaration,
                    id,
                    format!(
                        "recursive module definition: {} -> {id}",
                        active.join(" -> ")
                    ),
                ));
            }
            if active.len() >= catalog.limits.max_depth {
                return Err(CompileDiagnostic::at(
                    CompilePhase::Declaration,
                    id,
                    format!(
                        "module expansion depth exceeds {}",
                        catalog.limits.max_depth
                    ),
                ));
            }
            if visited.contains(id) {
                return Ok(());
            }
            let definition = catalog.definitions.get(id).ok_or_else(|| {
                CompileDiagnostic::at(
                    CompilePhase::Declaration,
                    id,
                    format!("unknown module definition {id}"),
                )
            })?;
            ModuleExpander::validate_local_definition(definition)
                .map_err(|error| CompileDiagnostic::at(CompilePhase::Declaration, id, error))?;
            active.push(id.to_owned());
            for instance in &definition.instances {
                if !catalog.definitions.contains_key(&instance.definition) {
                    return Err(CompileDiagnostic::at(
                        CompilePhase::Declaration,
                        id,
                        format!(
                            "instance {} references unknown definition {}",
                            instance.name, instance.definition
                        ),
                    ));
                }
                visit(catalog, &instance.definition, active, visited)?;
            }
            active.pop();
            visited.insert(id.to_owned());
            Ok(())
        }
        visit(self, root, &mut Vec::new(), &mut BTreeSet::new())
    }
    /// Compiles all contributions only after validation. The diagnostic phase
    /// shows whether a failure came from declarations, expansion, trigger
    /// binding or final physical indexing; no runtime state is instantiated.
    pub fn compile(&self, root: &str) -> Result<Arc<CompiledGraph>, CompileDiagnostic> {
        self.validate(root)?;
        let mut expander = ModuleExpander {
            catalog: self,
            builder: FlatGraphBuilder::default(),
            metadata: CompiledMetadata {
                definitions: BTreeMap::new(),
                ..CompiledMetadata::default()
            },
            next_node: 1,
            next_edge: 1,
            expanded_instances: Cell::new(0),
            expansion_work: Cell::new(0),
            limits: self.limits,
        };
        let mut stack = Vec::new();
        let mut path = Vec::new();
        let expanded = expander
            .expand_scope(root, &mut stack, &mut path)
            .map_err(|error| CompileDiagnostic::at(CompilePhase::Expansion, root, error))?;
        let mut trigger_ids = BTreeSet::new();
        let mut trigger_names = BTreeSet::new();
        for trigger in self.entry_triggers.get(root).into_iter().flatten() {
            if !trigger_ids.insert(trigger.id) {
                return Err(CompileDiagnostic::at(
                    CompilePhase::Triggers,
                    root,
                    "duplicate module trigger ID",
                ));
            }
            validate_name_segment(&trigger.name).map_err(|error| {
                CompileDiagnostic::at(CompilePhase::Triggers, root, error.to_string())
            })?;
            if !trigger_names.insert(trigger.name.as_str()) {
                return Err(CompileDiagnostic::at(
                    CompilePhase::Triggers,
                    root,
                    "duplicate module trigger name",
                ));
            }
            expander
                .metadata
                .trigger_names
                .insert(trigger.name.clone(), trigger.id);
            let mut targets = Vec::new();
            let mut sources = BTreeSet::new();
            for target in &trigger.targets {
                if let ModuleTriggerTarget::Source(name) = target {
                    sources.extend(expander.source_roots(name).map_err(|error| {
                        CompileDiagnostic::at(CompilePhase::Triggers, root, error)
                    })?);
                    continue;
                }
                let ModuleTriggerTarget::PublicInput(port) = target else {
                    return Err(CompileDiagnostic::at(
                        CompilePhase::Triggers,
                        root,
                        "module trigger bypasses its public input boundary",
                    ));
                };
                let addresses = expanded
                    .inputs
                    .get(port)
                    .ok_or_else(|| {
                        CompileDiagnostic::at(
                            CompilePhase::Triggers,
                            root,
                            format!("unknown public input {port} on root module"),
                        )
                    })?
                    .clone();
                expander
                    .charge(addresses.len() as u64)
                    .map_err(|error| CompileDiagnostic::at(CompilePhase::Triggers, root, error))?;
                targets.push(addresses);
            }
            expander.builder.triggers.insert(trigger.id, targets);
            expander
                .builder
                .source_triggers
                .insert(trigger.id, sources.into_iter().collect());
        }
        expander.metadata.root_definition = Some(root.to_owned());
        expander.metadata.inputs = expanded.inputs;
        expander.metadata.output = expanded.output;
        interface::validate_metadata(&expander.metadata)
            .map_err(|error| CompileDiagnostic::at(CompilePhase::Index, root, error.to_string()))?;
        expander.builder.metadata = expander.metadata;
        expander
            .builder
            .build()
            .map_err(|error| CompileDiagnostic::at(CompilePhase::Index, root, error))
    }
}

#[derive(Default)]
struct ExpandedScope {
    inputs: BTreeMap<u16, Vec<InputAddr>>,
    output: Option<OutputAddr>,
}
struct ModuleExpander<'a> {
    catalog: &'a ModuleCatalog,
    builder: FlatGraphBuilder,
    metadata: CompiledMetadata,
    next_node: u32,
    next_edge: u32,
    expanded_instances: Cell<u64>,
    expansion_work: Cell<u64>,
    limits: CompileLimits,
}
impl ModuleExpander<'_> {
    fn source_roots(&self, name: &str) -> Result<BTreeSet<NodeId>, String> {
        self.charge(1)?;
        validate_name_segment(name).map_err(|error| error.to_string())?;
        let (output, boundary) = match self.metadata.name_index.get(name) {
            Some(NamedUnit::Node(id)) => {
                if !self.builder.nodes[id].ports.is_empty() {
                    return Err("source start requires a zero-input node".into());
                }
                return Ok(BTreeSet::from([*id]));
            }
            Some(NamedUnit::Module(lookup)) => {
                let definition = &self.catalog.definitions[&lookup.definition];
                if !definition.inputs.is_empty() {
                    return Err("source start requires a module with no public inputs".into());
                }
                let instance = &self.metadata.instances[lookup.instance_metadata];
                (
                    instance
                        .output
                        .ok_or("source module has no public output")?,
                    &lookup.instance_path,
                )
            }
            None => return Err(format!("unknown source operation {name:?}")),
        };
        // Only producers inside this boundary that can contribute to its output
        // are roots. Disconnected child sources remain dormant.
        self.charge(self.builder.edges.len() as u64)?;
        let mut upstream: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
        for edge in &self.builder.edges {
            upstream
                .entry(edge.to.node)
                .or_default()
                .push(edge.from.node);
        }
        let mut pending = vec![output.node];
        let mut visited = BTreeSet::new();
        let mut roots = BTreeSet::new();
        while let Some(node) = pending.pop() {
            if !visited.insert(node)
                || !self.metadata.nodes[&node]
                    .instance_path
                    .starts_with(boundary)
            {
                continue;
            }
            self.charge(1)?;
            if self.builder.nodes[&node].ports.is_empty() {
                roots.insert(node);
            }
            pending.extend(upstream.get(&node).into_iter().flatten().copied());
        }
        if roots.is_empty() {
            return Err("source module output has no reachable zero-input producer".into());
        }
        Ok(roots)
    }

    fn expand_scope(
        &mut self,
        id: &str,
        stack: &mut Vec<String>,
        path: &mut Vec<String>,
    ) -> Result<ExpandedScope, String> {
        if stack.iter().any(|active| active == id) {
            return Err(format!(
                "recursive module definition: {} -> {id}",
                stack.join(" -> ")
            ));
        }
        if stack.len() >= self.limits.max_depth {
            return Err(format!(
                "module expansion depth exceeds {}",
                self.limits.max_depth
            ));
        }
        let definition = self
            .catalog
            .definitions
            .get(id)
            .ok_or_else(|| format!("unknown module definition {id}"))?
            .clone();
        self.metadata
            .definitions
            .entry(id.to_owned())
            .or_insert_with(|| Arc::clone(&definition));
        let body = &definition;
        Self::validate_local_definition(&definition)?;
        let work = 1_u64
            .saturating_add(body.nodes.len() as u64)
            .saturating_add(body.instances.len() as u64)
            .saturating_add(body.edges.len() as u64)
            .saturating_add(
                definition
                    .inputs
                    .iter()
                    .map(|port| port.targets.len() as u64)
                    .sum::<u64>(),
            )
            .saturating_add(u64::from(definition.output.is_some()));
        self.charge(work)?;
        stack.push(id.to_owned());

        let mut local_nodes = BTreeMap::new();
        let mut ordered_nodes: Vec<_> = body.nodes.iter().collect();
        ordered_nodes.sort_by_key(|node| node.id);
        for node in ordered_nodes {
            let flat_id = NodeId(
                u16::try_from(self.next_node)
                    .map_err(|_| "expanded graph exceeds node ID capacity")?,
            );
            self.next_node += 1;
            let flat_node = node.instantiate(flat_id);
            if self
                .builder
                .nodes
                .insert(flat_id, Arc::new(flat_node))
                .is_some()
            {
                return Err("expanded node ID collision".into());
            }
            local_nodes.insert(node.id, flat_id);
            if let Some(name) = &node.name {
                let qualified = qualified_name(path, name);
                if self.metadata.name_index.contains_key(&qualified) {
                    return Err(format!("duplicate qualified node name {qualified}"));
                }
                self.metadata
                    .name_index
                    .insert(qualified, NamedUnit::Node(flat_id));
            }
            self.metadata.nodes.insert(
                flat_id,
                NodeProvenance {
                    definition: definition.id.clone(),
                    instance_path: path.clone(),
                    local_node: node.id,
                },
            );
        }

        let mut children = BTreeMap::new();
        let mut ordered_instances: Vec<_> = body.instances.iter().collect();
        ordered_instances.sort_by(|a, b| a.name.cmp(&b.name));
        for instance in ordered_instances {
            let count = self.expanded_instances.get().saturating_add(1);
            if count > self.limits.max_instances {
                return Err(format!(
                    "module expansion exceeds {} instances",
                    self.limits.max_instances
                ));
            }
            self.expanded_instances.set(count);
            let child_id = self
                .catalog
                .definitions
                .get(&instance.definition)
                .ok_or_else(|| {
                    format!(
                        "instance {} references unknown definition {}",
                        instance.name, instance.definition
                    )
                })?
                .id
                .clone();
            let mut child_path = path.clone();
            child_path.push(instance.name.clone());
            let qualified = child_path.join("/");
            if self.metadata.name_index.contains_key(&qualified) {
                return Err(format!("duplicate qualified module name {qualified}"));
            }
            let instance_metadata = self.metadata.instances.len();
            self.metadata.name_index.insert(
                qualified,
                NamedUnit::Module(ModuleLookup {
                    definition: child_id.clone(),
                    instance_path: child_path.clone(),
                    instance_metadata,
                }),
            );
            self.metadata.instances.push(ModuleInstanceProvenance {
                declaration_id: body.declaration_ids.get(&instance.name).copied(),
                definition: child_id.clone(),
                instance_path: child_path,
                inputs: BTreeMap::new(),
                output: None,
            });
            path.push(instance.name.clone());
            let expanded_child = self.expand_scope(&child_id, stack, path)?;
            if expanded_child.output.is_none() {
                return Err(format!(
                    "instantiated module {child_id} needs an explicit output source"
                ));
            }
            path.pop();
            self.metadata.instances[instance_metadata].inputs = expanded_child.inputs.clone();
            self.metadata.instances[instance_metadata].output = expanded_child.output;
            children.insert(instance.name.clone(), expanded_child);
        }

        let resolve_input = |target: &ModuleInputTarget| -> Result<Vec<InputAddr>, String> {
            match target {
                ModuleInputTarget::NodeNamed { node, port, slot } => {
                    let declaration = body
                        .nodes
                        .iter()
                        .find(|candidate| candidate.name.as_deref() == Some(node.as_str()))
                        .ok_or_else(|| {
                            format!("unknown named node {node:?} in module {}", definition.id)
                        })?;
                    let addr = declaration.input(port, slot)?;
                    Ok(vec![InputAddr {
                        node: local_nodes[&declaration.id],
                        port: addr.port,
                        slot: addr.slot,
                    }])
                }
                ModuleInputTarget::Node(addr) => {
                    let node = body
                        .nodes
                        .iter()
                        .find(|node| node.id == addr.node)
                        .ok_or_else(|| {
                            format!(
                                "unknown local node {:?} in module {}",
                                addr.node, definition.id
                            )
                        })?;
                    if !node.ports.iter().any(|port| {
                        port.id == addr.port && port.slots.iter().any(|slot| slot.id == addr.slot)
                    }) {
                        return Err(format!(
                            "unknown local input {addr:?} in module {}",
                            definition.id
                        ));
                    }
                    Ok(vec![InputAddr {
                        node: local_nodes[&addr.node],
                        port: addr.port,
                        slot: addr.slot,
                    }])
                }
                ModuleInputTarget::InstanceNamed { name, port } => {
                    let instance = body
                        .instances
                        .iter()
                        .find(|instance| instance.name == *name)
                        .ok_or_else(|| {
                            format!("unknown module instance {name} in {}", definition.id)
                        })?;
                    let child_def = self.catalog.definitions.get(&instance.definition).unwrap();
                    let port_id = child_def
                        .inputs
                        .iter()
                        .find(|input| input.name == *port)
                        .ok_or_else(|| format!("unknown named input {port:?} on instance {name}"))?
                        .id;
                    let addresses = children
                        .get(name)
                        .unwrap()
                        .inputs
                        .get(&port_id)
                        .ok_or_else(|| {
                            format!("unresolved named input {port:?} on instance {name}")
                        })?;
                    if addresses.is_empty() {
                        return Err(format!(
                            "named input {port:?} on instance {name} has no targets"
                        ));
                    }
                    Ok(addresses.clone())
                }
                ModuleInputTarget::Instance { name, port } => {
                    let instance = body
                        .instances
                        .iter()
                        .find(|instance| instance.name == *name)
                        .ok_or_else(|| {
                            format!("unknown module instance {name} in {}", definition.id)
                        })?;
                    let child_def = self.catalog.definitions.get(&instance.definition).unwrap();
                    let child_port = child_def
                        .inputs
                        .iter()
                        .find(|input| input.id == *port)
                        .ok_or_else(|| format!("unknown input port {port} on instance {name}"))?;
                    let addresses =
                        children
                            .get(name)
                            .unwrap()
                            .inputs
                            .get(port)
                            .ok_or_else(|| {
                                format!("unresolved input port {port} on instance {name}")
                            })?;
                    if addresses.is_empty() {
                        return Err(format!(
                            "input port {port} on instance {name} has no targets"
                        ));
                    }
                    let _ = child_port;
                    Ok(addresses.clone())
                }
            }
        };
        let resolve_output =
            |source: &ModuleOutputSource| -> Result<(OutputAddr, TypeIdentity), String> {
                match source {
                    ModuleOutputSource::NodeNamed { node, output } => {
                        let declaration = body
                            .nodes
                            .iter()
                            .find(|candidate| candidate.name.as_deref() == Some(node.as_str()))
                            .ok_or_else(|| {
                                format!("unknown named node {node:?} in module {}", definition.id)
                            })?;
                        let addr = declaration.output(output)?;
                        let kind = declaration.output.identity();
                        Ok((
                            OutputAddr {
                                node: local_nodes[&declaration.id],
                                slot: addr.slot,
                            },
                            kind,
                        ))
                    }
                    ModuleOutputSource::Node(addr) => {
                        let node = body
                            .nodes
                            .iter()
                            .find(|node| node.id == addr.node)
                            .ok_or_else(|| {
                                format!(
                                    "unknown local node {:?} in module {}",
                                    addr.node, definition.id
                                )
                            })?;
                        let slot = Some(&node.output)
                            .filter(|slot| slot.id == addr.slot && addr.slot == SlotId(0))
                            .ok_or_else(|| {
                                format!("unknown local output {addr:?} in module {}", definition.id)
                            })?;
                        Ok((
                            OutputAddr {
                                node: local_nodes[&addr.node],
                                slot: addr.slot,
                            },
                            slot.identity(),
                        ))
                    }
                    ModuleOutputSource::InstanceNamed { name, port } => {
                        let instance = body
                            .instances
                            .iter()
                            .find(|instance| instance.name == *name)
                            .ok_or_else(|| {
                                format!("unknown module instance {name} in {}", definition.id)
                            })?;
                        let child_def = self.catalog.definitions.get(&instance.definition).unwrap();
                        let child_port = child_def
                            .output
                            .as_ref()
                            .filter(|output| output.name == *port)
                            .ok_or_else(|| {
                                format!("unknown named output {port:?} on instance {name}")
                            })?;
                        let addr =
                            children.get(name).unwrap().output.as_ref().ok_or_else(|| {
                                format!("unresolved named output {port:?} on instance {name}")
                            })?;
                        Ok((*addr, child_port.identity()))
                    }
                    ModuleOutputSource::Instance { name, port } => {
                        let instance = body
                            .instances
                            .iter()
                            .find(|instance| instance.name == *name)
                            .ok_or_else(|| {
                                format!("unknown module instance {name} in {}", definition.id)
                            })?;
                        let child_def = self.catalog.definitions.get(&instance.definition).unwrap();
                        let child_port = child_def
                            .output
                            .as_ref()
                            .filter(|output| output.id == *port)
                            .ok_or_else(|| {
                                format!("unknown output port {port} on instance {name}")
                            })?;
                        let addr =
                            children.get(name).unwrap().output.as_ref().ok_or_else(|| {
                                format!("unresolved output port {port} on instance {name}")
                            })?;
                        Ok((*addr, child_port.identity()))
                    }
                }
            };

        let mut expanded = ExpandedScope::default();
        let mut input_ids = BTreeSet::new();
        let mut input_names = BTreeSet::new();
        let mut input_slots = BTreeSet::new();
        let mut ordered_inputs: Vec<_> = definition.inputs.iter().collect();
        ordered_inputs.sort_by_key(|input| input.id);
        for input in ordered_inputs {
            validate_name_segment(&input.name).map_err(|error| error.to_string())?;
            validate_name_segment(&input.port).map_err(|error| error.to_string())?;
            validate_name_segment(&input.slot).map_err(|error| error.to_string())?;
            if !input_ids.insert(input.id)
                || !input_names.insert(input.name.as_str())
                || !input_slots.insert((&input.port, &input.slot))
            {
                return Err(format!("duplicate module public port {}", input.name));
            }
            if input.targets.is_empty() {
                return Err(format!("module input {} has no targets", input.name));
            }
            let mut addresses = Vec::new();
            let mut unique_addresses = BTreeSet::new();
            for target in &input.targets {
                let resolved = resolve_input(target)?;
                self.charge(resolved.len() as u64)?;
                for addr in resolved {
                    if !unique_addresses.insert(addr) {
                        return Err(format!("duplicate target on module input {}", input.name));
                    }
                    let kind = self.builder.nodes[&addr.node]
                        .ports
                        .iter()
                        .find(|port| port.id == addr.port)
                        .unwrap()
                        .slots
                        .iter()
                        .find(|slot| slot.id == addr.slot)
                        .unwrap()
                        .identity();
                    if kind != input.identity() {
                        return Err(format!("type mismatch on module input {}", input.name));
                    }
                    addresses.push(addr);
                }
            }
            expanded.inputs.insert(input.id, addresses);
        }
        if let Some(output) = &definition.output {
            validate_name_segment(&output.name).map_err(|error| error.to_string())?;
            if output.id != 0 {
                return Err("module output must use canonical slot 0".into());
            }
            let (addr, kind) = resolve_output(&output.source)?;
            if kind != output.identity() {
                return Err(format!("type mismatch on module output {}", output.name));
            }
            expanded.output = Some(addr);
        }

        // Preserve declaration-owned connection identity across reorder/edit.
        // Flattened runtime EdgeIds are still generated independently.
        for edge in &body.edges {
            let local_edge = edge.declaration_id;
            let (from, source_kind) = resolve_output(&edge.from)?;
            let destinations = resolve_input(&edge.to)?;
            self.charge(destinations.len() as u64)?;
            for to in destinations {
                let dest_kind = self.builder.nodes[&to.node]
                    .ports
                    .iter()
                    .find(|port| port.id == to.port)
                    .unwrap()
                    .slots
                    .iter()
                    .find(|slot| slot.id == to.slot)
                    .unwrap()
                    .identity();
                if source_kind != dest_kind {
                    return Err(format!(
                        "type identity mismatch on connection {local_edge} in module {}",
                        definition.id
                    ));
                }
                let flat_edge_id = EdgeId(
                    u16::try_from(self.next_edge)
                        .map_err(|_| "expanded graph exceeds edge ID capacity")?,
                );
                self.next_edge += 1;
                self.builder.edges.push(Edge {
                    id: flat_edge_id,
                    from,
                    to,
                });
                self.metadata.edges.insert(
                    flat_edge_id,
                    EdgeProvenance {
                        definition: definition.id.clone(),
                        instance_path: path.clone(),
                        local_edge,
                    },
                );
            }
        }
        stack.pop();
        Ok(expanded)
    }

    fn validate_local_definition(definition: &ModuleDef) -> Result<(), String> {
        let mut node_ids = BTreeSet::new();
        let mut names = BTreeSet::new();
        for node in &definition.nodes {
            if !node_ids.insert(node.id) {
                return Err(format!(
                    "duplicate local node ID in module {}",
                    definition.id
                ));
            }
            if let Some(name) = &node.name {
                validate_name_segment(name).map_err(|error| error.to_string())?;
                if !names.insert(name.as_str()) {
                    return Err(format!(
                        "duplicate name {name:?} in module {}",
                        definition.id
                    ));
                }
            }
        }
        let mut edge_ids = BTreeSet::new();
        for edge in &definition.edges {
            if edge.declaration_id == 0 || !edge_ids.insert(edge.declaration_id) {
                return Err(format!(
                    "duplicate or zero connection identity in module {}",
                    definition.id
                ));
            }
        }
        let mut instance_names = BTreeSet::new();
        for instance in &definition.instances {
            validate_name_segment(&instance.name).map_err(|error| error.to_string())?;
            if !instance_names.insert(instance.name.as_str()) {
                return Err(format!(
                    "duplicate instance name {} in module {}",
                    instance.name, definition.id
                ));
            }
            if !names.insert(instance.name.as_str()) {
                return Err(format!(
                    "duplicate name {:?} in module {}",
                    instance.name, definition.id
                ));
            }
        }
        Ok(())
    }
    fn charge(&self, amount: u64) -> Result<(), String> {
        let work = self.expansion_work.get().saturating_add(amount);
        if work > self.limits.max_work {
            return Err(format!(
                "module expansion work exceeds {}",
                self.limits.max_work
            ));
        }
        self.expansion_work.set(work);
        Ok(())
    }
}

/// A partition is a compiler-generated indexing hint, never an execution unit.
/// The prototype uses the closest containing module instance. A flat graph has
/// one root partition; future graph partitioners can replace this assignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct PartitionId(usize);
type PortKey = (NodeId, PortId);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Reachability {
    preserve: bool,
    reset: bool,
}

struct PartitionIndex {
    node_partition: BTreeMap<NodeId, PartitionId>,
    reverse_regions: Vec<BTreeSet<PartitionId>>,
    forward_regions: Vec<BTreeSet<PartitionId>>,
    // Lazy: otherwise a long module chain would eagerly allocate O(R^2)
    // closure entries even when only a handful of regions become active.
    upstream: Vec<OnceLock<BTreeSet<PartitionId>>>,
    downstream: Vec<OnceLock<BTreeSet<PartitionId>>>,
}
impl PartitionIndex {
    fn compile(graph: &CompiledGraph) -> Self {
        let mut paths = BTreeSet::new();
        for id in graph.nodes.keys() {
            paths.insert(
                graph
                    .metadata
                    .nodes
                    .get(id)
                    .map_or_else(Vec::new, |p| p.instance_path.clone()),
            );
        }
        let paths: Vec<_> = paths.into_iter().collect();
        let by_path: BTreeMap<_, _> = paths
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, path)| (path, PartitionId(i)))
            .collect();
        let node_partition = graph
            .nodes
            .keys()
            .map(|&id| {
                let path = graph
                    .metadata
                    .nodes
                    .get(&id)
                    .map_or_else(Vec::new, |p| p.instance_path.clone());
                (id, by_path[&path])
            })
            .collect::<BTreeMap<_, _>>();
        let mut reverse_regions = vec![BTreeSet::new(); paths.len()];
        let mut forward_regions = vec![BTreeSet::new(); paths.len()];
        for edge in graph.by_node.values().flatten() {
            let from = node_partition[&edge.from.node];
            let to = node_partition[&edge.to.node];
            if from != to {
                reverse_regions[to.0].insert(from);
                forward_regions[from.0].insert(to);
            }
        }
        let upstream = (0..paths.len()).map(|_| OnceLock::new()).collect();
        let downstream = (0..paths.len()).map(|_| OnceLock::new()).collect();
        Self {
            node_partition,
            reverse_regions,
            forward_regions,
            upstream,
            downstream,
        }
    }
    fn closure(adjacency: &[BTreeSet<PartitionId>], start: PartitionId) -> BTreeSet<PartitionId> {
        let mut seen = BTreeSet::new();
        let mut todo = vec![start];
        while let Some(region) = todo.pop() {
            if seen.insert(region) {
                todo.extend(adjacency[region.0].iter().copied());
            }
        }
        seen
    }
    fn region(&self, node: NodeId) -> PartitionId {
        self.node_partition[&node]
    }
    fn upstream(&self, target: NodeId) -> &BTreeSet<PartitionId> {
        let i = self.region(target).0;
        self.upstream[i].get_or_init(|| Self::closure(&self.reverse_regions, PartitionId(i)))
    }
    fn downstream(&self, source: NodeId) -> &BTreeSet<PartitionId> {
        let i = self.region(source).0;
        self.downstream[i].get_or_init(|| Self::closure(&self.forward_regions, PartitionId(i)))
    }
    fn possibly_reaches(&self, source: NodeId, target: NodeId) -> bool {
        self.upstream(target).contains(&self.region(source))
    }
}

struct StructuralIndex {
    partitions: PartitionIndex,
    incoming_by_node: BTreeMap<NodeId, Vec<Edge>>,
    incoming_by_port: BTreeMap<PortKey, Vec<Edge>>,
    // Initialized only for ports queried during an execution. Each entry is a
    // complete reverse reachability map, so repeated lookups are O(log V).
    reachability: BTreeMap<PortKey, OnceLock<BTreeMap<NodeId, Reachability>>>,
}
impl StructuralIndex {
    fn compile(graph: &CompiledGraph) -> Self {
        let mut incoming_by_node: BTreeMap<NodeId, Vec<Edge>> = BTreeMap::new();
        let mut incoming_by_port: BTreeMap<PortKey, Vec<Edge>> = BTreeMap::new();
        for edges in graph.by_node.values() {
            for &edge in edges {
                incoming_by_node.entry(edge.to.node).or_default().push(edge);
                incoming_by_port
                    .entry((edge.to.node, edge.to.port))
                    .or_default()
                    .push(edge);
            }
        }
        let reachability = graph
            .nodes
            .values()
            .flat_map(|node| {
                node.ports
                    .iter()
                    .map(|port| ((node.id, port.id), OnceLock::new()))
            })
            .collect();
        Self {
            partitions: PartitionIndex::compile(graph),
            incoming_by_node,
            incoming_by_port,
            reachability,
        }
    }
}

pub struct CompiledGraph {
    composition_origins: BTreeMap<u64, GraphOrigin>,
    nodes: BTreeMap<NodeId, Arc<PhysicalNode>>,
    by_output: BTreeMap<OutputAddr, Vec<Edge>>,
    by_node: BTreeMap<NodeId, Vec<Edge>>,
    triggers: BTreeMap<TriggerId, Vec<Vec<InputAddr>>>,
    source_triggers: BTreeMap<TriggerId, Vec<NodeId>>,
    order: BTreeMap<NodeId, usize>,
    metadata: Arc<CompiledMetadata>,
    structural: OnceLock<StructuralIndex>,
}
type Graph = CompiledGraph;

pub enum FindResult<'g> {
    Node(NodeRef<'g>),
    Module(ModuleRef<'g>),
}
pub struct NodeRef<'g> {
    graph: &'g CompiledGraph,
    id: NodeId,
}
pub struct ModuleRef<'g> {
    graph: &'g CompiledGraph,
    lookup: &'g ModuleLookup,
}
pub struct ModulePortInfo<'g> {
    id: u16,
    name: &'g str,
    port: &'g str,
    slot: &'g str,
    schema: Arc<str>,
    local_type: Option<TypeId>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedInputEndpoint {
    node: u16,
    port: u16,
    slot: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedOutputEndpoint {
    node: u16,
    slot: u16,
}

impl<'g> NodeRef<'g> {
    /// Look up the physical endpoint after module expansion.
    pub fn input(&self, port: &str, slot: &str) -> Option<InputAddr> {
        let node = self.graph.nodes.get(&self.id)?;
        let port = node
            .ports
            .iter()
            .find(|p| p.name.as_deref() == Some(port))?;
        let slot = port
            .slots
            .iter()
            .find(|s| s.name.as_deref() == Some(slot))?;
        Some(InputAddr {
            node: self.id,
            port: port.id,
            slot: slot.id,
        })
    }
    pub fn output(&self, output: &str) -> Option<OutputAddr> {
        let slot = &self.graph.nodes.get(&self.id)?.output;
        if slot.name.as_deref() != Some(output) {
            return None;
        }
        Some(OutputAddr {
            node: self.id,
            slot: slot.id,
        })
    }
    pub fn id(&self) -> u16 {
        self.id.0
    }
    pub fn name(&self) -> Option<&str> {
        self.graph.nodes.get(&self.id)?.name.as_deref()
    }
    pub fn definition_id(&self) -> Option<&str> {
        self.graph
            .metadata
            .nodes
            .get(&self.id)
            .map(|origin| origin.definition.as_str())
    }
    pub fn instance_path(&self) -> &[String] {
        self.graph
            .metadata
            .nodes
            .get(&self.id)
            .map_or(&[], |origin| origin.instance_path.as_slice())
    }
    pub fn input_port_count(&self) -> usize {
        self.graph
            .nodes
            .get(&self.id)
            .map_or(0, |node| node.ports.len())
    }
    pub fn creates_fresh_cohort(&self) -> bool {
        self.graph
            .nodes
            .get(&self.id)
            .is_some_and(|node| node.cohort_mode == CohortMode::FreshPerInvocation)
    }
}
impl<'g> ModuleRef<'g> {
    fn definition(&self) -> &'g ModuleDef {
        self.graph
            .metadata
            .definitions
            .get(&self.lookup.definition)
            .unwrap()
            .as_ref()
    }
    fn instance_metadata(&self) -> &'g ModuleInstanceProvenance {
        &self.graph.metadata.instances[self.lookup.instance_metadata]
    }
    pub fn name(&self) -> &str {
        self.lookup.instance_path.last().unwrap()
    }
    pub fn qualified_name(&self) -> String {
        self.lookup.instance_path.join("/")
    }
    pub fn definition_id(&self) -> &str {
        &self.lookup.definition
    }
    pub fn expanded_node_count(&self) -> usize {
        self.graph
            .metadata
            .nodes
            .values()
            .filter(|origin| origin.instance_path.starts_with(&self.lookup.instance_path))
            .count()
    }
    pub fn input_ports(&self) -> impl Iterator<Item = ModulePortInfo<'g>> + 'g {
        self.definition().inputs.iter().map(|port| ModulePortInfo {
            id: port.id,
            name: port.name.as_str(),
            port: port.port.as_str(),
            slot: port.slot.as_str(),
            schema: port.schema.clone(),
            local_type: port.local_type,
        })
    }
    pub fn output_port(&self) -> Option<ModulePortInfo<'g>> {
        self.definition()
            .output
            .as_ref()
            .map(|port| ModulePortInfo {
                id: port.id,
                name: port.name.as_str(),
                port: port.name.as_str(),
                slot: "value",
                schema: port.schema.clone(),
                local_type: port.local_type,
            })
    }
    pub fn resolved_input_targets(&self, port: u16) -> Option<Vec<ResolvedInputEndpoint>> {
        self.instance_metadata().inputs.get(&port).map(|targets| {
            targets
                .iter()
                .map(|target| ResolvedInputEndpoint {
                    node: target.node.0,
                    port: target.port.0,
                    slot: target.slot.0,
                })
                .collect()
        })
    }
    pub fn resolved_output_source(&self) -> Option<ResolvedOutputEndpoint> {
        self.instance_metadata()
            .output
            .as_ref()
            .map(|source| ResolvedOutputEndpoint {
                node: source.node.0,
                slot: source.slot.0,
            })
    }
    pub fn find(&self, relative_path: &str) -> Option<FindResult<'g>> {
        let segments = parse_name_path(relative_path)?;
        let mut absolute = self.lookup.instance_path.clone();
        absolute.extend(segments.into_iter().map(str::to_owned));
        self.graph.find(&absolute.join("/"))
    }
}
impl ModulePortInfo<'_> {
    pub fn port_name(&self) -> &str {
        self.port
    }
    pub fn slot_name(&self) -> &str {
        self.slot
    }
    pub fn id(&self) -> u16 {
        self.id
    }
    pub fn name(&self) -> &str {
        self.name
    }
    pub fn schema(&self) -> &Arc<str> {
        &self.schema
    }
    pub fn identity(&self) -> TypeIdentity {
        identity(&self.schema, self.local_type)
    }
}
impl ResolvedInputEndpoint {
    pub fn node_id(&self) -> u16 {
        self.node
    }
    pub fn port_id(&self) -> u16 {
        self.port
    }
    pub fn slot_id(&self) -> u16 {
        self.slot
    }
}
impl ResolvedOutputEndpoint {
    pub fn node_id(&self) -> u16 {
        self.node
    }
    pub fn slot_id(&self) -> u16 {
        self.slot
    }
}

impl CompiledGraph {
    pub fn physical_node_count(&self) -> usize {
        self.nodes.len()
    }
    /// Describe an actual connected target; arbitrary node names never enter
    /// the filter's candidate set. Physical edge IDs remain internal identity.
    fn destination(&self, edge: Edge) -> output::Destination {
        let node = &self.nodes[&edge.to.node];
        let path = node
            .name
            .as_deref()
            .map(|name| {
                self.metadata
                    .nodes
                    .get(&edge.to.node)
                    .map(|origin| {
                        origin
                            .instance_path
                            .iter()
                            .map(String::as_str)
                            .chain(std::iter::once(name))
                            .collect::<Vec<_>>()
                            .join("/")
                    })
                    .unwrap_or_else(|| name.to_owned())
            })
            .unwrap_or_else(|| format!("#{}", edge.to.node.0));
        let port = node
            .ports
            .iter()
            .find(|port| port.id == edge.to.port)
            .expect("compiled port exists");
        let slot = port
            .slots
            .iter()
            .find(|slot| slot.id == edge.to.slot)
            .expect("compiled slot exists");
        output::Destination {
            edge: edge.id,
            node_path: path,
            port: port
                .name
                .clone()
                .unwrap_or_else(|| format!("#{}", port.id.0)),
            slot: slot
                .name
                .clone()
                .unwrap_or_else(|| format!("#{}", slot.id.0)),
            input: edge.to,
        }
    }
    /// Resolve a captured typed input against *this producer's module scope*.
    /// Resolve only against the current output's candidates: a valid target
    /// elsewhere in the graph is not sufficient authority for delivery.
    fn targets_for_output(
        &self,
        from: OutputAddr,
        requested: &[output::DestinationTarget],
        candidates: &[Edge],
    ) -> Result<Vec<EdgeId>, String> {
        let prefix = self
            .metadata
            .nodes
            .get(&from.node)
            .map(|node| node.instance_path.as_slice())
            .unwrap_or(&[]);
        let qualify = |name: &str| {
            prefix
                .iter()
                .map(String::as_str)
                .chain(std::iter::once(name))
                .collect::<Vec<_>>()
                .join("/")
        };
        let mut selected = Vec::new();
        let mut seen = BTreeSet::new();
        let own_scope = self.nodes[&from.node].declaring_scope;
        for target in requested {
            let requested_scope = match target {
                output::DestinationTarget::Node { scope, .. }
                | output::DestinationTarget::Module { scope, .. } => *scope,
            };
            if own_scope != Some(requested_scope) {
                return Err("selected destination belongs to a different authoring graph".into());
            }
            let addresses: Vec<InputAddr> = match target {
                output::DestinationTarget::Node {
                    name,
                    port,
                    slot,
                    declaration_id,
                    ..
                } => {
                    let key = qualify(name);
                    match self.find(&key) {
                        Some(FindResult::Node(node)) => {
                            if self.nodes[&NodeId(node.id())].declaration_id != *declaration_id {
                                return Err(
                                    "selected destination is stale or from a divergent declaration"
                                        .into(),
                                );
                            }
                            node.input(port, slot).into_iter().collect()
                        }
                        _ => Vec::new(),
                    }
                }
                output::DestinationTarget::Module {
                    name,
                    port,
                    declaration_id,
                    ..
                } => {
                    let key = qualify(name);
                    match self.find(&key) {
                        Some(FindResult::Module(module)) => {
                            if module.instance_metadata().declaration_id != Some(*declaration_id) {
                                return Err("selected module destination is stale or from a divergent declaration".into());
                            }
                            module
                                .definition()
                                .inputs
                                .iter()
                                .find(|input| input.name == *port)
                                .and_then(|input| module.instance_metadata().inputs.get(&input.id))
                                .cloned()
                                .unwrap_or_default()
                        }
                        _ => Vec::new(),
                    }
                }
            };
            let matched: Vec<EdgeId> = candidates
                .iter()
                .filter(|edge| addresses.contains(&edge.to))
                .map(|edge| edge.id)
                .collect();
            if matched.is_empty() {
                return Err(format!(
                    "selected destination {target:?} is not connected to this output"
                ));
            }
            for edge in matched {
                if !seen.insert(edge) {
                    return Err("duplicate output destination selection".into());
                }
                selected.push(edge);
            }
        }
        Ok(selected)
    }
    /// A deterministic, human-readable *topology* descriptor for review,
    /// reproducibility checks and structural regression tests. Paths rather
    /// than compiler-assigned node/edge IDs form its primary keys.
    ///
    /// This is **not** an executable identity or a security hash: Rust closure
    /// bodies, custom input policies, selected output functions and the full
    /// meaning of local `TypeId`s cannot be serialized by the graph. Kernel
    /// generation identity must additionally bind explicit operation/policy
    /// identities and the verified plugin composition, at integration time.
    /// Policy type names are diagnostic only: they do not encode captured state.
    pub fn topology_manifest(&self) -> String {
        let mut names = BTreeMap::<NodeId, String>::new();
        for (path, unit) in &self.metadata.name_index {
            if let NamedUnit::Node(id) = unit {
                names.insert(*id, path.clone());
            }
        }
        for id in self.nodes.keys() {
            names.entry(*id).or_insert_with(|| format!("#{}", id.0));
        }
        fn path(names: &BTreeMap<NodeId, String>, id: NodeId) -> &str {
            names[&id].as_str()
        }
        let input_label = |addr: InputAddr| {
            let node = &self.nodes[&addr.node];
            let port = node
                .ports
                .iter()
                .find(|p| p.id == addr.port)
                .expect("compiled input port exists");
            let slot = port
                .slots
                .iter()
                .find(|s| s.id == addr.slot)
                .expect("compiled input slot exists");
            format!(
                "{}.{}/{}",
                path(&names, addr.node),
                port.name
                    .clone()
                    .unwrap_or_else(|| format!("#{}", port.id.0)),
                slot.name
                    .clone()
                    .unwrap_or_else(|| format!("#{}", slot.id.0))
            )
        };
        let output_label = |addr: OutputAddr| {
            let node = &self.nodes[&addr.node];
            let slot = &node.output;
            format!(
                "{}.{}",
                path(&names, addr.node),
                slot.name
                    .clone()
                    .unwrap_or_else(|| format!("#{}", slot.id.0))
            )
        };
        let mut manifest = vec!["phenix-topology".to_owned()];
        // Public boundaries are part of the graph's structure. Two graphs
        // exposing different names or targets must not compare identical.
        if let Some(root) = self
            .metadata
            .root_definition
            .as_ref()
            .and_then(|id| self.metadata.definitions.get(id))
        {
            let mut inputs = root.inputs.iter().collect::<Vec<_>>();
            inputs.sort_by(|a, b| a.name.cmp(&b.name));
            for port in inputs {
                let mut targets = self
                    .metadata
                    .inputs
                    .get(&port.id)
                    .into_iter()
                    .flatten()
                    .map(|addr| input_label(*addr))
                    .collect::<Vec<_>>();
                targets.sort();
                manifest.push(format!(
                    "public input {:?} {:?} local={} -> {targets:?}",
                    port.name,
                    port.schema,
                    port.local_type.is_some()
                ));
            }
            if let Some(port) = &root.output {
                let source = self
                    .metadata
                    .output
                    .as_ref()
                    .map(|addr| output_label(*addr));
                manifest.push(format!(
                    "public output {:?} {:?} local={} -> {source:?}",
                    port.name,
                    port.schema,
                    port.local_type.is_some()
                ));
            }
        }
        // A module's public boundary belongs to the compiled graph, even
        // though the module is flattened away at runtime. Otherwise renaming
        // a module port would leave the structural descriptor unchanged.
        let mut instances = self.metadata.instances.iter().collect::<Vec<_>>();
        instances.sort_by(|a, b| a.instance_path.cmp(&b.instance_path));
        for instance in instances {
            let name = instance.instance_path.join("/");
            let definition = &self.metadata.definitions[&instance.definition];
            manifest.push(format!(
                "module {name:?} definition={:?}",
                instance.definition
            ));
            let mut inputs = definition.inputs.iter().collect::<Vec<_>>();
            inputs.sort_by(|a, b| a.name.cmp(&b.name));
            for port in inputs {
                let mut targets = instance
                    .inputs
                    .get(&port.id)
                    .into_iter()
                    .flatten()
                    .map(|addr| input_label(*addr))
                    .collect::<Vec<_>>();
                targets.sort();
                manifest.push(format!(
                    " module input {:?} {:?} local={} -> {targets:?}",
                    port.name,
                    port.schema,
                    port.local_type.is_some()
                ));
            }
            if let Some(port) = &definition.output {
                let source = instance.output.as_ref().map(|addr| output_label(*addr));
                manifest.push(format!(
                    " module output {:?} {:?} local={} -> {source:?}",
                    port.name,
                    port.schema,
                    port.local_type.is_some()
                ));
            }
        }
        let mut ordered_nodes = names
            .iter()
            .map(|(id, name)| (name.as_str(), id))
            .collect::<Vec<_>>();
        ordered_nodes.sort();
        for (name, id) in ordered_nodes {
            let node = &self.nodes[id];
            manifest.push(format!("node {:?} cohort={:?}", name, node.cohort_mode));
            let mut ports = node.ports.iter().collect::<Vec<_>>();
            ports.sort_by(|a, b| {
                (a.name.as_deref().unwrap_or(""), a.id)
                    .cmp(&(b.name.as_deref().unwrap_or(""), b.id))
            });
            for port in ports {
                manifest.push(format!(
                    " port {:?} policy={}",
                    port.name,
                    port.policy.diagnostic_type()
                ));
                let mut slots = port.slots.iter().collect::<Vec<_>>();
                slots.sort_by(|a, b| {
                    (a.name.as_deref().unwrap_or(""), a.id)
                        .cmp(&(b.name.as_deref().unwrap_or(""), b.id))
                });
                for slot in slots {
                    manifest.push(format!(
                        " input {:?} {:?} {:?} local={} buffer={:?}",
                        port.name,
                        slot.name,
                        slot.schema,
                        slot.local_type.is_some(),
                        slot.buffer
                    ));
                }
            }
            {
                let slot = &node.output;
                manifest.push(format!(
                    " output {:?} {:?} local={}",
                    slot.name,
                    slot.schema,
                    slot.local_type.is_some()
                ));
            }
            let mut grants = node
                .grants
                .iter()
                .map(|g| format!("{:?}:{:?}", g.access, g.key))
                .collect::<Vec<_>>();
            grants.sort();
            for grant in grants {
                manifest.push(format!(" resource {grant}"));
            }
        }
        let mut edges = self
            .by_output
            .values()
            .flat_map(|edges| edges.iter())
            .map(|edge| (output_label(edge.from), input_label(edge.to)))
            .collect::<Vec<_>>();
        edges.sort();
        for (source, target) in edges {
            manifest.push(format!("edge {source:?} -> {target:?}"));
        }
        let trigger_names = self
            .metadata
            .trigger_names
            .iter()
            .map(|(name, id)| (*id, name.as_str()))
            .collect::<BTreeMap<_, _>>();
        let mut triggers = self
            .triggers
            .iter()
            .map(|(id, groups)| {
                let name = trigger_names
                    .get(id)
                    .copied()
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("#{}", id.0));
                let targets = groups
                    .iter()
                    .map(|group| {
                        group
                            .iter()
                            .map(|addr| input_label(*addr))
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>();
                let mut sources = self
                    .source_triggers
                    .get(id)
                    .into_iter()
                    .flatten()
                    .map(|id| path(&names, *id))
                    .collect::<Vec<_>>();
                sources.sort();
                (name, targets, sources)
            })
            .collect::<Vec<_>>();
        triggers.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, targets, sources) in triggers {
            let mut line = format!("trigger {name:?} -> {targets:?}");
            if !sources.is_empty() {
                line.push_str(&format!(" sources={sources:?}"));
            }
            manifest.push(line);
        }
        manifest.join("\n")
    }
    /// Resolve a stable trigger name after module expansion. Names identify
    /// the public entry point, not a physical node or a plugin-provided ID.
    pub fn trigger(&self, name: &str) -> Option<TriggerId> {
        self.metadata.trigger_names.get(name).copied()
    }
    pub fn find(&self, path: &str) -> Option<FindResult<'_>> {
        let segments = parse_name_path(path)?;
        let qualified = segments.join("/");
        match self.metadata.name_index.get(&qualified)? {
            NamedUnit::Node(id) => Some(FindResult::Node(NodeRef {
                graph: self,
                id: *id,
            })),
            NamedUnit::Module(lookup) => Some(FindResult::Module(ModuleRef {
                graph: self,
                lookup,
            })),
        }
    }

    /// Nodes whose inputs arrive atomically in the same batch must have
    /// upstream invocations reserved before downstream policies evaluate
    /// their colors. Topological order is a scheduling preference, not a
    /// restriction on normal value delivery. Cyclic leftovers are stable but
    /// have no guaranteed liveness in this finite prototype.
    fn topological_order(&self) -> BTreeMap<NodeId, usize> {
        let mut successors: BTreeMap<NodeId, BTreeSet<NodeId>> =
            self.nodes.keys().map(|&n| (n, BTreeSet::new())).collect();
        let mut indegree: BTreeMap<NodeId, usize> = self.nodes.keys().map(|&n| (n, 0)).collect();
        for edge in self.by_node.values().flat_map(|edges| edges.iter()) {
            if successors
                .get_mut(&edge.from.node)
                .unwrap()
                .insert(edge.to.node)
            {
                *indegree.get_mut(&edge.to.node).unwrap() += 1;
            }
        }
        let mut available: BTreeSet<_> = indegree
            .iter()
            .filter_map(|(&node, &count)| (count == 0).then_some(node))
            .collect();
        let mut ordered = BTreeMap::new();
        while let Some(node) = available.iter().next().copied() {
            available.remove(&node);
            let rank = ordered.len();
            ordered.insert(node, rank);
            for successor in &successors[&node] {
                let n = indegree.get_mut(successor).unwrap();
                *n -= 1;
                if *n == 0 {
                    available.insert(*successor);
                }
            }
        }
        for &node in self.nodes.keys() {
            if !ordered.contains_key(&node) {
                ordered.insert(node, ordered.len());
            }
        }
        ordered
    }

    fn structural(&self) -> &StructuralIndex {
        self.structural
            .get()
            .expect("compiled graph has a structural index")
    }

    /// Reverse reachability includes both preservation and cohort reset paths.
    /// It stops at the exact target port, not merely at the target node. This
    /// is correct even with feedback edges and other ports on the same node.
    fn compute_reverse_reachability(&self, target: PortKey) -> BTreeMap<NodeId, Reachability> {
        let index = self.structural();
        let is_fresh =
            |node: NodeId| self.nodes[&node].cohort_mode == CohortMode::FreshPerInvocation;
        let mut queue = VecDeque::new();
        for edge in index.incoming_by_port.get(&target).into_iter().flatten() {
            queue.push_back((edge.from.node, is_fresh(edge.from.node)));
        }
        let mut result = BTreeMap::<NodeId, Reachability>::new();
        while let Some((node, reset)) = queue.pop_front() {
            let flags = result.entry(node).or_default();
            let marked = if reset {
                &mut flags.reset
            } else {
                &mut flags.preserve
            };
            if *marked {
                continue;
            }
            *marked = true;
            for edge in index.incoming_by_node.get(&node).into_iter().flatten() {
                // A path entering the target port is terminal; going through
                // that port and back out would invent an extra path.
                if (edge.to.node, edge.to.port) != target {
                    queue.push_back((edge.from.node, reset || is_fresh(edge.from.node)));
                }
            }
        }
        result
    }

    fn can_reach_port(&self, from: NodeId, to: PortKey) -> (bool, bool) {
        let index = self.structural();
        if !index.partitions.possibly_reaches(from, to.0) {
            return (false, false);
        }
        let reach = index.reachability[&to].get_or_init(|| self.compute_reverse_reachability(to));
        let flags = reach.get(&from).copied().unwrap_or_default();
        (flags.preserve, flags.reset)
    }

    fn input_slot(&self, addr: InputAddr) -> Result<&InputSlot, String> {
        self.nodes
            .get(&addr.node)
            .and_then(|n| n.ports.iter().find(|p| p.id == addr.port))
            .and_then(|p| p.slots.iter().find(|s| s.id == addr.slot))
            .ok_or_else(|| format!("unknown input {addr:?}"))
    }
    fn output_slot(&self, addr: OutputAddr) -> Result<&OutputSlot, String> {
        self.nodes
            .get(&addr.node)
            .map(|n| &n.output)
            .filter(|s| s.id == addr.slot && addr.slot == SlotId(0))
            .ok_or_else(|| format!("unknown output {addr:?}"))
    }
    #[cfg(test)]
    fn node_provenance(&self, node: NodeId) -> Option<&NodeProvenance> {
        self.metadata.nodes.get(&node)
    }
    #[cfg(test)]
    fn edge_provenance(&self, edge: EdgeId) -> Option<&EdgeProvenance> {
        self.metadata.edges.get(&edge)
    }
    #[cfg(test)]
    fn module_output(&self) -> Option<OutputAddr> {
        self.metadata.output
    }
}

mod scheduler;
pub use scheduler::{
    DEFAULT_MAX_ACTIVATIONS, DeliveryEvent, EdgeAdapter, Engine, ExecutionSession, ExecutionTicket,
    Executor, IdentityEdgeAdapter, LocalExecutor, NodeState, PublicationEvent, PublicationRef,
    ReadyInvocation, ReportRetention, TriggerSelector, WaveCancellation, WaveError, WaveReport,
    WaveTrace,
};

#[cfg(test)]
mod schema_ownership_tests;
