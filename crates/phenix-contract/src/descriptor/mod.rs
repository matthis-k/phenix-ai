use crate::{Code, Failure, Value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Copy)]
pub enum Schema {
    Bool,
    I64,
    U64,
    String,
    Bytes,
    Struct(&'static [Field]),
    Variants(&'static [&'static str]),
}
#[derive(Clone, Copy)]
pub struct Field {
    pub name: &'static str,
    pub schema: Schema,
}
#[derive(Clone, Copy)]
pub struct OperationDef {
    pub key: &'static str,
    pub request: Schema,
    pub response: Schema,
    pub error: Schema,
}
#[derive(Clone, Copy)]
pub struct Declaration {
    pub operation: OperationDef,
    pub provenance: &'static str,
}
#[derive(Clone, Copy)]
pub struct ImportPolicy {
    pub optional: bool,
    pub implementation: Option<&'static str>,
}
impl ImportPolicy {
    pub const REQUIRED: Self = Self {
        optional: false,
        implementation: None,
    };
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum SchemaSpec {
    Bool,
    I64,
    U64,
    String,
    Bytes,
    Struct {
        fields: BTreeMap<String, SchemaSpec>,
    },
    Variants {
        variants: Vec<String>,
    },
}
impl SchemaSpec {
    fn valid(&self) -> bool {
        match self {
            Self::Struct { fields } => fields
                .iter()
                .all(|(name, schema)| identifier(name) && schema.valid()),
            Self::Variants { variants } => {
                !variants.is_empty()
                    && variants.iter().all(|v| identifier(v))
                    && variants.windows(2).all(|p| p[0] < p[1])
            }
            _ => true,
        }
    }
    pub fn accepts(&self, value: &Value) -> bool {
        match (self, value) {
            (Self::Bool, Value::Bool(_))
            | (Self::I64, Value::I64(_))
            | (Self::U64, Value::U64(_))
            | (Self::String, Value::String(_))
            | (Self::Bytes, Value::Bytes(_)) => true,
            (Self::Struct { fields }, Value::Map(values)) => {
                fields.len() == values.len()
                    && fields
                        .iter()
                        .all(|(k, s)| values.get(k).is_some_and(|v| s.accepts(v)))
            }
            (Self::Variants { variants }, Value::String(v)) => variants.contains(v),
            _ => false,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    pub id: String,
    pub kind: String,
    pub owner: String,
    pub provenance: String,
    pub role: String,
    pub key: String,
    pub request: SchemaSpec,
    pub response: SchemaSpec,
    pub error: SchemaSpec,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contribution {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implementation: Option<String>,
    pub id: String,
    pub kind: String,
    pub operation: String,
    pub owner: String,
    pub provenance: String,
    pub role: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Descriptor {
    pub abi_major: u32,
    pub abi_minor: u32,
    pub authority_ceiling: Vec<String>,
    pub contracts: Vec<Contract>,
    pub execution: String,
    pub exports: Vec<Contribution>,
    pub features: u32,
    pub format: String,
    pub imports: Vec<Contribution>,
    pub plugin_id: String,
    pub plugin_version: String,
    pub schema_version: u32,
}
pub fn identifier(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
}
pub fn operation_key(key: &str) -> bool {
    let Some((interface, operation)) = key.split_once('/') else {
        return false;
    };
    let Some((name, major)) = interface.rsplit_once('@') else {
        return false;
    };
    identifier(name)
        && identifier(operation)
        && major
            .parse::<u32>()
            .is_ok_and(|m| m > 0 && major == m.to_string())
}
impl Descriptor {
    pub fn decode(bytes: &[u8]) -> Result<Self, Failure> {
        let fail = |m: String| Failure::new(Code::DescriptorInvalid, m);
        let v = Value::decode(bytes).map_err(|e| fail(e.to_string()))?;
        if v.encode().map_err(|e| fail(e.to_string()))? != bytes {
            return Err(fail("noncanonical descriptor".into()));
        }
        // JSON numbers erase the signed/unsigned wire distinction. Enforce the
        // frozen field types before using serde for the remaining shape checks.
        let Value::Map(fields) = &v else {
            return Err(fail("descriptor must be a map".into()));
        };
        for key in ["abi_major", "abi_minor", "features", "schema_version"] {
            if !matches!(fields.get(key), Some(Value::U64(_))) {
                return Err(fail(format!("{key} must use the U64 wire type")));
            }
        }
        // The original contribution shape stays exact. Enhanced imports carry
        // an explicitly versioned fixed kind with both policy fields present.
        for field in ["imports", "exports"] {
            if let Some(Value::List(items)) = fields.get(field) {
                for item in items {
                    let Value::Map(item) = item else {
                        return Err(fail("contribution must be a map".into()));
                    };
                    let enhanced =
                        item.get("kind") == Some(&Value::String("phenix.import.v1".into()));
                    if enhanced {
                        if field != "imports"
                            || !matches!(item.get("optional"), Some(Value::Bool(_)))
                            || !matches!(
                                item.get("implementation"),
                                Some(Value::Null | Value::String(_))
                            )
                        {
                            return Err(fail("invalid enhanced import fields".into()));
                        }
                    } else if item.contains_key("optional") || item.contains_key("implementation") {
                        return Err(fail(
                            "policy fields require the enhanced import kind".into(),
                        ));
                    }
                }
            }
        }
        let d: Self = serde_json::from_value(v.to_json()).map_err(|e| fail(e.to_string()))?;
        if d.format != "phenix.descriptor.v1"
            || d.schema_version != 1
            || !matches!(d.execution.as_str(), "sync" | "pending")
            || !identifier(&d.plugin_id)
            || d.plugin_id == "root"
            || d.plugin_version.is_empty()
        {
            return Err(fail("invalid descriptor profile or identity".into()));
        }
        if !((d.abi_major == 1 && d.features == 0 && d.execution == "sync")
            || (d.abi_major == 2 && d.features == 1 && d.execution == "pending"))
            || d.abi_minor != 0
        {
            return Err(Failure::new(
                Code::AbiIncompatible,
                "unsupported offline ABI requirements",
            ));
        }
        let mut previous = "";
        for c in &d.contracts {
            if !operation_key(&c.key)
                || c.key.as_str() <= previous
                || c.id != format!("{}/{}/Declare", d.plugin_id, c.key)
                || c.owner != d.plugin_id
                || c.role != "Declare"
                || c.kind != "phenix.contract.v1"
                || c.provenance.is_empty()
                || !c.request.valid()
                || !c.response.valid()
                || !c.error.valid()
            {
                return Err(fail("invalid or duplicate contract key".into()));
            }
            previous = &c.key;
        }
        if d.authority_ceiling
            != d.contracts
                .iter()
                .map(|c| c.key.clone())
                .collect::<Vec<_>>()
        {
            return Err(fail("ceiling differs from declared operations".into()));
        }
        for (role, items) in [("Provide", &d.exports), ("Require", &d.imports)] {
            let mut previous = "";
            for c in items {
                if c.id != format!("{}/{}/{role}", d.plugin_id, c.operation)
                    || c.id.as_str() <= previous
                    || c.owner != d.plugin_id
                    || c.role != role
                    || !((c.kind == "phenix.operation.v1"
                        && (role == "Require" || d.execution == "sync"))
                        || (role == "Provide"
                            && d.execution == "pending"
                            && c.kind == "phenix.pending-operation.v1")
                        || (role == "Require" && c.kind == "phenix.import.v1"))
                    || c.implementation
                        .as_ref()
                        .is_some_and(|id| !identifier(id) || id == "root")
                    || c.provenance.is_empty()
                    || !d.contracts.iter().any(|op| op.key == c.operation)
                {
                    return Err(fail(
                        "invalid contribution identity, owner or schema".into(),
                    ));
                }
                previous = &c.id;
            }
        }
        if d.contracts.iter().any(|c| {
            !d.exports
                .iter()
                .chain(&d.imports)
                .any(|x| x.operation == c.key)
        }) {
            return Err(fail("undeclared contract".into()));
        }
        Ok(d)
    }
    pub fn contract(&self, key: &str) -> Option<&Contract> {
        self.contracts.iter().find(|x| x.key == key)
    }
    pub fn permits(&self, key: &str) -> bool {
        self.authority_ceiling
            .binary_search_by(|k| k.as_str().cmp(key))
            .is_ok()
    }
}
mod encode;
pub use encode::{descriptor, descriptor_pending, descriptor_with_policy, SECTION_BYTES};

#[cfg(test)]
mod tests {
    use super::*;
    const OP: OperationDef = OperationDef {
        key: "test.text@1/apply",
        request: Schema::String,
        response: Schema::String,
        error: Schema::Variants(&["Empty"]),
    };
    #[test]
    fn canonical_descriptor() {
        let section = descriptor(
            "test.plugin",
            "1.0.0",
            [],
            [Declaration {
                operation: OP,
                provenance: "test::apply",
            }],
        );
        let n = u32::from_be_bytes(section[..4].try_into().unwrap()) as usize;
        let d = Descriptor::decode(&section[4..n + 4]).unwrap();
        assert_eq!(d.exports[0].owner, "test.plugin");
        assert!(section[n + 4..].iter().all(|&b| b == 0));
        let mut v = Value::decode(&section[4..n + 4]).unwrap();
        let Value::Map(ref mut m) = v else {
            unreachable!()
        };
        m.insert("features".into(), Value::U64(1));
        assert_eq!(
            Descriptor::decode(&v.encode().unwrap()).unwrap_err().code,
            Code::AbiIncompatible
        );
    }
    #[test]
    fn pending_profile_is_fixed_and_validated_offline() {
        let section = descriptor_pending(
            "test.pending",
            "1.0.0",
            [],
            [],
            [Declaration {
                operation: OP,
                provenance: "test::pending",
            }],
        );
        let n = u32::from_be_bytes(section[..4].try_into().unwrap()) as usize;
        let original = Value::decode(&section[4..n + 4]).unwrap();
        let d = Descriptor::decode(&section[4..n + 4]).unwrap();
        assert_eq!(d.abi_major, 2);
        assert_eq!(d.exports[0].kind, "phenix.pending-operation.v1");
        for (key, value) in [
            ("features", Value::U64(0)),
            ("abi_minor", Value::U64(1)),
            ("execution", Value::String("sync".into())),
        ] {
            let mut changed = original.clone();
            let Value::Map(fields) = &mut changed else {
                unreachable!()
            };
            fields.insert(key.into(), value);
            assert_eq!(
                Descriptor::decode(&changed.encode().unwrap())
                    .unwrap_err()
                    .code,
                Code::AbiIncompatible
            );
        }
    }
    #[test]
    fn descriptor_version_fields_preserve_unsigned_wire_types() {
        let section = descriptor(
            "test.plugin",
            "1.0.0",
            [],
            [Declaration {
                operation: OP,
                provenance: "test::apply",
            }],
        );
        let n = u32::from_be_bytes(section[..4].try_into().unwrap()) as usize;
        let original = Value::decode(&section[4..n + 4]).unwrap();
        for key in ["abi_major", "abi_minor", "features", "schema_version"] {
            let mut value = original.clone();
            let Value::Map(fields) = &mut value else {
                unreachable!()
            };
            let Value::U64(number) = fields[key] else {
                unreachable!()
            };
            fields.insert(key.into(), Value::I64(number as i64));
            assert_eq!(
                Descriptor::decode(&value.encode().unwrap())
                    .unwrap_err()
                    .code,
                Code::DescriptorInvalid,
                "{key}"
            );
        }
    }
}
