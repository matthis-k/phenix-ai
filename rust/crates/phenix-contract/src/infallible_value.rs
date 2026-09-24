use crate::{PhenixValue, Type, TypeKind, ValueCodec, ValueError};
use std::convert::Infallible;

impl ValueCodec for Infallible {
    fn phenix_type() -> Type {
        Type::Never
    }

    fn to_value(&self) -> PhenixValue {
        match *self {}
    }

    fn from_value(value: &PhenixValue) -> Result<Self, ValueError> {
        Err(ValueError::TypeMismatch {
            expected: TypeKind::Never,
            actual: value.kind(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infallible_uses_the_uninhabited_structural_type() {
        assert_eq!(Infallible::phenix_type(), Type::Never);
        assert!(Infallible::from_value(&PhenixValue::Unit).is_err());
    }
}
