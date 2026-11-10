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
    let shape = InterfaceShape::node_ports(&new.ports, &new.output).map_err(|e| e.to_string())?;
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
        InterfaceShape::module_ports(&old.inputs, &old.output).map_err(|e| e.to_string())?;
    let new_shape =
        InterfaceShape::module_ports(&new.inputs, &new.output).map_err(|e| e.to_string())?;
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

#[derive(Clone)]
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
        self.check_interface(contract, true)?;
        Ok(BoundAuthoringUnit {
            unit: self,
            contract: contract.clone(),
        })
    }
}

/// Borrowed authoring access restricted to the endpoints promised by a contract.
/// Handles own their declaration tokens and can be connected after this borrow ends.
#[derive(Clone)]
pub struct BoundAuthoringUnit<'g> {
    unit: AuthoringUnit<'g>,
    contract: GraphContract,
}
impl<'g> BoundAuthoringUnit<'g> {
    /// Project the common operation API, restricted to promised contract leaves.
    pub fn operation(&self) -> Result<BoundOperation<'g>, InterfaceError> {
        self.operation_snapshot()?;
        Ok(BoundOperation::new(self.clone()))
    }
    pub(super) fn operation_snapshot(&self) -> Result<DiscoveredOperation, InterfaceError> {
        self.unit.check_interface(&self.contract, true)?;
        self.unit
            .graph
            .discover(&self.unit.path)
            .map(|operation| operation.restrict(self.contract.required()))
            .map_err(|error| self.lookup_error(error))
    }
    pub(super) fn lookup_error(&self, message: String) -> InterfaceError {
        InterfaceError::new(message).context(
            InterfacePhase::Authoring,
            &self.unit.path,
            Some(self.contract.id()),
        )
    }
}
impl sealed::Sealed for AuthoringUnit<'_> {}
impl GraphUnit for AuthoringUnit<'_> {
    fn inspect(&self) -> Result<UnitSnapshot, InterfaceError> {
        self.graph
            .inspect_snapshot(&self.path)
            .map_err(|e| e.context(InterfacePhase::Authoring, &self.path, None))
    }

    fn check_interface(
        &self,
        contract: &GraphContract,
        membership: bool,
    ) -> Result<(), InterfaceError> {
        self.graph
            .with_inspected(&self.path, |shape, claims, _, _| {
                interface::check_interface(&shape, claims, contract, membership)
            })
            .map_err(|e| e.context(InterfacePhase::Authoring, &self.path, Some(contract.id())))
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
    fn validate_inspected_boundary(&self, def: ModuleDef) -> Result<(), InterfaceError> {
        let id = def.id.clone();
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
                definition: def,
                triggers: Vec::new(),
            })
            .map_err(InterfaceError::new)?;
        catalog
            .validate(&id)
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
            .expand_scope(&id, &mut Vec::new(), &mut Vec::new())
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
        self.with_inspected(path, |_, _, _, _| Ok(()))
            .map_err(|e| e.context(InterfacePhase::Authoring, path, None))?;
        Ok(unit)
    }
    fn inspect_snapshot(&self, path: &str) -> Result<UnitSnapshot, InterfaceError> {
        self.with_inspected(path, |shape, claims, definition, instances| {
            Ok(UnitSnapshot {
                shape,
                claims: claims.to_vec(),
                provenance: interface::UnitProvenance {
                    phase: InterfacePhase::Authoring,
                    definition: definition.into(),
                    path: path.into(),
                    instance_path: instances.to_vec(),
                },
            })
        })
    }

    // Resolve afresh on every call: retained NodeHandles can mutate pending layouts
    // even while an AuthoringUnit or BoundAuthoringUnit borrows this graph.
    fn with_inspected<T>(
        &self,
        path: &str,
        inspect: impl FnOnce(
            InterfaceShape,
            &[GraphContract],
            &str,
            &[String],
        ) -> Result<T, InterfaceError>,
    ) -> Result<T, InterfaceError> {
        if path.is_empty() {
            let pending = self.pending.borrow();
            // The existing boundary validator owns its catalog. Copy only the
            // effective declaration, not triggers or superseded node layouts.
            let root = ModuleDef {
                declaration_ids: self.module.declaration_ids.clone(),
                claims: self.module.claims.clone(),
                contract_requests: self.module.contract_requests.clone(),
                id: self.module.id.clone(),
                nodes: self
                    .module
                    .nodes
                    .iter()
                    .map(|node| {
                        node.name
                            .as_ref()
                            .and_then(|n| pending.get(n))
                            .unwrap_or(node)
                            .clone()
                    })
                    .collect(),
                instances: self.module.instances.clone(),
                edges: self.module.edges.clone(),
                inputs: self.module.inputs.clone(),
                output: self.module.output.clone(),
            };
            self.validate_inspected_boundary(root)?;
            return inspect(
                InterfaceShape::module_ports(&self.module.inputs, &self.module.output)?,
                &self.module.claims,
                &self.module.id,
                &[],
            );
        }
        let parts =
            parse_name_path(path).ok_or_else(|| InterfaceError::new("invalid unit path"))?;
        let mut def: Option<&ModuleDef> = None;
        let mut instances = Vec::new();
        for (index, part) in parts.iter().enumerate() {
            let (nodes, children, id) = match def {
                Some(def) => (&def.nodes, &def.instances, &def.id),
                None => (&self.module.nodes, &self.module.instances, &self.module.id),
            };
            if index + 1 == parts.len()
                && let Some(node) = nodes.iter().find(|n| n.name.as_deref() == Some(*part))
            {
                let pending = self.pending.borrow();
                let node = if def.is_none() {
                    pending.get(*part).unwrap_or(node)
                } else {
                    node
                };
                return inspect(
                    InterfaceShape::node_ports(&node.ports, &node.output)?,
                    &node.claims,
                    id,
                    &instances,
                );
            }
            let instance = children
                .iter()
                .find(|i| i.name == *part)
                .ok_or_else(|| InterfaceError::new("unknown unit path"))?;
            def = Some(
                &self
                    .dependencies
                    .get(&instance.definition)
                    .ok_or_else(|| InterfaceError::new("missing module definition"))?
                    .definition,
            );
            instances.push((*part).into());
        }
        let def = def.ok_or_else(|| InterfaceError::new("unknown unit path"))?;
        self.validate_inspected_boundary(def.clone())?;
        inspect(
            InterfaceShape::module_ports(&def.inputs, &def.output)?,
            &def.claims,
            &def.id,
            &instances,
        )
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
        let already_claimed = self
            .with_inspected(path, |shape, claims, _, _| {
                contract.required().check_shape(&shape)?;
                let claimed = claims.iter().any(|c| c.id() == contract.id());
                if claimed {
                    interface::check_interface(&shape, claims, contract, true)?;
                }
                Ok(claimed)
            })
            .map_err(|e| e.context(InterfacePhase::Authoring, path, Some(contract.id())))?;
        if already_claimed {
            return Ok(());
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
