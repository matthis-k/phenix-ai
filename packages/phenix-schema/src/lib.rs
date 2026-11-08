//! Shared wire contract. Equality is authoritative, not a hash or Rust type name.
use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schema {
    pub name: String,
    pub revision: u32,
    pub structure: String,
    pub codec: String,
}

impl Schema {
    /// Length framing makes names and nested descriptors unambiguous.
    pub fn canonical(&self) -> String {
        let mut out = format!("phenix-schema-v1:{};", self.revision);
        for part in [&self.name, &self.structure, &self.codec] {
            out.push_str(&format!("{}:{}", part.len(), part));
        }
        out
    }

    pub fn record(name: &str, fields: &[(&str, bool, Schema)]) -> Self {
        let mut structure = String::from("record-v1:");
        for (name, optional, schema) in fields {
            let nested = schema.canonical();
            structure.push_str(&format!(
                "{}:{}{}{}:{}",
                name.len(),
                name,
                u8::from(*optional),
                nested.len(),
                nested
            ));
        }
        Self {
            name: name.into(),
            revision: 1,
            structure,
            codec: "phenix-record-le64-v1".into(),
        }
    }

    /// Interning retains the graph's existing static-string identity API.
    pub fn intern(&self) -> &'static str {
        static IDENTITIES: OnceLock<Mutex<BTreeSet<&'static str>>> = OnceLock::new();
        let canonical = self.canonical();
        let mut identities = IDENTITIES.get_or_init(Default::default).lock().unwrap();
        if let Some(value) = identities.get(canonical.as_str()) {
            return value;
        }
        let value = Box::leak(canonical.into_boxed_str());
        identities.insert(value);
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_and_all_contract_components_are_authoritative() {
        let base = Schema {
            name: "x".into(),
            revision: 1,
            structure: "y".into(),
            codec: "z".into(),
        };
        for changed in [
            Schema {
                name: "x:y".into(),
                ..base.clone()
            },
            Schema {
                revision: 2,
                ..base.clone()
            },
            Schema {
                structure: "y:z".into(),
                ..base.clone()
            },
            Schema {
                codec: "other".into(),
                ..base.clone()
            },
        ] {
            assert_ne!(base.canonical(), changed.canonical());
        }
        assert!(std::ptr::eq(base.intern(), base.clone().intern()));
        let required = Schema::record("r", &[("a", false, base.clone())]);
        assert_ne!(required, Schema::record("r", &[("a", true, base.clone())]));
        assert_ne!(required, Schema::record("r", &[("b", false, base)]));
    }
}
