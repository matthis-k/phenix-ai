//! Typed workflow data, independent of provider and host capability handles.
//!
//! A frame owns values only. Authority stays with the root and never crosses
//! an Invoke/Fork/Join transition through serialized frame contents.

use crate::{Key, PhenixSchema, PhenixValue};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt, sync::Arc};

/// Versioned schema for the values visible to an execution plan.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowFrameSchema {
    pub revision: u64,
    pub slots: BTreeMap<Key, PhenixSchema>,
}

/// Authored schema bound to one selected workflow before generation activation.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowFrameDeclaration {
    pub owner: crate::ComponentId,
    pub name: String,
    pub schema: WorkflowFrameSchema,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkflowFrameError {
    InvalidRevision,
    MissingSlot(Key),
    UnknownSlot(Key),
    CapabilitySlot(Key),
    CapabilityValue(Key),
    InvalidValue { slot: Key, reason: String },
    IncompatibleSchema,
}

impl fmt::Display for WorkflowFrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for WorkflowFrameError {}

/// A copy-on-write data snapshot. Cloning it cannot delegate authority.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkflowFrame {
    schema: Arc<WorkflowFrameSchema>,
    values: Arc<BTreeMap<Key, PhenixValue>>,
}

fn schema_contains_capability(schema: &PhenixSchema) -> bool {
    match schema {
        PhenixSchema::Callable { .. } | PhenixSchema::Object { .. } => true,
        PhenixSchema::Option(inner)
        | PhenixSchema::List(inner)
        | PhenixSchema::Map(inner) => schema_contains_capability(inner),
        PhenixSchema::Array { item, .. } => schema_contains_capability(item),
        PhenixSchema::Table(fields) | PhenixSchema::Variant(fields) => {
            fields.values().any(schema_contains_capability)
        }
        _ => false,
    }
}

fn value_contains_capability(value: &PhenixValue) -> bool {
    match value {
        PhenixValue::Callable(_) | PhenixValue::Object(_) => true,
        PhenixValue::Option(Some(inner)) => value_contains_capability(inner),
        PhenixValue::List(values) => values.iter().any(value_contains_capability),
        PhenixValue::Map(values) => values.values().any(value_contains_capability),
        PhenixValue::Table(values) => values.values().any(value_contains_capability),
        PhenixValue::Variant { value, .. } => value_contains_capability(value),
        _ => false,
    }
}

impl WorkflowFrameSchema {
    /// A frame's schema must not authorize live references, including refs
    /// concealed in nested fields. Any is legal for ordinary data only.
    pub fn validate(&self) -> Result<(), WorkflowFrameError> {
        if self.revision == 0 {
            return Err(WorkflowFrameError::InvalidRevision);
        }
        for (name, schema) in &self.slots {
            if schema_contains_capability(schema) {
                return Err(WorkflowFrameError::CapabilitySlot(name.clone()));
            }
        }
        Ok(())
    }

    fn validate_value(&self, slot: &Key, value: &PhenixValue) -> Result<(), WorkflowFrameError> {
        let schema = self
            .slots
            .get(slot)
            .ok_or_else(|| WorkflowFrameError::UnknownSlot(slot.clone()))?;
        if value_contains_capability(value) {
            return Err(WorkflowFrameError::CapabilityValue(slot.clone()));
        }
        schema
            .parse(value)
            .map_err(|error| WorkflowFrameError::InvalidValue {
                slot: slot.clone(),
                reason: error.to_string(),
            })
    }
}

impl WorkflowFrame {
    /// Full initialization is mandatory: even optional slots must contain an
    /// explicit Option(None) value. There are no implicit defaults.
    pub fn new(
        schema: WorkflowFrameSchema,
        values: BTreeMap<Key, PhenixValue>,
    ) -> Result<Self, WorkflowFrameError> {
        schema.validate()?;
        for key in schema.slots.keys() {
            if !values.contains_key(key) {
                return Err(WorkflowFrameError::MissingSlot(key.clone()));
            }
        }
        for (key, value) in &values {
            schema.validate_value(key, value)?;
        }
        Ok(Self {
            schema: Arc::new(schema),
            values: Arc::new(values),
        })
    }

    pub fn schema(&self) -> &WorkflowFrameSchema {
        &self.schema
    }

    pub fn get(&self, key: &Key) -> Option<&PhenixValue> {
        self.values.get(key)
    }

    pub fn values(&self) -> &BTreeMap<Key, PhenixValue> {
        &self.values
    }

    /// Mutate only a declared slot with an authorized structural value.
    /// The write is validated before changing the shared frame snapshot.
    pub fn set(&mut self, slot: &Key, value: PhenixValue) -> Result<(), WorkflowFrameError> {
        self.schema.validate_value(slot, &value)?;
        Arc::make_mut(&mut self.values).insert(slot.clone(), value);
        Ok(())
    }

    /// Explicitly select branch-produced data by slot. Unselected parent
    /// values remain intact. The caller must supply a schema-compatible branch.
    /// Core never combines or elevates the branches' authority.
    pub fn collect_from(
        &mut self,
        branch: &Self,
        slots: &[Key],
    ) -> Result<(), WorkflowFrameError> {
        if self.schema != branch.schema {
            return Err(WorkflowFrameError::IncompatibleSchema);
        }
        let mut candidate = self.clone();
        for slot in slots {
            let value = branch
                .get(slot)
                .ok_or_else(|| WorkflowFrameError::UnknownSlot(slot.clone()))?;
            candidate.set(slot, value.clone())?;
        }
        *self = candidate;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CallableRef, InterfaceId, PluginId, ReferenceGenerationId, ReferenceId, ReferenceOwnerId, Type};

    fn key(name: &str) -> Key {
        Key::parse(name).unwrap()
    }

    fn schema() -> WorkflowFrameSchema {
        WorkflowFrameSchema {
            revision: 1,
            slots: BTreeMap::from([
                (key("counter"), Type::U64),
                (key("payload"), Type::Any),
            ]),
        }
    }

    fn frame() -> WorkflowFrame {
        WorkflowFrame::new(
            schema(),
            BTreeMap::from([
                (key("counter"), PhenixValue::U64(0)),
                (key("payload"), PhenixValue::Unit),
            ]),
        )
        .unwrap()
    }

    #[test]
    fn forked_snapshots_are_isolated_and_join_selects_declared_fields() {
        let original = frame();
        let mut first = original.clone();
        let mut second = original.clone();
        first.set(&key("counter"), PhenixValue::U64(5)).unwrap();
        second.set(&key("counter"), PhenixValue::U64(9)).unwrap();
        assert_eq!(original.get(&key("counter")), Some(&PhenixValue::U64(0)));
        let mut result = original.clone();
        result.collect_from(&first, &[key("counter")]).unwrap();
        assert_eq!(result.get(&key("counter")), Some(&PhenixValue::U64(5)));
        assert_eq!(second.get(&key("counter")), Some(&PhenixValue::U64(9)));
        assert_eq!(original.get(&key("counter")), Some(&PhenixValue::U64(0)));
    }

    #[test]
    fn missing_extra_and_incorrectly_typed_fields_fail_closed() {
        assert!(matches!(
            WorkflowFrame::new(schema(), BTreeMap::new()),
            Err(WorkflowFrameError::MissingSlot(_))
        ));
        let mut frame = frame();
        assert!(matches!(
            frame.set(&key("other"), PhenixValue::Unit),
            Err(WorkflowFrameError::UnknownSlot(_))
        ));
        assert!(matches!(
            frame.set(&key("counter"), PhenixValue::String("not u64".into())),
            Err(WorkflowFrameError::InvalidValue { .. })
        ));
        assert_eq!(frame.get(&key("counter")), Some(&PhenixValue::U64(0)));
    }

    #[test]
    fn even_any_and_nested_values_cannot_carry_live_capabilities() {
        let reference = CallableRef::new(
            InterfaceId::parse("fixture.capability@1").unwrap(),
            ReferenceOwnerId::Plugin(PluginId::parse("fixture.owner").unwrap()),
            ReferenceGenerationId::parse("fixture.generation").unwrap(),
            ReferenceId::parse("fixture.callback").unwrap(),
        );
        let mut frame = frame();
        let payload = PhenixValue::List(vec![
            PhenixValue::Map(BTreeMap::from([(
                "hidden".into(),
                PhenixValue::Callable(reference),
            )])),
        ]);
        assert!(matches!(
            frame.set(&key("payload"), payload),
            Err(WorkflowFrameError::CapabilityValue(_))
        ));
        let unsafe_schema = WorkflowFrameSchema {
            revision: 1,
            slots: BTreeMap::from([(
                key("secret"),
                Type::List(Box::new(Type::Object {
                    contract: InterfaceId::parse("fixture.object@1").unwrap(),
                })),
            )]),
        };
        assert!(matches!(
            unsafe_schema.validate(),
            Err(WorkflowFrameError::CapabilitySlot(_))
        ));
    }

    #[test]
    fn wrong_revision_and_cross_schema_join_are_rejected() {
        let mut bad = schema();
        bad.revision = 0;
        assert!(matches!(bad.validate(), Err(WorkflowFrameError::InvalidRevision)));
        let mut different = schema();
        different.revision = 2;
        let other = WorkflowFrame::new(different, (*frame().values).clone()).unwrap();
        assert!(matches!(
            frame().collect_from(&other, &[key("counter")]),
            Err(WorkflowFrameError::IncompatibleSchema)
        ));
    }
}
