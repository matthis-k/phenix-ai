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
/// Heap allocation changes Rust storage, not the canonical schema or wire value.
impl<T: Portable> Portable for Box<T> {
    const SCHEMA: StaticSchema = T::SCHEMA;
    fn to_value(&self) -> WireValue {
        self.as_ref().to_value()
    }
    fn from_value(value: WireValue) -> Result<Self, Failure> {
        T::from_value(value).map(Box::new)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boxed_portable_is_schema_and_wire_transparent() {
        let boxed = Box::new(42_u64);
        assert_eq!(
            <Box<u64>>::SCHEMA.owned().unwrap(),
            u64::SCHEMA.owned().unwrap()
        );
        assert_eq!(boxed.to_value(), 42_u64.to_value());
        let bytes = boxed.to_value().encode().unwrap();
        assert_eq!(bytes, 42_u64.to_value().encode().unwrap());
        assert_eq!(
            Box::<u64>::from_value(WireValue::decode(&bytes).unwrap()).unwrap(),
            boxed
        );
        assert_eq!(
            Box::<u64>::from_value(WireValue::String("invalid".into()))
                .unwrap_err()
                .code,
            Code::SchemaRejected
        );
    }
}
