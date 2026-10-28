use phenix_artifact::{VerifiedContractPackage, VerifiedPackage};
use phenix_contract::{
    identifier, import_identity, operation_key, property_contract, property_definitions, Authority,
    Cardinality, Code, ContractDefinition, Failure, OperationSchema,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub interface: String,
    pub operation: String,
    pub provider: String,
}
impl Selection {
    pub fn key(&self) -> String {
        format!("{}/{}", self.interface, self.operation)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopedGrant {
    pub caller: String,
    pub interface: String,
    pub operation: String,
    pub resources: BTreeSet<String>,
}
impl ScopedGrant {
    pub fn key(&self) -> String {
        format!("{}/{}", self.interface, self.operation)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Substitution {
    pub from: String,
    pub to: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlotBinding {
    pub caller: String,
    pub slot: String,
    pub index: u64,
    pub provider: String,
    pub export: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Composition {
    pub format: String,
    pub artifacts: Vec<String>,
    pub definitions: Vec<String>,
    pub entry: Selection,
    pub slots: Vec<SlotBinding>,
    pub grants: Vec<ScopedGrant>,
    pub excluded: BTreeSet<String>,
    pub substitutions: Vec<Substitution>,
    pub authority: Authority,
    pub resource: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CellOperation {
    Get,
    Set,
    Watch,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EffectTarget {
    Native,
    Cell {
        property: String,
        operation: CellOperation,
    },
}
#[derive(Clone, Debug)]
pub(crate) struct PreparedOperation {
    pub(crate) schema: OperationSchema,
    pub(crate) target: EffectTarget,
}
pub struct Prepared {
    pub(crate) packages: BTreeMap<String, VerifiedPackage>,
    pub(crate) entry: Selection,
    pub(crate) bindings: BTreeMap<(String, String), String>,
    pub(crate) grants: BTreeMap<(String, String), BTreeSet<String>>,
    pub(crate) authority: Authority,
    pub(crate) resource: Option<String>,
    pub(crate) slot_counts: BTreeMap<(String, String), u64>,
    pub(crate) state_identity: Vec<String>,
    pub(crate) graph: Option<crate::graph::CompiledGraph>,
    pub(crate) graph_v2: Option<crate::graph::v2::CompiledGraph>,
    pub(crate) startup_order: Vec<String>,
    pub(crate) operations: BTreeMap<(String, String), PreparedOperation>,
    pub(crate) ceilings: BTreeMap<String, BTreeSet<String>>,
}
fn error(code: Code, message: impl Into<String>) -> Failure {
    Failure::new(code, message)
}
fn invalid(message: &str) -> Failure {
    error(Code::DescriptorInvalid, message)
}
type Definitions = BTreeMap<(String, u64), ContractDefinition>;
fn validate_graph_profile(
    graph_v2_profile: bool,
    has_v1: bool,
    has_v2: bool,
) -> Result<(), Failure> {
    if graph_v2_profile {
        if has_v1 {
            return Err(invalid(
                "graph-v1 contributions are unsupported in graph-v2 preparation",
            ));
        }
        if !has_v2 {
            return Err(invalid(
                "graph-v2 preparation requires a verified contribution",
            ));
        }
    } else if has_v2 {
        return Err(invalid(
            "graph-v2 contributions require explicit graph-v2 preparation",
        ));
    }
    Ok(())
}
fn add_definition(
    definitions: &mut Definitions,
    definition: ContractDefinition,
) -> Result<(), Failure> {
    let identity = (definition.contract.clone(), definition.revision);
    if let Some(previous) = definitions.insert(identity, definition.clone()) {
        if previous != definition {
            return Err(invalid("conflicting contract definitions"));
        }
    }
    Ok(())
}
fn definition<'a>(
    definitions: &'a Definitions,
    contract: &str,
    revision: u64,
) -> Result<&'a ContractDefinition, Failure> {
    definitions
        .get(&(contract.into(), revision))
        .ok_or_else(|| {
            error(
                Code::MissingBinding,
                format!("missing contract definition {contract} revision {revision}"),
            )
        })
}
impl Prepared {
    pub fn prepare(composition: Composition) -> Result<Self, Failure> {
        Self::prepare_inner(composition, false)
    }
    /// Verify and retain an offline plan and trusted host policy in this preparation.
    /// The plan identity participates in this preparation's generation identity.
    /// This does not initialize native code or authorize graph-v2 execution.
    pub fn prepare_graph_v2(
        composition: Composition,
        policy: &crate::graph::v2::TrustedCompositionPolicy,
    ) -> Result<Self, Failure> {
        let mut prepared = Self::prepare_inner(composition, true)?;
        let declarations = prepared
            .packages
            .values()
            .filter_map(|package| package.graph_v2().cloned())
            .collect::<Vec<_>>();
        prepared.graph_v2 = Some(prepared.compile_graph_v2(&declarations, policy)?);
        Ok(prepared)
    }
    fn prepare_inner(
        mut composition: Composition,
        graph_v2_profile: bool,
    ) -> Result<Self, Failure> {
        if composition.format != "phenix.composition.v3"
            || !composition.authority.valid()
            || composition
                .resource
                .as_ref()
                .is_some_and(|id| !identifier(id))
        {
            return Err(invalid("unsupported composition or invalid root authority"));
        }
        if composition.artifacts.len() > phenix_contract::MAX_COUNT
            || composition.definitions.len() > phenix_contract::MAX_COUNT
            || composition.slots.len() > phenix_contract::MAX_COUNT
            || composition.grants.len() > phenix_contract::MAX_COUNT
        {
            return Err(invalid("composition count limit"));
        }
        composition.artifacts.sort();
        let mut packages = BTreeMap::new();
        for path in &composition.artifacts {
            let package = VerifiedPackage::verify_trusted_read_only(Path::new(path))?;
            if packages
                .insert(package.metadata().plugin.clone(), package)
                .is_some()
            {
                return Err(invalid("duplicate selected plugin identity"));
            }
        }
        for id in &composition.excluded {
            if !identifier(id) || packages.remove(id).is_none() {
                return Err(invalid("excluded plugin absent"));
            }
        }
        let has_v2 = packages
            .values()
            .any(|package| package.graph_v2().is_some());
        if !graph_v2_profile {
            validate_graph_profile(false, false, has_v2)?;
        }
        let mut replacements = BTreeMap::new();
        for substitution in composition.substitutions {
            if !identifier(&substitution.from)
                || substitution.from == substitution.to
                || !packages.contains_key(&substitution.to)
                || replacements
                    .insert(substitution.from, substitution.to)
                    .is_some()
            {
                return Err(invalid("invalid or duplicate substitution"));
            }
        }
        if replacements
            .values()
            .any(|id| replacements.contains_key(id))
        {
            return Err(invalid("chained substitution"));
        }
        let substitute = |id: &str| replacements.get(id).cloned().unwrap_or_else(|| id.into());
        composition.entry.provider = substitute(&composition.entry.provider);
        let mut definitions = Definitions::new();
        let mut owners: BTreeSet<_> = packages.keys().cloned().collect();
        let mut state_identity = BTreeSet::new();
        let mut graph_inputs = Vec::new();
        composition.definitions.sort();
        for path in composition.definitions {
            let package = VerifiedContractPackage::verify_trusted_read_only(Path::new(&path))?;
            let metadata = package.metadata();
            if !owners.insert(metadata.plugin.clone()) {
                return Err(invalid("duplicate definition package owner"));
            }
            state_identity.insert(format!("{}:{}", metadata.plugin, package.package_sha256()));
            if let Some(graph) = package.graph() {
                graph_inputs.push(graph.clone());
            }
            for d in &metadata.definitions {
                add_definition(&mut definitions, d.clone())?;
            }
        }
        for package in packages.values() {
            let metadata = package.metadata();
            state_identity.insert(format!(
                "{}:{}",
                metadata.plugin,
                package.record().descriptor_sha256
            ));
            for d in metadata
                .definitions
                .iter()
                .cloned()
                .chain(property_definitions(metadata)?)
            {
                add_definition(&mut definitions, d)?;
            }
        }
        validate_graph_profile(
            graph_v2_profile,
            !graph_inputs.is_empty() || packages.values().any(|package| package.graph().is_some()),
            has_v2,
        )?;
        let mut operations = BTreeMap::<(String, String), PreparedOperation>::new();
        let mut ceilings = BTreeMap::<String, BTreeSet<String>>::new();
        for (id, package) in &packages {
            let metadata = package.metadata();
            let ceiling = ceilings.entry(id.clone()).or_default();
            for d in &metadata.definitions {
                for op in &d.operations {
                    ceiling.insert(format!("{}/{}", d.contract, op.name));
                }
            }
            for export in &metadata.provides {
                let d = definition(&definitions, &export.contract, export.revision)?;
                for op in &d.operations {
                    let key = format!("{}/{}", d.contract, op.name);
                    ceiling.insert(key.clone());
                    if let Some(previous) = operations.insert(
                        (id.clone(), key),
                        PreparedOperation {
                            schema: op.clone(),
                            target: EffectTarget::Native,
                        },
                    ) {
                        if previous.schema != *op {
                            return Err(error(
                                Code::SchemaRejected,
                                "conflicting selected operation schemas",
                            ));
                        }
                    }
                }
            }
            for property in &metadata.properties {
                let contract = property_contract(id, &property.id)?;
                let d = definition(&definitions, &contract, 1)?;
                for op in &d.operations {
                    let operation = match op.name.as_str() {
                        "get" => CellOperation::Get,
                        "set" => CellOperation::Set,
                        "watch" => CellOperation::Watch,
                        _ => return Err(invalid("unknown property operation")),
                    };
                    let key = format!("{contract}/{}", op.name);
                    ceiling.insert(key.clone());
                    if operations
                        .insert(
                            (id.clone(), key),
                            PreparedOperation {
                                schema: op.clone(),
                                target: EffectTarget::Cell {
                                    property: property.id.clone(),
                                    operation,
                                },
                            },
                        )
                        .is_some()
                    {
                        return Err(invalid("property conflicts with native export"));
                    }
                }
            }
        }
        let mut selections = BTreeMap::<(String, String), BTreeMap<u64, SlotBinding>>::new();
        for mut selection in composition.slots {
            selection.provider = substitute(&selection.provider);
            if !identifier(&selection.slot)
                || !identifier(&selection.export)
                || !packages.get(&selection.caller).is_some_and(|p| {
                    p.metadata()
                        .requires
                        .iter()
                        .any(|r| r.slot == selection.slot)
                })
                || selections
                    .entry((selection.caller.clone(), selection.slot.clone()))
                    .or_default()
                    .insert(selection.index, selection)
                    .is_some()
            {
                return Err(invalid("invalid, undeclared or duplicate slot selection"));
            }
        }
        let mut bindings = BTreeMap::new();
        let mut slot_counts = BTreeMap::new();
        let mut predecessors: BTreeMap<String, BTreeSet<String>> = packages
            .keys()
            .cloned()
            .map(|id| (id, BTreeSet::new()))
            .collect();
        for (caller, package) in &packages {
            for import in &package.metadata().requires {
                let chosen = selections
                    .remove(&(caller.clone(), import.slot.clone()))
                    .unwrap_or_default();
                let count = chosen.len() as u64;
                let valid = match import.cardinality {
                    Cardinality::One => count == 1,
                    Cardinality::Optional => count <= 1,
                    Cardinality::Many { minimum } => count >= minimum,
                };
                if !valid || chosen.keys().copied().ne(0..count) {
                    return Err(error(
                        Code::MissingBinding,
                        "slot cardinality or noncontiguous indices",
                    ));
                }
                slot_counts.insert((caller.clone(), import.slot.clone()), count);
                let mut unique = BTreeSet::new();
                if count == 0 {
                    let d = definition(&definitions, &import.contract, import.revision_min)?;
                    for op in &d.operations {
                        ceilings
                            .get_mut(caller)
                            .unwrap()
                            .insert(format!("{}/{}", d.contract, op.name));
                    }
                }
                for (index, selected) in chosen {
                    if import
                        .provider
                        .as_ref()
                        .is_some_and(|provider| provider != &selected.provider)
                    {
                        return Err(error(
                            Code::MissingBinding,
                            "selected provider violates exact import identity",
                        ));
                    }
                    if !unique.insert((selected.provider.clone(), selected.export.clone())) {
                        return Err(invalid("duplicate provider/provide-slot selection"));
                    }
                    let provider = packages
                        .get(&selected.provider)
                        .ok_or_else(|| error(Code::MissingBinding, "selected provider absent"))?
                        .metadata();
                    let (contract, revision) = if let Some(export) =
                        provider.provides.iter().find(|e| e.slot == selected.export)
                    {
                        (export.contract.clone(), export.revision)
                    } else if let Some(property) = selected
                        .export
                        .strip_prefix("property.")
                        .and_then(|id| provider.properties.iter().find(|p| p.id == id))
                    {
                        (property_contract(&provider.plugin, &property.id)?, 1)
                    } else {
                        return Err(error(
                            Code::MissingBinding,
                            "selected provide slot is absent",
                        ));
                    };
                    if contract != import.contract
                        || revision < import.revision_min
                        || revision > import.revision_max
                    {
                        return Err(error(
                            Code::SchemaRejected,
                            "slot contract/revision mismatch",
                        ));
                    }
                    let d = definition(&definitions, &contract, revision)?;
                    let selector = if matches!(import.cardinality, Cardinality::Many { .. }) {
                        format!("{}#{index}", import.slot)
                    } else {
                        import.slot.clone()
                    };
                    for op in &d.operations {
                        let key = format!("{contract}/{}", op.name);
                        let prepared = operations
                            .get(&(selected.provider.clone(), key.clone()))
                            .ok_or_else(|| {
                                error(Code::MissingBinding, "selected operation absent")
                            })?;
                        if prepared.schema != *op {
                            return Err(error(Code::SchemaRejected, "selected schema mismatch"));
                        }
                        ceilings.get_mut(caller).unwrap().insert(key.clone());
                        bindings.insert(
                            (caller.clone(), import_identity(&selector, &key)),
                            selected.provider.clone(),
                        );
                        if import.cardinality != Cardinality::Optional
                            && prepared.target == EffectTarget::Native
                        {
                            predecessors
                                .get_mut(caller)
                                .unwrap()
                                .insert(selected.provider.clone());
                        }
                    }
                    state_identity.insert(format!(
                        "{caller}:{}:{index}:{}:{}:{revision}",
                        import.slot, selected.provider, selected.export
                    ));
                }
            }
        }
        // A selected implementation may advertise explicit downstream
        // delegation maxima without forcing its callers to depend on those
        // private contracts. Propagate *definitions only* along selected
        // import edges; mere artifact presence never expands an ancestor.
        // Runtime effects still require root authority, a caller grant,
        // resource scope, and every selected provider's own ceiling.
        let mut delegated_definitions = BTreeMap::<String, BTreeSet<String>>::new();
        for (id, package) in &packages {
            let mut keys = BTreeSet::new();
            for definition in &package.metadata().definitions {
                for operation in &definition.operations {
                    keys.insert(format!("{}/{}", definition.contract, operation.name));
                }
            }
            delegated_definitions.insert(id.clone(), keys);
        }
        let selected_edges: BTreeSet<(String, String)> = bindings
            .iter()
            .map(|((caller, _), provider)| (caller.clone(), provider.clone()))
            .collect();
        loop {
            let mut grew = false;
            for (caller, provider) in &selected_edges {
                let downstream = delegated_definitions
                    .get(provider)
                    .cloned()
                    .unwrap_or_default();
                let inherited = delegated_definitions
                    .get_mut(caller)
                    .expect("selected import caller has a loaded descriptor");
                let previous_len = inherited.len();
                inherited.extend(downstream);
                grew |= inherited.len() != previous_len;
            }
            if !grew {
                break;
            }
        }
        for (provider, extensions) in delegated_definitions {
            ceilings
                .get_mut(&provider)
                .expect("loaded provider has an operation ceiling")
                .extend(extensions);
        }
        let startup_order = crate::dag::order(&predecessors).map_err(|cycle| {
            error(
                Code::MissingBinding,
                format!("required dependency cycle: {}", cycle.join(" -> ")),
            )
        })?;
        let exports = |key: &str| operations.keys().any(|(_, operation)| operation == key);
        let mut grants = BTreeMap::new();
        for grant in composition.grants {
            let key = grant.key();
            if !operation_key(&key)
                || !exports(&key)
                || grant.resources.iter().any(|id| !identifier(id))
                || (grant.caller != "root"
                    && !ceilings
                        .get(&grant.caller)
                        .is_some_and(|ceiling| ceiling.contains(&key)))
                || grants
                    .insert((grant.caller, key), grant.resources)
                    .is_some()
            {
                return Err(invalid("invalid or duplicate operation grant"));
            }
        }
        if composition
            .authority
            .operations
            .iter()
            .any(|key| !exports(key))
        {
            return Err(invalid("root ceiling operation absent"));
        }
        graph_inputs.extend(
            packages
                .values()
                .filter_map(|package| package.graph().cloned()),
        );
        let mut prepared = Self {
            packages,
            entry: composition.entry,
            bindings,
            grants,
            authority: composition.authority,
            resource: composition.resource,
            slot_counts,
            state_identity: state_identity.into_iter().collect(),
            graph: None,
            graph_v2: None,
            startup_order,
            operations,
            ceilings,
        };
        prepared.check_entry(&prepared.entry)?;
        if !prepared
            .grants
            .contains_key(&("root".into(), prepared.entry.key()))
        {
            return Err(error(
                Code::Denied,
                "default root entry lacks admission grant",
            ));
        }
        if !graph_inputs.is_empty() {
            prepared.graph = Some(prepared.compile_graph(&graph_inputs)?);
        }
        Ok(prepared)
    }
    pub fn compiled_graph(&self) -> Option<&crate::graph::CompiledGraph> {
        self.graph.as_ref()
    }
    pub fn compiled_graph_v2(&self) -> Option<&crate::graph::v2::CompiledGraph> {
        self.graph_v2.as_ref()
    }
    /// Discover only actions whose declared admission requirements have root grants.
    pub fn discover_actions(&self) -> Vec<crate::graph::ActionInfo> {
        self.graph
            .iter()
            .flat_map(|graph| graph.discover())
            .filter(|action| {
                action.required_operations.iter().all(|key| {
                    self.authority.operations.contains(key)
                        && self.grants.contains_key(&("root".into(), key.clone()))
                })
            })
            .cloned()
            .collect()
    }
    pub(crate) fn permits(&self, provider: &str, key: &str) -> bool {
        self.ceilings
            .get(provider)
            .is_some_and(|ceiling| ceiling.contains(key))
    }
    pub(crate) fn operation(&self, selection: &Selection) -> Result<&PreparedOperation, Failure> {
        self.operations
            .get(&(selection.provider.clone(), selection.key()))
            .ok_or_else(|| error(Code::MissingBinding, "operation is not a prepared export"))
    }
    pub(crate) fn check_entry(&self, entry: &Selection) -> Result<(), Failure> {
        if !operation_key(&entry.key()) {
            return Err(error(Code::MissingBinding, "invalid root entry"));
        }
        self.operation(entry).map(|_| ())
    }
    pub fn inspect(&self) -> serde_json::Value {
        serde_json::json!({"format":"phenix.inspect.v2","plugins":self.packages.iter().map(|(id,p)|serde_json::json!({"id":id,"artifact_sha256":p.record().artifact_sha256,"descriptor_sha256":p.record().descriptor_sha256,"offline_verified":true,"abi_negotiated":false})).collect::<Vec<_>>(),"entry":self.entry,
        "bindings":self.bindings.iter().map(|((caller,key),provider)|serde_json::json!({"caller":caller,"operation":key,"provider":provider})).collect::<Vec<_>>(),"authority":self.authority,"resource":self.resource,
        "lifecycle":{"startup_order":self.startup_order,"stopped":false,"stop_failures":[]},"state":{"definition_inputs":self.state_identity,"slots":self.slot_counts.iter().map(|((caller,slot),count)|serde_json::json!({"caller":caller,"slot":slot,"count":count})).collect::<Vec<_>>()},
        "roots":{"active":null,"completed":0,"last_root_id":null},"calls":0,"denied_operations":0,"last_failure":null})
    }
}

#[cfg(test)]
mod tests {
    use super::validate_graph_profile;

    #[test]
    fn default_profile_preserves_v1_and_rejects_v2() {
        assert!(validate_graph_profile(false, false, false).is_ok());
        assert!(validate_graph_profile(false, true, false).is_ok());
        assert!(validate_graph_profile(false, false, true).is_err());
        assert!(validate_graph_profile(false, true, true).is_err());
    }

    #[test]
    fn v2_profile_requires_v2_and_rejects_all_v1_contributions() {
        assert!(validate_graph_profile(true, false, true).is_ok());
        assert!(validate_graph_profile(true, false, false).is_err());
        assert!(validate_graph_profile(true, true, false).is_err());
        // has_v1 includes metadata-only definition package graphs.
        assert!(validate_graph_profile(true, true, true).is_err());
    }
}
