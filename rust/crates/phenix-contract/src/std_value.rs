use crate::{CallableId, PhenixValue, Type, TypeKind, ValueCodec, ValueError};
use std::collections::{BTreeMap, BTreeSet};

macro_rules! unsigned_value {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl ValueCodec for $ty {
                fn phenix_type() -> Type {
                    Type::U64
                }

                fn to_value(&self) -> PhenixValue {
                    PhenixValue::U64(u64::from(*self))
                }

                fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
                    match value {
                        PhenixValue::U64(value) => <$ty>::try_from(*value).map_err(|_| ValueError::TypeMismatch {
                            expected: TypeKind::U64,
                            actual: TypeKind::U64,
                        }),
                        _ => Err(ValueError::TypeMismatch {
                            expected: TypeKind::U64,
                            actual: value.kind(),
                        }),
                    }
                }
            }
        )+
    };
}

macro_rules! signed_value {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl ValueCodec for $ty {
                fn phenix_type() -> Type {
                    Type::I64
                }

                fn to_value(&self) -> PhenixValue {
                    PhenixValue::I64(i64::from(*self))
                }

                fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
                    match value {
                        PhenixValue::I64(value) => <$ty>::try_from(*value).map_err(|_| ValueError::TypeMismatch {
                            expected: TypeKind::I64,
                            actual: TypeKind::I64,
                        }),
                        _ => Err(ValueError::TypeMismatch {
                            expected: TypeKind::I64,
                            actual: value.kind(),
                        }),
                    }
                }
            }
        )+
    };
}

unsigned_value!(u8, u16, u32);
signed_value!(i8, i16, i32);

impl ValueCodec for usize {
    fn phenix_type() -> Type {
        Type::U64
    }

    fn to_value(&self) -> PhenixValue {
        PhenixValue::U64(*self as u64)
    }

    fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        let PhenixValue::U64(value) = value else {
            return Err(ValueError::TypeMismatch {
                expected: TypeKind::U64,
                actual: value.kind(),
            });
        };
        usize::try_from(*value)
            .map_err(|_| ValueError::InvalidValue("u64 does not fit usize".into()))
    }
}

impl From<&usize> for PhenixValue {
    fn from(value: &usize) -> Self {
        <usize as ValueCodec>::to_value(value)
    }
}

impl<'value> TryFrom<crate::Exact<&'value PhenixValue>> for usize {
    type Error = ValueError;

    fn try_from(value: crate::Exact<&'value PhenixValue>) -> Result<Self, Self::Error> {
        <Self as ValueCodec>::from_value(value.0)
    }
}

impl<'value> TryFrom<crate::Project<&'value PhenixValue>> for usize {
    type Error = ValueError;

    fn try_from(value: crate::Project<&'value PhenixValue>) -> Result<Self, Self::Error> {
        <Self as ValueCodec>::project_from_value(value.0)
    }
}

impl From<serde_json::Value> for PhenixValue {
    fn from(value: serde_json::Value) -> Self {
        value.to_value()
    }
}

impl ValueCodec for serde_json::Value {
    fn phenix_type() -> Type {
        Type::Any
    }

    fn to_value(&self) -> PhenixValue {
        match self {
            Self::Null => PhenixValue::Unit,
            Self::Bool(value) => PhenixValue::Bool(*value),
            Self::Number(value) => {
                if let Some(value) = value.as_u64() {
                    PhenixValue::U64(value)
                } else if let Some(value) = value.as_i64() {
                    PhenixValue::I64(value)
                } else {
                    PhenixValue::F64(value.as_f64().expect("JSON numbers are finite"))
                }
            }
            Self::String(value) => PhenixValue::String(value.clone()),
            Self::Array(values) => {
                PhenixValue::List(values.iter().map(ValueCodec::to_value).collect())
            }
            Self::Object(values) => PhenixValue::Map(
                values
                    .iter()
                    .map(|(key, value)| (key.clone(), value.to_value()))
                    .collect(),
            ),
        }
    }

    fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        match value {
            PhenixValue::Unit => Ok(Self::Null),
            PhenixValue::Bool(value) => Ok(Self::Bool(*value)),
            PhenixValue::I64(value) => Ok(Self::Number((*value).into())),
            PhenixValue::U64(value) => Ok(Self::Number((*value).into())),
            PhenixValue::F64(value) => serde_json::Number::from_f64(*value)
                .map(Self::Number)
                .ok_or_else(|| ValueError::InvalidValue("non-finite float is not JSON".into())),
            PhenixValue::String(value) => Ok(Self::String(value.clone())),
            PhenixValue::List(values) => values
                .iter()
                .map(Self::from_value)
                .collect::<Result<Vec<_>, _>>()
                .map(Self::Array),
            PhenixValue::Map(values) => values
                .iter()
                .map(|(key, value)| Self::from_value(value).map(|value| (key.clone(), value)))
                .collect::<Result<serde_json::Map<_, _>, _>>()
                .map(Self::Object),
            other => Err(ValueError::TypeMismatch {
                expected: TypeKind::Any,
                actual: other.kind(),
            }),
        }
    }

    fn project_from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        Self::from_value(value)
    }
}

impl<T: ValueCodec> ValueCodec for BTreeMap<String, T> {
    fn phenix_type() -> Type {
        Type::Map(Box::new(T::phenix_type()))
    }

    fn to_value(&self) -> PhenixValue {
        PhenixValue::Map(
            self.iter()
                .map(|(key, value)| (key.clone(), value.to_value()))
                .collect(),
        )
    }

    fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        let PhenixValue::Map(values) = value else {
            return Err(ValueError::TypeMismatch {
                expected: TypeKind::Map,
                actual: value.kind(),
            });
        };
        values
            .iter()
            .map(|(key, value)| T::from_value(value).map(|value| (key.clone(), value)))
            .collect()
    }

    fn project_from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        let PhenixValue::Map(values) = value else {
            return Err(ValueError::TypeMismatch {
                expected: TypeKind::Map,
                actual: value.kind(),
            });
        };
        values
            .iter()
            .map(|(key, value)| T::project_from_value(value).map(|value| (key.clone(), value)))
            .collect()
    }
}

impl<T: ValueCodec> ValueCodec for BTreeMap<CallableId, T> {
    fn phenix_type() -> Type {
        Type::Map(Box::new(T::phenix_type()))
    }

    fn to_value(&self) -> PhenixValue {
        PhenixValue::Map(
            self.iter()
                .map(|(key, value)| (key.as_str().to_owned(), value.to_value()))
                .collect(),
        )
    }

    fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        let PhenixValue::Map(values) = value else {
            return Err(ValueError::TypeMismatch {
                expected: TypeKind::Map,
                actual: value.kind(),
            });
        };
        values
            .iter()
            .map(|(key, value)| {
                let key = CallableId::parse(key.clone())
                    .map_err(|error| ValueError::InvalidValue(error.into()))?;
                T::from_value(value).map(|value| (key, value))
            })
            .collect()
    }

    fn project_from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        let PhenixValue::Map(values) = value else {
            return Err(ValueError::TypeMismatch {
                expected: TypeKind::Map,
                actual: value.kind(),
            });
        };
        values
            .iter()
            .map(|(key, value)| {
                let key = CallableId::parse(key.clone())
                    .map_err(|error| ValueError::InvalidValue(error.into()))?;
                T::project_from_value(value).map(|value| (key, value))
            })
            .collect()
    }
}

impl<T> ValueCodec for BTreeSet<T>
where
    T: ValueCodec + Ord,
{
    fn phenix_type() -> Type {
        Type::List(Box::new(T::phenix_type()))
    }

    fn to_value(&self) -> PhenixValue {
        PhenixValue::List(self.iter().map(ValueCodec::to_value).collect())
    }

    fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        Self::phenix_type().parse(value)?;
        Self::project_from_value(value)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_width_integers_reject_out_of_range_values() {
        assert!(u8::from_value(&PhenixValue::U64(256)).is_err());
        assert!(i8::from_value(&PhenixValue::I64(128)).is_err());
    }

    #[test]
    fn json_values_lower_into_structural_values() {
        let json = serde_json::json!({
            "enabled": true,
            "nested": [1, "two", null],
        });
        assert_eq!(serde_json::Value::phenix_type(), Type::Any);
        assert_eq!(
            serde_json::Value::from_value(&json.to_value()).unwrap(),
            json
        );
    }

    #[test]
    fn structural_values_serialize_at_explicit_json_output_boundaries() {
        let value = PhenixValue::Map(BTreeMap::from([
            ("enabled".into(), PhenixValue::Bool(true)),
            ("count".into(), PhenixValue::U64(2)),
        ]));

        assert_eq!(
            serde_json::Value::from_value(&value).unwrap(),
            serde_json::json!({"enabled": true, "count": 2})
        );
    }

    #[test]
    fn string_maps_have_a_homogeneous_dynamic_representation() {
        let values = BTreeMap::from([("first".to_owned(), 1_u64), ("second".to_owned(), 2_u64)]);
        assert_eq!(
            BTreeMap::<String, u64>::phenix_type(),
            Type::Map(Box::new(Type::U64))
        );
        assert_eq!(
            BTreeMap::<String, u64>::from_value(&values.to_value()).unwrap(),
            values
        );
    }

    #[test]
    fn sets_have_a_deterministic_list_representation() {
        let values = BTreeSet::from(["b".to_owned(), "a".to_owned()]);
        assert_eq!(
            values.to_value(),
            PhenixValue::List(vec![
                PhenixValue::String("a".into()),
                PhenixValue::String("b".into())
            ])
        );
        assert_eq!(
            BTreeSet::<String>::from_value(&values.to_value()).unwrap(),
            values
        );
    }
}
