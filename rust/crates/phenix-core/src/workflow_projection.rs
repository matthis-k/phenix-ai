//! Portable projection from a selected service result to a declared plan edge.
//!
//! A projection interprets *normal* structural results only. Transport,
//! provider, authority and cancellation errors never reach this layer.
//! The Rust agent adapter may use this instead of inventing its own policy;
//! portable artifact selection/integration remains a separate preparation gate.

use crate::{Key, PhenixValue, Type};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const WORKFLOW_PROJECTION_REVISION: u32 = 1;

/// The selector may inspect only a structurally typed, versioned result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "source", content = "field", rename_all = "snake_case")]
pub enum WorkflowProjectionSelector {
    /// Read the declared tag of a typed Phenix Variant.
    VariantTag,
    /// Read one String field of a typed Phenix Table.
    TableString(Key),
    /// A service result is itself a string discriminant.
    DirectString,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowOutcomeProjection {
    pub revision: u32,
    pub selector: WorkflowProjectionSelector,
    /// Structural result discriminant => plan outcome identity.
    pub cases: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkflowProjectionError {
    UnsupportedRevision(u32),
    EmptyCases,
    IncompatibleResultSchema,
    UnknownDiscriminant(String),
    UnknownEdge { discriminant: String, outcome: String },
    MissingVariantCase(String),
    InvalidResult,
}

impl WorkflowOutcomeProjection {
    /// Candidate-time check against the selected response contract and the
    /// owning workflow node's declared normal branches.
    pub fn validate(
        &self,
        result_schema: &Type,
        declared_outcomes: &BTreeSet<String>,
    ) -> Result<(), WorkflowProjectionError> {
        if self.revision != WORKFLOW_PROJECTION_REVISION {
            return Err(WorkflowProjectionError::UnsupportedRevision(self.revision));
        }
        if self.cases.is_empty() {
            return Err(WorkflowProjectionError::EmptyCases);
        }
        match (&self.selector, result_schema) {
            (WorkflowProjectionSelector::VariantTag, Type::Variant(variants)) => {
                for name in variants.keys() {
                    if !self.cases.contains_key(name.as_str()) {
                        return Err(WorkflowProjectionError::MissingVariantCase(name.to_string()));
                    }
                }
            }
            (WorkflowProjectionSelector::TableString(field), Type::Table(fields))
                if fields.get(field) == Some(&Type::String) => {}
            (WorkflowProjectionSelector::DirectString, Type::String) => {}
            _ => return Err(WorkflowProjectionError::IncompatibleResultSchema),
        }
        for (discriminant, outcome) in &self.cases {
            if !declared_outcomes.contains(outcome) {
                return Err(WorkflowProjectionError::UnknownEdge {
                    discriminant: discriminant.clone(),
                    outcome: outcome.clone(),
                });
            }
        }
        Ok(())
    }

    /// Runtime normal-result projection. Unrecognized results do not become
    /// synthetic failure edges, retries, or fallback provider invocations.
    pub fn project(&self, result: &PhenixValue) -> Result<String, WorkflowProjectionError> {
        let discriminant = match (&self.selector, result) {
            (WorkflowProjectionSelector::VariantTag, PhenixValue::Variant { tag, .. }) => {
                tag.as_str()
            }
            (WorkflowProjectionSelector::TableString(field), PhenixValue::Table(values)) => {
                match values.get(field) {
                    Some(PhenixValue::String(value)) => value,
                    _ => return Err(WorkflowProjectionError::InvalidResult),
                }
            }
            (WorkflowProjectionSelector::DirectString, PhenixValue::String(value)) => value,
            _ => return Err(WorkflowProjectionError::InvalidResult),
        };
        self.cases
            .get(discriminant)
            .cloned()
            .ok_or_else(|| WorkflowProjectionError::UnknownDiscriminant(discriminant.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(value: &str) -> Key {
        Key::parse(value).unwrap()
    }

    #[test]
    fn variant_projection_is_exhaustive_and_deterministic() {
        let projection = WorkflowOutcomeProjection {
            revision: WORKFLOW_PROJECTION_REVISION,
            selector: WorkflowProjectionSelector::VariantTag,
            cases: BTreeMap::from([
                ("answer".into(), "finish".into()),
                ("request_tool".into(), "continue".into()),
            ]),
        };
        let schema = Type::Variant(BTreeMap::from([
            (key("answer"), Type::String),
            (key("request_tool"), Type::List(Box::new(Type::String))),
        ]));
        let outcomes = BTreeSet::from(["finish".into(), "continue".into()]);
        projection.validate(&schema, &outcomes).unwrap();
        let value = PhenixValue::Variant {
            tag: key("request_tool"),
            value: Box::new(PhenixValue::List(vec![PhenixValue::String("read".into())])),
        };
        assert_eq!(projection.project(&value), Ok("continue".into()));
        assert!(matches!(
            projection.project(&PhenixValue::String("answer".into())),
            Err(WorkflowProjectionError::InvalidResult)
        ));
        let bytes = serde_json::to_vec(&projection).unwrap();
        assert_eq!(serde_json::from_slice::<WorkflowOutcomeProjection>(&bytes).unwrap(), projection);
    }

    #[test]
    fn missing_variant_mapping_unknown_edge_and_unsupported_revision_fail_preparation() {
        let schema = Type::Variant(BTreeMap::from([
            (key("good"), Type::Unit),
            (key("bad"), Type::Unit),
        ]));
        let mut projection = WorkflowOutcomeProjection {
            revision: 1,
            selector: WorkflowProjectionSelector::VariantTag,
            cases: BTreeMap::from([("good".into(), "finish".into())]),
        };
        let branches = BTreeSet::from(["finish".into()]);
        assert!(matches!(
            projection.validate(&schema, &branches),
            Err(WorkflowProjectionError::MissingVariantCase(_))
        ));
        projection.cases.insert("bad".into(), "unbound".into());
        assert!(matches!(
            projection.validate(&schema, &branches),
            Err(WorkflowProjectionError::UnknownEdge { .. })
        ));
        projection.revision = 4;
        assert!(matches!(
            projection.validate(&schema, &branches),
            Err(WorkflowProjectionError::UnsupportedRevision(4))
        ));
    }

    #[test]
    fn table_selector_must_have_typed_string_field() {
        let projection = WorkflowOutcomeProjection {
            revision: 1,
            selector: WorkflowProjectionSelector::TableString(key("outcome")),
            cases: BTreeMap::from([("done".into(), "final".into())]),
        };
        let branches = BTreeSet::from(["final".into()]);
        assert!(matches!(
            projection.validate(&Type::Table(BTreeMap::from([(key("outcome"), Type::U64)])), &branches),
            Err(WorkflowProjectionError::IncompatibleResultSchema)
        ));
        projection.validate(&Type::Table(BTreeMap::from([(key("outcome"), Type::String)])), &branches).unwrap();
        assert_eq!(
            projection.project(&PhenixValue::Table(BTreeMap::from([
                (key("outcome"), PhenixValue::String("done".into()))
            ]))),
            Ok("final".into())
        );
        assert!(matches!(
            projection.project(&PhenixValue::String("done".into())),
            Err(WorkflowProjectionError::InvalidResult)
        ));
    }
}
