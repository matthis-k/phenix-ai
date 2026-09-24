use crate::{PhenixValue, Type, ValueCodec, ValueError};

impl ValueCodec for PhenixValue {
    fn phenix_type() -> Type {
        Type::Any
    }

    fn to_value(&self) -> PhenixValue {
        self.clone()
    }

    fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        Ok(value.clone())
    }
}

impl ValueCodec for Type {
    fn phenix_type() -> Type {
        Type::Any
    }

    fn to_value(&self) -> PhenixValue {
        let json = serde_json::to_value(self).expect("Phenix schemas are serializable");
        <serde_json::Value as ValueCodec>::to_value(&json)
    }

    fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        let json = <serde_json::Value as ValueCodec>::from_value(value)?;
        serde_json::from_value(json)
            .map_err(|error| ValueError::InvalidValue(format!("invalid Phenix schema: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn dynamic_values_are_identity_codec_values() {
        let value = PhenixValue::Map(BTreeMap::from([
            ("enabled".into(), PhenixValue::Bool(true)),
            ("weight".into(), PhenixValue::F64(0.5)),
        ]));

        assert_eq!(PhenixValue::phenix_type(), Type::Any);
        assert_eq!(value.to_value(), value);
        assert_eq!(PhenixValue::from_value(&value).unwrap(), value);
    }

    #[test]
    fn schemas_round_trip_through_the_structural_boundary() {
        let schema = Type::Table(BTreeMap::from([
            (crate::Key::parse("name").unwrap(), Type::String),
            (
                crate::Key::parse("tags").unwrap(),
                Type::List(Box::new(Type::String)),
            ),
        ]));

        let encoded = schema.to_value();
        assert!(matches!(encoded, PhenixValue::Map(_)));
        assert_eq!(Type::from_value(&encoded).unwrap(), schema);
    }
}
