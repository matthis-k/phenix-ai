//! Golden-vector conformance checks for independently implemented wire codecs.
//!
//! Expected descriptors and bytes should be literal fixtures from the wire
//! contract, not computed using the codec under test. Run the same fixtures for
//! every implementation. This checks the supplied cases, not all possible values.
use crate::{ContractValue, Failure, FailureKind, Schema};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortableError {
    IncompleteDescriptor,
    DescriptorMismatch { expected: Schema, actual: Schema },
    IdentityMismatch,
    EmptyVectors,
    EncodingMismatch { vector: String },
    ValueMismatch { vector: String },
    ReencodingMismatch { vector: String },
    MalformedAccepted { index: usize },
}
impl std::fmt::Display for PortableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IncompleteDescriptor => f.write_str("incomplete expected portable descriptor"),
            Self::DescriptorMismatch { .. } => f.write_str("portable descriptor mismatch"),
            Self::IdentityMismatch => {
                f.write_str("wire identity disagrees with portable descriptor")
            }
            Self::EmptyVectors => f.write_str("at least one golden vector is required"),
            Self::EncodingMismatch { vector } => {
                write!(f, "{vector}: encoding differs from golden bytes")
            }
            Self::ValueMismatch { vector } => {
                write!(f, "{vector}: decoded value differs from golden value")
            }
            Self::ReencodingMismatch { vector } => {
                write!(f, "{vector}: re-encoding differs from golden bytes")
            }
            Self::MalformedAccepted { index } => write!(f, "malformed vector {index} was accepted"),
        }
    }
}
impl std::error::Error for PortableError {}
impl PortableError {
    fn failure(self) -> Failure {
        Failure::external(format!("{self}"), self).with_kind(FailureKind::Framework)
    }
}

/// A semantic value and its exact canonical wire representation.
pub struct GoldenVector<'a, T> {
    pub name: &'a str,
    pub value: T,
    pub bytes: &'a [u8],
}

/// Check the complete descriptor, canonical identity, encode/decode agreement,
/// and rejection of explicitly supplied malformed encodings.
///
/// Truncations are deliberately caller-supplied: a prefix may be another valid
/// value for variable-length codecs. No nominal schema fallback is accepted.
///
/// ```
/// use phenix_graph::{Schema, testing::portable::{GoldenVector, check}};
/// let expected = Schema {
///     name: "example.u8".into(), revision: 1,
///     structure: "u8".into(), codec: "u8-le-v1".into(),
/// };
/// check(&expected, &[
///     GoldenVector { name: "zero", value: 0u8, bytes: &[0] },
///     GoldenVector { name: "max", value: 255u8, bytes: &[255] },
/// ], &[&[], &[0, 0]]).unwrap();
/// ```
pub fn check<T: ContractValue + PartialEq>(
    expected: &Schema,
    vectors: &[GoldenVector<'_, T>],
    malformed: &[&[u8]],
) -> Result<(), Failure> {
    if expected.name.trim().is_empty()
        || expected.structure.trim().is_empty()
        || expected.codec.trim().is_empty()
    {
        return Err(PortableError::IncompleteDescriptor.failure());
    }
    if T::schema() != *expected {
        return Err(PortableError::DescriptorMismatch {
            expected: expected.clone(),
            actual: T::schema(),
        }
        .failure());
    }
    if T::wire_schema() != expected.identity() {
        return Err(PortableError::IdentityMismatch.failure());
    }
    if vectors.is_empty() {
        return Err(PortableError::EmptyVectors.failure());
    }
    for vector in vectors {
        if vector.value.encode() != vector.bytes {
            return Err(PortableError::EncodingMismatch {
                vector: vector.name.into(),
            }
            .failure());
        }
        let decoded = T::decode(vector.bytes).map_err(|failure| {
            let mut context = failure.context().clone();
            if context.field.is_none() {
                context.field = Some(vector.name.into());
            }
            failure.with_context(context)
        })?;
        if decoded != vector.value {
            return Err(PortableError::ValueMismatch {
                vector: vector.name.into(),
            }
            .failure());
        }
        if decoded.encode() != vector.bytes {
            return Err(PortableError::ReencodingMismatch {
                vector: vector.name.into(),
            }
            .failure());
        }
    }
    for (index, bytes) in malformed.iter().enumerate() {
        if T::decode(bytes).is_ok() {
            return Err(PortableError::MalformedAccepted { index }.failure());
        }
    }
    Ok(())
}
