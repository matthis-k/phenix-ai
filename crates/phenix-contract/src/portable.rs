use crate::{Code, Failure, Portable, StaticSchema, Value as WireValue};

/// Canonical opaque bytes, distinct from a portable list of scalar values.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Blob(pub Vec<u8>);
impl Portable for Blob {
    const SCHEMA: StaticSchema = StaticSchema::Bytes;
    fn to_value(&self) -> WireValue {
        WireValue::Bytes(self.0.clone())
    }
    fn from_value(value: WireValue) -> Result<Self, Failure> {
        if let WireValue::Bytes(bytes) = value {
            Ok(Self(bytes))
        } else {
            Err(invalid())
        }
    }
}
fn invalid() -> Failure {
    Failure::new(Code::SchemaRejected, "invalid typed portable value")
}
macro_rules! scalar {
    ($t:ty,$variant:ident,$schema:ident) => {
        impl Portable for $t {
            const SCHEMA: StaticSchema = StaticSchema::$schema;
            fn to_value(&self) -> WireValue {
                WireValue::$variant(self.clone())
            }
            fn from_value(value: WireValue) -> Result<Self, Failure> {
                if let WireValue::$variant(v) = value {
                    Ok(v)
                } else {
                    Err(invalid())
                }
            }
        }
    };
}
scalar!(bool, Bool, Bool);
scalar!(i64, I64, I64);
scalar!(u64, U64, U64);
scalar!(String, String, String);
impl Portable for () {
    const SCHEMA: StaticSchema = StaticSchema::Struct(&[]);
    fn to_value(&self) -> WireValue {
        WireValue::map([] as [(&str, WireValue); 0])
    }
    fn from_value(v: WireValue) -> Result<Self, Failure> {
        if matches!(v,WireValue::Map(ref m) if m.is_empty()) {
            Ok(())
        } else {
            Err(invalid())
        }
    }
}
impl<T: Portable> Portable for Vec<T> {
    const SCHEMA: StaticSchema = StaticSchema::List(&T::SCHEMA);
    fn to_value(&self) -> WireValue {
        WireValue::List(self.iter().map(T::to_value).collect())
    }
    fn from_value(v: WireValue) -> Result<Self, Failure> {
        if let WireValue::List(items) = v {
            items.into_iter().map(T::from_value).collect()
        } else {
            Err(invalid())
        }
    }
}
impl<T: Portable> Portable for Option<T> {
    const SCHEMA: StaticSchema = StaticSchema::Option(&T::SCHEMA);
    fn to_value(&self) -> WireValue {
        self.as_ref()
            .map_or(WireValue::Null, |v| WireValue::List(vec![v.to_value()]))
    }
    fn from_value(v: WireValue) -> Result<Self, Failure> {
        match v {
            WireValue::Null => Ok(None),
            WireValue::List(mut v) if v.len() == 1 => Ok(Some(T::from_value(v.pop().unwrap())?)),
            _ => Err(invalid()),
        }
    }
}
