//! Typed workflow data, independent of provider and host capability handles.
//!
//! A frame owns values only. Authority stays with the root and never crosses
//! an Invoke/Fork/Join transition through serialized frame contents.

use crate::{Key, PhenixSchema, PhenixValue};
use serde::{Deserialize, Serialize, de::Error as _};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::Arc,
};

/// Portable frame schema decoding must reject repeated keys, including
/// byte-identical duplicates. Canonicalization cannot silently choose one.
fn deserialize_unique_frame_slots<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<Key, PhenixSchema>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct UniqueSlots;

    impl<'de> serde::de::Visitor<'de> for UniqueSlots {
        type Value = BTreeMap<Key, PhenixSchema>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a typed frame schema without duplicate slot identities")
        }

        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut entries: M,
        ) -> Result<Self::Value, M::Error> {
            let mut result = BTreeMap::new();
            while let Some((key, value)) = entries.next_entry::<Key, PhenixSchema>()? {
                if result.insert(key.clone(), value).is_some() {
                    return Err(M::Error::custom(format!("duplicate frame slot {key}")));
                }
            }
            Ok(result)
        }
    }

    deserializer.deserialize_map(UniqueSlots)
}

/// Versioned schema for the values visible to an execution plan.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowFrameSchema {
    pub revision: u64,
    #[serde(deserialize_with = "deserialize_unique_frame_slots")]
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
    DuplicateOutputSlot(Key),
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
        PhenixSchema::Option(inner) | PhenixSchema::List(inner) | PhenixSchema::Map(inner) => {
            schema_contains_capability(inner)
        }
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

    pub(crate) fn validate_value(&self, slot: &Key, value: &PhenixValue) -> Result<(), WorkflowFrameError> {
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

    /// Copy declared source fields into target fields as one atomic handoff.
    ///
    /// All reads observe the same pre-handoff snapshot, including swaps and
    /// overlapping aliases. No partial target mutation is exposed if a slot
    /// is missing or fails schema validation. This only moves data; it never
    /// delegates authority or references to live host objects.
    pub fn transfer_slots(
        &mut self,
        mappings: &BTreeMap<Key, Key>,
    ) -> Result<(), WorkflowFrameError> {
        let mut candidate = self.clone();
        let mut destinations = BTreeSet::new();
        for (source, target) in mappings {
            if !destinations.insert(target) {
                return Err(WorkflowFrameError::DuplicateOutputSlot(target.clone()));
            }
            let value = self
                .get(source)
                .ok_or_else(|| WorkflowFrameError::UnknownSlot(source.clone()))?;
            candidate.set(target, value.clone())?;
        }
        *self = candidate;
        Ok(())
    }

    /// Prepare a genuinely isolated subplan frame using only declared inputs.
    ///
    /// The child has its own schema and local identities. There is no implicit
    /// read access to parent fields, even if they have the same name. Private
    /// child fields require explicit initialized values. Input aliases are
    /// validated before the child can execute.
    pub fn isolate_subplan(
        &self,
        child_schema: WorkflowFrameSchema,
        inputs: &BTreeMap<Key, Key>,
        mut private_initial: BTreeMap<Key, PhenixValue>,
    ) -> Result<Self, WorkflowFrameError> {
        let mut destinations = BTreeSet::new();
        for (parent_slot, child_slot) in inputs {
            if !destinations.insert(child_slot) || private_initial.contains_key(child_slot) {
                return Err(WorkflowFrameError::DuplicateOutputSlot(child_slot.clone()));
            }
            let value = self
                .get(parent_slot)
                .ok_or_else(|| WorkflowFrameError::UnknownSlot(parent_slot.clone()))?;
            private_initial.insert(child_slot.clone(), value.clone());
        }
        Self::new(child_schema, private_initial)
    }

    /// Publish *only* declared local child outputs into the parent as one
    /// atomic data transfer, without leaking any child-private fields.
    /// Independently authored child and parent schemas may differ.
    pub fn publish_subplan(
        &mut self,
        child: &Self,
        outputs: &BTreeMap<Key, Key>,
    ) -> Result<(), WorkflowFrameError> {
        let mut candidate = self.clone();
        let mut destinations = BTreeSet::new();
        for (child_slot, parent_slot) in outputs {
            if !destinations.insert(parent_slot) {
                return Err(WorkflowFrameError::DuplicateOutputSlot(parent_slot.clone()));
            }
            let value = child
                .get(child_slot)
                .ok_or_else(|| WorkflowFrameError::UnknownSlot(child_slot.clone()))?;
            candidate.set(parent_slot, value.clone())?;
        }
        *self = candidate;
        Ok(())
    }

    /// Explicitly select branch-produced data by slot. Unselected parent
    /// values remain intact. The caller must supply a schema-compatible branch.
    /// Core never combines or elevates the branches' authority.
    pub fn collect_from(&mut self, branch: &Self, slots: &[Key]) -> Result<(), WorkflowFrameError> {
        if self.schema != branch.schema {
            return Err(WorkflowFrameError::IncompatibleSchema);
        }
        let mut candidate = self.clone();
        let mut seen = BTreeSet::new();
        for slot in slots {
            if !seen.insert(slot) {
                return Err(WorkflowFrameError::DuplicateOutputSlot(slot.clone()));
            }
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
    use crate::{
        CallableRef, InterfaceId, PluginId, ReferenceGenerationId, ReferenceId, ReferenceOwnerId,
        Type,
    };

    fn key(name: &str) -> Key {
        Key::parse(name).unwrap()
    }

    fn schema() -> WorkflowFrameSchema {
        WorkflowFrameSchema {
            revision: 1,
            slots: BTreeMap::from([(key("counter"), Type::U64), (key("payload"), Type::Any)]),
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
    fn mapped_handoff_is_atomic_and_sources_are_snapshot_isolated() {
        let mut frame = frame();
        frame.set(&key("counter"), PhenixValue::U64(42)).unwrap();
        frame.set(&key("payload"), PhenixValue::U64(7)).unwrap();
        let original = frame.clone();
        assert!(
            frame
                .transfer_slots(&BTreeMap::from([
                    (key("counter"), key("payload")),
                    (key("payload"), key("missing")),
                ]))
                .is_err()
        );
        assert_eq!(frame, original);
        assert!(matches!(
            frame.transfer_slots(&BTreeMap::from([
                (key("counter"), key("payload")),
                (key("payload"), key("payload")),
            ])),
            Err(WorkflowFrameError::DuplicateOutputSlot(_))
        ));
        let mut frame = original.clone();
        frame
            .transfer_slots(&BTreeMap::from([
                (key("counter"), key("payload")),
                (key("payload"), key("counter")),
            ]))
            .unwrap();
        assert_eq!(frame.get(&key("counter")), Some(&PhenixValue::U64(7)));
        assert_eq!(frame.get(&key("payload")), Some(&PhenixValue::U64(42)));
        assert_eq!(original.get(&key("counter")), Some(&PhenixValue::U64(42)));
    }

    #[test]
    fn independently_included_subplans_isolate_private_values_and_publish_atomically() {
        let parent = WorkflowFrame::new(
            WorkflowFrameSchema {
                revision: 1,
                slots: BTreeMap::from([
                    (key("public_input"), Type::U64),
                    (key("public_output"), Type::U64),
                    (key("parent_secret"), Type::String),
                ]),
            },
            BTreeMap::from([
                (key("public_input"), PhenixValue::U64(5)),
                (key("public_output"), PhenixValue::U64(0)),
                (key("parent_secret"), PhenixValue::String("opaque".into())),
            ]),
        )
        .unwrap();
        let child_schema = WorkflowFrameSchema {
            revision: 3,
            slots: BTreeMap::from([
                (key("input"), Type::U64),
                (key("local_private"), Type::U64),
                (key("result"), Type::U64),
            ]),
        };
        let inputs = BTreeMap::from([(key("public_input"), key("input"))]);
        let initial = BTreeMap::from([
            (key("local_private"), PhenixValue::U64(0)),
            (key("result"), PhenixValue::U64(0)),
        ]);
        let mut first = parent
            .isolate_subplan(child_schema.clone(), &inputs, initial.clone())
            .unwrap();
        let second = parent
            .isolate_subplan(child_schema, &inputs, initial)
            .unwrap();
        assert_eq!(first.get(&key("input")), Some(&PhenixValue::U64(5)));
        assert_eq!(first.get(&key("parent_secret")), None);
        assert_eq!(
            second.get(&key("local_private")),
            Some(&PhenixValue::U64(0))
        );
        assert!(first.set(&key("parent_secret"), PhenixValue::Unit).is_err());
        first
            .set(&key("local_private"), PhenixValue::U64(44))
            .unwrap();
        first.set(&key("result"), PhenixValue::U64(77)).unwrap();

        let mut published = parent.clone();
        let before = published.clone();
        assert!(
            published
                .publish_subplan(
                    &first,
                    &BTreeMap::from([
                        (key("result"), key("public_output")),
                        (key("local_private"), key("parent_secret")),
                    ]),
                )
                .is_err()
        );
        assert_eq!(
            published, before,
            "failed output publication must roll back"
        );
        published
            .publish_subplan(
                &first,
                &BTreeMap::from([(key("result"), key("public_output"))]),
            )
            .unwrap();
        assert_eq!(
            published.get(&key("public_output")),
            Some(&PhenixValue::U64(77))
        );
        assert_eq!(
            published.get(&key("parent_secret")),
            parent.get(&key("parent_secret"))
        );
        assert_eq!(
            second.get(&key("local_private")),
            Some(&PhenixValue::U64(0))
        );
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
        let payload = PhenixValue::List(vec![PhenixValue::Map(BTreeMap::from([(
            "hidden".into(),
            PhenixValue::Callable(reference),
        )]))]);
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
    fn portable_schema_and_join_outputs_reject_duplicate_identities() {
        let encoded =
            r#"{"revision":1,"slots":{"counter":{"type":"u64"},"counter":{"type":"u64"}}}"#;
        assert!(serde_json::from_str::<WorkflowFrameSchema>(encoded).is_err());
        let mut frame = frame();
        let branch = frame.clone();
        assert!(matches!(
            frame.collect_from(&branch, &[key("counter"), key("counter")]),
            Err(WorkflowFrameError::DuplicateOutputSlot(_))
        ));
        assert_eq!(frame.get(&key("counter")), Some(&PhenixValue::U64(0)));
    }

    #[test]
    fn wrong_revision_and_cross_schema_join_are_rejected() {
        let mut bad = schema();
        bad.revision = 0;
        assert!(matches!(
            bad.validate(),
            Err(WorkflowFrameError::InvalidRevision)
        ));
        let mut different = schema();
        different.revision = 2;
        let other = WorkflowFrame::new(different, (*frame().values).clone()).unwrap();
        assert!(matches!(
            frame().collect_from(&other, &[key("counter")]),
            Err(WorkflowFrameError::IncompatibleSchema)
        ));
    }
}
