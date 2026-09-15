use crate::{
    CapabilityGenerationId, ClientConnectionId, GraphGenerationId, InterfaceId, PluginId, RuntimeId,
};
use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};
use std::{
    borrow::Borrow,
    collections::BTreeMap,
    error::Error,
    fmt::{self, Display, Formatter},
    str::FromStr,
};

pub type ContractId = InterfaceId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Exact<T>(pub T);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Project<T>(pub T);

impl<'value, T> TryFrom<&'value PhenixValue> for Exact<T>
where
    T: TryFrom<Exact<&'value PhenixValue>, Error = ValueError>,
{
    type Error = ValueError;

    fn try_from(value: &'value PhenixValue) -> Result<Self, Self::Error> {
        T::try_from(Exact(value)).map(Self)
    }
}

impl<'value, T> TryFrom<&'value PhenixValue> for Project<T>
where
    T: TryFrom<Project<&'value PhenixValue>, Error = ValueError>,
{
    type Error = ValueError;

    fn try_from(value: &'value PhenixValue) -> Result<Self, Self::Error> {
        T::try_from(Project(value)).map(Self)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Key(String);

impl Key {
    pub fn parse(value: impl Into<String>) -> Result<Self, &'static str> {
        value.into().try_into()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Key {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.to_owned().try_into()
    }
}

impl TryFrom<String> for Key {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err("structural key must not be empty");
        }
        if value.chars().any(char::is_control) {
            return Err("structural key must not contain control characters");
        }
        Ok(Self(value))
    }
}

impl Borrow<str> for Key {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl Display for Key {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for Key {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ReferenceId(String);

impl ReferenceId {
    pub fn parse(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        if value.is_empty() {
            return Err("reference id must not be empty");
        }
        if !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/' | b':' | b'@')
        }) {
            return Err("reference id contains unsupported characters");
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for ReferenceId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ReferenceId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ReferenceId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum CapabilityOwnerId {
    Plugin(PluginId),
    Client(ClientConnectionId),
    Runtime(RuntimeId),
}

impl CapabilityOwnerId {
    #[must_use]
    pub const fn plugin(provider: PluginId) -> Self {
        Self::Plugin(provider)
    }
}

impl From<PluginId> for CapabilityOwnerId {
    fn from(value: PluginId) -> Self {
        Self::Plugin(value)
    }
}

impl From<ClientConnectionId> for CapabilityOwnerId {
    fn from(value: ClientConnectionId) -> Self {
        Self::Client(value)
    }
}

impl From<RuntimeId> for CapabilityOwnerId {
    fn from(value: RuntimeId) -> Self {
        Self::Runtime(value)
    }
}

impl From<&GraphGenerationId> for CapabilityGenerationId {
    fn from(value: &GraphGenerationId) -> Self {
        Self::parse(value.as_str()).expect("graph generation ids are capability generation ids")
    }
}

macro_rules! capability_ref {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            contract: ContractId,
            owner: CapabilityOwnerId,
            generation: CapabilityGenerationId,
            id: ReferenceId,
        }

        impl $name {
            pub fn new(
                contract: ContractId,
                owner: CapabilityOwnerId,
                generation: CapabilityGenerationId,
                id: ReferenceId,
            ) -> Self {
                Self {
                    contract,
                    owner,
                    generation,
                    id,
                }
            }

            pub fn contract(&self) -> &ContractId {
                &self.contract
            }

            pub fn owner(&self) -> &CapabilityOwnerId {
                &self.owner
            }

            pub fn generation(&self) -> &CapabilityGenerationId {
                &self.generation
            }

            pub fn for_plugin(
                contract: ContractId,
                provider: PluginId,
                generation: &GraphGenerationId,
                id: ReferenceId,
            ) -> Self {
                Self::new(
                    contract,
                    CapabilityOwnerId::Plugin(provider),
                    CapabilityGenerationId::from(generation),
                    id,
                )
            }

            pub fn id(&self) -> &ReferenceId {
                &self.id
            }
        }
    };
}

capability_ref!(CallableRef);
capability_ref!(ObjectRef);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Type {
    Any,
    Never,
    Unit,
    Bool,
    I64,
    U64,
    F64,
    String,
    Bytes,
    Option(Box<Type>),
    Array {
        item: Box<Type>,
        len: usize,
    },
    List(Box<Type>),
    Map(Box<Type>),
    Table(BTreeMap<Key, Type>),
    Variant(BTreeMap<Key, Type>),
    Callable {
        contract: ContractId,
        input: Box<Type>,
        output: Box<Type>,
    },
    Object {
        contract: ContractId,
    },
}

pub type PhenixSchema = Type;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SchemaCompatibility {
    Exact,
    Compatible,
    Incompatible(SchemaMismatch),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaMismatch {
    path: Vec<String>,
    reason: String,
}

impl SchemaMismatch {
    fn new(path: Vec<String>, reason: impl Into<String>) -> Self {
        Self {
            path,
            reason: reason.into(),
        }
    }

    pub fn path(&self) -> &[String] {
        &self.path
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl Display for SchemaMismatch {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            return f.write_str(&self.reason);
        }
        write!(f, "{}: {}", self.path.join("."), self.reason)
    }
}

impl Error for SchemaMismatch {}

impl Type {
    pub fn accepts(&self, provided: &Self) -> SchemaCompatibility {
        if self == provided {
            return SchemaCompatibility::Exact;
        }
        match schema_compatible(provided, self, &[]) {
            Ok(()) => SchemaCompatibility::Compatible,
            Err(error) => SchemaCompatibility::Incompatible(error),
        }
    }
    pub fn parse(&self, value: &PhenixValue) -> Result<(), ValueError> {
        match (self, value) {
            (Self::Any, _) => Ok(()),
            (Self::Unit, PhenixValue::Unit)
            | (Self::Bool, PhenixValue::Bool(_))
            | (Self::I64, PhenixValue::I64(_))
            | (Self::U64, PhenixValue::U64(_))
            | (Self::F64, PhenixValue::F64(_))
            | (Self::String, PhenixValue::String(_))
            | (Self::Bytes, PhenixValue::Bytes(_)) => Ok(()),
            (Self::Option(expected), PhenixValue::Option(Some(value))) => expected.parse(value),
            (Self::Option(_), PhenixValue::Option(None)) => Ok(()),
            (Self::Array { item, len }, PhenixValue::List(values)) => {
                if values.len() != *len {
                    return Err(ValueError::InvalidValue(format!(
                        "expected {} values, got {}",
                        *len,
                        values.len()
                    )));
                }
                values.iter().try_for_each(|value| item.parse(value))
            }
            (Self::List(expected), PhenixValue::List(values)) => {
                values.iter().try_for_each(|value| expected.parse(value))
            }
            (Self::Map(expected), PhenixValue::Map(values)) => {
                values.values().try_for_each(|value| expected.parse(value))
            }
            (Self::Table(expected), PhenixValue::Table(values)) => parse_table(expected, values),
            (Self::Variant(variants), PhenixValue::Variant { tag, value }) => variants
                .get(tag)
                .ok_or_else(|| ValueError::UnknownVariant(tag.clone()))?
                .parse(value),
            (Self::Callable { contract, .. }, PhenixValue::Callable(reference)) => {
                parse_reference_contract(contract, reference.contract())
            }
            (Self::Object { contract }, PhenixValue::Object(reference)) => {
                parse_reference_contract(contract, reference.contract())
            }
            _ => Err(ValueError::TypeMismatch {
                expected: self.kind(),
                actual: value.kind(),
            }),
        }
    }

    pub fn kind(&self) -> TypeKind {
        match self {
            Self::Any => TypeKind::Any,
            Self::Never => TypeKind::Never,
            Self::Unit => TypeKind::Unit,
            Self::Bool => TypeKind::Bool,
            Self::I64 => TypeKind::I64,
            Self::U64 => TypeKind::U64,
            Self::F64 => TypeKind::F64,
            Self::String => TypeKind::String,
            Self::Bytes => TypeKind::Bytes,
            Self::Option(_) => TypeKind::Option,
            Self::Array { .. } => TypeKind::Array,
            Self::List(_) => TypeKind::List,
            Self::Map(_) => TypeKind::Map,
            Self::Table(_) => TypeKind::Table,
            Self::Variant(_) => TypeKind::Variant,
            Self::Callable { .. } => TypeKind::Callable,
            Self::Object { .. } => TypeKind::Object,
        }
    }
}

fn nested_path(path: &[String], segment: impl Into<String>) -> Vec<String> {
    let mut nested = path.to_vec();
    nested.push(segment.into());
    nested
}

fn schema_compatible(
    provided: &Type,
    accepted: &Type,
    path: &[String],
) -> Result<(), SchemaMismatch> {
    if provided == accepted {
        return Ok(());
    }

    match (provided, accepted) {
        (Type::Never, _) | (_, Type::Any) => Ok(()),
        (Type::Option(provided), Type::Option(accepted))
        | (Type::List(provided), Type::List(accepted))
        | (Type::Map(provided), Type::Map(accepted)) => schema_compatible(provided, accepted, path),
        (
            Type::Array {
                item: provided,
                len: provided_len,
            },
            Type::Array {
                item: accepted,
                len: accepted_len,
            },
        ) => {
            if provided_len != accepted_len {
                return Err(SchemaMismatch::new(
                    path.to_vec(),
                    format!(
                        "provided array length {provided_len} does not satisfy required length {accepted_len}"
                    ),
                ));
            }
            schema_compatible(provided, accepted, path)
        }
        (Type::Array { item: provided, .. }, Type::List(accepted)) => {
            schema_compatible(provided, accepted, path)
        }
        (Type::Table(provided), Type::Table(accepted)) => {
            for (key, accepted) in accepted {
                let field_path = nested_path(path, key.to_string());
                let provided = provided.get(key).ok_or_else(|| {
                    SchemaMismatch::new(field_path.clone(), "required field is not provided")
                })?;
                schema_compatible(provided, accepted, &field_path)?;
            }
            Ok(())
        }
        (Type::Variant(provided), Type::Variant(accepted)) => {
            for (key, provided) in provided {
                let variant_path = nested_path(path, key.to_string());
                let accepted = accepted.get(key).ok_or_else(|| {
                    SchemaMismatch::new(
                        variant_path.clone(),
                        "provider may emit a variant the consumer does not accept",
                    )
                })?;
                schema_compatible(provided, accepted, &variant_path)?;
            }
            Ok(())
        }
        (
            Type::Callable {
                contract: provided_contract,
                input: provided_input,
                output: provided_output,
            },
            Type::Callable {
                contract: accepted_contract,
                input: accepted_input,
                output: accepted_output,
            },
        ) => {
            if provided_contract != accepted_contract {
                return Err(SchemaMismatch::new(
                    path.to_vec(),
                    format!(
                        "provided contract {provided_contract} does not satisfy {accepted_contract}"
                    ),
                ));
            }
            schema_compatible(accepted_input, provided_input, &nested_path(path, "input"))?;
            schema_compatible(
                provided_output,
                accepted_output,
                &nested_path(path, "output"),
            )
        }
        (Type::Object { contract: provided }, Type::Object { contract: accepted })
            if provided == accepted =>
        {
            Ok(())
        }
        _ => Err(SchemaMismatch::new(
            path.to_vec(),
            format!(
                "provided {} does not satisfy required {}",
                provided.kind(),
                accepted.kind()
            ),
        )),
    }
}

fn join_schema(left: Type, right: Type) -> Type {
    if left == right {
        return left;
    }

    match (left, right) {
        (Type::Never, other) | (other, Type::Never) => other,
        (Type::Any, _) | (_, Type::Any) => Type::Any,
        (Type::Option(left), Type::Option(right)) => {
            Type::Option(Box::new(join_schema(*left, *right)))
        }
        (
            Type::Array {
                item: left,
                len: left_len,
            },
            Type::Array {
                item: right,
                len: right_len,
            },
        ) if left_len == right_len => Type::Array {
            item: Box::new(join_schema(*left, *right)),
            len: left_len,
        },
        (Type::Array { item: left, .. }, Type::Array { item: right, .. })
        | (Type::Array { item: left, .. }, Type::List(right))
        | (Type::List(left), Type::Array { item: right, .. })
        | (Type::List(left), Type::List(right)) => Type::List(Box::new(join_schema(*left, *right))),
        (Type::Map(left), Type::Map(right)) => Type::Map(Box::new(join_schema(*left, *right))),
        (Type::Table(left), Type::Table(right)) => Type::Table(
            left.into_iter()
                .filter_map(|(key, left)| {
                    right
                        .get(&key)
                        .cloned()
                        .map(|right| (key, join_schema(left, right)))
                })
                .collect(),
        ),
        (Type::Variant(mut left), Type::Variant(right)) => {
            for (key, right) in right {
                match left.remove(&key) {
                    Some(left_value) => {
                        left.insert(key, join_schema(left_value, right));
                    }
                    None => {
                        left.insert(key, right);
                    }
                }
            }
            Type::Variant(left)
        }
        (Type::Object { contract: left }, Type::Object { contract: right }) if left == right => {
            Type::Object { contract: left }
        }
        (
            Type::Callable {
                contract: left,
                input: left_input,
                output: left_output,
            },
            Type::Callable {
                contract: right,
                input: right_input,
                output: right_output,
            },
        ) if left == right && left_input == right_input && left_output == right_output => {
            Type::Callable {
                contract: left,
                input: left_input,
                output: left_output,
            }
        }
        _ => Type::Any,
    }
}

fn join_value_schemas<'value>(values: impl IntoIterator<Item = &'value PhenixValue>) -> Type {
    values
        .into_iter()
        .map(PhenixValue::schema)
        .reduce(join_schema)
        .unwrap_or(Type::Never)
}

fn parse_table(
    expected: &BTreeMap<Key, Type>,
    values: &BTreeMap<Key, PhenixValue>,
) -> Result<(), ValueError> {
    for (key, expected) in expected {
        expected.parse(
            values
                .get(key)
                .ok_or_else(|| ValueError::MissingKey(key.clone()))?,
        )?;
    }
    if let Some(key) = values.keys().find(|key| !expected.contains_key(*key)) {
        return Err(ValueError::UnexpectedKey(key.clone()));
    }
    Ok(())
}

fn parse_reference_contract(expected: &ContractId, actual: &ContractId) -> Result<(), ValueError> {
    if expected == actual {
        return Ok(());
    }
    Err(ValueError::ContractMismatch {
        expected: expected.clone(),
        actual: actual.clone(),
    })
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum PhenixValue {
    Unit,
    Bool(bool),
    I64(i64),
    U64(u64),
    F64(f64),
    String(String),
    Bytes(Vec<u8>),
    Option(Option<Box<PhenixValue>>),
    List(Vec<PhenixValue>),
    Map(BTreeMap<String, PhenixValue>),
    Table(BTreeMap<Key, PhenixValue>),
    Variant { tag: Key, value: Box<PhenixValue> },
    Callable(CallableRef),
    Object(ObjectRef),
}

#[derive(Debug, PartialEq)]
pub enum ValueMatch<T> {
    Exact(T),
    Compatible(T),
    Incompatible(ValueError),
}

impl<'value, T> From<&'value PhenixValue> for ValueMatch<T>
where
    T: HasPhenixSchema
        + TryFrom<Exact<&'value PhenixValue>, Error = ValueError>
        + TryFrom<Project<&'value PhenixValue>, Error = ValueError>,
{
    fn from(value: &'value PhenixValue) -> Self {
        if T::phenix_schema().parse(value).is_ok() {
            return match T::try_from(Exact(value)) {
                Ok(value) => Self::Exact(value),
                Err(error) => Self::Incompatible(error),
            };
        }
        match T::try_from(Project(value)) {
            Ok(value) => Self::Compatible(value),
            Err(error) => Self::Incompatible(error),
        }
    }
}

impl<T> From<PhenixValue> for ValueMatch<T>
where
    T: HasPhenixSchema,
    for<'value> T: TryFrom<Exact<&'value PhenixValue>, Error = ValueError>
        + TryFrom<Project<&'value PhenixValue>, Error = ValueError>,
{
    fn from(value: PhenixValue) -> Self {
        Self::from(&value)
    }
}

impl PhenixValue {
    pub fn schema(&self) -> PhenixSchema {
        match self {
            Self::Unit => Type::Unit,
            Self::Bool(_) => Type::Bool,
            Self::I64(_) => Type::I64,
            Self::U64(_) => Type::U64,
            Self::F64(_) => Type::F64,
            Self::String(_) => Type::String,
            Self::Bytes(_) => Type::Bytes,
            Self::Option(Some(value)) => Type::Option(Box::new(value.schema())),
            Self::Option(None) => Type::Option(Box::new(Type::Never)),
            Self::List(values) => Type::Array {
                item: Box::new(join_value_schemas(values)),
                len: values.len(),
            },
            Self::Map(values) => Type::Map(Box::new(join_value_schemas(values.values()))),
            Self::Table(values) => Type::Table(
                values
                    .iter()
                    .map(|(key, value)| (key.clone(), value.schema()))
                    .collect(),
            ),
            Self::Variant { tag, value } => {
                Type::Variant(BTreeMap::from([(tag.clone(), value.schema())]))
            }
            Self::Callable(reference) => Type::Callable {
                contract: reference.contract().clone(),
                input: Box::new(Type::Any),
                output: Box::new(Type::Never),
            },
            Self::Object(reference) => Type::Object {
                contract: reference.contract().clone(),
            },
        }
    }

    pub fn satisfies(&self, schema: &PhenixSchema) -> SchemaCompatibility {
        schema.accepts(&self.schema())
    }

    pub fn match_as<T>(&self) -> ValueMatch<T>
    where
        T: HasPhenixSchema,
        for<'value> T: TryFrom<Exact<&'value PhenixValue>, Error = ValueError>
            + TryFrom<Project<&'value PhenixValue>, Error = ValueError>,
    {
        ValueMatch::from(self)
    }
    pub fn kind(&self) -> TypeKind {
        match self {
            Self::Unit => TypeKind::Unit,
            Self::Bool(_) => TypeKind::Bool,
            Self::I64(_) => TypeKind::I64,
            Self::U64(_) => TypeKind::U64,
            Self::F64(_) => TypeKind::F64,
            Self::String(_) => TypeKind::String,
            Self::Bytes(_) => TypeKind::Bytes,
            Self::Option(_) => TypeKind::Option,
            Self::List(_) => TypeKind::List,
            Self::Map(_) => TypeKind::Map,
            Self::Table(_) => TypeKind::Table,
            Self::Variant { .. } => TypeKind::Variant,
            Self::Callable(_) => TypeKind::Callable,
            Self::Object(_) => TypeKind::Object,
        }
    }

    pub fn get(&self, key: &str) -> Result<&PhenixValue, ValueError> {
        let Self::Table(table) = self else {
            return Err(ValueError::TypeMismatch {
                expected: TypeKind::Table,
                actual: self.kind(),
            });
        };
        table
            .get(key)
            .ok_or_else(|| ValueError::MissingKey(error_key(key)))
    }

    pub fn value<T>(&self) -> Result<T, ValueError>
    where
        for<'value> T: TryFrom<Exact<&'value PhenixValue>, Error = ValueError>,
    {
        self.exact()
    }

    pub fn exact<T>(&self) -> Result<T, ValueError>
    where
        for<'value> T: TryFrom<Exact<&'value PhenixValue>, Error = ValueError>,
    {
        T::try_from(Exact(self))
    }

    pub fn project<T>(&self) -> Result<T, ValueError>
    where
        for<'value> T: TryFrom<Project<&'value PhenixValue>, Error = ValueError>,
    {
        T::try_from(Project(self))
    }

    pub fn list(&self) -> Result<&[PhenixValue], ValueError> {
        match self {
            Self::List(values) => Ok(values),
            _ => Err(ValueError::TypeMismatch {
                expected: TypeKind::List,
                actual: self.kind(),
            }),
        }
    }

    pub fn variant(&self) -> Result<(&Key, &PhenixValue), ValueError> {
        match self {
            Self::Variant { tag, value } => Ok((tag, value)),
            _ => Err(ValueError::TypeMismatch {
                expected: TypeKind::Variant,
                actual: self.kind(),
            }),
        }
    }

    pub fn callable(&self) -> Result<&CallableRef, ValueError> {
        match self {
            Self::Callable(reference) => Ok(reference),
            _ => Err(ValueError::TypeMismatch {
                expected: TypeKind::Callable,
                actual: self.kind(),
            }),
        }
    }

    pub fn object(&self) -> Result<&ObjectRef, ValueError> {
        match self {
            Self::Object(reference) => Ok(reference),
            _ => Err(ValueError::TypeMismatch {
                expected: TypeKind::Object,
                actual: self.kind(),
            }),
        }
    }
}

impl From<&PhenixValue> for PhenixSchema {
    fn from(value: &PhenixValue) -> Self {
        value.schema()
    }
}

impl From<PhenixValue> for PhenixSchema {
    fn from(value: PhenixValue) -> Self {
        Self::from(&value)
    }
}

fn error_key(key: &str) -> Key {
    Key::parse(key.to_owned()).unwrap_or_else(|_| {
        Key::parse("<invalid-key>").expect("static fallback structural key is valid")
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeKind {
    Any,
    Never,
    Unit,
    Bool,
    I64,
    U64,
    F64,
    String,
    Bytes,
    Option,
    Array,
    List,
    Map,
    Table,
    Variant,
    Callable,
    Object,
}

impl Display for TypeKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValueError {
    TypeMismatch {
        expected: TypeKind,
        actual: TypeKind,
    },
    MissingKey(Key),
    UnexpectedKey(Key),
    UnknownVariant(Key),
    ContractMismatch {
        expected: ContractId,
        actual: ContractId,
    },
    InvalidValue(String),
}

impl ValueError {
    pub fn missing_key(key: impl Into<String>) -> Self {
        Self::MissingKey(error_key(&key.into()))
    }

    pub fn unknown_variant(key: Key) -> Self {
        Self::UnknownVariant(key)
    }

    pub fn contract_mismatch(expected: ContractId, actual: ContractId) -> Self {
        Self::ContractMismatch { expected, actual }
    }
}

impl Display for ValueError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::TypeMismatch { expected, actual } => {
                write!(f, "expected {expected}, got {actual}")
            }
            Self::MissingKey(key) => write!(f, "missing key {key}"),
            Self::UnexpectedKey(key) => write!(f, "unexpected key {key}"),
            Self::UnknownVariant(tag) => write!(f, "unknown variant {tag}"),
            Self::ContractMismatch { expected, actual } => {
                write!(f, "expected contract {expected}, got {actual}")
            }
            Self::InvalidValue(message) => f.write_str(message),
        }
    }
}

impl Error for ValueError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    id: ContractId,
    value_type: Type,
}

impl Contract {
    pub fn new(id: ContractId, value_type: Type) -> Self {
        Self { id, value_type }
    }

    pub fn id(&self) -> &ContractId {
        &self.id
    }

    pub fn value_type(&self) -> &Type {
        &self.value_type
    }

    pub fn parse(&self, value: PhenixValue) -> Result<ContractValue, ValueError> {
        self.value_type.parse(&value)?;
        Ok(ContractValue {
            contract: self.clone(),
            value,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContractValue {
    contract: Contract,
    value: PhenixValue,
}

impl ContractValue {
    pub fn contract(&self) -> &Contract {
        &self.contract
    }

    pub fn as_value(&self) -> &PhenixValue {
        &self.value
    }

    pub fn get(&self, key: &str) -> Result<&PhenixValue, ValueError> {
        self.value.get(key)
    }

    pub fn value<T>(&self) -> Result<T, ValueError>
    where
        for<'value> T: TryFrom<Exact<&'value PhenixValue>, Error = ValueError>,
    {
        T::try_from(Exact(&self.value))
    }

    pub fn into_value(self) -> PhenixValue {
        self.value
    }
}

pub trait HasPhenixSchema {
    fn phenix_schema() -> PhenixSchema;
}

#[doc(hidden)]
pub trait ValueCodec: Sized {
    fn phenix_type() -> Type;
    fn to_value(&self) -> PhenixValue;
    fn from_value(value: &PhenixValue) -> Result<Self, ValueError>;

    fn project_from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        Self::from_value(value)
    }
}

impl<T: ValueCodec> HasPhenixSchema for T {
    fn phenix_schema() -> PhenixSchema {
        <T as ValueCodec>::phenix_type()
    }
}

pub trait PhenixContract: HasPhenixSchema + Sized {
    fn contract_id() -> ContractId;

    fn contract() -> Contract {
        Contract::new(
            Self::contract_id(),
            <Self as HasPhenixSchema>::phenix_schema(),
        )
    }

    fn to_contract_value(&self) -> Result<ContractValue, ValueError>
    where
        for<'value> PhenixValue: From<&'value Self>,
    {
        Self::contract().parse(PhenixValue::from(self))
    }

    fn from_contract_value(value: &ContractValue) -> Result<Self, ValueError>
    where
        for<'value> Self: TryFrom<Exact<&'value PhenixValue>, Error = ValueError>,
    {
        let expected = Self::contract_id();
        if value.contract().id() != &expected {
            return Err(ValueError::contract_mismatch(
                expected,
                value.contract().id().clone(),
            ));
        }
        Self::try_from(Exact(value.as_value()))
    }
}

macro_rules! primitive_value {
    ($ty:ty, $kind:ident, $variant:ident) => {
        impl ValueCodec for $ty {
            fn phenix_type() -> Type {
                Type::$kind
            }

            fn to_value(&self) -> PhenixValue {
                PhenixValue::$variant(self.clone())
            }

            fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
                match value {
                    PhenixValue::$variant(value) => Ok(value.clone()),
                    _ => Err(ValueError::TypeMismatch {
                        expected: TypeKind::$kind,
                        actual: value.kind(),
                    }),
                }
            }
        }

        impl From<&$ty> for PhenixValue {
            fn from(value: &$ty) -> Self {
                <$ty as ValueCodec>::to_value(value)
            }
        }

        impl<'value> TryFrom<Exact<&'value PhenixValue>> for $ty {
            type Error = ValueError;

            fn try_from(value: Exact<&'value PhenixValue>) -> Result<Self, Self::Error> {
                <Self as ValueCodec>::from_value(value.0)
            }
        }

        impl<'value> TryFrom<Project<&'value PhenixValue>> for $ty {
            type Error = ValueError;

            fn try_from(value: Project<&'value PhenixValue>) -> Result<Self, Self::Error> {
                <Self as ValueCodec>::project_from_value(value.0)
            }
        }
    };
}

impl ValueCodec for () {
    fn phenix_type() -> Type {
        Type::Unit
    }

    fn to_value(&self) -> PhenixValue {
        PhenixValue::Unit
    }

    fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        match value {
            PhenixValue::Unit => Ok(()),
            _ => Err(ValueError::TypeMismatch {
                expected: TypeKind::Unit,
                actual: value.kind(),
            }),
        }
    }
}

impl From<&()> for PhenixValue {
    fn from(value: &()) -> Self {
        <() as ValueCodec>::to_value(value)
    }
}

impl<'value> TryFrom<Exact<&'value PhenixValue>> for () {
    type Error = ValueError;

    fn try_from(value: Exact<&'value PhenixValue>) -> Result<Self, Self::Error> {
        <Self as ValueCodec>::from_value(value.0)
    }
}

impl<'value> TryFrom<Project<&'value PhenixValue>> for () {
    type Error = ValueError;

    fn try_from(value: Project<&'value PhenixValue>) -> Result<Self, Self::Error> {
        <Self as ValueCodec>::project_from_value(value.0)
    }
}

primitive_value!(bool, Bool, Bool);
primitive_value!(i64, I64, I64);
primitive_value!(u64, U64, U64);
primitive_value!(f64, F64, F64);
primitive_value!(String, String, String);

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Bytes(Vec<u8>);

impl Bytes {
    pub fn new(value: Vec<u8>) -> Self {
        Self(value)
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }
}

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(value: Vec<u8>) -> Self {
        Self::new(value)
    }
}

impl From<Bytes> for Vec<u8> {
    fn from(value: Bytes) -> Self {
        value.into_vec()
    }
}

impl ValueCodec for Bytes {
    fn phenix_type() -> Type {
        Type::Bytes
    }

    fn to_value(&self) -> PhenixValue {
        PhenixValue::Bytes(self.0.clone())
    }

    fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        match value {
            PhenixValue::Bytes(value) => Ok(Self(value.clone())),
            _ => Err(ValueError::TypeMismatch {
                expected: TypeKind::Bytes,
                actual: value.kind(),
            }),
        }
    }
}

impl From<&Bytes> for PhenixValue {
    fn from(value: &Bytes) -> Self {
        <Bytes as ValueCodec>::to_value(value)
    }
}

impl<'value> TryFrom<Exact<&'value PhenixValue>> for Bytes {
    type Error = ValueError;

    fn try_from(value: Exact<&'value PhenixValue>) -> Result<Self, Self::Error> {
        <Self as ValueCodec>::from_value(value.0)
    }
}

impl<'value> TryFrom<Project<&'value PhenixValue>> for Bytes {
    type Error = ValueError;

    fn try_from(value: Project<&'value PhenixValue>) -> Result<Self, Self::Error> {
        <Self as ValueCodec>::project_from_value(value.0)
    }
}

impl<T: ValueCodec> ValueCodec for Option<T> {
    fn phenix_type() -> Type {
        Type::Option(Box::new(T::phenix_type()))
    }

    fn to_value(&self) -> PhenixValue {
        PhenixValue::Option(self.as_ref().map(|value| Box::new(value.to_value())))
    }

    fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        match value {
            PhenixValue::Option(Some(value)) => T::from_value(value).map(Some),
            PhenixValue::Option(None) => Ok(None),
            _ => Err(ValueError::TypeMismatch {
                expected: TypeKind::Option,
                actual: value.kind(),
            }),
        }
    }

    fn project_from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        match value {
            PhenixValue::Option(Some(value)) => T::project_from_value(value).map(Some),
            PhenixValue::Option(None) => Ok(None),
            _ => Err(ValueError::TypeMismatch {
                expected: TypeKind::Option,
                actual: value.kind(),
            }),
        }
    }
}

impl<T: ValueCodec> ValueCodec for Vec<T> {
    fn phenix_type() -> Type {
        Type::List(Box::new(T::phenix_type()))
    }

    fn to_value(&self) -> PhenixValue {
        PhenixValue::List(self.iter().map(ValueCodec::to_value).collect())
    }

    fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        let PhenixValue::List(values) = value else {
            return Err(ValueError::TypeMismatch {
                expected: TypeKind::List,
                actual: value.kind(),
            });
        };
        values.iter().map(T::from_value).collect()
    }

    fn project_from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        let PhenixValue::List(values) = value else {
            return Err(ValueError::TypeMismatch {
                expected: TypeKind::List,
                actual: value.kind(),
            });
        };
        values.iter().map(T::project_from_value).collect()
    }
}

impl<T: ValueCodec, const N: usize> ValueCodec for [T; N] {
    fn phenix_type() -> Type {
        Type::Array {
            item: Box::new(T::phenix_type()),
            len: N,
        }
    }

    fn to_value(&self) -> PhenixValue {
        PhenixValue::List(self.iter().map(ValueCodec::to_value).collect())
    }

    fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        decode_array(value, T::from_value)
    }

    fn project_from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        decode_array(value, T::project_from_value)
    }
}

fn decode_array<T, const N: usize>(
    value: &PhenixValue,
    decode: impl Fn(&PhenixValue) -> Result<T, ValueError>,
) -> Result<[T; N], ValueError> {
    let PhenixValue::List(values) = value else {
        return Err(ValueError::TypeMismatch {
            expected: TypeKind::Array,
            actual: value.kind(),
        });
    };
    if values.len() != N {
        return Err(ValueError::InvalidValue(format!(
            "expected {} values, got {}",
            N,
            values.len()
        )));
    }
    values
        .iter()
        .map(decode)
        .collect::<Result<Vec<_>, _>>()?
        .try_into()
        .map_err(|values: Vec<T>| {
            ValueError::InvalidValue(format!("expected {} values, got {}", N, values.len()))
        })
}

impl<T: ValueCodec> ValueCodec for Box<T> {
    fn phenix_type() -> Type {
        T::phenix_type()
    }

    fn to_value(&self) -> PhenixValue {
        self.as_ref().to_value()
    }

    fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        T::from_value(value).map(Box::new)
    }

    fn project_from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        T::project_from_value(value).map(Box::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(value: &str) -> Key {
        Key::parse(value).unwrap()
    }

    #[test]
    fn contract_parse_is_the_shape_boundary() {
        let contract = Contract::new(
            ContractId::parse("fixture.coverage@1").unwrap(),
            Type::Table(BTreeMap::from([
                (key("covered"), Type::U64),
                (key("label"), Type::String),
            ])),
        );
        let parsed = contract
            .parse(PhenixValue::Table(BTreeMap::from([
                (key("covered"), PhenixValue::U64(5)),
                (key("label"), PhenixValue::String("ok".into())),
            ])))
            .unwrap();

        assert_eq!(parsed.get("covered").unwrap().value::<u64>().unwrap(), 5);
        assert_eq!(
            parsed.get("label").unwrap().value::<String>().unwrap(),
            "ok"
        );
        assert!(contract
            .parse(PhenixValue::Table(BTreeMap::from([(
                key("covered"),
                PhenixValue::String("wrong".into())
            )])))
            .is_err());
    }

    #[test]
    fn exact_tables_reject_extra_fields() {
        let contract = Contract::new(
            ContractId::parse("fixture.exact@1").unwrap(),
            Type::Table(BTreeMap::from([(key("value"), Type::U64)])),
        );
        assert_eq!(
            contract
                .parse(PhenixValue::Table(BTreeMap::from([
                    (key("value"), PhenixValue::U64(1)),
                    (key("extra"), PhenixValue::Bool(true)),
                ])))
                .unwrap_err(),
            ValueError::UnexpectedKey(key("extra"))
        );
    }

    #[test]
    fn keys_use_standard_fallible_conversions() {
        let key = Key::try_from("coverage".to_owned()).unwrap();
        assert_eq!("coverage".parse::<Key>().unwrap(), key);
        assert!(Key::try_from(String::new()).is_err());
    }

    #[test]
    fn fixed_arrays_keep_length_in_the_structural_type() {
        let value = [1_u64, 2_u64];
        assert_eq!(
            <[u64; 2] as ValueCodec>::phenix_type(),
            Type::Array {
                item: Box::new(Type::U64),
                len: 2,
            }
        );
        let encoded = value.to_value();
        assert_eq!(
            <[u64; 2] as ValueCodec>::from_value(&encoded).unwrap(),
            value
        );
        assert_eq!(
            <[u64; 3] as ValueCodec>::from_value(&encoded).unwrap_err(),
            ValueError::InvalidValue(format!("expected {} values, got {}", 3, 2))
        );
    }

    #[test]
    fn values_derive_structural_schemas_for_reverse_compatibility_checks() {
        let value = PhenixValue::Table(BTreeMap::from([
            (key("covered"), PhenixValue::U64(5)),
            (key("label"), PhenixValue::String("ok".into())),
            (key("extra"), PhenixValue::Bool(true)),
        ]));
        let expected = Type::Table(BTreeMap::from([
            (key("covered"), Type::U64),
            (key("label"), Type::String),
        ]));

        assert!(matches!(
            value.satisfies(&expected),
            SchemaCompatibility::Compatible
        ));
        assert!(matches!(
            value.satisfies(&Type::Table(BTreeMap::from([(
                key("covered"),
                Type::String,
            )]))),
            SchemaCompatibility::Incompatible(_)
        ));
    }

    #[test]
    fn empty_values_use_never_as_the_uninhabited_inner_schema() {
        let empty_list = PhenixValue::List(Vec::new());
        assert_eq!(
            empty_list.schema(),
            Type::Array {
                item: Box::new(Type::Never),
                len: 0,
            }
        );
        assert!(matches!(
            empty_list.satisfies(&Type::List(Box::new(Type::String))),
            SchemaCompatibility::Compatible
        ));

        let none = PhenixValue::Option(None);
        assert!(matches!(
            none.satisfies(&Type::Option(Box::new(Type::U64))),
            SchemaCompatibility::Compatible
        ));
    }

    #[test]
    fn heterogeneous_sequences_join_to_the_narrowest_safe_schema() {
        let records = PhenixValue::List(vec![
            PhenixValue::Table(BTreeMap::from([
                (key("id"), PhenixValue::U64(1)),
                (key("left"), PhenixValue::Bool(true)),
            ])),
            PhenixValue::Table(BTreeMap::from([
                (key("id"), PhenixValue::U64(2)),
                (key("right"), PhenixValue::String("x".into())),
            ])),
        ]);
        let common = Type::List(Box::new(Type::Table(BTreeMap::from([(
            key("id"),
            Type::U64,
        )]))));
        assert!(matches!(
            records.satisfies(&common),
            SchemaCompatibility::Compatible
        ));

        let mixed = PhenixValue::List(vec![PhenixValue::U64(1), PhenixValue::String("x".into())]);
        assert!(matches!(
            mixed.satisfies(&Type::List(Box::new(Type::U64))),
            SchemaCompatibility::Incompatible(_)
        ));
        assert!(matches!(
            mixed.satisfies(&Type::List(Box::new(Type::Any))),
            SchemaCompatibility::Compatible
        ));
    }

    #[test]
    fn concrete_sequences_preserve_length_for_fixed_array_checks() {
        let value = PhenixValue::List(vec![PhenixValue::U64(1), PhenixValue::U64(2)]);
        let fixed = Type::Array {
            item: Box::new(Type::U64),
            len: 2,
        };
        assert_eq!(value.schema(), fixed);
        assert_eq!(value.satisfies(&fixed), SchemaCompatibility::Exact);
    }

    #[test]
    fn options_and_lists_round_trip_without_runtime_validation_state() {
        let value = vec![Some(1_u64), None];
        assert_eq!(
            <Vec<Option<u64>> as ValueCodec>::phenix_type(),
            Type::List(Box::new(Type::Option(Box::new(Type::U64))))
        );
        assert_eq!(
            Vec::<Option<u64>>::from_value(&value.to_value()).unwrap(),
            value
        );
    }

    #[test]
    fn capability_references_preserve_non_plugin_owners_without_aliasing_them_to_plugins() {
        let reference = CallableRef::new(
            ContractId::parse("fixture.callback@1").unwrap(),
            CapabilityOwnerId::Client(ClientConnectionId::parse("client.connection").unwrap()),
            CapabilityGenerationId::parse("connection.generation").unwrap(),
            ReferenceId::parse("callback").unwrap(),
        );

        let encoded = serde_json::to_value(&reference).unwrap();
        assert_eq!(encoded["owner"]["kind"], "client");
        assert_eq!(encoded["owner"]["id"], "client.connection");
        assert_eq!(
            serde_json::from_value::<CallableRef>(encoded).unwrap(),
            reference
        );
    }

    #[test]
    fn plugin_references_keep_plugin_ownership_distinct_from_client_ownership() {
        let reference = ObjectRef::new(
            ContractId::parse("fixture.object@1").unwrap(),
            CapabilityOwnerId::Plugin(PluginId::parse("fixture.plugin").unwrap()),
            CapabilityGenerationId::parse("sha256:fixture").unwrap(),
            ReferenceId::parse("object").unwrap(),
        );

        assert_eq!(
            reference.generation(),
            &CapabilityGenerationId::parse("sha256:fixture").unwrap()
        );
        assert!(matches!(reference.owner(), CapabilityOwnerId::Plugin(_)));
    }
}
