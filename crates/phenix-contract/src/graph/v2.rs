//! Bounded offline graph-v2 base declarations and security primitives.
//!
//! The supported profile is Source, Invoke, Constant, Flow and Order. Other
//! deferred edges use typed next-wave admissions. Patches
//! are empty-only. This is not full V0 or a graph compiler/runtime integration.
//!
//! These checks do not attest metadata, authorize delivery or reference access,
//! grant declassification/endorsement, or wire graph-v2 into the kernel.
use crate::{Code, Failure, Value, ValueSchema};

mod declarations;
pub use declarations::*;
pub mod composition;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Classification {
    Public,
    Internal,
    Confidential,
    Secret,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IntegrityClass {
    Untrusted,
    Validated,
    Trusted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortSecurity {
    pub confidentiality: Classification,
    pub integrity: IntegrityClass,
}

impl PortSecurity {
    /// Conservative bound over all possible inputs, including control inputs.
    pub fn join(self, other: Self) -> Self {
        Self {
            confidentiality: self.confidentiality.max(other.confidentiality),
            integrity: self.integrity.min(other.integrity),
        }
    }

    /// Whether delivery/transform to `destination` needs neither
    /// declassification nor endorsement. This is not an authorization grant.
    pub fn can_flow_to(self, destination: Self) -> bool {
        self.confidentiality <= destination.confidentiality
            && self.integrity >= destination.integrity
    }
}

/// Both ceilings must come from verified authorities, not source self-description.
/// Integrity attestation and source admission remain the caller's responsibility.
pub fn validate_source_ceiling(
    source: PortSecurity,
    owner_ceiling: Classification,
    composition_ceiling: Classification,
) -> Result<(), Failure> {
    if source.confidentiality > owner_ceiling {
        return Err(Failure::new(Code::Denied, "source exceeds owner ceiling"));
    }
    if source.confidentiality > composition_ceiling {
        return Err(Failure::new(
            Code::Denied,
            "source exceeds composition ceiling",
        ));
    }
    Ok(())
}

/// Validate an inspectable constant/default using the canonical contract model.
/// Public string contents remain an authoring trust obligation: this does not
/// detect credentials hidden in a mislabelled literal.
pub fn validate_inspectable_literal(
    security: PortSecurity,
    schema: &ValueSchema,
    value: &Value,
) -> Result<(), Failure> {
    if security
        != (PortSecurity {
            confidentiality: Classification::Public,
            integrity: IntegrityClass::Untrusted,
        })
    {
        return Err(Failure::new(
            Code::Denied,
            "inspectable literal requires Public/Untrusted",
        ));
    }
    // Bound and validate both trees before any recursive inspection or matching.
    schema
        .encode()
        .map_err(|_| Failure::new(Code::SchemaRejected, "invalid or oversized literal schema"))?;
    value
        .encode()
        .map_err(|_| Failure::new(Code::SchemaRejected, "invalid or oversized literal value"))?;

    fn schema_has_ref(schema: &ValueSchema) -> bool {
        match schema {
            ValueSchema::Ref(_) => true,
            ValueSchema::List(inner) | ValueSchema::Option(inner) => schema_has_ref(inner),
            ValueSchema::Struct(fields) => fields.values().any(schema_has_ref),
            ValueSchema::Tagged(cases) => cases.values().flatten().any(schema_has_ref),
            _ => false,
        }
    }
    fn value_has_ref(value: &Value) -> bool {
        match value {
            Value::Ref(_) => true,
            Value::List(items) => items.iter().any(value_has_ref),
            Value::Map(fields) => fields.values().any(value_has_ref),
            _ => false,
        }
    }
    if schema_has_ref(schema) || value_has_ref(value) {
        return Err(Failure::new(
            Code::Denied,
            "inspectable literal contains reference",
        ));
    }
    if !schema.accepts(value) {
        return Err(Failure::new(
            Code::SchemaRejected,
            "literal schema mismatch",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        GenerationDigest, OwnerId, SchemaId, ValueId, ValueLocation, MAX_BYTES, MAX_COUNT,
        MAX_DEPTH,
    };
    use std::collections::BTreeMap;

    const PUBLIC: PortSecurity = PortSecurity {
        confidentiality: Classification::Public,
        integrity: IntegrityClass::Untrusted,
    };
    const CLASSES: [Classification; 4] = [
        Classification::Public,
        Classification::Internal,
        Classification::Confidential,
        Classification::Secret,
    ];
    const INTEGRITIES: [IntegrityClass; 3] = [
        IntegrityClass::Untrusted,
        IntegrityClass::Validated,
        IntegrityClass::Trusted,
    ];

    fn labels() -> Vec<PortSecurity> {
        CLASSES
            .into_iter()
            .flat_map(|confidentiality| {
                INTEGRITIES.into_iter().map(move |integrity| PortSecurity {
                    confidentiality,
                    integrity,
                })
            })
            .collect()
    }

    #[test]
    fn exhaustive_product_lattice() {
        let labels = labels();
        for a in &labels {
            assert_eq!(a.join(*a), *a);
            for b in &labels {
                let joined = a.join(*b);
                assert_eq!(
                    joined.confidentiality,
                    a.confidentiality.max(b.confidentiality)
                );
                assert_eq!(joined.integrity, a.integrity.min(b.integrity));
                assert_eq!(joined, b.join(*a));
                assert_eq!(
                    a.can_flow_to(*b),
                    a.confidentiality <= b.confidentiality && a.integrity >= b.integrity
                );
                assert!(a.can_flow_to(joined));
                assert!(b.can_flow_to(joined));
                for c in &labels {
                    assert_eq!(a.join(*b).join(*c), a.join(b.join(*c)));
                }
            }
        }
    }

    #[test]
    fn confidentiality_and_integrity_are_independent() {
        let secret = PortSecurity {
            confidentiality: Classification::Secret,
            ..PUBLIC
        };
        let trusted = PortSecurity {
            integrity: IntegrityClass::Trusted,
            ..PUBLIC
        };
        assert!(!secret.can_flow_to(PUBLIC));
        assert!(!PUBLIC.can_flow_to(trusted));
        assert!(PUBLIC.can_flow_to(secret));
        assert!(trusted.can_flow_to(PUBLIC));
    }

    #[test]
    fn both_source_ceilings_are_required() {
        for source in labels() {
            for owner in CLASSES {
                for composition in CLASSES {
                    let result = validate_source_ceiling(source, owner, composition);
                    assert_eq!(
                        result.is_ok(),
                        source.confidentiality <= owner && source.confidentiality <= composition
                    );
                    if let Err(error) = result {
                        assert_eq!(error.code, Code::Denied);
                    }
                }
            }
        }
    }

    #[test]
    fn valid_primitives_and_aggregates() {
        let cases = [
            (ValueSchema::Bool, Value::Bool(true)),
            (ValueSchema::I64, Value::I64(-1)),
            (ValueSchema::U64, Value::U64(1)),
            (ValueSchema::String, Value::String("text".into())),
            (ValueSchema::Bytes, Value::Bytes(vec![0, 255])),
            (
                ValueSchema::Option(Box::new(ValueSchema::Bool)),
                Value::Null,
            ),
            (
                ValueSchema::Option(Box::new(ValueSchema::Bool)),
                Value::List(vec![Value::Bool(false)]),
            ),
            (
                ValueSchema::List(Box::new(ValueSchema::U64)),
                Value::List(vec![Value::U64(1)]),
            ),
            (
                ValueSchema::Struct(BTreeMap::from([("field".into(), ValueSchema::Bool)])),
                Value::map([("field", Value::Bool(true))]),
            ),
            (
                ValueSchema::Variants(vec!["case".into()]),
                Value::String("case".into()),
            ),
            (
                ValueSchema::Tagged(BTreeMap::from([("case".into(), Some(ValueSchema::Bool))])),
                Value::map([("case", Value::Bool(true))]),
            ),
        ];
        for (schema, value) in cases {
            assert!(validate_inspectable_literal(PUBLIC, &schema, &value).is_ok());
        }
        for label in labels() {
            assert_eq!(
                validate_inspectable_literal(label, &ValueSchema::Bool, &Value::Bool(true)).is_ok(),
                label == PUBLIC
            );
        }
    }

    #[test]
    fn references_reject_even_in_absent_or_unselected_schema_branches() {
        let id = SchemaId::from_bytes(&[1; 32]).unwrap();
        let reference = Value::Ref(Box::new(ValueLocation {
            generation: GenerationDigest::from_bytes(&[1; 32]).unwrap(),
            owner: OwnerId::from_bytes(&[1; 32]).unwrap(),
            cell: ValueId::from_bytes(&[1; 16]).unwrap(),
            schema: id,
        }));
        for value in [
            reference.clone(),
            Value::List(vec![Value::map([("nested", reference)])]),
        ] {
            assert_eq!(
                validate_inspectable_literal(PUBLIC, &ValueSchema::Bool, &value)
                    .unwrap_err()
                    .code,
                Code::Denied
            );
        }
        for schema in [
            ValueSchema::Ref(id),
            ValueSchema::Option(Box::new(ValueSchema::Ref(id))),
            ValueSchema::List(Box::new(ValueSchema::Ref(id))),
            ValueSchema::Struct(BTreeMap::from([("nested".into(), ValueSchema::Ref(id))])),
            ValueSchema::Tagged(BTreeMap::from([(
                "unused".into(),
                Some(ValueSchema::Ref(id)),
            )])),
        ] {
            assert_eq!(
                validate_inspectable_literal(PUBLIC, &schema, &Value::Null)
                    .unwrap_err()
                    .code,
                Code::Denied
            );
        }
    }

    #[test]
    fn invalid_and_oversized_literals_reject_before_inspection() {
        let mut deep_value = Value::Null;
        let mut deep_schema = ValueSchema::Bool;
        for _ in 0..MAX_DEPTH + 2 {
            deep_value = Value::List(vec![deep_value]);
            deep_schema = ValueSchema::Option(Box::new(deep_schema));
        }
        for (schema, value) in [
            (ValueSchema::Bool, Value::String("private payload".into())),
            (ValueSchema::Bytes, Value::Bytes(vec![0; MAX_BYTES])),
            (ValueSchema::String, Value::String("x".repeat(MAX_BYTES))),
            (
                ValueSchema::List(Box::new(ValueSchema::Bool)),
                Value::List(vec![Value::Bool(true); MAX_COUNT + 1]),
            ),
            (ValueSchema::Bool, deep_value),
            (deep_schema, Value::Null),
            (ValueSchema::Variants(vec![]), Value::Null),
            (
                ValueSchema::Struct(BTreeMap::from([(
                    "invalid field!".into(),
                    ValueSchema::Bool,
                )])),
                Value::Null,
            ),
            (
                ValueSchema::Variants((0..MAX_COUNT + 1).map(|i| format!("v{i:06}")).collect()),
                Value::Null,
            ),
        ] {
            let error = validate_inspectable_literal(PUBLIC, &schema, &value).unwrap_err();
            assert_eq!(error.code, Code::SchemaRejected);
            assert!(!error.message.contains("private payload"));
        }
    }
}
