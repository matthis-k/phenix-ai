use crate::state_resolve::normalize_state;
use phenix_artifact::VerifiedPackage;
use phenix_contract::{identifier, operation_key, Authority, Code, Contract, Failure};
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
pub struct Grant {
    pub caller: String,
    pub interface: String,
    pub operation: String,
}
impl Grant {
    pub fn key(&self) -> String {
        format!("{}/{}", self.interface, self.operation)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deployment {
    pub format: String,
    pub artifacts: Vec<String>,
    pub entry: Selection,
    pub bindings: Vec<Selection>,
    pub grants: Vec<Grant>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportBinding {
    pub caller: String,
    pub interface: String,
    pub operation: String,
    pub provider: String,
}
impl ImportBinding {
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
pub struct Composition {
    pub format: String,
    pub artifacts: Vec<String>,
    pub entry: Selection,
    pub bindings: Vec<ImportBinding>,
    pub grants: Vec<ScopedGrant>,
    pub excluded: BTreeSet<String>,
    pub substitutions: Vec<Substitution>,
    pub authority: Authority,
    pub resource: Option<String>,
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
pub struct StateComposition {
    pub format: String,
    pub composition: Composition,
    pub definitions: Vec<String>,
    pub slots: Vec<SlotBinding>,
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
    composition: bool,
    pub(crate) startup_order: Vec<String>,
}
struct Policy {
    artifacts: Vec<String>,
    entry: Selection,
    bindings: Vec<ImportBinding>,
    global: Vec<Selection>,
    grants: Vec<ScopedGrant>,
    excluded: BTreeSet<String>,
    substitutions: Vec<Substitution>,
    authority: Authority,
    resource: Option<String>,
    composition: bool,
}
fn error(code: Code, message: impl Into<String>) -> Failure {
    Failure::new(code, message)
}
fn invalid(message: &str) -> Failure {
    error(Code::DescriptorInvalid, message)
}
fn schema_matches(a: &Contract, b: &Contract) -> bool {
    a.key == b.key && a.request == b.request && a.response == b.response && a.error == b.error
}
impl Prepared {
    pub fn prepare(d: Deployment) -> Result<Self, Failure> {
        if d.format != "phenix.deployment.v1" {
            return Err(invalid("unsupported deployment profile"));
        }
        let authority = Authority {
            operations: d.grants.iter().map(Grant::key).collect(),
            resources: BTreeSet::new(),
        };
        Self::resolve(
            Policy {
                artifacts: d.artifacts,
                entry: d.entry,
                global: d.bindings,
                bindings: vec![],
                grants: d
                    .grants
                    .into_iter()
                    .map(|g| ScopedGrant {
                        caller: g.caller,
                        interface: g.interface,
                        operation: g.operation,
                        resources: BTreeSet::new(),
                    })
                    .collect(),
                excluded: BTreeSet::new(),
                substitutions: vec![],
                authority,
                resource: None,
                composition: false,
            },
            vec![],
            vec![],
        )
    }
    pub fn prepare_composition(c: Composition) -> Result<Self, Failure> {
        Self::resolve(Self::composition_policy(c)?, vec![], vec![])
    }
    pub fn prepare_state(c: StateComposition) -> Result<Self, Failure> {
        if c.format != "phenix.state-composition.v2" {
            return Err(invalid("unsupported state composition profile"));
        }
        Self::resolve(
            Self::composition_policy(c.composition)?,
            c.definitions,
            c.slots,
        )
    }
    fn composition_policy(c: Composition) -> Result<Policy, Failure> {
        if c.format != "phenix.composition.v1" {
            return Err(invalid("unsupported composition profile"));
        }
        Ok(Policy {
            artifacts: c.artifacts,
            entry: c.entry,
            bindings: c.bindings,
            global: vec![],
            grants: c.grants,
            excluded: c.excluded,
            substitutions: c.substitutions,
            authority: c.authority,
            resource: c.resource,
            composition: true,
        })
    }
    // One preparation/resolution implementation for both published profiles.
    fn resolve(
        mut policy: Policy,
        definition_paths: Vec<String>,
        slots: Vec<SlotBinding>,
    ) -> Result<Self, Failure> {
        if !policy.authority.valid() || policy.resource.as_ref().is_some_and(|id| !identifier(id)) {
            return Err(invalid("invalid root authority"));
        }
        policy.artifacts.sort();
        let mut packages = BTreeMap::new();
        for path in &policy.artifacts {
            let p = VerifiedPackage::verify_trusted_read_only(Path::new(path))?;
            if packages
                .insert(p.descriptor().plugin_id.clone(), p)
                .is_some()
            {
                return Err(invalid("duplicate selected plugin identity"));
            }
        }
        for id in &policy.excluded {
            if !identifier(id) || packages.remove(id).is_none() {
                return Err(invalid("excluded plugin absent"));
            }
        }
        let mut substitutions = BTreeMap::new();
        policy
            .substitutions
            .sort_by(|a, b| a.from.cmp(&b.from).then(a.to.cmp(&b.to)));
        for s in policy.substitutions {
            if !identifier(&s.from)
                || s.from == s.to
                || !packages.contains_key(&s.to)
                || substitutions.insert(s.from, s.to).is_some()
            {
                return Err(invalid("invalid or duplicate substitution"));
            }
        }
        if substitutions
            .values()
            .any(|to| substitutions.contains_key(to))
        {
            return Err(invalid("chained or cyclic substitution"));
        }
        let substitute = |id: &str| substitutions.get(id).cloned().unwrap_or_else(|| id.into());
        let (mut bindings, slot_counts, state_identity) = normalize_state(
            &mut packages,
            definition_paths,
            slots
                .into_iter()
                .map(|mut s| {
                    s.provider = substitute(&s.provider);
                    s
                })
                .collect(),
        )?;
        let exports = |key: &str| {
            packages
                .iter()
                .filter(|(_, p)| p.descriptor().exports.iter().any(|x| x.operation == key))
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>()
        };
        policy.entry.provider = substitute(&policy.entry.provider);
        let mut global = BTreeMap::new();
        policy.global.sort_by_key(Selection::key);
        for b in policy.global {
            if !operation_key(&b.key()) || global.insert(b.key(), b.provider).is_some() {
                return Err(invalid("invalid or duplicate explicit binding"));
            }
        }
        // Expand the minimal global selections once. Dispatch never resolves.
        for (key, provider) in &global {
            let mut used = false;
            for (caller, p) in &packages {
                if p.descriptor()
                    .imports
                    .iter()
                    .any(|x| x.binding_key() == *key)
                {
                    let (interface, operation) = key.split_once('/').unwrap();
                    policy.bindings.push(ImportBinding {
                        caller: caller.clone(),
                        interface: interface.into(),
                        operation: operation.into(),
                        provider: provider.clone(),
                    });
                    used = true;
                }
            }
            if !used {
                return Err(error(
                    Code::MissingBinding,
                    "binding is not an installed import/export pair",
                ));
            }
        }
        policy.bindings.sort_by(|a, b| {
            (&a.caller, a.key(), &a.provider).cmp(&(&b.caller, b.key(), &b.provider))
        });
        for b in policy.bindings {
            let key = b.key();
            let provider = substitute(&b.provider);
            if !operation_key(&key)
                || !identifier(&b.caller)
                || !identifier(&provider)
                || bindings.insert((b.caller, key), provider).is_some()
            {
                return Err(invalid("invalid or duplicate explicit binding"));
            }
        }
        for (caller, p) in &packages {
            for import in &p.descriptor().imports {
                let expected = p
                    .descriptor()
                    .contract(&import.operation)
                    .ok_or_else(|| invalid("import schema absent"))?;
                let available = exports(&import.operation);
                let Some(selected) = bindings.get(&(caller.clone(), import.binding_key())) else {
                    if import.optional {
                        continue;
                    }
                    let eligible = available
                        .iter()
                        .filter(|id| {
                            packages[*id]
                                .descriptor()
                                .contract(&import.operation)
                                .is_some_and(|actual| schema_matches(expected, actual))
                        })
                        .count();
                    return Err(error(
                        if eligible > 1 {
                            Code::AmbiguousBinding
                        } else {
                            Code::MissingBinding
                        },
                        format!(
                            "{caller} requires explicit selection for {}",
                            import.operation
                        ),
                    ));
                };
                if !available.contains(selected) {
                    return Err(error(
                        Code::MissingBinding,
                        format!(
                            "selected provider {selected} does not export {}",
                            import.operation
                        ),
                    ));
                }
                if import
                    .implementation
                    .as_ref()
                    .is_some_and(|id| id != selected)
                {
                    return Err(error(
                        Code::MissingBinding,
                        format!(
                            "{caller} requires exact implementation for {}",
                            import.operation
                        ),
                    ));
                }
                let actual = packages[selected]
                    .descriptor()
                    .contract(&import.operation)
                    .ok_or_else(|| invalid("export schema absent"))?;
                if !schema_matches(expected, actual) {
                    return Err(error(
                        Code::SchemaRejected,
                        format!("incompatible import {}", import.operation),
                    ));
                }
            }
        }
        for ((caller, key), provider) in &bindings {
            let actual = key.split_once(':').map_or(key.as_str(), |(_, key)| key);
            if !exports(actual).contains(provider)
                || !packages.get(caller).is_some_and(|p| {
                    p.descriptor()
                        .imports
                        .iter()
                        .any(|x| x.binding_key() == *key)
                })
            {
                return Err(error(
                    Code::MissingBinding,
                    "binding is not an installed import/export pair",
                ));
            }
        }
        let startup_order = if policy.composition {
            dependency_order(&packages, &bindings)?
        } else {
            packages.keys().cloned().collect()
        };
        let mut grants = BTreeMap::new();
        policy
            .grants
            .sort_by(|a, b| (&a.caller, a.key()).cmp(&(&b.caller, b.key())));
        for g in policy.grants {
            let key = g.key();
            if !operation_key(&key)
                || exports(&key).is_empty()
                || g.resources.iter().any(|id| !identifier(id))
                || (g.caller != "root"
                    && !packages.get(&g.caller).is_some_and(|p| {
                        p.descriptor().imports.iter().any(|x| x.operation == key)
                            || (p.descriptor().abi_major == 3
                                && p.descriptor().exports.iter().any(|x| x.operation == key))
                    }))
                || grants.insert((g.caller, key), g.resources).is_some()
            {
                return Err(invalid("invalid or duplicate operation grant"));
            }
        }
        if policy
            .authority
            .operations
            .iter()
            .any(|key| exports(key).is_empty())
        {
            return Err(invalid("root ceiling operation absent"));
        }
        let prepared = Self {
            packages,
            slot_counts,
            state_identity,
            entry: policy.entry,
            bindings,
            grants,
            authority: policy.authority,
            resource: policy.resource,
            composition: policy.composition,
            startup_order,
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
        Ok(prepared)
    }
    pub(crate) fn check_entry(&self, entry: &Selection) -> Result<(), Failure> {
        if !operation_key(&entry.key())
            || !self.packages.get(&entry.provider).is_some_and(|p| {
                p.descriptor()
                    .exports
                    .iter()
                    .any(|x| x.operation == entry.key())
            })
        {
            return Err(error(Code::MissingBinding, "explicit root entry absent"));
        }
        Ok(())
    }
    pub fn inspect(&self) -> serde_json::Value {
        let bindings = if self.composition {
            serde_json::json!(self.bindings.iter().map(|((caller,key),provider)|serde_json::json!({"caller":caller,"operation":key,"provider":provider})).collect::<Vec<_>>())
        } else {
            serde_json::json!(self
                .bindings
                .iter()
                .map(|((_, key), provider)| (key, provider))
                .collect::<BTreeMap<_, _>>())
        };
        let mut result = serde_json::json!({"format":"phenix.inspect.v1","plugins":self.packages.iter().map(|(id,p)|serde_json::json!({"id":id,"artifact_sha256":p.record().artifact_sha256,"descriptor_sha256":p.record().descriptor_sha256,"offline_verified":true,"abi_negotiated":false})).collect::<Vec<_>>(),"entry":self.entry,"bindings":bindings,"roots":{"active":null,"completed":0,"last_root_id":null},"calls":0,"denied_operations":0,"last_failure":null});
        if self.composition {
            result["format"] = "phenix.inspect.v2".into();
            result["authority"] = serde_json::json!(self.authority);
            result["resource"] = serde_json::json!(self.resource);
            result["lifecycle"] = serde_json::json!({"startup_order":self.startup_order,"stopped":false,"stop_failures":[]});
        }
        if !self.state_identity.is_empty() {
            result["state"] = serde_json::json!({"definition_inputs":self.state_identity,"slots":self.slot_counts.iter().map(|((caller,slot),count)|serde_json::json!({"caller":caller,"slot":slot,"count":count})).collect::<Vec<_>>()});
        }
        result
    }
}
fn dependency_order(
    packages: &BTreeMap<String, VerifiedPackage>,
    bindings: &BTreeMap<(String, String), String>,
) -> Result<Vec<String>, Failure> {
    fn visit(
        id: &str,
        packages: &BTreeMap<String, VerifiedPackage>,
        bindings: &BTreeMap<(String, String), String>,
        done: &mut BTreeSet<String>,
        path: &mut Vec<String>,
        order: &mut Vec<String>,
    ) -> Result<(), Failure> {
        if let Some(at) = path.iter().position(|x| x == id) {
            let mut cycle = path[at..].to_vec();
            cycle.push(id.into());
            return Err(error(
                Code::MissingBinding,
                format!("required dependency cycle: {}", cycle.join(" -> ")),
            ));
        }
        if done.contains(id) {
            return Ok(());
        }
        path.push(id.into());
        for import in &packages[id].descriptor().imports {
            if !import.optional {
                visit(
                    &bindings[&(id.into(), import.binding_key())],
                    packages,
                    bindings,
                    done,
                    path,
                    order,
                )?;
            }
        }
        path.pop();
        done.insert(id.into());
        order.push(id.into());
        Ok(())
    }
    let mut done = BTreeSet::new();
    let mut order = Vec::new();
    for id in packages.keys() {
        visit(id, packages, bindings, &mut done, &mut vec![], &mut order)?;
    }
    Ok(order)
}
