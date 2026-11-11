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
use crate::component::{Body, ComponentDef, DefinitionKey};
use crate::contract::{Contract, catalog};
use crate::interface::InterfacePhase;
use crate::interface::{Interface, InterfaceError, PortDef};
use crate::process::ProcessFactory;
use crate::{CompileLimits, ContractValue, GraphOrigin, TypeIdentity};
use std::any::TypeId;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::marker::PhantomData;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PortPath {
    pub process: String,
    pub port: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetworkError {
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
    InterfaceMismatch(String),
    DuplicateConnection {
        from: PortPath,
        to: PortPath,
    },
    InvalidProjection(String),
    InvalidSource(String),
    AmbiguousDefinition,
    UnknownDefinition,
    RecursiveComposite(String),
    LimitExceeded {
        limit: &'static str,
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
        write!(f, "network: {self:?}")
    }
}
impl std::error::Error for NetworkError {}

#[derive(Clone, Debug)]
struct Capability {
    scope: Arc<()>,
    transactions: Vec<Arc<AtomicBool>>,
    revision: Arc<AtomicU64>,
    expected: u64,
}
impl Capability {
    fn check(&self) -> Result<(), NetworkError> {
        if self
            .transactions
            .iter()
            .all(|token| token.load(Ordering::Acquire))
            && self.revision.load(Ordering::Acquire) == self.expected
        {
            Ok(())
        } else {
            Err(NetworkError::StaleHandle)
        }
    }
}
#[derive(Clone, Debug)]
struct Endpoint {
    path: PortPath,
    capability: Capability,
    value: TypeIdentity,
}

/// Direction-specific erased authoring authority, not a naked symbolic path.
#[derive(Clone, Debug)]
pub struct InputEndpoint(Endpoint);
#[derive(Clone, Debug)]
pub struct OutputEndpoint(Endpoint);
impl InputEndpoint {
    pub fn path(&self) -> &PortPath {
        &self.0.path
    }
    pub fn value_type(&self) -> &TypeIdentity {
        &self.0.value
    }
}
impl OutputEndpoint {
    pub fn path(&self) -> &PortPath {
        &self.0.path
    }
    pub fn value_type(&self) -> &TypeIdentity {
        &self.0.value
    }
}

pub struct InputRef<T> {
    endpoint: InputEndpoint,
    marker: PhantomData<fn() -> T>,
}
pub struct OutputRef<T> {
    endpoint: OutputEndpoint,
    marker: PhantomData<fn() -> T>,
}
impl<T> Clone for InputRef<T> {
    fn clone(&self) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
            marker: PhantomData,
        }
    }
}
impl<T> Clone for OutputRef<T> {
    fn clone(&self) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
            marker: PhantomData,
        }
    }
}
impl<T> fmt::Debug for InputRef<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.endpoint.fmt(f)
    }
}
impl<T> fmt::Debug for OutputRef<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.endpoint.fmt(f)
    }
}
impl<T> InputRef<T> {
    pub fn endpoint(&self) -> &InputEndpoint {
        &self.endpoint
    }
    pub fn path(&self) -> &PortPath {
        self.endpoint.path()
    }
}
impl<T> OutputRef<T> {
    pub fn endpoint(&self) -> &OutputEndpoint {
        &self.endpoint
    }
    pub fn path(&self) -> &PortPath {
        self.endpoint.path()
    }
}

/// The same handle for primitive and composite bodies. Its cached interface is
/// usable only while its instance revision is current; it never authorizes a
/// different builder, even when the symbolic process names happen to match.
#[derive(Clone, Debug)]
pub struct ComponentRef {
    process: String,
    interface: Arc<Interface>,
    capability: Capability,
}
impl ComponentRef {
    pub fn path(&self) -> &str {
        &self.process
    }
    pub fn input_endpoint(&self, name: &str) -> Result<InputEndpoint, NetworkError> {
        self.capability.check()?;
        let path = PortPath {
            process: self.process.clone(),
            port: name.into(),
        };
        let port = self
            .interface
            .input(name)
            .ok_or_else(|| NetworkError::UnknownInput(path.clone()))?;
        Ok(InputEndpoint(Endpoint {
            path,
            capability: self.capability.clone(),
            value: port.value.clone(),
        }))
    }
    pub fn output_endpoint(&self, name: &str) -> Result<OutputEndpoint, NetworkError> {
        self.capability.check()?;
        let path = PortPath {
            process: self.process.clone(),
            port: name.into(),
        };
        let port = self
            .interface
            .output(name)
            .ok_or_else(|| NetworkError::UnknownOutput(path.clone()))?;
        Ok(OutputEndpoint(Endpoint {
            path,
            capability: self.capability.clone(),
            value: port.value.clone(),
        }))
    }
    pub fn input<T: 'static>(&self, name: &str) -> Result<InputRef<T>, NetworkError> {
        self.typed_input(name, TypeIdentity::Local(TypeId::of::<T>()))
    }
    pub fn output<T: 'static>(&self, name: &str) -> Result<OutputRef<T>, NetworkError> {
        self.typed_output(name, TypeIdentity::Local(TypeId::of::<T>()))
    }
    pub fn wire_input<T: ContractValue>(&self, name: &str) -> Result<InputRef<T>, NetworkError> {
        self.typed_input(name, PortDef::wire::<T>(name)?.value)
    }
    pub fn wire_output<T: ContractValue>(&self, name: &str) -> Result<OutputRef<T>, NetworkError> {
        self.typed_output(name, PortDef::wire::<T>(name)?.value)
    }
    fn typed_input<T>(
        &self,
        name: &str,
        expected: TypeIdentity,
    ) -> Result<InputRef<T>, NetworkError> {
        let endpoint = self.input_endpoint(name)?;
        check_type(&endpoint.0.path, &expected, &endpoint.0.value)?;
        Ok(InputRef {
            endpoint,
            marker: PhantomData,
        })
    }
    fn typed_output<T>(
        &self,
        name: &str,
        expected: TypeIdentity,
    ) -> Result<OutputRef<T>, NetworkError> {
        let endpoint = self.output_endpoint(name)?;
        check_type(&endpoint.0.path, &expected, &endpoint.0.value)?;
        Ok(OutputRef {
            endpoint,
            marker: PhantomData,
        })
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
    pub fn compile(&self) -> Result<CompiledNetwork, NetworkError> {
        self.compile_with_limits(CompileLimits::default())
    }
    pub fn compile_with_limits(
        &self,
        limits: CompileLimits,
    ) -> Result<CompiledNetwork, NetworkError> {
        Compiler::new(limits).compile(self)
    }
}

pub struct NetworkBuilder {
    additions_only: bool,
    transactions: Vec<Arc<AtomicBool>>,
    scope: Arc<()>,
    revisions: BTreeMap<String, Arc<AtomicU64>>,
    instances: BTreeMap<String, Arc<ComponentDef>>,
    connections: Vec<ConnectionDef>,
    inputs: BTreeMap<String, (PortDef, InputProjection)>,
    outputs: BTreeMap<String, (PortDef, PortPath)>,
    sources: BTreeSet<String>,
    replacement_points: BTreeSet<String>,
    claims: Vec<Contract>,
    origin: Option<GraphOrigin>,
    requirements: Vec<(String, Contract)>,
}
impl Default for NetworkBuilder {
    fn default() -> Self {
        Self::new()
    }
}
impl NetworkBuilder {
    pub fn new() -> Self {
        Self {
            additions_only: false,
            transactions: Vec::new(),
            scope: Arc::new(()),
            revisions: BTreeMap::new(),
            instances: BTreeMap::new(),
            connections: Vec::new(),
            inputs: BTreeMap::new(),
            outputs: BTreeMap::new(),
            sources: BTreeSet::new(),
            replacement_points: BTreeSet::new(),
            claims: Vec::new(),
            origin: None,
            requirements: Vec::new(),
        }
    }
    /// A fresh authoring branch. No capability from the old builder is accepted.
    pub fn from_def(network: &NetworkDef) -> Self {
        Self {
            additions_only: false,
            transactions: Vec::new(),
            scope: Arc::new(()),
            revisions: network
                .instances
                .keys()
                .map(|name| (name.clone(), Arc::new(AtomicU64::new(0))))
                .collect(),
            instances: network.instances.clone(),
            connections: network.connections.clone(),
            inputs: network
                .inputs
                .iter()
                .map(|(name, projection)| {
                    (
                        name.clone(),
                        (network.interface.inputs()[name].clone(), projection.clone()),
                    )
                })
                .collect(),
            outputs: network
                .outputs
                .iter()
                .map(|(name, path)| {
                    (
                        name.clone(),
                        (network.interface.outputs()[name].clone(), path.clone()),
                    )
                })
                .collect(),
            sources: network.sources.iter().cloned().collect(),
            replacement_points: network.replacement_points.clone(),
            claims: network.claims.clone(),
            origin: network.origin.clone(),
            requirements: network.requirements.clone(),
        }
    }
    /// Atomically author additions, committing only when `edit` returns `Ok`.
    ///
    /// The callback may add components, connections, public input fanout targets,
    /// outputs, source starts, claims, and requirements. `replace` is rejected
    /// throughout the callback, even for newly added components. Transactions
    /// may nest; an outer rollback also revokes handles from committed inner edits.
    /// Handles returned on success remain valid, and preexisting capabilities
    /// retain their scope and revision lineage on either outcome.
    ///
    /// On `Err` or an unwinding panic, all builder metadata (including origin)
    /// is restored and newly issued component/endpoint capabilities are revoked,
    /// even if retained outside the callback. A panic is then resumed unchanged.
    /// External side effects are not rolled back; aborting panics cannot be caught.
    /// Leaving a different builder in the callback slot restores the original
    /// and panics, unless the callback is already unwinding.
    /// Building a frozen definition during the callback is rejected.
    /// Definitions/factories are shared immutably; snapshotting spawns no processes.
    pub fn additions<R, E>(
        &mut self,
        edit: impl FnOnce(&mut NetworkBuilder) -> Result<R, E>,
    ) -> Result<R, E> {
        self.check_active().expect("revoked additions builder");
        let snapshot = Self {
            additions_only: self.additions_only,
            transactions: self.transactions.clone(),
            scope: self.scope.clone(),
            revisions: self.revisions.clone(),
            instances: self.instances.clone(),
            connections: self.connections.clone(),
            inputs: self.inputs.clone(),
            outputs: self.outputs.clone(),
            sources: self.sources.clone(),
            replacement_points: self.replacement_points.clone(),
            claims: self.claims.clone(),
            origin: self.origin.clone(),
            requirements: self.requirements.clone(),
        };
        // Tokens follow moves and nested capabilities; revocation is never undone.
        let token = Arc::new(AtomicBool::new(true));
        self.transactions.push(token.clone());
        self.additions_only = true;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| edit(self)));
        let same_builder = Arc::ptr_eq(&self.scope, &snapshot.scope)
            && self
                .transactions
                .last()
                .is_some_and(|current| Arc::ptr_eq(current, &token));
        if same_builder && matches!(&result, Ok(Ok(_))) {
            self.additions_only = snapshot.additions_only;
            self.transactions = snapshot.transactions;
        } else {
            token.store(false, Ordering::Release);
            *self = snapshot;
        }
        match result {
            Ok(_) if !same_builder => panic!("builder replaced during additions"),
            Ok(result) => result,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
    pub fn add(
        &mut self,
        name: impl Into<String>,
        definition: Arc<ComponentDef>,
    ) -> Result<ComponentRef, NetworkError> {
        self.check_active()?;
        let name = name.into();
        validate_process_name(&name)?;
        if self.instances.contains_key(&name) {
            return Err(NetworkError::DuplicateInstance(name));
        }
        validate_claims(&definition.interface, &definition.claims, &name)?;
        self.instances.insert(name.clone(), definition);
        self.revisions
            .insert(name.clone(), Arc::new(AtomicU64::new(0)));
        self.component(&name)
    }
    pub fn node(
        &mut self,
        name: impl Into<String>,
        interface: Arc<Interface>,
        factory: Arc<dyn ProcessFactory>,
    ) -> Result<ComponentRef, NetworkError> {
        self.add(name, Arc::new(ComponentDef::primitive(interface, factory)))
    }
    pub fn module(
        &mut self,
        name: impl Into<String>,
        network: NetworkDef,
    ) -> Result<ComponentRef, NetworkError> {
        self.add(name, Arc::new(ComponentDef::composite(network)))
    }
    pub fn component(&self, name: &str) -> Result<ComponentRef, NetworkError> {
        self.check_active()?;
        let definition = self
            .instances
            .get(name)
            .ok_or_else(|| NetworkError::UnknownProcess(name.into()))?;
        let revision = self.revisions[name].clone();
        Ok(ComponentRef {
            process: name.into(),
            interface: definition.interface.clone(),
            capability: Capability {
                scope: self.scope.clone(),
                transactions: self.transactions.clone(),
                expected: revision.load(Ordering::Acquire),
                revision,
            },
        })
    }
    /// Shape-identical replacement changes only the definition. Adjacent edges,
    /// exports, and explicit source references remain symbolic and unchanged.
    pub fn replace(
        &mut self,
        current: &ComponentRef,
        definition: Arc<ComponentDef>,
    ) -> Result<ComponentRef, NetworkError> {
        self.check_active()?;
        if self.additions_only {
            return Err(NetworkError::ReplacementDuringAdditions);
        }
        self.check_component(current)?;
        let old = &self.instances[&current.process];
        if old.interface != definition.interface {
            return Err(NetworkError::InterfaceMismatch(current.process.clone()));
        }
        validate_claims(&definition.interface, &definition.claims, &current.process)?;
        let mut obligations = Vec::new();
        collect_component_obligations(old, &current.process, &mut obligations);
        for (path, contract) in &obligations {
            let relative = path
                .strip_prefix(&current.process)
                .unwrap()
                .trim_start_matches('/');
            let (interface, claims) = declaration(&definition, relative)?;
            contract.check(interface, claims)?;
        }
        if self.sources.contains(&current.process) {
            validate_source(&current.process, &definition)?;
        }
        let revision = current
            .capability
            .expected
            .checked_add(1)
            .ok_or(NetworkError::RevisionExhausted)?;
        self.instances.insert(current.process.clone(), definition);
        self.requirements.extend(obligations);
        current
            .capability
            .revision
            .store(revision, Ordering::Release);
        self.component(&current.process)
    }
    pub fn connect<T>(
        &mut self,
        from: OutputRef<T>,
        to: InputRef<T>,
        capacity: NonZeroUsize,
    ) -> Result<(), NetworkError> {
        self.connect_dynamic(&from.endpoint, &to.endpoint, capacity)
    }
    pub fn connect_dynamic(
        &mut self,
        from: &OutputEndpoint,
        to: &InputEndpoint,
        capacity: NonZeroUsize,
    ) -> Result<(), NetworkError> {
        self.check_endpoint(&from.0, false)?;
        self.check_endpoint(&to.0, true)?;
        check_type(&to.0.path, &from.0.value, &to.0.value)?;
        if self
            .connections
            .iter()
            .any(|edge| edge.from == from.0.path && edge.to == to.0.path)
        {
            return Err(NetworkError::DuplicateConnection {
                from: from.0.path.clone(),
                to: to.0.path.clone(),
            });
        }
        self.connections.push(ConnectionDef {
            from: from.0.path.clone(),
            to: to.0.path.clone(),
            capacity,
        });
        Ok(())
    }
    /// Repeating a public input name adds an explicit fanout projection. Its
    /// identity and ingress capacity must agree; duplicate targets are rejected.
    pub fn expose_input<T>(
        &mut self,
        name: impl Into<String>,
        to: InputRef<T>,
        capacity: NonZeroUsize,
    ) -> Result<(), NetworkError> {
        self.expose_input_dynamic(name, &to.endpoint, capacity)
    }
    pub fn expose_input_dynamic(
        &mut self,
        name: impl Into<String>,
        to: &InputEndpoint,
        capacity: NonZeroUsize,
    ) -> Result<(), NetworkError> {
        self.check_endpoint(&to.0, true)?;
        let name = name.into();
        let port = PortDef {
            name: name.clone(),
            value: to.0.value.clone(),
        };
        Interface::new([port.clone()], [])?;
        if let Some((declared, projection)) = self.inputs.get_mut(&name) {
            if declared != &port
                || projection.capacity != capacity
                || projection.targets.contains(&to.0.path)
            {
                return Err(NetworkError::InvalidProjection(name));
            }
            projection.targets.push(to.0.path.clone());
        } else {
            self.inputs.insert(
                name,
                (
                    port,
                    InputProjection {
                        targets: vec![to.0.path.clone()],
                        capacity,
                    },
                ),
            );
        }
        Ok(())
    }
    pub fn expose_output<T>(
        &mut self,
        name: impl Into<String>,
        from: OutputRef<T>,
    ) -> Result<(), NetworkError> {
        self.expose_output_dynamic(name, &from.endpoint)
    }
    pub fn expose_output_dynamic(
        &mut self,
        name: impl Into<String>,
        from: &OutputEndpoint,
    ) -> Result<(), NetworkError> {
        self.check_endpoint(&from.0, false)?;
        let name = name.into();
        let port = PortDef {
            name: name.clone(),
            value: from.0.value.clone(),
        };
        Interface::new([], [port.clone()])?;
        if self.outputs.contains_key(&name) {
            return Err(NetworkError::InvalidProjection(name));
        }
        self.outputs.insert(name, (port, from.0.path.clone()));
        Ok(())
    }
    /// Mark an immediate child component as a deliberate, narrowly writable
    /// replacement seam for later host-authorized composition. This grants
    /// nothing by itself; exact path authorization is still required.
    pub fn allow_replace(&mut self, child: &ComponentRef) -> Result<(), NetworkError> {
        self.check_component(child)?;
        self.replacement_points.insert(child.process.clone());
        Ok(())
    }
    /// Primitive sources must have no inputs. Starting a composite starts only
    /// its explicitly declared source set, never all of its disconnected roots.
    pub fn start(&mut self, component: &ComponentRef) -> Result<(), NetworkError> {
        self.check_component(component)?;
        validate_source(&component.process, &self.instances[&component.process])?;
        if !self.sources.insert(component.process.clone()) {
            return Err(NetworkError::InvalidSource(component.process.clone()));
        }
        Ok(())
    }
    pub fn build(self) -> Result<NetworkDef, NetworkError> {
        self.check_active()?;
        if self.additions_only {
            return Err(NetworkError::BuildDuringAdditions);
        }
        let interface = Arc::new(Interface::new(
            self.inputs.values().map(|(port, _)| port.clone()),
            self.outputs.values().map(|(port, _)| port.clone()),
        )?);
        let mut network = NetworkDef {
            interface,
            instances: self.instances,
            connections: self.connections,
            inputs: self
                .inputs
                .into_iter()
                .map(|(name, (_, projection))| (name, projection))
                .collect(),
            outputs: self
                .outputs
                .into_iter()
                .map(|(name, (_, path))| (name, path))
                .collect(),
            sources: self.sources.into_iter().collect(),
            replacement_points: self.replacement_points,
            claims: self.claims,
            origin: self.origin,
            requirements: self.requirements,
        };
        for (name, component) in &network.instances {
            collect_component_obligations(component, name, &mut network.requirements);
        }
        normalize_requirements(&mut network.requirements);
        validate_network(&network)?;
        Ok(network)
    }
    fn check_component(&self, component: &ComponentRef) -> Result<(), NetworkError> {
        self.check_capability(&component.process, &component.capability)
    }
    pub fn with_origin(mut self, origin: GraphOrigin) -> Self {
        self.check_active().expect("revoked additions builder");
        self.origin = Some(origin);
        self
    }
    /// Explicitly claim the current public interface. Final compilation checks it again.
    pub fn claim(&mut self, contract: &Contract) -> Result<(), NetworkError> {
        self.check_active()?;
        let interface = Interface::new(
            self.inputs.values().map(|(port, _)| port.clone()),
            self.outputs.values().map(|(port, _)| port.clone()),
        )?;
        contract.check_shape(&interface)?;
        catalog(self.claims.iter().chain(std::iter::once(contract)))?;
        if !self.claims.contains(contract) {
            self.claims.push(contract.clone());
        }
        Ok(())
    }
    /// Record a final-state requirement; the target may be authored later.
    pub fn require_contract(
        &mut self,
        path: &str,
        contract: &Contract,
    ) -> Result<(), NetworkError> {
        self.check_active()?;
        if !path.is_empty() {
            for part in path.split('/') {
                validate_process_name(part)?;
            }
        }
        catalog(
            self.requirements
                .iter()
                .map(|(_, c)| c)
                .chain(std::iter::once(contract)),
        )?;
        self.requirements.push((path.into(), contract.clone()));
        Ok(())
    }
    pub fn check_implementation(
        &self,
        component: &ComponentRef,
        contract: &Contract,
    ) -> Result<(), NetworkError> {
        self.check_component(component)?;
        contract.check_implementation(&self.instances[&component.process])?;
        Ok(())
    }
    pub fn inspect(&self, component: &ComponentRef) -> Result<ComponentSnapshot, NetworkError> {
        self.check_component(component)?;
        let definition = &self.instances[&component.process];
        Ok(ComponentSnapshot {
            interface: definition.interface.clone(),
            claims: definition.claims.clone(),
            origin: definition.origin.clone().or_else(|| self.origin.clone()),
            declared_origin: definition.origin.clone(),
            definition_key: Some(definition.definition_key()),
            physical_index: None,
            path: component.process.clone(),
        })
    }
    /// The restricted handle retains the actual declaration's scope and revision.
    pub fn bind(
        &self,
        component: &ComponentRef,
        contract: &Contract,
    ) -> Result<ComponentRef, NetworkError> {
        self.check_implementation(component, contract)?;
        let mut capability = component.capability.clone();
        capability
            .transactions
            .extend(self.transactions.iter().cloned());
        Ok(ComponentRef {
            process: component.process.clone(),
            interface: contract.required().clone(),
            capability,
        })
    }
    fn check_capability(&self, name: &str, capability: &Capability) -> Result<(), NetworkError> {
        self.check_active()?;
        if !Arc::ptr_eq(&self.scope, &capability.scope) {
            return Err(NetworkError::ForeignScope);
        }
        capability.check()?;
        let revision = self
            .revisions
            .get(name)
            .ok_or_else(|| NetworkError::UnknownProcess(name.into()))?;
        if !Arc::ptr_eq(revision, &capability.revision) {
            return Err(NetworkError::StaleHandle);
        }
        Ok(())
    }
    fn check_active(&self) -> Result<(), NetworkError> {
        if self
            .transactions
            .iter()
            .all(|token| token.load(Ordering::Acquire))
        {
            Ok(())
        } else {
            Err(NetworkError::StaleHandle)
        }
    }
    fn check_endpoint(&self, endpoint: &Endpoint, input: bool) -> Result<(), NetworkError> {
        self.check_capability(&endpoint.path.process, &endpoint.capability)?;
        let actual = port(&self.instances, &endpoint.path, input)?;
        check_type(&endpoint.path, &endpoint.value, &actual.value)
    }
}

fn check_type(
    path: &PortPath,
    expected: &TypeIdentity,
    actual: &TypeIdentity,
) -> Result<(), NetworkError> {
    if expected == actual {
        Ok(())
    } else {
        Err(NetworkError::WrongType {
            path: path.clone(),
            expected: expected.clone(),
            actual: actual.clone(),
        })
    }
}
fn validate_process_name(name: &str) -> Result<(), NetworkError> {
    if name.is_empty()
        || matches!(name, "." | "..")
        || name.contains('/')
        || name.chars().any(char::is_control)
    {
        Err(NetworkError::InvalidName(name.into()))
    } else {
        Ok(())
    }
}
fn port<'a>(
    instances: &'a BTreeMap<String, Arc<ComponentDef>>,
    path: &PortPath,
    input: bool,
) -> Result<&'a PortDef, NetworkError> {
    let component = instances
        .get(&path.process)
        .ok_or_else(|| NetworkError::UnknownProcess(path.process.clone()))?;
    if input {
        component
            .interface
            .input(&path.port)
            .ok_or_else(|| NetworkError::UnknownInput(path.clone()))
    } else {
        component
            .interface
            .output(&path.port)
            .ok_or_else(|| NetworkError::UnknownOutput(path.clone()))
    }
}
fn validate_source(name: &str, component: &ComponentDef) -> Result<(), NetworkError> {
    if matches!(component.body, Body::Primitive(_)) && !component.interface.inputs().is_empty() {
        Err(NetworkError::InvalidSource(name.into()))
    } else {
        Ok(())
    }
}
fn validate_claims(
    interface: &Interface,
    claims: &[Contract],
    path: &str,
) -> Result<(), NetworkError> {
    catalog(claims)?;
    for claim in claims {
        claim
            .check(interface, claims)
            .map_err(|e| e.context(InterfacePhase::Compiled, path, Some(claim.id())))?;
    }
    Ok(())
}
fn declaration<'a>(
    component: &'a ComponentDef,
    path: &str,
) -> Result<(&'a Interface, &'a [Contract]), NetworkError> {
    if path.is_empty() {
        return Ok((&component.interface, &component.claims));
    }
    match &component.body {
        Body::Composite(network) => network_declaration(network, path),
        Body::Primitive(_) => Err(NetworkError::UnknownProcess(path.into())),
    }
}
fn network_declaration<'a>(
    network: &'a NetworkDef,
    path: &str,
) -> Result<(&'a Interface, &'a [Contract]), NetworkError> {
    if path.is_empty() {
        return Ok((&network.interface, &network.claims));
    }
    let (first, rest) = path.split_once('/').unwrap_or((path, ""));
    let component = network
        .instances
        .get(first)
        .ok_or_else(|| NetworkError::UnknownProcess(path.into()))?;
    declaration(component, rest)
}
pub(crate) fn collect_component_obligations(
    component: &ComponentDef,
    path: &str,
    result: &mut Vec<(String, Contract)>,
) {
    result.extend(component.claims.iter().cloned().map(|c| (path.into(), c)));
    if let Body::Composite(network) = &component.body {
        result.extend(
            network
                .requirements
                .iter()
                .map(|(p, c)| (qualify(path, p), c.clone())),
        );
    }
}
fn qualify(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.into()
    } else if name.is_empty() {
        prefix.into()
    } else {
        format!("{prefix}/{name}")
    }
}
/// Remove identical obligations without masking conflicting contract shapes.
/// Builder round-trips and composer replay may checkpoint the same requirement
/// repeatedly. Keep any same-ID/different-shape entries for compilation to
/// reject at the existing contract-catalog validation boundary.
pub(crate) fn normalize_requirements(requirements: &mut Vec<(String, Contract)>) {
    let mut seen = BTreeMap::new();
    requirements.retain(|(path, contract)| {
        let key = (path.clone(), contract.id().to_owned());
        if let Some(previous) = seen.get(&key) {
            // Retain conflicting definitions so the existing compilation
            // boundary can reject them with its complete contract catalog.
            previous != &*contract
        } else {
            seen.insert(key, contract.clone());
            true
        }
    });
}

fn validate_network(network: &NetworkDef) -> Result<(), NetworkError> {
    validate_claims(&network.interface, &network.claims, "")?;
    for name in &network.replacement_points {
        if !network.instances.contains_key(name) {
            return Err(NetworkError::UnknownProcess(name.clone()));
        }
    }
    for (name, component) in &network.instances {
        validate_process_name(name)?;
        validate_claims(&component.interface, &component.claims, name)?;
        if let Body::Composite(child) = &component.body
            && component.interface != child.interface
        {
            return Err(NetworkError::InterfaceMismatch(name.clone()));
        }
    }
    let mut edges = BTreeSet::new();
    for edge in &network.connections {
        let from = port(&network.instances, &edge.from, false)?;
        let to = port(&network.instances, &edge.to, true)?;
        check_type(&edge.to, &from.value, &to.value)?;
        if !edges.insert((&edge.from, &edge.to)) {
            return Err(NetworkError::DuplicateConnection {
                from: edge.from.clone(),
                to: edge.to.clone(),
            });
        }
    }
    if !network.interface.inputs().keys().eq(network.inputs.keys())
        || !network
            .interface
            .outputs()
            .keys()
            .eq(network.outputs.keys())
    {
        return Err(NetworkError::InvalidProjection(
            "incomplete public boundary".into(),
        ));
    }
    for (name, projection) in &network.inputs {
        if projection.targets.is_empty() {
            return Err(NetworkError::InvalidProjection(name.clone()));
        }
        let mut targets = BTreeSet::new();
        for path in &projection.targets {
            let actual = port(&network.instances, path, true)?;
            check_type(path, &network.interface.inputs()[name].value, &actual.value)?;
            if !targets.insert(path) {
                return Err(NetworkError::InvalidProjection(name.clone()));
            }
        }
    }
    for (name, path) in &network.outputs {
        let actual = port(&network.instances, path, false)?;
        check_type(
            path,
            &network.interface.outputs()[name].value,
            &actual.value,
        )?;
    }
    let mut sources = BTreeSet::new();
    for name in &network.sources {
        let component = network
            .instances
            .get(name)
            .ok_or_else(|| NetworkError::UnknownProcess(name.clone()))?;
        validate_source(name, component)?;
        if !sources.insert(name) {
            return Err(NetworkError::InvalidSource(name.clone()));
        }
    }
    Ok(())
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
        (!self.leases.0.is_empty()).then(|| Arc::new(self.leases.clone()) as Arc<dyn Send + Sync>)
    }
    pub fn inspect(&self, path: &str) -> Result<ComponentSnapshot, NetworkError> {
        self.inspections
            .get(path)
            .cloned()
            .ok_or_else(|| NetworkError::UnknownProcess(path.into()))
    }
    pub fn check_implementation(
        &self,
        path: &str,
        contract: &Contract,
    ) -> Result<(), NetworkError> {
        let snapshot = self
            .inspections
            .get(path)
            .ok_or_else(|| NetworkError::UnknownProcess(path.into()))?;
        contract
            .check(&snapshot.interface, &snapshot.claims)
            .map_err(|e| e.context(InterfacePhase::Compiled, path, Some(contract.id())))?;
        Ok(())
    }
    pub fn check_contract_catalog<'a>(
        &'a self,
        registered: impl IntoIterator<Item = &'a Contract>,
    ) -> Result<(), NetworkError> {
        catalog(self.contracts.iter().chain(registered))?;
        Ok(())
    }
    pub fn lookup_definition(
        &self,
        key: DefinitionKey,
    ) -> Result<PhysicalDeclaration, NetworkError> {
        let mut matches = self
            .inspections
            .values()
            .filter(|s| s.definition_key == Some(key) && s.physical_index.is_some());
        let found = matches.next().ok_or(NetworkError::UnknownDefinition)?;
        if matches.next().is_some() {
            return Err(NetworkError::AmbiguousDefinition);
        }
        Ok(PhysicalDeclaration {
            path: found.path.clone(),
            index: found.physical_index.unwrap(),
        })
    }
    pub fn bind<'a>(
        &'a self,
        path: &str,
        contract: &Contract,
    ) -> Result<BoundComponent<'a>, NetworkError> {
        self.check_implementation(path, contract)?;
        Ok(BoundComponent {
            network: self,
            path: path.into(),
            contract: contract.clone(),
        })
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
    /// Read-only resolution of a declared input boundary, including flattened
    /// module aliases. Empty process path denotes the root public interface.
    /// A symbolic path supplies no authoring or injection authority.
    pub fn resolve_input(&self, path: &PortPath) -> Option<&[(usize, String)]> {
        self.boundaries
            .get(&path.process)?
            .inputs
            .get(&path.port)
            .map(Vec::as_slice)
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
}

#[derive(Clone)]
pub struct BoundComponent<'a> {
    network: &'a CompiledNetwork,
    path: String,
    contract: Contract,
}
pub struct BoundInput<'a, T> {
    network: &'a CompiledNetwork,
    targets: Vec<(usize, String)>,
    marker: PhantomData<fn() -> T>,
}
pub struct BoundOutput<'a, T> {
    network: &'a CompiledNetwork,
    source: (usize, String),
    marker: PhantomData<fn() -> T>,
}
impl<T> BoundInput<'_, T> {
    pub fn resolve(&self, network: &CompiledNetwork) -> Result<&[(usize, String)], NetworkError> {
        if !Arc::ptr_eq(&self.network.generation, &network.generation) {
            return Err(NetworkError::ForeignScope);
        }
        Ok(&self.targets)
    }
}
impl<T> BoundOutput<'_, T> {
    pub fn resolve(&self, network: &CompiledNetwork) -> Result<(usize, &str), NetworkError> {
        if !Arc::ptr_eq(&self.network.generation, &network.generation) {
            return Err(NetworkError::ForeignScope);
        }
        Ok((self.source.0, &self.source.1))
    }
}
impl<'a> BoundComponent<'a> {
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn contract(&self) -> &Contract {
        &self.contract
    }
    pub fn input<T: 'static>(&self, name: &str) -> Result<BoundInput<'a, T>, NetworkError> {
        self.typed_input(name, TypeIdentity::Local(TypeId::of::<T>()))
    }
    pub fn output<T: 'static>(&self, name: &str) -> Result<BoundOutput<'a, T>, NetworkError> {
        self.typed_output(name, TypeIdentity::Local(TypeId::of::<T>()))
    }
    pub fn wire_input<T: ContractValue>(
        &self,
        name: &str,
    ) -> Result<BoundInput<'a, T>, NetworkError> {
        self.typed_input(name, PortDef::wire::<T>(name)?.value)
    }
    pub fn wire_output<T: ContractValue>(
        &self,
        name: &str,
    ) -> Result<BoundOutput<'a, T>, NetworkError> {
        self.typed_output(name, PortDef::wire::<T>(name)?.value)
    }
    fn typed_input<T>(
        &self,
        name: &str,
        expected: TypeIdentity,
    ) -> Result<BoundInput<'a, T>, NetworkError> {
        let path = PortPath {
            process: self.path.clone(),
            port: name.into(),
        };
        let port = self
            .contract
            .required()
            .input(name)
            .ok_or_else(|| NetworkError::UnknownInput(path.clone()))?;
        check_type(&path, &expected, &port.value)?;
        Ok(BoundInput {
            network: self.network,
            targets: self.network.boundaries[&self.path].inputs[name].clone(),
            marker: PhantomData,
        })
    }
    fn typed_output<T>(
        &self,
        name: &str,
        expected: TypeIdentity,
    ) -> Result<BoundOutput<'a, T>, NetworkError> {
        let path = PortPath {
            process: self.path.clone(),
            port: name.into(),
        };
        let port = self
            .contract
            .required()
            .output(name)
            .ok_or_else(|| NetworkError::UnknownOutput(path.clone()))?;
        check_type(&path, &expected, &port.value)?;
        Ok(BoundOutput {
            network: self.network,
            source: self.network.boundaries[&self.path].outputs[name].clone(),
            marker: PhantomData,
        })
    }
}

type PhysicalPort = (String, String);
#[derive(Default, Clone)]
struct Boundary {
    inputs: BTreeMap<String, Vec<PhysicalPort>>,
    outputs: BTreeMap<String, PhysicalPort>,
    sources: BTreeSet<String>,
}
struct Compiler {
    limits: CompileLimits,
    instances: u64,
    work: u64,
    active: BTreeSet<usize>,
    processes: BTreeMap<String, FlatProcess>,
    connections: Vec<(PhysicalPort, PhysicalPort, NonZeroUsize)>,
    edges: BTreeSet<(PhysicalPort, PhysicalPort)>,
    inspections: BTreeMap<String, ComponentSnapshot>,
    boundaries: BTreeMap<String, Boundary>,
    requirements: Vec<(String, Contract)>,
}
impl Compiler {
    fn new(limits: CompileLimits) -> Self {
        Self {
            limits,
            instances: 0,
            work: 0,
            active: BTreeSet::new(),
            processes: BTreeMap::new(),
            connections: Vec::new(),
            edges: BTreeSet::new(),
            inspections: BTreeMap::new(),
            boundaries: BTreeMap::new(),
            requirements: Vec::new(),
        }
    }
    fn charge(&mut self, amount: usize, path: &str) -> Result<(), NetworkError> {
        self.work = self
            .work
            .checked_add(u64::try_from(amount).unwrap_or(u64::MAX))
            .filter(|work| *work <= self.limits.max_work)
            .ok_or_else(|| NetworkError::LimitExceeded {
                limit: "max_work",
                path: path.into(),
            })?;
        Ok(())
    }
    fn compile(mut self, network: &NetworkDef) -> Result<CompiledNetwork, NetworkError> {
        let boundary = self.expand(network, "", 0, network.origin.as_ref())?;
        self.inspections.insert(
            String::new(),
            ComponentSnapshot {
                interface: network.interface.clone(),
                claims: network.claims.clone(),
                origin: network.origin.clone(),
                declared_origin: network.origin.clone(),
                definition_key: None,
                physical_index: None,
                path: String::new(),
            },
        );
        self.boundaries.insert(String::new(), boundary.clone());
        let contracts: Vec<_> = self
            .inspections
            .values()
            .flat_map(|s| s.claims.iter().cloned())
            .chain(self.requirements.iter().map(|(_, c)| c.clone()))
            .collect();
        catalog(&contracts)?;
        for (path, contract) in &self.requirements {
            let snapshot = self
                .inspections
                .get(path)
                .ok_or_else(|| NetworkError::UnknownProcess(path.clone()))?;
            contract
                .check(&snapshot.interface, &snapshot.claims)
                .map_err(|e| e.context(InterfacePhase::Compiled, path, Some(contract.id())))?;
        }
        let indexes: BTreeMap<_, _> = self
            .processes
            .keys()
            .enumerate()
            .map(|(index, path)| (path.clone(), index))
            .collect();
        let lower = |port: PhysicalPort| (indexes[&port.0], port.1);
        for (path, snapshot) in &mut self.inspections {
            snapshot.physical_index = indexes.get(path).copied();
        }
        let boundaries = self
            .boundaries
            .into_iter()
            .map(|(path, boundary)| {
                (
                    path,
                    ResolvedBoundary {
                        inputs: boundary
                            .inputs
                            .into_iter()
                            .map(|(name, ports)| (name, ports.into_iter().map(&lower).collect()))
                            .collect(),
                        outputs: boundary
                            .outputs
                            .into_iter()
                            .map(|(name, port)| (name, lower(port)))
                            .collect(),
                    },
                )
            })
            .collect();
        let mut connections: Vec<_> = self
            .connections
            .into_iter()
            .map(|(from, to, capacity)| FlatConnection {
                from: lower(from),
                to: lower(to),
                capacity,
            })
            .collect();
        connections.sort_by(|a, b| (&a.from, &a.to).cmp(&(&b.from, &b.to)));
        let inputs = boundary
            .inputs
            .into_iter()
            .map(|(name, targets)| (name, targets.into_iter().map(&lower).collect()))
            .collect();
        Ok(CompiledNetwork {
            inspections: self.inspections,
            boundaries,
            contracts,
            generation: Arc::new(()),
            leases: DeclarationLeases::default(),
            interface: network.interface.clone(),
            processes: self.processes.into_values().collect(),
            connections,
            inputs,
            ingress_capacities: network
                .inputs
                .iter()
                .map(|(name, projection)| (name.clone(), projection.capacity))
                .collect(),
            outputs: boundary
                .outputs
                .into_iter()
                .map(|(name, port)| (name, lower(port)))
                .collect(),
            sources: boundary
                .sources
                .into_iter()
                .map(|path| indexes[&path])
                .collect(),
        })
    }
    fn expand(
        &mut self,
        network: &NetworkDef,
        prefix: &str,
        depth: usize,
        inherited_origin: Option<&GraphOrigin>,
    ) -> Result<Boundary, NetworkError> {
        let identity = std::ptr::from_ref(network) as usize;
        if !self.active.insert(identity) {
            return Err(NetworkError::RecursiveComposite(prefix.into()));
        }
        // Charge declarations before validation allocates any of its working sets.
        self.charge(network.instances.len(), prefix)?;
        self.charge(network.connections.len(), prefix)?;
        self.charge(network.interface.inputs().len(), prefix)?;
        self.charge(network.interface.outputs().len(), prefix)?;
        self.charge(network.sources.len(), prefix)?;
        for projection in network.inputs.values() {
            self.charge(projection.targets.len(), prefix)?;
        }
        validate_network(network)?;
        self.charge(network.claims.len(), prefix)?;
        self.charge(network.requirements.len(), prefix)?;
        self.requirements.extend(
            network
                .requirements
                .iter()
                .map(|(path, c)| (qualify(prefix, path), c.clone())),
        );
        let mut children = BTreeMap::new();
        for (name, component) in &network.instances {
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if depth >= self.limits.max_depth {
                return Err(NetworkError::LimitExceeded {
                    limit: "max_depth",
                    path,
                });
            }
            self.instances = self
                .instances
                .checked_add(1)
                .filter(|count| *count <= self.limits.max_instances)
                .ok_or_else(|| NetworkError::LimitExceeded {
                    limit: "max_instances",
                    path: path.clone(),
                })?;
            self.charge(component.interface.inputs().len(), &path)?;
            self.charge(component.interface.outputs().len(), &path)?;
            // Closest explicit declaration origin wins; inheritance is provenance
            // only and cannot establish semantic membership or host authorization.
            let origin = component.origin.as_ref().or(inherited_origin);
            self.charge(component.claims.len(), &path)?;
            self.inspections.insert(
                path.clone(),
                ComponentSnapshot {
                    interface: component.interface.clone(),
                    claims: component.claims.clone(),
                    origin: origin.cloned(),
                    declared_origin: component.origin.clone(),
                    definition_key: Some(component.definition_key()),
                    physical_index: None,
                    path: path.clone(),
                },
            );
            let boundary = match &component.body {
                Body::Primitive(factory) => {
                    self.processes.insert(
                        path.clone(),
                        FlatProcess {
                            path: path.clone(),
                            interface: component.interface.clone(),
                            factory: factory.clone(),
                            origin: origin.cloned(),
                        },
                    );
                    Boundary {
                        inputs: component
                            .interface
                            .inputs()
                            .keys()
                            .map(|port| (port.clone(), vec![(path.clone(), port.clone())]))
                            .collect(),
                        outputs: component
                            .interface
                            .outputs()
                            .keys()
                            .map(|port| (port.clone(), (path.clone(), port.clone())))
                            .collect(),
                        sources: if component.interface.inputs().is_empty() {
                            BTreeSet::from([path.clone()])
                        } else {
                            BTreeSet::new()
                        },
                    }
                }
                Body::Composite(child) => self.expand(child, &path, depth + 1, origin)?,
            };
            self.boundaries.insert(path, boundary.clone());
            children.insert(name.clone(), boundary);
        }
        for edge in &network.connections {
            let from = &children[&edge.from.process].outputs[&edge.from.port];
            let targets = &children[&edge.to.process].inputs[&edge.to.port];
            self.charge(targets.len(), prefix)?;
            for to in targets {
                if !self.edges.insert((from.clone(), to.clone())) {
                    return Err(NetworkError::DuplicateConnection {
                        from: edge.from.clone(),
                        to: edge.to.clone(),
                    });
                }
                self.connections
                    .push((from.clone(), to.clone(), edge.capacity));
            }
        }
        let mut boundary = Boundary::default();
        for (name, projection) in &network.inputs {
            let mut targets = BTreeSet::new();
            for target in &projection.targets {
                let resolved = &children[&target.process].inputs[&target.port];
                self.charge(resolved.len(), prefix)?;
                for target in resolved {
                    if !targets.insert(target.clone()) {
                        return Err(NetworkError::InvalidProjection(name.clone()));
                    }
                }
            }
            boundary
                .inputs
                .insert(name.clone(), targets.into_iter().collect());
        }
        for (name, source) in &network.outputs {
            boundary.outputs.insert(
                name.clone(),
                children[&source.process].outputs[&source.port].clone(),
            );
        }
        for source in &network.sources {
            let resolved = &children[source].sources;
            self.charge(resolved.len(), prefix)?;
            boundary.sources.extend(resolved.iter().cloned());
        }
        self.active.remove(&identity);
        Ok(boundary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_ancestor_identity_is_rejected_as_recursive_not_as_a_data_cycle() {
        // Safe public construction cannot form strong Arc definition cycles.
        // Exercise the defensive ancestor check used during nested expansion.
        let network = NetworkBuilder::new().build().unwrap();
        let mut compiler = Compiler::new(CompileLimits::default());
        compiler
            .active
            .insert(std::ptr::from_ref(&network) as usize);
        assert!(
            matches!(compiler.expand(&network, "again", 1, None), Err(NetworkError::RecursiveComposite(path)) if path == "again")
        );
    }

    #[test]
    fn final_validation_fails_closed_on_unprojected_ports_and_invalid_claims() {
        let mut network = NetworkBuilder::new().build().unwrap();
        network.interface =
            Arc::new(Interface::new([PortDef::local::<i64>("missing")], []).unwrap());
        assert!(matches!(
            network.compile(),
            Err(NetworkError::InvalidProjection(_))
        ));

        let mut definition = ComponentDef::composite(NetworkBuilder::new().build().unwrap());
        definition.claims.push(
            Contract::new(
                "native/invalid",
                Arc::new(Interface::new([PortDef::local::<i64>("missing")], []).unwrap()),
            )
            .unwrap(),
        );
        let mut network = NetworkBuilder::new().build().unwrap();
        network
            .instances
            .insert("claimed".into(), Arc::new(definition));
        assert!(matches!(network.compile(), Err(NetworkError::Interface(_))));
    }

    #[test]
    fn final_validation_checks_nested_and_root_claims_even_after_internal_edits() {
        let invalid = Contract::new(
            "native/invalid",
            Arc::new(Interface::new([], [PortDef::local::<i64>("missing")]).unwrap()),
        )
        .unwrap();
        let mut child = NetworkBuilder::new().build().unwrap();
        child.claims.push(invalid.clone());
        let mut outer = NetworkBuilder::new().build().unwrap();
        let mut definition = ComponentDef::composite(child);
        definition.claims.clear();
        outer
            .instances
            .insert("nested".into(), Arc::new(definition));
        assert!(matches!(outer.compile(), Err(NetworkError::Interface(_))));
        let mut root = NetworkBuilder::new().build().unwrap();
        root.claims.push(invalid);
        assert!(matches!(root.compile(), Err(NetworkError::Interface(_))));
    }

    #[test]
    fn inherited_private_requirements_survive_direct_model_replacement() {
        let claim = Contract::new("native/empty", Arc::new(Interface::default())).unwrap();
        let mut child = NetworkBuilder::new();
        child
            .add(
                "private",
                Arc::new(
                    ComponentDef::composite(NetworkBuilder::new().build().unwrap())
                        .claim(&claim)
                        .unwrap(),
                ),
            )
            .unwrap();
        child.require_contract("private", &claim).unwrap();
        let mut root = NetworkBuilder::new();
        root.module("module", child.build().unwrap()).unwrap();
        let mut network = root.build().unwrap();
        network.instances.insert(
            "module".into(),
            Arc::new(ComponentDef::composite(
                NetworkBuilder::new().build().unwrap(),
            )),
        );
        assert!(
            matches!(network.compile(), Err(NetworkError::UnknownProcess(path)) if path == "module/private")
        );
    }
}
