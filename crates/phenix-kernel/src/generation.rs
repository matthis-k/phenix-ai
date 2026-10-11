//! Trusted generation selection; execution remains Kernel/Root.
use crate::{
    resolve::{Prepared, Selection},
    Kernel, Root,
};
use phenix_contract::{identifier, Code, Failure, ScopedRequest, Value};
use std::{collections::BTreeMap, rc::Rc};
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(transparent)]
pub struct GenerationId(String);
impl GenerationId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
fn strings(values: impl Iterator<Item = String>) -> Value {
    Value::List(values.map(Value::String).collect())
}
fn tuple(prepared: &Prepared, environment: &str) -> Result<Value, Failure> {
    if !identifier(environment) {
        return Err(Failure::new(
            Code::SchemaRejected,
            "invalid environment identity",
        ));
    }
    let artifacts = prepared
        .packages
        .iter()
        .map(|(id, p)| {
            let d = p.descriptor();
            let r = p.record();
            Value::List(vec![
                Value::String(id.clone()),
                Value::String(r.artifact_sha256.clone()),
                Value::String(r.descriptor_sha256.clone()),
                Value::String(r.target.clone()),
                Value::U64(d.abi_major.into()),
                Value::U64(d.abi_minor.into()),
                Value::U64(d.features.into()),
                Value::String(d.execution.clone()),
            ])
        })
        .collect();
    let bindings = prepared
        .bindings
        .iter()
        .map(|((caller, key), provider)| {
            strings([caller.clone(), key.clone(), provider.clone()].into_iter())
        })
        .collect();
    let grants = prepared
        .grants
        .iter()
        .map(|((caller, key), resources)| {
            Value::List(vec![
                Value::String(caller.clone()),
                Value::String(key.clone()),
                strings(resources.iter().cloned()),
            ])
        })
        .collect();
    Ok(Value::map([
        ("artifacts", Value::List(artifacts)),
        ("authority", prepared.authority.to_value()),
        ("bindings", Value::List(bindings)),
        (
            "entry",
            strings(
                [
                    prepared.entry.interface.clone(),
                    prepared.entry.operation.clone(),
                    prepared.entry.provider.clone(),
                ]
                .into_iter(),
            ),
        ),
        ("environment", Value::String(environment.into())),
        ("grants", Value::List(grants)),
        (
            "resource",
            prepared.resource.clone().map_or(Value::Null, Value::String),
        ),
    ]))
}
fn identity(tuple: &Value) -> Result<GenerationId, Failure> {
    let mut bytes = b"phenix.generation.v1\0".to_vec();
    bytes.extend(tuple.encode()?);
    Ok(GenerationId(phenix_artifact::digest(&bytes)))
}
impl Prepared {
    pub fn generation_identity(&self, environment: &str) -> Result<GenerationId, Failure> {
        identity(&tuple(self, environment)?)
    }
}
struct Generation {
    prepared: Option<Prepared>,
    runtime: Option<Rc<Kernel>>,
    canonical: Value,
    offline: serde_json::Value,
    failure: Option<Failure>,
}
#[derive(Default)]
pub struct Generations {
    default: Option<GenerationId>,
    generations: BTreeMap<GenerationId, Generation>,
}
#[derive(Debug, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Retirement {
    Retired,
    Blocked {
        reasons: Vec<String>,
        inspection: serde_json::Value,
    },
}
impl Generations {
    pub fn install_candidate(
        &mut self,
        prepared: Prepared,
        environment: &str,
    ) -> Result<GenerationId, Failure> {
        let canonical = tuple(&prepared, environment)?;
        let id = identity(&canonical)?;
        self.generations
            .entry(id.clone())
            .or_insert_with(|| Generation {
                offline: prepared.inspect(),
                prepared: Some(prepared),
                runtime: None,
                canonical,
                failure: None,
            });
        Ok(id)
    }
    fn activate(&mut self, id: &GenerationId) -> Result<(), Failure> {
        let g = self
            .generations
            .get_mut(id)
            .ok_or_else(|| Failure::new(Code::MissingBinding, "unknown generation"))?;
        if g.runtime.is_some() {
            return Ok(());
        }
        if let Some(failure) = &g.failure {
            return Err(failure.clone());
        }
        let prepared = g
            .prepared
            .take()
            .ok_or_else(|| Failure::new(Code::PluginFailed, "generation cannot activate"))?;
        match prepared.load() {
            Ok(kernel) => {
                g.runtime = Some(Rc::new(kernel));
                Ok(())
            }
            Err(failure) => {
                g.failure = Some(failure.clone());
                Err(failure)
            }
        }
    }
    /// Activate explicitly without selecting the admission default.
    pub fn trial(&mut self, id: &GenerationId) -> Result<(), Failure> {
        self.activate(id)
    }
    /// Load successfully before atomically assigning the default for new roots.
    pub fn promote(&mut self, id: &GenerationId) -> Result<GenerationId, Failure> {
        self.activate(id)?;
        self.default = Some(id.clone());
        Ok(id.clone())
    }
    pub fn default_identity(&self) -> Option<&GenerationId> {
        self.default.as_ref()
    }
    fn resident(&self, id: &GenerationId) -> Result<Rc<Kernel>, Failure> {
        self.generations
            .get(id)
            .and_then(|g| g.runtime.clone())
            .ok_or_else(|| {
                Failure::new(
                    Code::MissingBinding,
                    "generation is not resident; trial or promote first",
                )
            })
    }
    pub fn admit_default_root(&self) -> Result<Root<'static>, Failure> {
        let id = self
            .default
            .as_ref()
            .ok_or_else(|| Failure::new(Code::MissingBinding, "no default generation"))?;
        let kernel = self.resident(id)?;
        let root = kernel.admit_default_root()?;
        Ok(root.pin(kernel.clone(), id.as_str()))
    }
    pub fn admit_root_on(
        &self,
        id: &GenerationId,
        entry: Selection,
    ) -> Result<Root<'static>, Failure> {
        let kernel = self.resident(id)?;
        let root = kernel.admit_root(entry)?;
        Ok(root.pin(kernel.clone(), id.as_str()))
    }
    pub fn admit_scoped_root_on(
        &self,
        id: &GenerationId,
        entry: Selection,
        scope: ScopedRequest,
    ) -> Result<Root<'static>, Failure> {
        let kernel = self.resident(id)?;
        let root = kernel.admit_scoped_root(entry, scope)?;
        Ok(root.pin(kernel.clone(), id.as_str()))
    }
    pub fn drain_cancelled(&self, id: &GenerationId) -> Result<usize, Failure> {
        self.resident(id)?.drain_cancelled()
    }
    pub fn inspect_candidate(&self, id: &GenerationId) -> Result<serde_json::Value, Failure> {
        let g = self
            .generations
            .get(id)
            .ok_or_else(|| Failure::new(Code::MissingBinding, "unknown generation"))?;
        let report = g
            .runtime
            .as_ref()
            .map_or_else(|| g.offline.clone(), |k| k.inspect());
        Ok(
            serde_json::json!({"identity":id,"state":if g.runtime.is_some(){"resident"}else if g.failure.is_some(){"failed"}else{"candidate"},"canonical":g.canonical.to_json(),"failure":g.failure,"runtime":report,"pins":{"roots":u64::from(g.runtime.as_ref().is_some_and(|k|k.state.borrow().active_root.is_some())),"physical_leases":g.runtime.as_ref().map_or(0,|k|k.state.borrow().tickets.values().filter(|r|!r.quiescent).count())},"retirement_blockers":self.blockers(id)}),
        )
    }
    fn blockers(&self, id: &GenerationId) -> Vec<String> {
        let mut reasons = Vec::new();
        if self.default.as_ref() == Some(id) {
            reasons.push("selected default".into());
        }
        if let Some(kernel) = self.generations.get(id).and_then(|g| g.runtime.as_ref()) {
            let state = kernel.state.borrow();
            if state.active_root.is_some() {
                reasons.push("pinned root admission".into());
            }
            for (ticket, record) in &state.tickets {
                if !record.quiescent {
                    reasons.push(format!(
                        "physical ticket {ticket} provider {}",
                        record.selection.provider
                    ));
                }
            }
        }
        reasons
    }
    pub fn inspect(&self) -> serde_json::Value {
        serde_json::json!({"format":"phenix.generations.v1","default":self.default,"generations":self.generations.keys().map(|id|self.inspect_candidate(id).unwrap()).collect::<Vec<_>>()})
    }
    pub fn retire(&mut self, id: &GenerationId) -> Result<Retirement, Failure> {
        let inspection = self.inspect_candidate(id)?;
        let reasons = self.blockers(id);
        if !reasons.is_empty() {
            return Ok(Retirement::Blocked {
                reasons,
                inspection,
            });
        }
        let g = self.generations.remove(id).unwrap();
        if let Some(kernel) = g.runtime {
            kernel.shutdown()?;
        }
        Ok(Retirement::Retired)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn value(v: &serde_json::Value) -> Value {
        match v {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(v) => Value::Bool(*v),
            serde_json::Value::Number(v) => Value::U64(v.as_u64().unwrap()),
            serde_json::Value::String(v) => Value::String(v.clone()),
            serde_json::Value::Array(v) => Value::List(v.iter().map(value).collect()),
            serde_json::Value::Object(v) => {
                Value::Map(v.iter().map(|(k, v)| (k.clone(), value(v))).collect())
            }
        }
    }
    #[test]
    fn canonical_hash_matches_independent_frozen_vector_and_every_field_matters() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/generation-identity-v1.json"))
                .unwrap();
        let canonical = value(&fixture["tuple"]);
        let encoded: String = canonical
            .encode()
            .unwrap()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(encoded, fixture["bytes_hex"].as_str().unwrap());
        assert_eq!(
            identity(&canonical).unwrap().as_str(),
            fixture["sha256"].as_str().unwrap()
        );
        if let Value::Map(fields) = canonical {
            for key in fields.keys() {
                let mut variant = fields.clone();
                variant.insert(key.clone(), Value::Null);
                assert_ne!(
                    identity(&Value::Map(variant)).unwrap().as_str(),
                    fixture["sha256"].as_str().unwrap()
                );
            }
        }
    }
}
