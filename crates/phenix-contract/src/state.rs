//! State request vocabulary; decoding a reference never grants live authority.
use crate::{identifier, Code, Failure, Revision, Value, ValueAccessError, ValueLocation};
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompareExchange {
    pub location: ValueLocation,
    pub expected: Revision,
    pub value: Value,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateRequest {
    Create { property: String, initial: Value },
    Read(ValueLocation),
    CompareExchange(CompareExchange),
    Subscribe(ValueLocation),
    Next(u64),
    Unsubscribe(u64),
    Batch(Vec<CompareExchange>),
}
fn invalid() -> Failure {
    Failure::new(Code::SchemaRejected, "invalid state request")
}
fn location(value: Value) -> Result<ValueLocation, Failure> {
    if let Value::Ref(r) = value {
        Ok(*r)
    } else {
        Err(invalid())
    }
}
fn number(value: Value) -> Result<u64, Failure> {
    if let Value::U64(n) = value {
        Ok(n)
    } else {
        Err(invalid())
    }
}
fn triple(value: Value) -> Result<CompareExchange, Failure> {
    let Value::List(v) = value else {
        return Err(invalid());
    };
    let [r, n, value]: [Value; 3] = v.try_into().map_err(|_| invalid())?;
    Ok(CompareExchange {
        location: location(r)?,
        expected: Revision(number(n)?),
        value,
    })
}
fn triple_value(c: &CompareExchange) -> Value {
    Value::List(vec![
        Value::Ref(Box::new(c.location)),
        Value::U64(c.expected.0),
        c.value.clone(),
    ])
}
impl StateRequest {
    pub fn opcode(&self) -> u32 {
        match self {
            Self::Create { .. } => 1,
            Self::Read(_) => 2,
            Self::CompareExchange(_) => 3,
            Self::Subscribe(_) => 4,
            Self::Next(_) => 5,
            Self::Unsubscribe(_) => 6,
            Self::Batch(_) => 7,
        }
    }
    pub fn encode(&self) -> Result<Vec<u8>, Failure> {
        let value = match self {
            Self::Create { property, initial } if identifier(property) => {
                Value::List(vec![Value::String(property.clone()), initial.clone()])
            }
            Self::Read(r) | Self::Subscribe(r) => Value::Ref(Box::new(*r)),
            Self::CompareExchange(c) => triple_value(c),
            Self::Next(n) | Self::Unsubscribe(n) if *n != 0 => Value::U64(*n),
            Self::Batch(c)
                if !c.is_empty() && c.windows(2).all(|p| p[0].location < p[1].location) =>
            {
                Value::List(c.iter().map(triple_value).collect())
            }
            _ => return Err(invalid()),
        };
        value.encode()
    }
    pub fn decode(opcode: u32, bytes: &[u8]) -> Result<Self, Failure> {
        // Unknown opcodes fail before parsing borrowed payloads or dispatching effects.
        if !(1..=7).contains(&opcode) {
            return Err(invalid());
        }
        let value = Value::decode(bytes)?;
        let request = match opcode {
            1 => {
                let Value::List(v) = value else {
                    return Err(invalid());
                };
                let [name, initial]: [Value; 2] = v.try_into().map_err(|_| invalid())?;
                let Value::String(property) = name else {
                    return Err(invalid());
                };
                Self::Create { property, initial }
            }
            2 => Self::Read(location(value)?),
            3 => Self::CompareExchange(triple(value)?),
            4 => Self::Subscribe(location(value)?),
            5 => Self::Next(number(value)?),
            6 => Self::Unsubscribe(number(value)?),
            7 => {
                let Value::List(v) = value else {
                    return Err(invalid());
                };
                Self::Batch(v.into_iter().map(triple).collect::<Result<_, _>>()?)
            }
            _ => return Err(invalid()),
        };
        if request.encode()? != bytes {
            return Err(invalid());
        }
        Ok(request)
    }
}
impl ValueAccessError {
    pub fn to_value(&self) -> Value {
        let name = match self {
            Self::Conflict { expected, current } => {
                return Value::List(vec![
                    Value::String("Conflict".into()),
                    Value::U64(expected.0),
                    Value::U64(current.0),
                ])
            }
            Self::OwnerGone => "OwnerGone",
            Self::StaleReference => "StaleReference",
            Self::SchemaMismatch => "SchemaMismatch",
            Self::Denied => "Denied",
            Self::RevisionExhausted => "RevisionExhausted",
            Self::EpochExhausted => "EpochExhausted",
        };
        Value::List(vec![Value::String(name.into())])
    }
    pub fn from_value(value: Value) -> Result<Self, Failure> {
        let Value::List(v) = value else {
            return Err(invalid());
        };
        let mut fields = v.into_iter();
        let Some(Value::String(name)) = fields.next() else {
            return Err(invalid());
        };
        let error = match name.as_str() {
            "Conflict" => Self::Conflict {
                expected: Revision(number(fields.next().ok_or_else(invalid)?)?),
                current: Revision(number(fields.next().ok_or_else(invalid)?)?),
            },
            "OwnerGone" => Self::OwnerGone,
            "StaleReference" => Self::StaleReference,
            "SchemaMismatch" => Self::SchemaMismatch,
            "Denied" => Self::Denied,
            "RevisionExhausted" => Self::RevisionExhausted,
            "EpochExhausted" => Self::EpochExhausted,
            _ => return Err(invalid()),
        };
        if fields.next().is_some() {
            return Err(invalid());
        }
        Ok(error)
    }
}
