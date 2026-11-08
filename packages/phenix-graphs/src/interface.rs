//! Graph public input/output bounds. The plugin system reuses these
//! semantics; it does not define its own operation-contract language.
use crate::{DeclaredValue, NodeSignature, TypeIdentity};
use std::{collections::BTreeMap, fmt};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterfaceError(String);
impl fmt::Display for InterfaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for InterfaceError {}

fn valid_segment(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'@'))
}
fn value_identity(value: DeclaredValue) -> TypeIdentity {
    match value.local_type {
        Some(id) => TypeIdentity::Local(id),
        None => TypeIdentity::Wire(value.schema),
    }
}

/// Semantic identity plus public named input ports, input slots and outputs.
///
/// The node signature is a 1-input/1-output convenience. Structured layouts
/// explicitly describe each input slot. Graph owns all value identities;
/// `Local(TypeId)` cannot be wired to a `Wire(schema)` value.
///
/// A schema *name* is not yet a canonical structural schema or codec proof.
/// Native ABI composition MUST verify canonical schema + codec revisions
/// before an independently compiled implementation may be invoked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterfaceBounds {
    id: String,
    inputs: BTreeMap<String, BTreeMap<String, DeclaredValue>>,
    outputs: BTreeMap<String, DeclaredValue>,
}
impl InterfaceBounds {
    pub fn new(id: impl Into<String>) -> Result<Self, InterfaceError> {
        let id = id.into();
        if id.len() > 512 || !id.split('/').all(valid_segment) {
            return Err(InterfaceError(format!(
                "invalid interface identity: {id:?}"
            )));
        }
        Ok(Self {
            id,
            inputs: BTreeMap::new(),
            outputs: BTreeMap::new(),
        })
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn node(id: impl Into<String>, sig: NodeSignature) -> Result<Self, InterfaceError> {
        Self::new(id)?
            .input_slot("run", "value", sig.input)?
            .output("value", sig.output)
    }
    pub fn input_slot(
        mut self,
        port: impl Into<String>,
        slot: impl Into<String>,
        value: DeclaredValue,
    ) -> Result<Self, InterfaceError> {
        let port = port.into();
        let slot = slot.into();
        if !valid_segment(&port) || !valid_segment(&slot) {
            return Err(InterfaceError("invalid graph input port/slot name".into()));
        }
        if self
            .inputs
            .entry(port.clone())
            .or_default()
            .insert(slot.clone(), value)
            .is_some()
        {
            return Err(InterfaceError(format!(
                "duplicate input slot {port}.{slot}"
            )));
        }
        Ok(self)
    }
    pub fn output(
        mut self,
        name: impl Into<String>,
        value: DeclaredValue,
    ) -> Result<Self, InterfaceError> {
        let name = name.into();
        if !valid_segment(&name) {
            return Err(InterfaceError("invalid graph output name".into()));
        }
        if self.outputs.insert(name.clone(), value).is_some() {
            return Err(InterfaceError(format!("duplicate output {name}")));
        }
        Ok(self)
    }
    pub fn inputs(&self) -> impl Iterator<Item = (&str, &str, DeclaredValue)> {
        self.inputs
            .iter()
            .flat_map(|(p, slots)| slots.iter().map(move |(s, v)| (p.as_str(), s.as_str(), *v)))
    }
    pub fn outputs(&self) -> impl Iterator<Item = (&str, DeclaredValue)> {
        self.outputs.iter().map(|(n, v)| (n.as_str(), *v))
    }
    /// Exact public boundary match; no name-based local coercion, input
    /// reordering, implicit width subtyping or permission inference.
    pub fn check_implementation(&self, provided: &Self) -> Result<(), InterfaceError> {
        if self.id != provided.id {
            return Err(InterfaceError(format!(
                "different interface identities: {:?} versus {:?}",
                self.id, provided.id
            )));
        }
        if !self.inputs.keys().eq(provided.inputs.keys()) {
            return Err(InterfaceError(format!(
                "{}: incompatible input port set",
                self.id
            )));
        }
        for (port, expected) in &self.inputs {
            let actual = &provided.inputs[port];
            if !expected.keys().eq(actual.keys()) {
                return Err(InterfaceError(format!(
                    "{}: incompatible input slots on {port}",
                    self.id
                )));
            }
            for (slot, type_id) in expected {
                if value_identity(*type_id) != value_identity(actual[slot]) {
                    return Err(InterfaceError(format!(
                        "{}: incompatible input {port}.{slot}",
                        self.id
                    )));
                }
            }
        }
        if !self.outputs.keys().eq(provided.outputs.keys()) {
            return Err(InterfaceError(format!(
                "{}: incompatible output port set",
                self.id
            )));
        }
        for (output, type_id) in &self.outputs {
            if value_identity(*type_id) != value_identity(provided.outputs[output]) {
                return Err(InterfaceError(format!(
                    "{}: incompatible output {output}",
                    self.id
                )));
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ContractValue;
    struct ProducerRequest(u64);
    struct ConsumerRequest(u64);
    impl ContractValue for ProducerRequest {
        const SCHEMA: &'static str = "plugina.model.request@1";
        fn encode(&self) -> Vec<u8> {
            self.0.to_le_bytes().to_vec()
        }
        fn decode(b: &[u8]) -> Result<Self, String> {
            Ok(Self(u64::from_le_bytes(
                b.try_into().map_err(|_| "bad length")?,
            )))
        }
    }
    impl ContractValue for ConsumerRequest {
        const SCHEMA: &'static str = "plugina.model.request@1";
        fn encode(&self) -> Vec<u8> {
            self.0.to_le_bytes().to_vec()
        }
        fn decode(b: &[u8]) -> Result<Self, String> {
            Ok(Self(u64::from_le_bytes(
                b.try_into().map_err(|_| "bad length")?,
            )))
        }
    }
    fn provider() -> InterfaceBounds {
        InterfaceBounds::new("plugina/provider@1")
            .unwrap()
            .input_slot("run", "request", DeclaredValue::wire::<ProducerRequest>())
            .unwrap()
            .output("response", DeclaredValue::wire::<String>())
            .unwrap()
    }
    #[test]
    fn independently_defined_wire_types_can_match_nominal_identity() {
        let consumer = InterfaceBounds::new("plugina/provider@1")
            .unwrap()
            .input_slot("run", "request", DeclaredValue::wire::<ConsumerRequest>())
            .unwrap()
            .output("response", DeclaredValue::wire::<String>())
            .unwrap();
        consumer.check_implementation(&provider()).unwrap();
        assert!(
            consumer
                .check_implementation(
                    &InterfaceBounds::new("pluginc/provider@1")
                        .unwrap()
                        .input_slot("run", "request", DeclaredValue::wire::<ConsumerRequest>())
                        .unwrap()
                        .output("response", DeclaredValue::wire::<String>())
                        .unwrap()
                )
                .is_err()
        );
    }
    #[test]
    fn local_type_identity_differs_from_wire_and_independent_rust_type() {
        let local = InterfaceBounds::new("plugina/provider@1")
            .unwrap()
            .input_slot("run", "request", DeclaredValue::local::<ProducerRequest>())
            .unwrap()
            .output("response", DeclaredValue::wire::<String>())
            .unwrap();
        let other = InterfaceBounds::new("plugina/provider@1")
            .unwrap()
            .input_slot("run", "request", DeclaredValue::local::<ConsumerRequest>())
            .unwrap()
            .output("response", DeclaredValue::wire::<String>())
            .unwrap();
        assert!(local.check_implementation(&provider()).is_err());
        assert!(local.check_implementation(&other).is_err());
    }
    #[test]
    fn rejects_extra_missing_mismatched_and_duplicated_slots() {
        let wrong = InterfaceBounds::new("plugina/provider@1")
            .unwrap()
            .input_slot("run", "other", DeclaredValue::wire::<ProducerRequest>())
            .unwrap()
            .output("response", DeclaredValue::wire::<String>())
            .unwrap();
        assert!(provider().check_implementation(&wrong).is_err());
        assert!(
            provider()
                .output("response", DeclaredValue::wire::<String>())
                .is_err()
        );
        assert!(
            InterfaceBounds::new("plugina/provider@1")
                .unwrap()
                .input_slot("run", "request", DeclaredValue::wire::<u64>())
                .unwrap()
                .input_slot("run", "request", DeclaredValue::wire::<u64>())
                .is_err()
        );
        assert!(InterfaceBounds::new("plugina//provider").is_err());
        assert!(
            provider()
                .check_implementation(
                    &InterfaceBounds::new("plugina/provider@1")
                        .unwrap()
                        .input_slot("run", "request", DeclaredValue::wire::<u64>())
                        .unwrap()
                        .output("response", DeclaredValue::wire::<String>())
                        .unwrap()
                )
                .is_err()
        );
    }
}
