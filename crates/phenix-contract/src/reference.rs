//! Portable locations carry identity, never authority. See the S1a wire contract.
use crate::{identifier, Code, Failure, Value, MAX_DEPTH};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

fn invalid(message: &str) -> Failure {
    Failure::new(Code::SchemaRejected, message)
}

macro_rules! identity {
    ($name:ident, $size:expr) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name([u8; $size]);
        impl $name {
            pub fn from_bytes(bytes: &[u8]) -> Result<Self, Failure> {
                let bytes: [u8; $size] = bytes
                    .try_into()
                    .map_err(|_| invalid("identity width mismatch"))?;
                if bytes.iter().all(|&byte| byte == 0) {
                    return Err(invalid("reserved zero identity"));
                }
                Ok(Self(bytes))
            }
            pub fn as_bytes(&self) -> &[u8; $size] {
                &self.0
            }
        }
    };
}
identity!(SchemaId, 32);
identity!(GenerationDigest, 32);
identity!(OwnerId, 32);
identity!(ValueId, 16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Revision(pub u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Epoch(pub u64);

/// A decoded location is not proof that a cell exists or a caller may use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ValueLocation {
    pub generation: GenerationDigest,
    pub owner: OwnerId,
    pub cell: ValueId,
    pub schema: SchemaId,
}

/// State protocol outcomes. S1b owns their native wire envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueAccessError {
    Conflict {
        expected: Revision,
        current: Revision,
    },
    OwnerGone,
    StaleReference,
    SchemaMismatch,
    Denied,
    RevisionExhausted,
    EpochExhausted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueSchema {
    Bool,
    I64,
    U64,
    String,
    Bytes,
    List(Box<ValueSchema>),
    Option(Box<ValueSchema>),
    Struct(BTreeMap<String, ValueSchema>),
    Variants(Vec<String>),
    Ref(SchemaId),
}

impl ValueSchema {
    fn validate(&self, depth: usize) -> Result<(), Failure> {
        if depth > MAX_DEPTH {
            return Err(invalid("schema depth limit"));
        }
        match self {
            Self::List(inner) | Self::Option(inner) => inner.validate(depth + 1)?,
            Self::Struct(fields) => {
                for (name, schema) in fields {
                    if !identifier(name) {
                        return Err(invalid("invalid schema field name"));
                    }
                    schema.validate(depth + 1)?;
                }
            }
            Self::Variants(names)
                if names.is_empty()
                    || names.iter().any(|name| !identifier(name))
                    || names.windows(2).any(|pair| pair[0] >= pair[1]) =>
            {
                return Err(invalid("invalid schema variants"));
            }
            _ => {}
        }
        Ok(())
    }

    fn descriptor(&self) -> Value {
        let pair = |name: &str, inner: Value| Value::List(vec![Value::String(name.into()), inner]);
        match self {
            Self::Bool => Value::String("Bool".into()),
            Self::I64 => Value::String("I64".into()),
            Self::U64 => Value::String("U64".into()),
            Self::String => Value::String("String".into()),
            Self::Bytes => Value::String("Bytes".into()),
            Self::List(inner) => pair("List", inner.descriptor()),
            Self::Option(inner) => pair("Option", inner.descriptor()),
            Self::Struct(fields) => pair(
                "Struct",
                Value::Map(
                    fields
                        .iter()
                        .map(|(k, s)| (k.clone(), s.descriptor()))
                        .collect(),
                ),
            ),
            Self::Variants(names) => pair(
                "Variants",
                Value::List(names.iter().cloned().map(Value::String).collect()),
            ),
            Self::Ref(id) => pair("Ref", Value::Bytes(id.as_bytes().to_vec())),
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>, Failure> {
        self.validate(0)?;
        self.descriptor().encode_schema()
    }

    pub fn id(&self) -> Result<SchemaId, Failure> {
        let mut hash = Sha256::new();
        hash.update(b"phenix.schema.v2\0");
        hash.update(self.encode()?);
        SchemaId::from_bytes(&hash.finalize())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, Failure> {
        fn parse(value: Value) -> Result<ValueSchema, Failure> {
            Ok(match value {
                Value::String(name) => match name.as_str() {
                    "Bool" => ValueSchema::Bool,
                    "I64" => ValueSchema::I64,
                    "U64" => ValueSchema::U64,
                    "String" => ValueSchema::String,
                    "Bytes" => ValueSchema::Bytes,
                    _ => return Err(invalid("unknown scalar schema")),
                },
                Value::List(mut pair) if pair.len() == 2 => {
                    let inner = pair.pop().ok_or_else(|| invalid("schema pair"))?;
                    let Some(Value::String(name)) = pair.pop() else {
                        return Err(invalid("schema kind must be a string"));
                    };
                    match (name.as_str(), inner) {
                        ("List", inner) => ValueSchema::List(Box::new(parse(inner)?)),
                        ("Option", inner) => ValueSchema::Option(Box::new(parse(inner)?)),
                        ("Struct", Value::Map(fields)) => ValueSchema::Struct(
                            fields
                                .into_iter()
                                .map(|(k, v)| Ok((k, parse(v)?)))
                                .collect::<Result<_, Failure>>()?,
                        ),
                        ("Variants", Value::List(names)) => ValueSchema::Variants(
                            names
                                .into_iter()
                                .map(|v| match v {
                                    Value::String(name) => Ok(name),
                                    _ => Err(invalid("variant name must be a string")),
                                })
                                .collect::<Result<_, _>>()?,
                        ),
                        ("Ref", Value::Bytes(id)) => ValueSchema::Ref(SchemaId::from_bytes(&id)?),
                        _ => return Err(invalid("invalid composite schema")),
                    }
                }
                _ => return Err(invalid("invalid schema descriptor")),
            })
        }
        let schema = parse(Value::decode_schema(bytes)?)?;
        if schema.encode()? != bytes {
            return Err(invalid("noncanonical schema"));
        }
        Ok(schema)
    }

    pub fn accepts(&self, value: &Value) -> bool {
        match (self, value) {
            (Self::Bool, Value::Bool(_))
            | (Self::I64, Value::I64(_))
            | (Self::U64, Value::U64(_))
            | (Self::String, Value::String(_))
            | (Self::Bytes, Value::Bytes(_)) => true,
            (Self::List(inner), Value::List(items)) => items.iter().all(|v| inner.accepts(v)),
            (Self::Option(_), Value::Null) => true,
            (Self::Option(inner), Value::List(items)) if items.len() == 1 => {
                inner.accepts(&items[0])
            }
            (Self::Struct(fields), Value::Map(values)) => {
                fields.len() == values.len()
                    && fields
                        .iter()
                        .all(|(k, s)| values.get(k).is_some_and(|v| s.accepts(v)))
            }
            (Self::Variants(names), Value::String(name)) => names.contains(name),
            (Self::Ref(schema), Value::Ref(location)) => schema == &location.schema,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynValue {
    schema: SchemaId,
    value: Value,
}
impl DynValue {
    pub fn new(schema: &ValueSchema, value: Value) -> Result<Self, Failure> {
        let id = schema.id()?;
        value.encode()?;
        if !schema.accepts(&value) {
            return Err(invalid("dynamic value schema mismatch"));
        }
        Ok(Self { schema: id, value })
    }
    pub fn schema(&self) -> SchemaId {
        self.schema
    }
    pub fn value(&self) -> &Value {
        &self.value
    }
}
