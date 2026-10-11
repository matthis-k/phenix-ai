//! Normalization within the existing preparation pass. Never loads code.
use crate::resolve::SlotBinding;
use phenix_artifact::{property_definitions, VerifiedContractPackage, VerifiedPackage};
use phenix_contract::{import_identity, Cardinality, Code, ContractDefinition, Failure};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

type Bindings = BTreeMap<(String, String), String>;
type Counts = BTreeMap<(String, String), u64>;
type Definitions = BTreeMap<(String, u64), ContractDefinition>;
fn invalid(message: &str) -> Failure {
    Failure::new(Code::DescriptorInvalid, message)
}
fn add(definitions: &mut Definitions, d: ContractDefinition) -> Result<(), Failure> {
    let key = (d.contract.clone(), d.revision);
    if let Some(previous) = definitions.insert(key, d.clone()) {
        if previous != d {
            return Err(invalid("conflicting contract definitions"));
        }
    }
    Ok(())
}
pub(crate) fn normalize_state(
    packages: &mut BTreeMap<String, VerifiedPackage>,
    mut paths: Vec<String>,
    slots: Vec<SlotBinding>,
) -> Result<(Bindings, Counts, Vec<String>), Failure> {
    if slots.len() > phenix_contract::MAX_COUNT || paths.len() > phenix_contract::MAX_COUNT {
        return Err(invalid("state composition count limit"));
    }
    let mut definitions = Definitions::new();
    let mut identity = BTreeSet::new();
    let mut owners: BTreeSet<_> = packages.keys().cloned().collect();
    paths.sort();
    for path in paths {
        let package = VerifiedContractPackage::verify_trusted_read_only(Path::new(&path))?;
        let metadata = package.metadata();
        if !owners.insert(metadata.plugin.clone()) {
            return Err(invalid("duplicate definition package owner"));
        }
        identity.insert(format!("{}:{}", metadata.plugin, package.package_sha256()));
        for d in &metadata.definitions {
            add(&mut definitions, d.clone())?;
        }
    }
    for p in packages.values() {
        if let Some(m) = p.metadata() {
            identity.insert(format!("{}:{}", m.plugin, p.record().descriptor_sha256));
            for d in m
                .definitions
                .iter()
                .cloned()
                .chain(property_definitions(m)?)
            {
                add(&mut definitions, d)?;
            }
        }
    }
    let mut selections: BTreeMap<(String, String), BTreeMap<u64, SlotBinding>> = BTreeMap::new();
    for s in slots {
        if !phenix_contract::identifier(&s.slot)
            || !phenix_contract::identifier(&s.export)
            || !packages
                .get(&s.caller)
                .and_then(|p| p.metadata())
                .is_some_and(|m| m.requires.iter().any(|r| r.slot == s.slot))
            || selections
                .entry((s.caller.clone(), s.slot.clone()))
                .or_default()
                .insert(s.index, s)
                .is_some()
        {
            return Err(invalid("invalid, undeclared or duplicate slot selection"));
        }
    }
    let mut bindings = Bindings::new();
    let mut counts = Counts::new();
    // Resolve all metadata from the immutable collection before mutating package views.
    let mut normalized = BTreeMap::new();
    for (caller, p) in packages.iter() {
        let Some(m) = p.metadata() else {
            continue;
        };
        let mut imports = vec![];
        for r in &m.requires {
            let chosen = selections
                .remove(&(caller.clone(), r.slot.clone()))
                .unwrap_or_default();
            let count = chosen.len() as u64;
            let valid = match r.cardinality {
                Cardinality::One => count == 1,
                Cardinality::Optional => count <= 1,
                Cardinality::Many { minimum } => count >= minimum,
            };
            if !valid || chosen.keys().copied().ne(0..count) {
                return Err(Failure::new(
                    Code::MissingBinding,
                    "slot cardinality or noncontiguous indices",
                ));
            }
            counts.insert((caller.clone(), r.slot.clone()), count);
            if count == 0 && r.cardinality == Cardinality::Optional {
                imports.push(r.clone());
            }
            let mut unique = BTreeSet::new();
            for (index, s) in chosen {
                if !unique.insert((s.provider.clone(), s.export.clone())) {
                    return Err(invalid("duplicate provider/provide-slot selection"));
                }
                let provider = packages
                    .get(&s.provider)
                    .and_then(|p| p.metadata())
                    .ok_or_else(|| invalid("slot provider is not an M1 package"))?;
                let (contract, revision) =
                    if let Some(e) = provider.provides.iter().find(|e| e.slot == s.export) {
                        (e.contract.clone(), e.revision)
                    } else if let Some(prop) = s
                        .export
                        .strip_prefix("property.")
                        .and_then(|name| provider.properties.iter().find(|p| p.id == name))
                    {
                        (
                            phenix_artifact::property_contract(&provider.plugin, &prop.id)?,
                            1,
                        )
                    } else {
                        return Err(Failure::new(
                            Code::MissingBinding,
                            "selected provide slot is absent",
                        ));
                    };
                if contract != r.contract || revision < r.revision_min || revision > r.revision_max
                {
                    return Err(Failure::new(
                        Code::SchemaRejected,
                        "slot contract/revision mismatch",
                    ));
                }
                let selector = if matches!(r.cardinality, Cardinality::Many { .. }) {
                    format!("{}#{index}", r.slot)
                } else {
                    r.slot.clone()
                };
                let definition =
                    definitions
                        .get(&(contract.clone(), revision))
                        .ok_or_else(|| {
                            Failure::new(
                                Code::MissingBinding,
                                "selected contract definition absent",
                            )
                        })?;
                for op in &definition.operations {
                    bindings.insert(
                        (
                            caller.clone(),
                            import_identity(&selector, &format!("{contract}/{}", op.name)),
                        ),
                        s.provider.clone(),
                    );
                }
                let mut import = r.clone();
                import.slot = selector;
                import.revision_min = revision;
                import.revision_max = revision;
                import.cardinality = Cardinality::One;
                imports.push(import);
                identity.insert(format!(
                    "{caller}:{}:{index}:{}:{}:{revision}",
                    r.slot, s.provider, s.export
                ));
            }
        }
        normalized.insert(caller.clone(), imports);
    }
    for (caller, imports) in normalized {
        packages
            .get_mut(&caller)
            .unwrap()
            .normalize(imports, &definitions)?;
    }
    Ok((bindings, counts, identity.into_iter().collect()))
}
