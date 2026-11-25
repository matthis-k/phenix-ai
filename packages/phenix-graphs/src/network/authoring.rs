//! Mutable authoring and the private capability/revision lineage it issues.

use super::validation::{
    check_type, collect_component_obligations, declaration, normalize_requirements, port,
    validate_claims, validate_network, validate_process_name, validate_source,
};
use super::{
    ComponentSnapshot, ConnectionDef, InputProjection, NetworkDef, NetworkError, PortPath,
};
use crate::component::ComponentDef;
use crate::contract::{Contract, catalog};
use crate::interface::{Interface, InterfacePhase, PortDef};
use crate::process::ProcessFactory;
use crate::{ContractValue, GraphOrigin, TypeIdentity};
use std::any::TypeId;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::marker::PhantomData;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

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
    extensions_allowed: bool,
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
            extensions_allowed: false,
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
            extensions_allowed: network.extensions_allowed,
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
            extensions_allowed: self.extensions_allowed,
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
        old.interface
            .check_shape(&definition.interface)
            .map_err(|e| e.context(InterfacePhase::Authoring, &current.process, None))?;
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
    /// Author-owned opt-in to *internal* nested Add/Connect edits. This does
    /// not grant any plugin authority: the host must separately approve each
    /// exact component or edge footprint. Every enclosing composite must
    /// opt in; existing replacement points do not imply extension authority.
    pub fn allow_extensions(&mut self) -> Result<(), NetworkError> {
        self.check_active()?;
        self.extensions_allowed = true;
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
            extensions_allowed: self.extensions_allowed,
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
