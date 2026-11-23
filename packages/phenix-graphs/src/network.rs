//! Canonical symbolic authoring and deterministic, bounded composite flattening.
//!
//! Typed edges unify their Rust type at the call site:
//! ```compile_fail
//! use phenix_graph::network::{NetworkBuilder, InputRef, OutputRef};
//! use std::num::NonZeroUsize;
//! fn mismatch(builder: &mut NetworkBuilder, from: OutputRef<i64>, to: InputRef<String>) {
//!     builder.connect(from, to, NonZeroUsize::new(1).unwrap()).unwrap();
//! }
//! ```
//! Direction is checked by the signature, not an integer slot convention:
//! ```compile_fail
//! use phenix_graph::network::{NetworkBuilder, InputRef, OutputRef};
//! use std::num::NonZeroUsize;
//! fn backwards(builder: &mut NetworkBuilder, from: InputRef<i64>, to: OutputRef<i64>) {
//!     builder.connect(from, to, NonZeroUsize::new(1).unwrap()).unwrap();
//! }
//! ```
//! Symbolic paths do not manufacture authoring authority:
//! ```compile_fail
//! use phenix_graph::network::{InputEndpoint, PortPath};
//! fn forge(path: PortPath) -> InputEndpoint { InputEndpoint(path) }
//! ```
use crate::component::{ComponentDef, DefinitionKey};
use crate::contract::Contract;
use crate::interface::{Interface, InterfaceError};
use crate::process::ProcessFactory;
use crate::{GraphOrigin, TypeIdentity};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::num::NonZeroUsize;
use std::sync::Arc;

mod authoring;
mod compiler;
mod resolution;
mod validation;

pub use crate::limits::{CompileLimit, LimitKind};
pub use authoring::{
    ComponentRef, InputEndpoint, InputRef, NetworkBuilder, OutputEndpoint, OutputRef,
};
pub(crate) use resolution::HandleLeases;
pub use resolution::{
    BoundComponent, BoundDeclaration, BoundInput, BoundOutput, CaptureHandle, IngressHandle,
    SourceHandle,
};
pub(crate) use validation::{collect_component_obligations, normalize_requirements};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PortPath {
    pub process: String,
    pub port: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetworkError {
    Readiness(crate::runtime::ReadinessError),
    CorrelationBounds(crate::correlation::InvalidCorrelationBounds),
    Interface(Box<InterfaceError>),
    InvalidName(String),
    DuplicateInstance(String),
    UnknownProcess(String),
    UnknownInput(PortPath),
    UnknownOutput(PortPath),
    WrongType {
        path: PortPath,
        expected: TypeIdentity,
        actual: TypeIdentity,
    },
    ForeignScope,
    StaleHandle,
    RevisionExhausted,
    ReplacementDuringAdditions,
    BuildDuringAdditions,
    DuplicateConnection {
        from: PortPath,
        to: PortPath,
    },
    InvalidProjection(String),
    IncompletePublicBoundary,
    InvalidSource(String),
    AmbiguousDefinition,
    UnknownDefinition,
    RecursiveComposite(String),
    LimitExceeded {
        limit: CompileLimit,
        path: String,
    },
}
impl From<InterfaceError> for NetworkError {
    fn from(error: InterfaceError) -> Self {
        Self::Interface(Box::new(error))
    }
}
impl fmt::Display for NetworkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Readiness(error) => write!(f, "network readiness: {error}"),
            Self::CorrelationBounds(error) => write!(f, "network: {error}"),
            Self::Interface(error) => write!(f, "network: {error}"),
            _ => write!(f, "network: {self:?}"),
        }
    }
}
impl std::error::Error for NetworkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Interface(error) => Some(error.as_ref()),
            Self::Readiness(error) => Some(error),
            Self::CorrelationBounds(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionDef {
    pub from: PortPath,
    pub to: PortPath,
    pub capacity: NonZeroUsize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputProjection {
    pub targets: Vec<PortPath>,
    /// Each root ingress fanout destination receives this connection capacity.
    pub capacity: NonZeroUsize,
}

/// Frozen topology. Mutable builder capability tokens are not retained here.
#[derive(Clone, Debug)]
pub struct NetworkDef {
    pub(crate) interface: Arc<Interface>,
    pub(crate) instances: BTreeMap<String, Arc<ComponentDef>>,
    pub(crate) connections: Vec<ConnectionDef>,
    pub(crate) inputs: BTreeMap<String, InputProjection>,
    pub(crate) outputs: BTreeMap<String, PortPath>,
    pub(crate) sources: Vec<String>,
    // Exact child declarations the composite author intentionally allows
    // host-granted plugins to replace; never inherited by descendants.
    pub(crate) replacement_points: BTreeSet<String>,
    // The composite author opts in to host-granted additions and internal edges.
    // Every composite crossed on a deep edit must opt in independently.
    pub(crate) extensions_allowed: bool,
    pub(crate) claims: Vec<Contract>,
    pub(crate) origin: Option<GraphOrigin>,
    // Frozen obligations survive replacement, including removed private paths.
    pub(crate) requirements: Vec<(String, Contract)>,
}
impl NetworkDef {
    pub fn claims(&self) -> &[Contract] {
        &self.claims
    }
    pub fn origin(&self) -> Option<&GraphOrigin> {
        self.origin.as_ref()
    }
    pub fn requirements(&self) -> &[(String, Contract)] {
        &self.requirements
    }
    pub fn interface(&self) -> &Arc<Interface> {
        &self.interface
    }
    pub fn instances(&self) -> &BTreeMap<String, Arc<ComponentDef>> {
        &self.instances
    }
    pub fn connections(&self) -> &[ConnectionDef] {
        &self.connections
    }
    pub fn input_projections(&self) -> &BTreeMap<String, InputProjection> {
        &self.inputs
    }
    pub fn output_projections(&self) -> &BTreeMap<String, PortPath> {
        &self.outputs
    }
    pub fn source_starts(&self) -> &[String] {
        &self.sources
    }
    /// Explicit, immediate child declarations available as replacement seams.
    pub fn replacement_points(&self) -> &BTreeSet<String> {
        &self.replacement_points
    }
    pub fn extensions_allowed(&self) -> bool {
        self.extensions_allowed
    }
}

/// Compiler-owned physical process. There is no composite runtime variant.
#[derive(Clone)]
pub struct FlatProcess {
    pub(crate) path: String,
    pub(crate) interface: Arc<Interface>,
    pub(crate) factory: Arc<dyn ProcessFactory>,
    pub(crate) origin: Option<GraphOrigin>,
}
impl FlatProcess {
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn interface(&self) -> &Arc<Interface> {
        &self.interface
    }
    pub fn factory(&self) -> &Arc<dyn ProcessFactory> {
        &self.factory
    }
    pub fn origin(&self) -> Option<&GraphOrigin> {
        self.origin.as_ref()
    }
}
impl fmt::Debug for FlatProcess {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FlatProcess")
            .field("path", &self.path)
            .field("interface", &self.interface)
            .field("origin", &self.origin)
            .finish_non_exhaustive()
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlatConnection {
    pub(crate) from: (usize, String),
    pub(crate) to: (usize, String),
    pub(crate) capacity: NonZeroUsize,
}
impl FlatConnection {
    pub fn from(&self) -> (usize, &str) {
        (self.from.0, &self.from.1)
    }
    pub fn to(&self) -> (usize, &str) {
        (self.to.0, &self.to.1)
    }
    pub fn capacity(&self) -> NonZeroUsize {
        self.capacity
    }
}
/// Read-only structural reachability, not proof of runtime execution,
/// fairness or deadlock. Declared starts can be disabled by session options;
/// public ingress can remain unused, and hosts can provide other wake paths.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StructuralReachability {
    pub from_public_ingress: BTreeSet<String>,
    pub from_declared_starts: BTreeSet<String>,
}

#[derive(Clone, Debug)]
pub struct CompiledNetwork {
    pub(crate) interface: Arc<Interface>,
    pub(crate) processes: Vec<FlatProcess>,
    pub(crate) connections: Vec<FlatConnection>,
    pub(crate) inputs: BTreeMap<String, Vec<(usize, String)>>,
    pub(crate) ingress_capacities: BTreeMap<String, NonZeroUsize>,
    pub(crate) outputs: BTreeMap<String, (usize, String)>,
    pub(crate) sources: Vec<usize>,
    inspections: BTreeMap<String, ComponentSnapshot>,
    boundaries: BTreeMap<String, ResolvedBoundary>,
    contracts: Vec<Contract>,
    generation: Arc<()>,
    leases: DeclarationLeases,
}

#[derive(Clone, Default)]
struct DeclarationLeases(Vec<Arc<dyn Send + Sync>>);
impl fmt::Debug for DeclarationLeases {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeclarationLeases")
            .field("count", &self.0.len())
            .finish_non_exhaustive()
    }
}
/// Detached inspection data. Mutating a copy cannot establish membership.
/// ```compile_fail
/// use phenix_graph::contract::Contract;
/// use phenix_graph::network::ComponentSnapshot;
/// fn forge(contract: &Contract, snapshot: &ComponentSnapshot) {
///     contract.check_implementation(snapshot).unwrap();
/// }
/// ```
#[derive(Clone, Debug)]
pub struct ComponentSnapshot {
    pub interface: Arc<Interface>,
    pub claims: Vec<Contract>,
    /// Nearest explicit contribution origin, including inherited origins.
    pub origin: Option<GraphOrigin>,
    pub declared_origin: Option<GraphOrigin>,
    pub definition_key: Option<DefinitionKey>,
    pub physical_index: Option<usize>,
    pub path: String,
}
#[derive(Clone, Debug)]
struct ResolvedBoundary {
    inputs: BTreeMap<String, Vec<(usize, String)>>,
    outputs: BTreeMap<String, (usize, String)>,
}
/// Immutable physical declaration lookup; never creates or exposes a process instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhysicalDeclaration {
    pub path: String,
    pub index: usize,
}
impl CompiledNetwork {
    /// Reuse a fully validated immutable topology while preserving the rule
    /// that every successful publication owns a fresh binding authority.
    /// Old bound handles may only resolve against the prior generation.
    pub(crate) fn with_fresh_generation(&self) -> Self {
        let mut compiled = self.clone();
        compiled.generation = Arc::new(());
        compiled
    }

    /// Clone this generation, retaining host code/resources in every executable
    /// factory, spawned process, and session value. Existing clones are unchanged.
    pub fn with_lease(&self, lease: Arc<dyn Send + Sync>) -> Self {
        let mut compiled = self.clone();
        for process in &mut compiled.processes {
            process.factory = Arc::new(crate::process::LeasedFactory {
                inner: process.factory.clone(),
                lease: lease.clone(),
            });
        }
        compiled.leases.0.push(lease);
        compiled
    }
    pub(crate) fn execution_lease(&self) -> Option<Arc<dyn Send + Sync>> {
        (!self.leases.0.is_empty())
            .then(|| Arc::new(HandleLeases(self.leases.0.clone())) as Arc<dyn Send + Sync>)
    }
    pub fn interface(&self) -> &Arc<Interface> {
        &self.interface
    }
    pub fn processes(&self) -> &[FlatProcess] {
        &self.processes
    }
    pub fn connections(&self) -> &[FlatConnection] {
        &self.connections
    }
    pub fn inputs(&self) -> &BTreeMap<String, Vec<(usize, String)>> {
        &self.inputs
    }
    pub fn ingress_capacities(&self) -> &BTreeMap<String, NonZeroUsize> {
        &self.ingress_capacities
    }
    pub fn outputs(&self) -> &BTreeMap<String, (usize, String)> {
        &self.outputs
    }
    pub fn source_starts(&self) -> &[usize] {
        &self.sources
    }

    /// Feedback components in the *compiled* process topology, including
    /// self-loops. These are static wiring warnings, NOT deadlock findings:
    /// cycles can make progress, and acyclic graphs can still stall on external
    /// ingress, captures, custom readiness or deferred work.
    ///
    /// Each group contains physical process paths in compiled-index order.
    /// Immutable inspection does not start processes, inspect runtime state,
    /// or grant any binding authority.
    pub fn structural_feedback_cycles(&self) -> Vec<Vec<&str>> {
        let size = self.processes.len();
        let mut edges = vec![Vec::new(); size];
        let mut reverse = vec![Vec::new(); size];
        for connection in &self.connections {
            edges[connection.from.0].push(connection.to.0);
            reverse[connection.to.0].push(connection.from.0);
        }
        crate::graph_analysis::cyclic_components(&edges, &reverse)
            .into_iter()
            .map(|members| {
                members
                    .into_iter()
                    .map(|index| self.processes[index].path.as_str())
                    .collect()
            })
            .collect()
    }

    /// Which physical processes have a *topological path* from a declared
    /// source start or a public root input. Does not inspect pending packets,
    /// callbacks, readiness, blocked queues or actual SessionOptions.
    ///
    /// A feedback SCC absent from both sets has no path from these two
    /// declaration roots; that is an actionable wiring observation, not a
    /// universal assertion that the SCC cannot be woken or make progress.
    /// Uses one iterative graph traversal per root category.
    pub fn structural_reachability(&self) -> StructuralReachability {
        let mut downstream = vec![Vec::new(); self.processes.len()];
        for connection in &self.connections {
            downstream[connection.from.0].push(connection.to.0);
        }
        let reachable = |roots: Vec<usize>| {
            let mut visited = vec![false; self.processes.len()];
            let mut pending = roots;
            while let Some(index) = pending.pop() {
                if visited[index] {
                    continue;
                }
                visited[index] = true;
                pending.extend(downstream[index].iter().copied());
            }
            visited
                .into_iter()
                .enumerate()
                .filter(|&(_, seen)| seen)
                .map(|(index, _)| self.processes[index].path.clone())
                .collect()
        };
        StructuralReachability {
            from_public_ingress: reachable(
                self.inputs
                    .values()
                    .flat_map(|targets| targets.iter().map(|(index, _)| *index))
                    .collect(),
            ),
            from_declared_starts: reachable(self.sources.clone()),
        }
    }
}
