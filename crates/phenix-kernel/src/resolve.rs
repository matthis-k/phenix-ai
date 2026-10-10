use phenix_artifact::VerifiedPackage;
use phenix_contract::{operation_key, Code, Contract, Failure};
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
pub struct Prepared {
    pub(crate) packages: BTreeMap<String, VerifiedPackage>,
    pub(crate) entry: Selection,
    pub(crate) bindings: BTreeMap<String, String>,
    pub(crate) grants: BTreeSet<(String, String)>,
}
fn schema_matches(a: &Contract, b: &Contract) -> bool {
    a.key == b.key && a.request == b.request && a.response == b.response && a.error == b.error
}
impl Prepared {
    pub fn prepare(deployment: Deployment) -> Result<Self, Failure> {
        if deployment.format != "phenix.deployment.v1" {
            return Err(Failure::new(
                Code::DescriptorInvalid,
                "unsupported deployment profile",
            ));
        }
        let mut packages = BTreeMap::new();
        // Complete offline verification of every installed package precedes loading.
        for path in &deployment.artifacts {
            let p = VerifiedPackage::verify_trusted_read_only(Path::new(path))?;
            if packages
                .insert(p.descriptor().plugin_id.clone(), p)
                .is_some()
            {
                return Err(Failure::new(
                    Code::DescriptorInvalid,
                    "duplicate selected plugin identity",
                ));
            }
        }
        let mut bindings = BTreeMap::new();
        for b in deployment.bindings {
            if !operation_key(&b.key()) || bindings.insert(b.key(), b.provider).is_some() {
                return Err(Failure::new(
                    Code::DescriptorInvalid,
                    "invalid or duplicate explicit binding",
                ));
            }
        }
        let exports = |key: &str| {
            packages
                .iter()
                .filter(|(_, p)| p.descriptor().exports.iter().any(|x| x.operation == key))
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>()
        };
        for p in packages.values() {
            for import in &p.descriptor().imports {
                let expected = p
                    .descriptor()
                    .contract(&import.operation)
                    .ok_or_else(|| Failure::new(Code::DescriptorInvalid, "import schema absent"))?;
                let available = exports(&import.operation);
                let Some(selected) = bindings.get(&import.operation) else {
                    let eligible = available
                        .iter()
                        .filter(|id| {
                            packages[*id]
                                .descriptor()
                                .contract(&import.operation)
                                .is_some_and(|actual| schema_matches(expected, actual))
                        })
                        .count();
                    let code = if eligible > 1 {
                        Code::AmbiguousBinding
                    } else {
                        Code::MissingBinding
                    };
                    return Err(Failure::new(
                        code,
                        format!(
                            "{} requires explicit selection for {}",
                            p.descriptor().plugin_id,
                            import.operation
                        ),
                    ));
                };
                if !available.contains(selected) {
                    return Err(Failure::new(
                        Code::MissingBinding,
                        format!(
                            "selected provider {selected} does not export {}",
                            import.operation
                        ),
                    ));
                }
                let actual = packages[selected]
                    .descriptor()
                    .contract(&import.operation)
                    .ok_or_else(|| Failure::new(Code::DescriptorInvalid, "export schema absent"))?;
                if !schema_matches(expected, actual) {
                    return Err(Failure::new(
                        Code::SchemaRejected,
                        format!("incompatible import {}", import.operation),
                    ));
                }
            }
        }
        for (key, provider) in &bindings {
            if !exports(key).contains(provider)
                || !packages
                    .values()
                    .any(|p| p.descriptor().imports.iter().any(|x| &x.operation == key))
            {
                return Err(Failure::new(
                    Code::MissingBinding,
                    "binding is not an installed import/export pair",
                ));
            }
        }
        let mut grants = BTreeSet::new();
        for g in deployment.grants {
            let key = g.key();
            if !operation_key(&key)
                || !packages
                    .values()
                    .any(|p| p.descriptor().exports.iter().any(|x| x.operation == key))
                || (g.caller != "root"
                    && !packages
                        .get(&g.caller)
                        .is_some_and(|p| p.descriptor().imports.iter().any(|x| x.operation == key)))
                || !grants.insert((g.caller, key))
            {
                return Err(Failure::new(
                    Code::DescriptorInvalid,
                    "invalid or duplicate operation grant",
                ));
            }
        }
        let prepared = Self {
            packages,
            entry: deployment.entry,
            bindings,
            grants,
        };
        prepared.check_entry(&prepared.entry)?;
        if !prepared
            .grants
            .contains(&("root".into(), prepared.entry.key()))
        {
            return Err(Failure::new(
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
            return Err(Failure::new(
                Code::MissingBinding,
                "explicit root entry absent",
            ));
        }
        Ok(())
    }
    pub fn inspect(&self) -> serde_json::Value {
        serde_json::json!({"format":"phenix.inspect.v1","plugins":self.packages.iter().map(|(id,p)|serde_json::json!({"id":id,"artifact_sha256":p.record().artifact_sha256,"descriptor_sha256":p.record().descriptor_sha256,"offline_verified":true,"abi_negotiated":false})).collect::<Vec<_>>(),"entry":self.entry,"bindings":self.bindings,"roots":{"active":null,"completed":0,"last_root_id":null},"calls":0,"denied_operations":0,"last_failure":null})
    }
}
