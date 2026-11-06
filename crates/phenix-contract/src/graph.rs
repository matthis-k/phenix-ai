//! One portable graph contribution vocabulary. All graph builds start with an empty patch set.
//! This is deliberately independent of executable Rust closures and native plugin loading.
use crate::{Code, Failure, Value, ValueSchema};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

fn bad(why: impl Into<String>) -> Failure { Failure::new(Code::DescriptorInvalid, why) }

/// A canonical schema descriptor, never a local Rust TypeId.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Schema(pub Vec<u8>);
impl Schema {
    pub fn of(schema: &ValueSchema) -> Result<Self, Failure> { Ok(Self(schema.encode()?)) }
    pub fn decode(&self) -> Result<ValueSchema, Failure> { ValueSchema::decode(&self.0) }
    pub fn accepts(&self, value: &Value) -> Result<bool, Failure> {
        Ok(self.decode()?.accepts(value))
    }
}

/// Absolute module-scoped paths are normalized by assembly, not by the executor.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputRef { pub node: String, pub port: String }
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputRef { pub node: String, pub port: String, pub slot: String }
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edge { pub from: OutputRef, pub to: InputRef }

/// A policy of an *output port*, not of a node action or an individual edge.
/// The runtime may choose zero or more of the port's declared edges.
/// Select is a symbolic host-bound policy: it cannot grant effect authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutputPolicy {
    /// Default: send each emission to every connected destination.
    Broadcast,
    /// Invoke the registered policy to choose a subset of connected destinations.
    /// Runtime implementations may inspect the emitted value or authorized store.
    Select { selector: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Readiness { All, Any }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputPort {
    pub slots: BTreeMap<String, Schema>,
    pub readiness: Readiness,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub path: String,
    pub inputs: BTreeMap<String, InputPort>,
    pub outputs: BTreeMap<String, Schema>,
    /// Host-resolved operation reference; this string cannot carry authority.
    pub action: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trigger { pub name: String, pub to: InputRef }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", content = "data", rename_all = "snake_case")]
pub enum Edit {
    AddNode(Node),
    ReplaceNode(Node),
    RemoveNode(String),
    Connect(Edge),
    Disconnect(Edge),
    SetOutputPolicy { from: OutputRef, policy: OutputPolicy },
    AddTrigger(Trigger),
    RemoveTrigger(String),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    pub id: String,
    #[serde(default)]
    pub after: BTreeSet<String>,
    #[serde(default)]
    pub priority: Option<i32>,
    pub edits: Vec<Edit>,
}
impl Patch {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into(), after: BTreeSet::new(), priority: None, edits: Vec::new() }
    }
    pub fn after(mut self, id: impl Into<String>) -> Self { self.after.insert(id.into()); self }
    pub fn push(&mut self, edit: Edit) { self.edits.push(edit); }
    /// Set routing on a port without modifying its producer node.
    pub fn output_policy(&mut self, from: OutputRef, policy: OutputPolicy) {
        self.push(Edit::SetOutputPolicy { from, policy });
    }
    pub fn validate(&self) -> Result<(), Failure> {
        if self.id.is_empty() || self.id.len() > 512 || self.after.contains(&self.id) {
            return Err(bad("invalid patch identity or self-dependency"));
        }
        if self.edits.len() > 65536 || self.after.len() > 4096 {
            return Err(bad("patch descriptor limit"));
        }
        for edit in &self.edits {
            match edit {
                Edit::AddNode(n) | Edit::ReplaceNode(n) => {
                    if n.path.is_empty() || n.action.is_empty() || n.inputs.is_empty() || n.outputs.is_empty() {
                        return Err(bad("node must declare path, action, inputs and outputs"));
                    }
                    for port in n.inputs.values() {
                        if port.slots.is_empty() { return Err(bad("empty input port")); }
                        for schema in port.slots.values() { schema.decode()?; }
                    }
                    for schema in n.outputs.values() { schema.decode()?; }
                }
                Edit::RemoveNode(path) | Edit::RemoveTrigger(path) if path.is_empty() =>
                    return Err(bad("empty removal target")),
                Edit::Connect(e) | Edit::Disconnect(e) if e.from.node.is_empty()
                    || e.from.port.is_empty() || e.to.node.is_empty()
                    || e.to.port.is_empty() || e.to.slot.is_empty() =>
                    return Err(bad("empty edge endpoint")),
                Edit::AddTrigger(t) if t.name.is_empty() || t.to.node.is_empty()
                    || t.to.port.is_empty() || t.to.slot.is_empty() =>
                    return Err(bad("invalid trigger")),
                Edit::SetOutputPolicy { from, policy } => {
                    if from.node.is_empty() || from.port.is_empty() {
                        return Err(bad("invalid output policy port"));
                    }
                    if let OutputPolicy::Select { selector } = policy {
                        if selector.is_empty() || selector.len() > 512 {
                            return Err(bad("invalid output policy selector"));
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    /// Transitional bounded canonical-value envelope. The JSON payload is
    /// deterministic for BTree collections but is NOT yet the final native
    /// link-time descriptor writer.
    pub fn encode(&self) -> Result<Vec<u8>, Failure> {
        self.validate()?;
        let json = serde_json::to_vec(self).map_err(|e| bad(e.to_string()))?;
        Value::map([
            ("format", Value::String("phenix.graph.patch".into())),
            ("data", Value::Bytes(json)),
        ]).encode()
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, Failure> {
        let Value::Map(mut fields) = Value::decode(bytes)? else { return Err(bad("patch envelope")); };
        if fields.remove("format") != Some(Value::String("phenix.graph.patch".into())) {
            return Err(bad("unknown graph contribution format"));
        }
        let Some(Value::Bytes(data)) = fields.remove("data") else {
            return Err(bad("missing patch descriptor"));
        };
        if !fields.is_empty() { return Err(bad("unknown patch envelope keys")); }
        let patch: Self = serde_json::from_slice(&data).map_err(|e| bad(e.to_string()))?;
        patch.validate()?;
        Ok(patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_patch_roundtrip() {
        let p = Patch::new("plugin.one").after("base");
        assert_eq!(Patch::decode(&p.encode().unwrap()).unwrap(), p);
    }
    #[test]
    fn reject_unknown_wire_format() {
        let bytes = Value::map([
            ("format", Value::String("phenix.graph.invalid".into())),
            ("data", Value::Bytes(vec![])),
        ]).encode().unwrap();
        assert!(Patch::decode(&bytes).is_err());
    }
    #[test]
    fn output_policy_roundtrips_and_rejects_an_empty_selector() {
        let port = OutputRef { node: "model".into(), port: "reply".into() };
        let mut patch = Patch::new("route");
        patch.output_policy(port.clone(), OutputPolicy::Select {
            selector: "route.model".into(),
        });
        assert_eq!(Patch::decode(&patch.encode().unwrap()).unwrap(), patch);
        patch.output_policy(port, OutputPolicy::Select { selector: "".into() });
        assert!(patch.validate().is_err());
    }
    #[test]
    fn reject_self_dependency() {
        let p = Patch::new("base").after("base");
        assert!(p.encode().is_err());
    }
}
