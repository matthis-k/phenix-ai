pub mod metadata;
pub mod static_metadata;
pub use metadata::*;
pub use static_metadata::{
    MetadataBytes, StaticContract, StaticExport, StaticImport, StaticOperation, StaticProperty,
    NATIVE_SECTION_BYTES,
};
pub fn identifier(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
}
pub fn operation_key(key: &str) -> bool {
    let Some((interface, operation)) = key.split_once('/') else {
        return false;
    };
    let Some((name, major)) = interface.rsplit_once('@') else {
        return false;
    };
    identifier(name)
        && identifier(operation)
        && major
            .parse::<u32>()
            .is_ok_and(|m| m > 0 && major == m.to_string())
}
pub fn import_identity(slot: &str, key: &str) -> String {
    if slot.is_empty() {
        key.into()
    } else {
        format!("{slot}:{key}")
    }
}
