use crate::{
    Exact, Key, ObjectRef, PhenixSchema, PhenixValue, PluginId, Project, ReferenceId, Type,
    TypeKind, ValueCodec, ValueError,
};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::{
    collections::BTreeMap,
    error::Error,
    fmt::{self, Display, Formatter},
    str::FromStr,
    sync::{Arc, Mutex},
};

pub const OBSERVABLE_CONTRACT: &str = "phenix.observable@1";

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct ValueId(String);

impl ValueId {
    pub fn parse(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        if value.is_empty() {
            return Err("observable value id must not be empty");
        }
        if !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/' | b':' | b'@')
        }) {
            return Err("observable value id contains unsupported characters");
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for ValueId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ValueId {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl TryFrom<String> for ValueId {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl From<ValueId> for String {
    fn from(value: ValueId) -> Self {
        value.0
    }
}

impl ValueCodec for ValueId {
    fn phenix_type() -> Type {
        Type::String
    }

    fn to_value(&self) -> PhenixValue {
        PhenixValue::String(self.0.clone())
    }

    fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        match value {
            PhenixValue::String(value) => Self::parse(value.clone())
                .map_err(|error| ValueError::InvalidValue(error.to_owned())),
            _ => Err(ValueError::TypeMismatch {
                expected: TypeKind::String,
                actual: value.kind(),
            }),
        }
    }
}

impl From<&ValueId> for PhenixValue {
    fn from(value: &ValueId) -> Self {
        value.to_value()
    }
}

impl<'value> TryFrom<Exact<&'value PhenixValue>> for ValueId {
    type Error = ValueError;

    fn try_from(value: Exact<&'value PhenixValue>) -> Result<Self, Self::Error> {
        Self::from_value(value.0)
    }
}

impl<'value> TryFrom<Project<&'value PhenixValue>> for ValueId {
    type Error = ValueError;

    fn try_from(value: Project<&'value PhenixValue>) -> Result<Self, Self::Error> {
        Self::project_from_value(value.0)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ValueVersion(u64);

impl ValueVersion {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CommitId(u64);

impl CommitId {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ObservationId(u64);

impl ObservationId {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ObservationGeneration(u64);

impl ObservationGeneration {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

macro_rules! u64_value_codec {
    ($name:ty) => {
        impl ValueCodec for $name {
            fn phenix_type() -> Type {
                Type::U64
            }

            fn to_value(&self) -> PhenixValue {
                PhenixValue::U64(self.get())
            }

            fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
                match value {
                    PhenixValue::U64(value) => Ok(Self::new(*value)),
                    _ => Err(ValueError::TypeMismatch {
                        expected: TypeKind::U64,
                        actual: value.kind(),
                    }),
                }
            }
        }

        impl From<&$name> for PhenixValue {
            fn from(value: &$name) -> Self {
                <$name as ValueCodec>::to_value(value)
            }
        }

        impl<'value> TryFrom<Exact<&'value PhenixValue>> for $name {
            type Error = ValueError;

            fn try_from(value: Exact<&'value PhenixValue>) -> Result<Self, Self::Error> {
                <Self as ValueCodec>::from_value(value.0)
            }
        }

        impl<'value> TryFrom<Project<&'value PhenixValue>> for $name {
            type Error = ValueError;

            fn try_from(value: Project<&'value PhenixValue>) -> Result<Self, Self::Error> {
                <Self as ValueCodec>::project_from_value(value.0)
            }
        }
    };
}

u64_value_codec!(ValueVersion);
u64_value_codec!(CommitId);
u64_value_codec!(ObservationId);
u64_value_codec!(ObservationGeneration);

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ValuePathSegment {
    Field(Key),
    MapKey(String),
    Index(u32),
    VariantPayload,
    OptionPayload,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ValuePath {
    segments: SmallVec<[ValuePathSegment; 4]>,
}

impl ValuePath {
    #[must_use]
    pub fn root() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn new(segments: impl IntoIterator<Item = ValuePathSegment>) -> Self {
        Self {
            segments: segments.into_iter().collect(),
        }
    }

    #[must_use]
    pub fn segments(&self) -> &[ValuePathSegment] {
        &self.segments
    }

    #[must_use]
    pub fn is_root(&self) -> bool {
        self.segments.is_empty()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.segments.len()
    }

    #[must_use]
    pub fn starts_with(&self, prefix: &Self) -> bool {
        self.segments.starts_with(&prefix.segments)
    }

    #[must_use]
    pub fn relative_to(&self, prefix: &Self) -> Option<Self> {
        self.starts_with(prefix).then(|| Self {
            segments: self.segments[prefix.len()..].iter().cloned().collect(),
        })
    }
}

impl FromIterator<ValuePathSegment> for ValuePath {
    fn from_iter<T: IntoIterator<Item = ValuePathSegment>>(iter: T) -> Self {
        Self::new(iter)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ValueAddress {
    pub value: ValueId,
    pub path: ValuePath,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationScope {
    Exact,
    Recursive,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationMode {
    Diff,
    Full,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotPolicy {
    CurrentOnly,
    CopyOnChange,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InitialObservation {
    None,
    Full,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObservationSpec {
    pub address: ValueAddress,
    pub scope: ObservationScope,
    pub mode: ObservationMode,
    pub initial: InitialObservation,
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ValueChange {
    Replace {
        path: ValuePath,
        value: PhenixValue,
    },
    Remove {
        path: ValuePath,
    },
    Splice {
        path: ValuePath,
        start: u32,
        delete_count: u32,
        inserted: SmallVec<[PhenixValue; 4]>,
    },
}

impl ValueChange {
    #[must_use]
    pub fn path(&self) -> &ValuePath {
        match self {
            Self::Replace { path, .. } | Self::Remove { path } | Self::Splice { path, .. } => path,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObservableSnapshot {
    pub value: ValueId,
    pub version: ValueVersion,
    pub current: Arc<PhenixValue>,
}

pub struct ObservationDelivery<'a> {
    pub commit: Option<CommitId>,
    pub value: &'a ValueId,
    pub from_version: ValueVersion,
    pub version: ValueVersion,
    pub address: &'a ValueAddress,
    pub observation: ObservationId,
    pub generation: ObservationGeneration,
    pub changes: &'a [ValueChange],
    pub snapshot: Option<&'a Arc<PhenixValue>>,
}

impl ObservationDelivery<'_> {
    pub fn full_value(&self) -> Result<Option<&PhenixValue>, ObservableError> {
        let Some(snapshot) = self.snapshot else {
            return Ok(None);
        };
        value_at(snapshot.as_ref(), self.address.path.segments())
            .map(Some)
            .map_err(|message| ObservableError::TransactionConflict {
                value: self.value.clone(),
                message,
            })
    }
}

pub trait ObservationHandler: Send + Sync {
    fn handle(&self, delivery: ObservationDelivery<'_>);
}

impl<F> ObservationHandler for F
where
    F: Fn(ObservationDelivery<'_>) + Send + Sync,
{
    fn handle(&self, delivery: ObservationDelivery<'_>) {
        self(delivery);
    }
}

#[derive(Clone)]
pub struct ObservationSubscription {
    pub id: ObservationId,
    pub generation: ObservationGeneration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObservableError {
    UnknownValue(ValueId),
    DuplicateValue(ValueId),
    InvalidPath {
        value: Box<ValueId>,
        path: Box<ValuePath>,
        message: Box<str>,
    },
    SchemaMismatch {
        value: Box<ValueId>,
        path: Box<ValuePath>,
        message: Box<str>,
    },
    StaleReference(ValueId),
    UnsupportedSnapshotPolicy {
        value: ValueId,
        policy: SnapshotPolicy,
        mode: ObservationMode,
        initial: InitialObservation,
    },
    TransactionConflict {
        value: ValueId,
        message: String,
    },
    SubscriptionCapacity,
    Closed,
}

impl ObservableError {
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::UnknownValue(_) => "unknown_value",
            Self::DuplicateValue(_) => "duplicate_value",
            Self::InvalidPath { .. } => "invalid_path",
            Self::SchemaMismatch { .. } => "schema_mismatch",
            Self::StaleReference(_) => "stale_reference",
            Self::UnsupportedSnapshotPolicy { .. } => "unsupported_snapshot_policy",
            Self::TransactionConflict { .. } => "transaction_conflict",
            Self::SubscriptionCapacity => "subscription_capacity",
            Self::Closed => "closed",
        }
    }
}

impl Display for ObservableError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownValue(value) => write!(f, "unknown observable value {value}"),
            Self::DuplicateValue(value) => write!(f, "observable value {value} is already registered"),
            Self::InvalidPath { value, message, .. } => {
                write!(f, "invalid path for observable value {value}: {message}")
            }
            Self::SchemaMismatch { value, message, .. } => {
                write!(f, "observable value {value} schema mismatch: {message}")
            }
            Self::StaleReference(value) => write!(f, "stale observable reference {value}"),
            Self::UnsupportedSnapshotPolicy {
                value,
                policy,
                mode,
                initial,
            } => write!(
                f,
                "observable value {value} with {policy:?} cannot satisfy {mode:?}/{initial:?}"
            ),
            Self::TransactionConflict { value, message } => {
                write!(f, "observable transaction conflict for {value}: {message}")
            }
            Self::SubscriptionCapacity => f.write_str("observable subscription capacity reached"),
            Self::Closed => f.write_str("observable store is closed"),
        }
    }
}

impl Error for ObservableError {}

#[derive(Clone, Debug, PartialEq)]
pub struct ObservableRegistration {
    pub id: ValueId,
    pub owner: PluginId,
    pub schema: PhenixSchema,
    pub snapshot_policy: SnapshotPolicy,
    pub initial: PhenixValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservableMetadata {
    pub id: ValueId,
    pub owner: PluginId,
    pub schema: PhenixSchema,
    pub version: ValueVersion,
    pub snapshot_policy: SnapshotPolicy,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ObservableRef(ObjectRef);

impl ObservableRef {
    pub fn new(
        provider: PluginId,
        generation: crate::GraphGenerationId,
        id: ReferenceId,
    ) -> Self {
        Self(ObjectRef::for_plugin(
            crate::ContractId::parse(OBSERVABLE_CONTRACT)
                .expect("static observable contract id is valid"),
            provider,
            &generation,
            id,
        ))
    }

    #[must_use]
    pub fn as_object(&self) -> &ObjectRef {
        &self.0
    }

    #[must_use]
    pub fn into_object(self) -> ObjectRef {
        self.0
    }
}

impl ValueCodec for ObservableRef {
    fn phenix_type() -> Type {
        Type::Object {
            contract: crate::ContractId::parse(OBSERVABLE_CONTRACT)
                .expect("static observable contract id is valid"),
        }
    }

    fn to_value(&self) -> PhenixValue {
        PhenixValue::Object(self.0.clone())
    }

    fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        Self::phenix_type().parse(value)?;
        match value {
            PhenixValue::Object(reference) => Ok(Self(reference.clone())),
            _ => Err(ValueError::TypeMismatch {
                expected: TypeKind::Object,
                actual: value.kind(),
            }),
        }
    }
}

impl From<&ObservableRef> for PhenixValue {
    fn from(value: &ObservableRef) -> Self {
        value.to_value()
    }
}

impl<'value> TryFrom<Exact<&'value PhenixValue>> for ObservableRef {
    type Error = ValueError;

    fn try_from(value: Exact<&'value PhenixValue>) -> Result<Self, Self::Error> {
        Self::from_value(value.0)
    }
}

impl<'value> TryFrom<Project<&'value PhenixValue>> for ObservableRef {
    type Error = ValueError;

    fn try_from(value: Project<&'value PhenixValue>) -> Result<Self, Self::Error> {
        Self::project_from_value(value.0)
    }
}

#[derive(Clone)]
struct SubscriptionEntry {
    id: ObservationId,
    spec: ObservationSpec,
    generation: ObservationGeneration,
    handler: Arc<dyn ObservationHandler>,
}

#[derive(Default)]
struct SubscriptionNode {
    exact: SmallVec<[ObservationId; 2]>,
    recursive: SmallVec<[ObservationId; 2]>,
    children: BTreeMap<ValuePathSegment, SubscriptionNode>,
}

impl SubscriptionNode {
    fn insert(&mut self, path: &[ValuePathSegment], id: ObservationId, scope: ObservationScope) {
        if let Some((head, tail)) = path.split_first() {
            self.children
                .entry(head.clone())
                .or_default()
                .insert(tail, id, scope);
            return;
        }
        match scope {
            ObservationScope::Exact => self.exact.push(id),
            ObservationScope::Recursive => self.recursive.push(id),
        }
    }

    fn remove(&mut self, path: &[ValuePathSegment], id: ObservationId, scope: ObservationScope) {
        if let Some((head, tail)) = path.split_first() {
            let remove_child = if let Some(child) = self.children.get_mut(head) {
                child.remove(tail, id, scope);
                child.is_empty()
            } else {
                false
            };
            if remove_child {
                self.children.remove(head);
            }
            return;
        }
        let ids = match scope {
            ObservationScope::Exact => &mut self.exact,
            ObservationScope::Recursive => &mut self.recursive,
        };
        ids.retain(|candidate| *candidate != id);
    }

    fn is_empty(&self) -> bool {
        self.exact.is_empty() && self.recursive.is_empty() && self.children.is_empty()
    }

    fn collect_all(&self, output: &mut SmallVec<[ObservationId; 8]>) {
        extend_unique(output, self.exact.iter().copied());
        extend_unique(output, self.recursive.iter().copied());
        for child in self.children.values() {
            child.collect_all(output);
        }
    }

    fn collect_for_path(
        &self,
        path: &[ValuePathSegment],
        output: &mut SmallVec<[ObservationId; 8]>,
    ) {
        extend_unique(output, self.recursive.iter().copied());
        let Some((head, tail)) = path.split_first() else {
            extend_unique(output, self.exact.iter().copied());
            self.collect_all(output);
            return;
        };
        if let Some(child) = self.children.get(head) {
            child.collect_for_path(tail, output);
        }
    }

    fn collect_for_splice(
        &self,
        list_path: &[ValuePathSegment],
        start: u32,
        delete_count: u32,
        inserted_count: u32,
        output: &mut SmallVec<[ObservationId; 8]>,
    ) {
        extend_unique(output, self.recursive.iter().copied());
        let Some((head, tail)) = list_path.split_first() else {
            extend_unique(output, self.exact.iter().copied());
            extend_unique(output, self.recursive.iter().copied());
            for (segment, child) in &self.children {
                let ValuePathSegment::Index(index) = segment else {
                    continue;
                };
                let affected = if inserted_count == delete_count {
                    *index >= start && *index < start.saturating_add(delete_count)
                } else {
                    *index >= start
                };
                if affected {
                    child.collect_all(output);
                }
            }
            return;
        };
        if let Some(child) = self.children.get(head) {
            child.collect_for_splice(tail, start, delete_count, inserted_count, output);
        }
    }
}

fn extend_unique(
    output: &mut SmallVec<[ObservationId; 8]>,
    values: impl IntoIterator<Item = ObservationId>,
) {
    for value in values {
        if !output.contains(&value) {
            output.push(value);
        }
    }
}
