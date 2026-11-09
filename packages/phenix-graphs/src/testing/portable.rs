//! Golden-vector conformance checks for independently implemented wire codecs.
//!
//! Expected descriptors and bytes should be literal fixtures from the wire
//! contract, not computed using the codec under test. Run the same fixtures for
//! every implementation. This checks the supplied cases, not all possible values.
use crate::{ContractValue, Schema};

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
) -> Result<(), String> {
    if expected.name.trim().is_empty()
        || expected.structure.trim().is_empty()
        || expected.codec.trim().is_empty()
    {
        return Err("incomplete expected portable descriptor".into());
    }
    if T::schema() != *expected {
        return Err("portable descriptor mismatch".into());
    }
    if T::wire_schema() != expected.identity() {
        return Err("wire identity disagrees with portable descriptor".into());
    }
    if vectors.is_empty() {
        return Err("at least one golden vector is required".into());
    }
    for vector in vectors {
        if vector.value.encode() != vector.bytes {
            return Err(format!(
                "{}: encoding differs from golden bytes",
                vector.name
            ));
        }
        let decoded = T::decode(vector.bytes)
            .map_err(|error| format!("{}: golden decode failed: {error}", vector.name))?;
        if decoded != vector.value {
            return Err(format!(
                "{}: decoded value differs from golden value",
                vector.name
            ));
        }
        if decoded.encode() != vector.bytes {
            return Err(format!(
                "{}: re-encoding differs from golden bytes",
                vector.name
            ));
        }
    }
    for (index, bytes) in malformed.iter().enumerate() {
        if T::decode(bytes).is_ok() {
            return Err(format!("malformed vector {index} was accepted"));
        }
    }
    Ok(())
}
