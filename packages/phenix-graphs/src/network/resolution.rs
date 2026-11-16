//! Immutable inspection and generation-bound contract resolution.

use super::validation::check_type;
use super::{CompiledNetwork, ComponentSnapshot, NetworkError, PhysicalDeclaration, PortPath};
use crate::component::DefinitionKey;
use crate::contract::{Contract, catalog};
use crate::interface::{InterfacePhase, PortDef};
use crate::{ContractValue, TypeIdentity};
use std::any::TypeId;
use std::marker::PhantomData;
use std::sync::Arc;

impl CompiledNetwork {
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
