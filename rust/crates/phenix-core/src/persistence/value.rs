use crate::{
    Key, NamespaceTransaction, PhenixValue, TransactionOp, Type, TypeKind, ValueCodec, ValueError,
};

fn key(value: &str) -> Key {
    Key::parse(value.to_owned()).expect("static structural key is valid")
}

fn table_type(fields: impl IntoIterator<Item = (&'static str, Type)>) -> Type {
    Type::Table(
        fields
            .into_iter()
            .map(|(name, ty)| (key(name), ty))
            .collect(),
    )
}

fn variant_type(variants: impl IntoIterator<Item = (&'static str, Type)>) -> Type {
    Type::Variant(
        variants
            .into_iter()
            .map(|(name, ty)| (key(name), ty))
            .collect(),
    )
}

fn table_value(fields: impl IntoIterator<Item = (&'static str, PhenixValue)>) -> PhenixValue {
    PhenixValue::Table(
        fields
            .into_iter()
            .map(|(name, value)| (key(name), value))
            .collect(),
    )
}

fn variant_value(tag: &'static str, value: PhenixValue) -> PhenixValue {
    PhenixValue::Variant {
        tag: key(tag),
        value: Box::new(value),
    }
}

fn bytes(value: &PhenixValue) -> Result<Vec<u8>, ValueError> {
    match value {
        PhenixValue::Bytes(value) => Ok(value.clone()),
        _ => Err(ValueError::TypeMismatch {
            expected: TypeKind::Bytes,
            actual: value.kind(),
        }),
    }
}

fn optional_bytes(value: &PhenixValue) -> Result<Option<Vec<u8>>, ValueError> {
    match value {
        PhenixValue::Option(None) => Ok(None),
        PhenixValue::Option(Some(value)) => bytes(value).map(Some),
        _ => Err(ValueError::TypeMismatch {
            expected: TypeKind::Option,
            actual: value.kind(),
        }),
    }
}

impl ValueCodec for TransactionOp {
    fn phenix_type() -> Type {
        variant_type([
            (
                "Put",
                table_type([("key", Type::String), ("value", Type::Bytes)]),
            ),
            ("Delete", table_type([("key", Type::String)])),
            (
                "AssertValue",
                table_type([
                    ("key", Type::String),
                    ("expected", Type::Option(Box::new(Type::Bytes))),
                ]),
            ),
        ])
    }

    fn to_value(&self) -> PhenixValue {
        match self {
            Self::Put { key, value } => variant_value(
                "Put",
                table_value([
                    ("key", key.to_value()),
                    ("value", PhenixValue::Bytes(value.clone())),
                ]),
            ),
            Self::Delete { key } => variant_value("Delete", table_value([("key", key.to_value())])),
            Self::AssertValue { key, expected } => variant_value(
                "AssertValue",
                table_value([
                    ("key", key.to_value()),
                    (
                        "expected",
                        PhenixValue::Option(
                            expected
                                .as_ref()
                                .map(|value| Box::new(PhenixValue::Bytes(value.clone()))),
                        ),
                    ),
                ]),
            ),
        }
    }

    fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        Self::phenix_type().parse(value)?;
        Self::project_from_value(value)
    }

    fn project_from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        let (tag, payload) = value.variant()?;
        match tag.as_str() {
            "Put" => Ok(Self::Put {
                key: String::project_from_value(payload.get("key")?)?,
                value: bytes(payload.get("value")?)?,
            }),
            "Delete" => Ok(Self::Delete {
                key: String::project_from_value(payload.get("key")?)?,
            }),
            "AssertValue" => Ok(Self::AssertValue {
                key: String::project_from_value(payload.get("key")?)?,
                expected: optional_bytes(payload.get("expected")?)?,
            }),
            _ => Err(ValueError::unknown_variant(tag.clone())),
        }
    }
}

impl From<&TransactionOp> for PhenixValue {
    fn from(value: &TransactionOp) -> Self {
        <TransactionOp as ValueCodec>::to_value(value)
    }
}

impl<'value> TryFrom<crate::Exact<&'value PhenixValue>> for TransactionOp {
    type Error = ValueError;

    fn try_from(value: crate::Exact<&'value PhenixValue>) -> Result<Self, Self::Error> {
        <Self as ValueCodec>::from_value(value.0)
    }
}

impl<'value> TryFrom<crate::Project<&'value PhenixValue>> for TransactionOp {
    type Error = ValueError;

    fn try_from(value: crate::Project<&'value PhenixValue>) -> Result<Self, Self::Error> {
        <Self as ValueCodec>::project_from_value(value.0)
    }
}

impl ValueCodec for NamespaceTransaction {
    fn phenix_type() -> Type {
        table_type([
            ("owner", crate::PluginId::phenix_type()),
            ("namespace", crate::ResourceNamespace::phenix_type()),
            (
                "operations",
                Type::List(Box::new(TransactionOp::phenix_type())),
            ),
        ])
    }

    fn to_value(&self) -> PhenixValue {
        table_value([
            ("owner", self.owner.to_value()),
            ("namespace", self.namespace.to_value()),
            (
                "operations",
                PhenixValue::List(self.operations.iter().map(ValueCodec::to_value).collect()),
            ),
        ])
    }

    fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        Self::phenix_type().parse(value)?;
        Self::project_from_value(value)
    }

    fn project_from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        let operations = match value.get("operations")? {
            PhenixValue::List(values) => values
                .iter()
                .map(TransactionOp::project_from_value)
                .collect::<Result<Vec<_>, _>>()?,
            other => {
                return Err(ValueError::TypeMismatch {
                    expected: TypeKind::List,
                    actual: other.kind(),
                })
            }
        };
        Ok(Self {
            owner: crate::PluginId::project_from_value(value.get("owner")?)?,
            namespace: crate::ResourceNamespace::project_from_value(value.get("namespace")?)?,
            operations,
        })
    }
}

impl From<&NamespaceTransaction> for PhenixValue {
    fn from(value: &NamespaceTransaction) -> Self {
        <NamespaceTransaction as ValueCodec>::to_value(value)
    }
}

impl<'value> TryFrom<crate::Exact<&'value PhenixValue>> for NamespaceTransaction {
    type Error = ValueError;

    fn try_from(value: crate::Exact<&'value PhenixValue>) -> Result<Self, Self::Error> {
        <Self as ValueCodec>::from_value(value.0)
    }
}

impl<'value> TryFrom<crate::Project<&'value PhenixValue>> for NamespaceTransaction {
    type Error = ValueError;

    fn try_from(value: crate::Project<&'value PhenixValue>) -> Result<Self, Self::Error> {
        <Self as ValueCodec>::project_from_value(value.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PluginId, ResourceNamespace};

    #[test]
    fn namespace_transaction_projects_from_superset_tables() {
        let transaction = NamespaceTransaction {
            owner: PluginId::parse("sessions").unwrap(),
            namespace: ResourceNamespace::parse("sessions.state").unwrap(),
            operations: vec![TransactionOp::AssertValue {
                key: "session/a".into(),
                expected: Some(vec![1, 2, 3]),
            }],
        };
        let mut value = match transaction.to_value() {
            PhenixValue::Table(fields) => fields,
            _ => unreachable!(),
        };
        value.insert(key("provider_only"), PhenixValue::Bool(true));

        assert_eq!(
            NamespaceTransaction::project_from_value(&PhenixValue::Table(value)).unwrap(),
            transaction
        );
    }
}
