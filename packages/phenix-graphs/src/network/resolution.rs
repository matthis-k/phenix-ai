//! Immutable inspection and generation-bound resolution. Coordinates are diagnostic only.

use super::validation::check_type;
use super::{
    CompiledNetwork, ComponentSnapshot, NetworkError, PhysicalDeclaration, PortPath,
    ResolvedBoundary,
};
use crate::component::DefinitionKey;
use crate::contract::{Contract, catalog};
use crate::interface::{InterfacePhase, PortDef};
use crate::{ContractValue, InputKey, OutputKey, TypeIdentity, Value};
use std::any::TypeId;
use std::marker::PhantomData;
use std::sync::Arc;

/// Handle-owned snapshots, deliberately separate from compiler declaration leases.
#[derive(Clone, Default)]
pub(crate) struct HandleLeases(pub(crate) Vec<Arc<dyn Send + Sync>>);
impl HandleLeases {
    /// Appended owners and their caught panic payloads are cleaned up before the
    /// base execution owner. Drop contains every release, including the base.
    pub(crate) fn append(
        base: Option<Arc<dyn Send + Sync>>,
        mut appended: Vec<Arc<dyn Send + Sync>>,
    ) -> Arc<dyn Send + Sync> {
        if let Some(base) = base {
            appended.insert(0, base);
        }
        Arc::new(Self(appended))
    }
}
impl std::fmt::Debug for HandleLeases {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HandleLeases")
            .field("count", &self.0.len())
            .finish()
    }
}
impl Drop for HandleLeases {
    fn drop(&mut self) {
        while let Some(lease) = self.0.pop() {
            if let Err(payload) =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(lease)))
            {
                crate::discard_panic_payload(payload);
            }
        }
    }
}

// A token is identity, not a code lease. Leases are copied only at issuance;
// enriching a compiled clone cannot retroactively enrich escaped capabilities.
#[derive(Clone, Debug)]
struct Owner {
    generation: Arc<()>,
    leases: HandleLeases,
}
impl Owner {
    fn new(network: &CompiledNetwork) -> Self {
        Self {
            generation: network.generation.clone(),
            leases: HandleLeases(network.leases.0.clone()),
        }
    }
    fn validate(&self, network: &CompiledNetwork) -> Result<(), NetworkError> {
        self.matches(&network.generation)
    }
    fn matches(&self, generation: &Arc<()>) -> Result<(), NetworkError> {
        if Arc::ptr_eq(&self.generation, generation) {
            Ok(())
        } else {
            Err(NetworkError::ForeignScope)
        }
    }
    fn retain(&self, mut value: Value) -> Value {
        for lease in &self.leases.0 {
            value = value.with_lease(lease.clone());
        }
        value
    }
}

impl CompiledNetwork {
    /// Exact physical process lookup from the compiler's immutable inspection
    /// index. Composite declarations and the root do not resolve to a process.
    pub(crate) fn physical_process_index(&self, path: &str) -> Option<usize> {
        self.inspections
            .get(path)
            .and_then(|snapshot| snapshot.physical_index)
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
    /// Detached diagnostic lookup of a unique physical occurrence, not authority.
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
    /// Select an actual declaration; the empty path explicitly selects the root.
    pub fn declaration(&self, path: &str) -> Result<BoundDeclaration, NetworkError> {
        let snapshot = self.inspect(path)?;
        let prefix = format!("{path}/");
        let sources =
            self.sources
                .iter()
                .filter_map(|&index| {
                    let source_path = self.processes[index].path();
                    (path.is_empty() || source_path == path || source_path.starts_with(&prefix))
                        .then(|| SourceHandle {
                            path: source_path.into(),
                            index,
                            owner: Owner::new(self),
                        })
                })
                .collect();
        Ok(BoundDeclaration {
            snapshot,
            boundary: self.boundaries[path].clone(),
            sources,
            owner: Owner::new(self),
        })
    }
    /// Includes composite occurrences. Reused definitions require explicit selection.
    pub fn bind_definition(&self, key: DefinitionKey) -> Result<BoundDeclaration, NetworkError> {
        let mut matches = self
            .inspections
            .values()
            .filter(|s| s.definition_key == Some(key));
        let found = matches.next().ok_or(NetworkError::UnknownDefinition)?;
        if matches.next().is_some() {
            return Err(NetworkError::AmbiguousDefinition);
        }
        self.declaration(&found.path)
    }
    pub fn declarations(&self, key: DefinitionKey) -> Vec<BoundDeclaration> {
        self.inspections
            .values()
            .filter(|s| s.definition_key == Some(key))
            .map(|s| self.declaration(&s.path).expect("compiled declaration"))
            .collect()
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
    pub fn bind_ingress<T: 'static>(
        &self,
        name: &str,
        key: InputKey<T>,
    ) -> Result<IngressHandle<T>, NetworkError> {
        let endpoint = self
            .declaration("")?
            .typed_input(name, key.definition()?.value)?;
        Ok(IngressHandle { key, endpoint })
    }
    pub fn bind_capture<T: 'static>(
        &self,
        name: &str,
        key: OutputKey<T>,
    ) -> Result<CaptureHandle, NetworkError> {
        let endpoint = self
            .declaration("")?
            .typed_output::<T>(name, key.definition()?.value)?;
        Ok(CaptureHandle {
            path: endpoint.path,
            source: endpoint.source,
            identity: endpoint.identity,
            owner: endpoint.owner,
        })
    }
    /// Erased capture selection for draining every actual exported alias.
    pub fn capture(&self, name: &str) -> Result<CaptureHandle, NetworkError> {
        let port = self.interface.output(name).ok_or_else(|| {
            NetworkError::UnknownOutput(PortPath {
                process: String::new(),
                port: name.into(),
            })
        })?;
        let endpoint = self
            .declaration("")?
            .typed_output::<()>(name, port.value.clone())?;
        Ok(CaptureHandle {
            path: endpoint.path,
            source: endpoint.source,
            identity: endpoint.identity,
            owner: endpoint.owner,
        })
    }
    pub fn source(&self, declaration: &BoundDeclaration) -> Result<SourceHandle, NetworkError> {
        declaration.validate(self)?;
        let index = declaration
            .snapshot
            .physical_index
            .ok_or_else(|| NetworkError::InvalidSource(declaration.path().into()))?;
        if !self.sources.contains(&index) {
            return Err(NetworkError::InvalidSource(declaration.path().into()));
        }
        Ok(SourceHandle {
            path: declaration.path().into(),
            index,
            owner: declaration.owner.clone(),
        })
    }
    /// Diagnostic flattened projection; it supplies no authoring or execution authority.
    pub fn resolve_input(&self, path: &PortPath) -> Option<&[(usize, String)]> {
        self.boundaries
            .get(&path.process)?
            .inputs
            .get(&path.port)
            .map(Vec::as_slice)
    }
}

#[derive(Clone)]
pub struct BoundComponent<'a> {
    network: &'a CompiledNetwork,
    path: String,
    contract: Contract,
}

/// An owned occurrence, including composites and the root, not a process instance.
#[derive(Clone, Debug)]
pub struct BoundDeclaration {
    snapshot: ComponentSnapshot,
    boundary: ResolvedBoundary,
    sources: Vec<SourceHandle>,
    owner: Owner,
}
impl BoundDeclaration {
    pub fn path(&self) -> &str {
        &self.snapshot.path
    }
    pub fn definition_key(&self) -> Option<DefinitionKey> {
        self.snapshot.definition_key
    }
    pub fn interface(&self) -> &Arc<crate::Interface> {
        &self.snapshot.interface
    }
    pub fn sources(&self) -> Vec<SourceHandle> {
        self.sources
            .iter()
            .cloned()
            .map(|mut source| {
                source.owner = self.owner.clone();
                source
            })
            .collect()
    }
    pub(crate) fn execution_lease(&self) -> Option<Arc<dyn Send + Sync>> {
        (!self.owner.leases.0.is_empty())
            .then(|| Arc::new(self.owner.leases.clone()) as Arc<dyn Send + Sync>)
    }
    pub fn check_implementation(&self, contract: &Contract) -> Result<(), NetworkError> {
        contract
            .check(&self.snapshot.interface, &self.snapshot.claims)
            .map_err(|e| e.context(InterfacePhase::Compiled, self.path(), Some(contract.id())))?;
        Ok(())
    }
    pub fn input_key<T: 'static>(&self, key: InputKey<T>) -> Result<BoundInput<T>, NetworkError> {
        self.typed_input(key.name(), key.definition()?.value)
    }
    pub fn output_key<T: 'static>(
        &self,
        key: OutputKey<T>,
    ) -> Result<BoundOutput<T>, NetworkError> {
        self.typed_output(key.name(), key.definition()?.value)
    }
    pub fn input<T: 'static>(&self, name: &str) -> Result<BoundInput<T>, NetworkError> {
        self.typed_input(name, TypeIdentity::Local(TypeId::of::<T>()))
    }
    pub fn output<T: 'static>(&self, name: &str) -> Result<BoundOutput<T>, NetworkError> {
        self.typed_output(name, TypeIdentity::Local(TypeId::of::<T>()))
    }
    pub fn wire_input<T: ContractValue>(&self, name: &str) -> Result<BoundInput<T>, NetworkError> {
        self.typed_input(name, PortDef::wire::<T>(name)?.value)
    }
    pub fn wire_output<T: ContractValue>(
        &self,
        name: &str,
    ) -> Result<BoundOutput<T>, NetworkError> {
        self.typed_output(name, PortDef::wire::<T>(name)?.value)
    }
    fn typed_input<T>(
        &self,
        name: &str,
        expected: TypeIdentity,
    ) -> Result<BoundInput<T>, NetworkError> {
        let path = PortPath {
            process: self.path().into(),
            port: name.into(),
        };
        let port = self
            .snapshot
            .interface
            .input(name)
            .ok_or_else(|| NetworkError::UnknownInput(path.clone()))?;
        check_type(&path, &expected, &port.value)?;
        Ok(BoundInput {
            path,
            identity: port.value.clone(),
            targets: self.boundary.inputs[name].clone(),
            marker: PhantomData,
            owner: self.owner.clone(),
        })
    }
    fn typed_output<T>(
        &self,
        name: &str,
        expected: TypeIdentity,
    ) -> Result<BoundOutput<T>, NetworkError> {
        let path = PortPath {
            process: self.path().into(),
            port: name.into(),
        };
        let port = self
            .snapshot
            .interface
            .output(name)
            .ok_or_else(|| NetworkError::UnknownOutput(path.clone()))?;
        check_type(&path, &expected, &port.value)?;
        Ok(BoundOutput {
            path,
            identity: port.value.clone(),
            source: self.boundary.outputs[name].clone(),
            marker: PhantomData,
            owner: self.owner.clone(),
        })
    }
    pub(crate) fn process_path(&self, network: &CompiledNetwork) -> Result<&str, NetworkError> {
        self.validate(network)?;
        self.snapshot
            .physical_index
            .ok_or_else(|| NetworkError::UnknownProcess(self.path().into()))?;
        Ok(self.path())
    }
}

pub struct BoundInput<T> {
    path: PortPath,
    identity: TypeIdentity,
    targets: Vec<(usize, String)>,
    marker: PhantomData<fn() -> T>,
    owner: Owner,
}
pub struct BoundOutput<T> {
    path: PortPath,
    identity: TypeIdentity,
    source: (usize, String),
    marker: PhantomData<fn() -> T>,
    owner: Owner,
}
impl<T> Clone for BoundInput<T> {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            identity: self.identity.clone(),
            targets: self.targets.clone(),
            marker: PhantomData,
            owner: self.owner.clone(),
        }
    }
}
impl<T> Clone for BoundOutput<T> {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            identity: self.identity.clone(),
            source: self.source.clone(),
            marker: PhantomData,
            owner: self.owner.clone(),
        }
    }
}
impl<T> BoundInput<T> {
    pub fn name(&self) -> &str {
        &self.path.port
    }
    pub fn value_type(&self) -> &TypeIdentity {
        &self.identity
    }
    pub fn resolve(&self, network: &CompiledNetwork) -> Result<&[(usize, String)], NetworkError> {
        self.validate(network)?;
        Ok(&self.targets)
    }
    /// Checked symbolic bridge for composition. This does not transfer authority
    /// to a later compilation; that compilation must issue its own handles.
    pub fn symbolic_path(&self, network: &CompiledNetwork) -> Result<&PortPath, NetworkError> {
        self.validate(network)?;
        Ok(&self.path)
    }
}
impl<T> BoundOutput<T> {
    pub fn name(&self) -> &str {
        &self.path.port
    }
    pub fn value_type(&self) -> &TypeIdentity {
        &self.identity
    }
    pub fn resolve(&self, network: &CompiledNetwork) -> Result<(usize, &str), NetworkError> {
        self.validate(network)?;
        Ok((self.source.0, &self.source.1))
    }
    pub fn symbolic_path(&self, network: &CompiledNetwork) -> Result<&PortPath, NetworkError> {
        self.validate(network)?;
        Ok(&self.path)
    }
    pub(crate) fn physical_declaration(
        &self,
        network: &CompiledNetwork,
    ) -> Result<BoundDeclaration, NetworkError> {
        self.validate(network)?;
        let mut declaration = network.declaration(network.processes[self.source.0].path())?;
        declaration.owner = self.owner.clone();
        Ok(declaration)
    }
}

/// Only actual public input maps issue ingress authority, never internal endpoints.
/// ```compile_fail,E0308
/// use phenix_graph::{BoundInput, NetworkSession};
/// fn internal(s: &mut NetworkSession, input: &BoundInput<i64>) { s.inject_bound(input, 1); }
/// ```
pub struct IngressHandle<T> {
    key: InputKey<T>,
    endpoint: BoundInput<T>,
}
impl<T> Clone for IngressHandle<T> {
    fn clone(&self) -> Self {
        Self {
            key: self.key,
            endpoint: self.endpoint.clone(),
        }
    }
}
impl<T> IngressHandle<T> {
    pub fn name(&self) -> &str {
        self.endpoint.name()
    }
    pub fn value_type(&self) -> &TypeIdentity {
        self.endpoint.value_type()
    }
    pub fn validate(&self, network: &CompiledNetwork) -> Result<(), NetworkError> {
        self.endpoint.validate(network)
    }
    pub fn with_lease(mut self, lease: Arc<dyn Send + Sync>) -> Self {
        self.endpoint.owner.leases.0.push(lease);
        self
    }
    pub fn matches_input(&self, input: &BoundInput<T>) -> Result<bool, NetworkError> {
        self.endpoint.owner.matches(&input.owner.generation)?;
        Ok(self.endpoint.identity == input.identity && self.endpoint.targets == input.targets)
    }
    /// Copy every known issuing-handle lease onto `value`, deduplicating by stable
    /// `Arc` identity. No conversion/re-encoding, validation, or session authority
    /// is performed or granted.
    pub fn retain_value(&self, value: Value) -> Value {
        self.endpoint.owner.retain(value)
    }
}
impl<T: Send + Sync + 'static> IngressHandle<T> {
    /// Conversion runs without a session lease, retaining known owners throughout
    /// codec execution and local payload destruction. Raw retry never re-encodes.
    pub fn value(&self, payload: T) -> Value {
        self.retain_value(self.key.value(payload))
    }
}

/// One actual public alias, even when several aliases share the same producer.
/// ```compile_fail
/// use phenix_graph::CaptureHandle;
/// let forged = CaptureHandle { name: "out".into() };
/// ```
#[derive(Clone, Debug)]
pub struct CaptureHandle {
    path: PortPath,
    source: (usize, String),
    identity: TypeIdentity,
    owner: Owner,
}
impl CaptureHandle {
    pub fn name(&self) -> &str {
        &self.path.port
    }
    pub fn value_type(&self) -> &TypeIdentity {
        &self.identity
    }
    pub fn matches_output<T>(&self, output: &BoundOutput<T>) -> Result<bool, NetworkError> {
        self.owner.matches(&output.owner.generation)?;
        Ok(self.identity == output.identity && self.source == output.source)
    }
    pub fn resolve(&self, network: &CompiledNetwork) -> Result<(usize, &str), NetworkError> {
        self.validate(network)?;
        Ok((self.source.0, &self.source.1))
    }
    pub(crate) fn retain_packet(
        &self,
        mut packet: crate::connection::Packet,
    ) -> crate::connection::Packet {
        for lease in &self.owner.leases.0 {
            packet = packet.with_lease(lease.clone());
        }
        packet
    }
}

#[derive(Clone, Debug)]
pub struct SourceHandle {
    path: String,
    index: usize,
    owner: Owner,
}
impl SourceHandle {
    pub(crate) fn known_leases(&self) -> &[Arc<dyn Send + Sync>] {
        &self.owner.leases.0
    }
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn resolve(&self, network: &CompiledNetwork) -> Result<usize, NetworkError> {
        self.validate(network)?;
        Ok(self.index)
    }
}

macro_rules! owned_methods {
    ($name:ident $(<$t:ident>)?) => {
        impl$(<$t>)? $name$(<$t>)? {
            /// Pure owner authentication, with no schema, readiness or runtime callbacks.
            pub fn validate(&self, network: &CompiledNetwork) -> Result<(), NetworkError> { self.owner.validate(network) }
            /// Explicitly retain an owner unknown to the compiler (for example a codec).
            pub fn with_lease(mut self, lease: Arc<dyn Send + Sync>) -> Self { self.owner.leases.0.push(lease); self }
        }
    };
}
owned_methods!(BoundDeclaration);
owned_methods!(BoundInput<T>);
owned_methods!(BoundOutput<T>);
owned_methods!(CaptureHandle);
owned_methods!(SourceHandle);

impl BoundComponent<'_> {
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn contract(&self) -> &Contract {
        &self.contract
    }
    fn declaration(&self) -> BoundDeclaration {
        let mut declaration = self
            .network
            .declaration(&self.path)
            .expect("bound declaration");
        declaration.snapshot.interface = self.contract.required().clone();
        declaration
    }
    pub fn input<T: 'static>(&self, name: &str) -> Result<BoundInput<T>, NetworkError> {
        self.declaration().input(name)
    }
    pub fn output<T: 'static>(&self, name: &str) -> Result<BoundOutput<T>, NetworkError> {
        self.declaration().output(name)
    }
    pub fn wire_input<T: ContractValue>(&self, name: &str) -> Result<BoundInput<T>, NetworkError> {
        self.declaration().wire_input(name)
    }
    pub fn wire_output<T: ContractValue>(
        &self,
        name: &str,
    ) -> Result<BoundOutput<T>, NetworkError> {
        self.declaration().wire_output(name)
    }
    pub fn input_key<T: 'static>(&self, key: InputKey<T>) -> Result<BoundInput<T>, NetworkError> {
        self.declaration().input_key(key)
    }
    pub fn output_key<T: 'static>(
        &self,
        key: OutputKey<T>,
    ) -> Result<BoundOutput<T>, NetworkError> {
        self.declaration().output_key(key)
    }
}
