//! Borrowed authoritative inspection and declaration-owned semantic claims.
use super::*;
use crate::interface::{self, sealed};

pub(super) fn claim_changes(before: &GraphBuilder, after: &GraphBuilder) -> Vec<AssemblyChange> {
    let mut changes = Vec::new();
    for node in &after.module.nodes {
        if let Some(old) = before.module.nodes.iter().find(|old| old.name == node.name)
            && old.claims != node.claims
        {
            changes.push(AssemblyChange::NodeReplaced(
                node.name.clone().unwrap_or_default(),
            ));
        }
    }
    if before.module.claims != after.module.claims
        || before.module.contract_requests != after.module.contract_requests
    {
        changes.push(AssemblyChange::InstanceRebound(String::new()));
    }
    for instance in &after.module.instances {
        // Nested declaration writes affect every alias, not unrelated packages.
        let mut pending = vec![instance.definition.as_str()];
        let mut visited = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            if let (Some(old), Some(new)) =
                (before.dependencies.get(id), after.dependencies.get(id))
            {
                if old.definition.claims != new.definition.claims
                    || old.definition.contract_requests != new.definition.contract_requests
                    || old
                        .definition
                        .nodes
                        .iter()
                        .map(|n| (n.id, &n.claims))
                        .ne(new.definition.nodes.iter().map(|n| (n.id, &n.claims)))
                {
                    changes.push(AssemblyChange::InstanceRebound(instance.name.clone()));
                    break;
                }
                pending.extend(
                    new.definition
                        .instances
                        .iter()
                        .map(|i| i.definition.as_str()),
                );
            }
        }
    }
    changes
}

pub(super) fn validate_node_replacement(
    old: &NodeTemplate,
    new: &NodeTemplate,
) -> Result<(), String> {
    let shape = InterfaceShape::node_ports(&new.ports, &new.outputs).map_err(|e| e.to_string())?;
    for claim in &old.claims {
        let provided = new
            .claims
            .iter()
            .find(|c| c.id() == claim.id())
            .ok_or_else(|| format!("replacement removes contract claim {}", claim.id()))?;
        claim
            .check_definition(provided)
            .map_err(|e| e.to_string())?;
    }
    for claim in &new.claims {
        claim
            .required()
            .check_shape(&shape)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub(super) fn validate_module_replacement(old: &ModuleDef, new: &ModuleDef) -> Result<(), String> {
    let old_shape =
        InterfaceShape::module_ports(&old.inputs, &old.outputs).map_err(|e| e.to_string())?;
    let new_shape =
        InterfaceShape::module_ports(&new.inputs, &new.outputs).map_err(|e| e.to_string())?;
    if !old_shape.same_shape(&new_shape) {
        return Err("replacement changes complete public module shape".into());
    }
    // Replacement may add requirements, but must retain existing obligations unchanged.
    for request in &old.contract_requests {
        if !new.contract_requests.contains(request) {
            return Err(format!(
                "replacement removes contract request {} at {:?}",
                request.1.id(),
                request.0
            ));
        }
    }
    for claim in &old.claims {
        let provided = new
            .claims
            .iter()
            .find(|c| c.id() == claim.id())
            .ok_or_else(|| format!("replacement removes contract claim {}", claim.id()))?;
        claim
            .check_definition(provided)
            .map_err(|e| e.to_string())?;
    }
    for claim in &new.claims {
        claim
            .required()
            .check_shape(&new_shape)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub struct AuthoringUnit<'a> {
    graph: &'a GraphBuilder,
    path: String,
}
impl<'g> AuthoringUnit<'g> {
    /// Bind only an explicitly claimed contract, not a merely matching shape.
    pub fn bind_contract(
        self,
        contract: &GraphContract,
    ) -> Result<BoundAuthoringUnit<'g>, InterfaceError> {
        self.inspect()?
            .check_contract(contract)
            .map_err(|e| e.context(InterfacePhase::Authoring, &self.path, Some(contract.id())))?;
        Ok(BoundAuthoringUnit {
            unit: self,
            contract: contract.clone(),
        })
    }
}

/// Borrowed authoring access restricted to the endpoints promised by a contract.
/// Handles own their declaration tokens and can be connected after this borrow ends.
pub struct BoundAuthoringUnit<'g> {
    unit: AuthoringUnit<'g>,
    contract: GraphContract,
}
impl BoundAuthoringUnit<'_> {
    fn check_endpoint(
        &self,
        port: Option<&str>,
        name: &str,
        requested: TypeIdentity,
    ) -> Result<(), InterfaceError> {
        // Pending node layouts can change while this binding still borrows the graph.
        self.unit
            .inspect()
            .and_then(|snapshot| snapshot.check_contract(&self.contract))
            .map_err(|e| {
                e.context(
                    InterfacePhase::Authoring,
                    &self.unit.path,
                    Some(self.contract.id()),
                )
            })?;
        let required = self.contract.required();
        let promised = match port {
            Some(port) => required
                .inputs()
                .find(|(p, s, _)| *p == port && *s == name)
                .map(|(_, _, v)| v),
            None => required.outputs().find(|(n, _)| *n == name).map(|(_, v)| v),
        };
        let actual = promised.map(interface::value_identity);
        if actual == Some(requested) {
            return Ok(());
        }
        let mut error = self.lookup_error(if actual.is_some() {
            "requested type does not match contract endpoint".into()
        } else {
            "endpoint is not promised by contract".into()
        });
        error.endpoint = Some(
            match port {
                Some(port) => format!("input {port}.{name}"),
                None => format!("output {name}"),
            }
            .into(),
        );
        error.expected = Some(requested);
        error.actual = actual;
        Err(error)
    }

    fn lookup_error(&self, message: String) -> InterfaceError {
        InterfaceError::new(message).context(
            InterfacePhase::Authoring,
            &self.unit.path,
            Some(self.contract.id()),
        )
    }

    pub fn wire_input<T: ContractValue>(
        &self,
        port: &str,
        slot: &str,
    ) -> Result<Input<T>, InterfaceError> {
        self.check_endpoint(Some(port), slot, TypeIdentity::Wire(T::wire_schema()))?;
        self.unit
            .graph
            .wire_input(&self.unit.path, port, slot)
            .map_err(|e| self.lookup_error(e))
    }

    pub fn wire_output<T: ContractValue>(
        &self,
        name: &str,
    ) -> Result<OutputRef<T>, InterfaceError> {
        self.check_endpoint(None, name, TypeIdentity::Wire(T::wire_schema()))?;
        self.unit
            .graph
            .wire_output(&self.unit.path, name)
            .map_err(|e| self.lookup_error(e))
    }

    pub fn local_input<T: Send + Sync + 'static>(
        &self,
        port: &str,
        slot: &str,
    ) -> Result<Input<T>, InterfaceError> {
        self.check_endpoint(Some(port), slot, TypeIdentity::Local(TypeId::of::<T>()))?;
        self.unit
            .graph
            .local_input(&self.unit.path, port, slot)
            .map_err(|e| self.lookup_error(e))
    }

    pub fn local_output<T: Send + Sync + 'static>(
        &self,
        name: &str,
    ) -> Result<OutputRef<T>, InterfaceError> {
        self.check_endpoint(None, name, TypeIdentity::Local(TypeId::of::<T>()))?;
        self.unit
            .graph
            .local_output(&self.unit.path, name)
            .map_err(|e| self.lookup_error(e))
    }
}
impl sealed::Sealed for AuthoringUnit<'_> {}
impl GraphUnit for AuthoringUnit<'_> {
    fn inspect(&self) -> Result<UnitSnapshot, InterfaceError> {
        self.graph
            .inspect_snapshot(&self.path)
            .map_err(|e| e.context(InterfacePhase::Authoring, &self.path, None))
    }
}
impl GraphBuilder {
    /// Bind a claimed authoring interface without exposing implementation extensions.
    pub fn bind_contract(
        &self,
        path: &str,
        contract: &GraphContract,
    ) -> Result<BoundAuthoringUnit<'_>, InterfaceError> {
        AuthoringUnit {
            graph: self,
            path: path.into(),
        }
        .bind_contract(contract)
    }
    fn validate_inspected_boundary(&self, def: &ModuleDef) -> Result<(), InterfaceError> {
        let mut catalog = ModuleCatalog::new().limits(self.compile_limits);
        for entry in self.dependencies.values() {
            if entry.definition.id != def.id {
                catalog
                    .add(copy_entry(entry))
                    .map_err(InterfaceError::new)?;
            }
        }
        catalog
            .add(ModuleEntry {
                definition: def.clone(),
                triggers: Vec::new(),
            })
            .map_err(InterfaceError::new)?;
        catalog
            .validate(&def.id)
            .map_err(|e| InterfaceError::new(e.to_string()))?;
        let mut expander = ModuleExpander {
            catalog: &catalog,
            builder: FlatGraphBuilder::default(),
            metadata: CompiledMetadata::default(),
            next_node: 1,
            next_edge: 1,
            expanded_instances: Cell::new(0),
            expansion_work: Cell::new(0),
            limits: self.compile_limits,
        };
        expander
            .expand_scope(&def.id, &mut Vec::new(), &mut Vec::new())
            .map_err(InterfaceError::new)?;
        Ok(())
    }
    /// Register a final-unit requirement. No early snapshot certifies this request.
    pub fn require_contract(
        &mut self,
        path: &str,
        contract: &GraphContract,
    ) -> Result<(), InterfaceError> {
        if !path.is_empty() && parse_name_path(path).is_none() {
            return Err(InterfaceError::new("invalid unit path"));
        }
        self.module
            .contract_requests
            .push((path.into(), contract.clone()));
        Ok(())
    }
    /// The view borrows this exact builder and always resolves effective declarations.
    pub fn inspect(&self, path: &str) -> Result<AuthoringUnit<'_>, InterfaceError> {
        let unit = AuthoringUnit {
            graph: self,
            path: path.into(),
        };
        unit.inspect()?;
        Ok(unit)
    }
    fn inspect_snapshot(&self, path: &str) -> Result<UnitSnapshot, InterfaceError> {
        let mut root = self.module.clone().build().definition;
        for node in &mut root.nodes {
            if let Some(pending) = node
                .name
                .as_ref()
                .and_then(|n| self.pending.borrow().get(n).cloned())
            {
                *node = pending;
            }
        }
        if path.is_empty() {
            self.validate_inspected_boundary(&root)?;
            return interface::declaration_snapshot(&root, path, InterfacePhase::Authoring);
        }
        let parts =
            parse_name_path(path).ok_or_else(|| InterfaceError::new("invalid unit path"))?;
        let mut def = &root;
        let mut instances = Vec::new();
        for (index, part) in parts.iter().enumerate() {
            if index + 1 == parts.len()
                && let Some(node) = def.nodes.iter().find(|n| n.name.as_deref() == Some(*part))
            {
                return interface::node_snapshot(
                    node,
                    &def.id,
                    path,
                    InterfacePhase::Authoring,
                    &instances,
                );
            }
            let instance = def
                .instances
                .iter()
                .find(|i| i.name == *part)
                .ok_or_else(|| InterfaceError::new("unknown unit path"))?;
            def = &self
                .dependencies
                .get(&instance.definition)
                .ok_or_else(|| InterfaceError::new("missing module definition"))?
                .definition;
            instances.push((*part).into());
        }
        self.validate_inspected_boundary(def)?;
        let mut snapshot = interface::declaration_snapshot(def, path, InterfacePhase::Authoring)?;
        snapshot.provenance.instance_path = instances;
        Ok(snapshot)
    }
    /// A scoped plugin may make claims on its own direct nodes only. Claims on
    /// imported module definitions are shared across every alias, and must be
    /// declared on the reusable ModulePackage before it is instantiated.
    /// Unscoped manual authoring retains the full low-level editing API.
    fn check_claim_owner(&self, path: &str) -> Result<(), InterfaceError> {
        let Some(origin) = &self.active_origin else {
            return Ok(());
        };
        if path.is_empty() {
            return Err(InterfaceError::new(
                "a scoped contribution cannot claim the composite graph root",
            ));
        }
        let parts = parse_name_path(path)
            .ok_or_else(|| InterfaceError::new("invalid claim target path"))?;
        if parts.len() != 1 {
            return Err(InterfaceError::new(
                "a scoped contribution cannot alter nested shared module claims; declare them on the module definition",
            ));
        }
        let name = parts[0];
        if self.module.instances.iter().any(|unit| unit.name == name) {
            return Err(InterfaceError::new(
                "a scoped contribution cannot alter shared module claims; declare them before instantiation",
            ));
        }
        let node = self
            .module
            .nodes
            .iter()
            .find(|node| node.name.as_deref() == Some(name))
            .ok_or_else(|| InterfaceError::new("unknown direct node for claim"))?;
        if node.composition_origin.as_ref() != Some(origin) {
            return Err(InterfaceError::new(
                "contract claim would alter a graph unit owned by another contribution generation",
            ));
        }
        Ok(())
    }

    pub fn implements_public(&mut self, contract: &GraphContract) -> Result<(), InterfaceError> {
        self.implements("", contract)
    }
    pub fn implements(
        &mut self,
        path: &str,
        contract: &GraphContract,
    ) -> Result<(), InterfaceError> {
        self.check_claim_owner(path)?;
        let snapshot = self
            .inspect_snapshot(path)
            .map_err(|e| e.context(InterfacePhase::Authoring, path, Some(contract.id())))?;
        contract
            .required()
            .check_shape(&snapshot.shape)
            .map_err(|e| e.context(InterfacePhase::Authoring, path, Some(contract.id())))?;
        if snapshot.claims.iter().any(|c| c.id() == contract.id()) {
            return snapshot.check_contract(contract);
        }
        if path.is_empty() {
            self.module.claims.push(contract.clone());
            return Ok(());
        }
        let parts =
            parse_name_path(path).ok_or_else(|| InterfaceError::new("invalid unit path"))?;
        let mut definition: Option<String> = None;
        for (index, part) in parts.iter().enumerate() {
            let last = index + 1 == parts.len();
            if let Some(id) = definition.clone() {
                let entry = self
                    .dependencies
                    .get_mut(&id)
                    .ok_or_else(|| InterfaceError::new("missing definition"))?;
                if last
                    && let Some(node) = entry
                        .definition
                        .nodes
                        .iter_mut()
                        .find(|n| n.name.as_deref() == Some(*part))
                {
                    node.claims.push(contract.clone());
                    self.refresh_dependency_identities(&id);
                    return Ok(());
                }
                definition = Some(
                    entry
                        .definition
                        .instances
                        .iter()
                        .find(|i| i.name == *part)
                        .ok_or_else(|| InterfaceError::new("missing instance"))?
                        .definition
                        .clone(),
                );
            } else {
                if last
                    && let Some(node) = self
                        .module
                        .nodes
                        .iter_mut()
                        .find(|n| n.name.as_deref() == Some(*part))
                {
                    node.claims.push(contract.clone());
                    if let Some(node) = self.pending.borrow_mut().get_mut(*part) {
                        node.claims.push(contract.clone());
                    }
                    return Ok(());
                }
                definition = Some(
                    self.module
                        .instances
                        .iter()
                        .find(|i| i.name == *part)
                        .ok_or_else(|| InterfaceError::new("missing instance"))?
                        .definition
                        .clone(),
                );
            }
        }
        let definition = definition.ok_or_else(|| InterfaceError::new("missing definition"))?;
        self.dependencies
            .get_mut(&definition)
            .ok_or_else(|| InterfaceError::new("missing definition"))?
            .definition
            .claims
            .push(contract.clone());
        self.refresh_dependency_identities(&definition);
        Ok(())
    }
}
