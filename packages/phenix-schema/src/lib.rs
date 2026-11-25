//! Shared wire contract. Equality is authoritative, not a hash or Rust type name.
//!
//! # Shared Schema Contract
//!
//! `Schema` is the authoritative descriptor used by Graph and Storage. Equality
//! compares name, revision, structure, and codec. Canonical strings use versioned,
//! length-framed components rather than hashes or Rust type names.
//!
//! Graph's `ContractValue` derive mechanically builds ordered record descriptors
//! from field names, optionality, and nested field contracts. Its codec identifier
//! specifies the existing little-endian u64 length-framed record format. Independent
//! structs with the same declared name and structure interoperate; different field
//! names, order, types, or optionality do not. Graph uses canonical identities for
//! authoring, compilation, admission, and checked value decoding.
//!
//! Storage reexports this same `Schema` type. A storage `Codec::schema` can return
//! `T::schema()` from a Graph contract directly. `SchemaCodec` binds an existing
//! codec to that descriptor. Registry registration and session operations compare
//! the full descriptor before invoking the codec. Codec implementations and manual
//! descriptors are trusted assertions, not verification of arbitrary codec code.
//!
//! Manual Graph implementations must implement `ContractValue::schema()` without
//! macros. There is no nominal `SCHEMA` constant or optional descriptor fallback.
//! Primitive and derived record bytes are unchanged, but all wire identities include
//! the complete descriptor. Old name-only envelopes require explicit migration.
//!
//! `Schema::identity()` returns an owned `Arc<str>` containing the canonical
//! descriptor. Graph values, slots, declarations, interfaces, diagnostics and compiled
//! metadata own or borrow these identities. Clones share their allocation, equality
//! compares content, and dropping the last consumer releases strong ownership. There
//! is no process-global interner or strong cache. Independent equal descriptors may
//! have different allocations; pointer equality is never contract authority. Local
//! Rust values remain a distinct `TypeId` identity and cannot alias a wire identity.
//!
//! The ignored `distinct_descriptor_reload_retention` release test measures distinct
//! reload batches and verifies with `Weak` that unloaded identities have no strong
//! owners. This is an ownership measurement, not allocator RSS profiling. A retained
//! value, graph, interface snapshot, diagnostic or report legitimately retains its
//! identity until that consumer is dropped.
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schema {
    pub name: String,
    pub revision: u32,
    pub structure: String,
    pub codec: String,
}

impl Schema {
    /// Length framing makes names and nested descriptors unambiguous.
    pub fn canonical(&self) -> String {
        let mut out = format!("phenix-schema-v1:{};", self.revision);
        for part in [&self.name, &self.structure, &self.codec] {
            out.push_str(&format!("{}:{}", part.len(), part));
        }
        out
    }

    pub fn record(name: &str, fields: &[(&str, bool, Schema)]) -> Self {
        let mut structure = String::from("record-v1:");
        for (name, optional, schema) in fields {
            let nested = schema.canonical();
            structure.push_str(&format!(
                "{}:{}{}{}:{}",
                name.len(),
                name,
                u8::from(*optional),
                nested.len(),
                nested
            ));
        }
        Self {
            name: name.into(),
            revision: 1,
            structure,
            codec: "phenix-record-le64-v1".into(),
        }
    }

    /// Complete, content-comparable identity owned only by its consumers.
    /// Cloning the returned Arc shares its allocation; no global cache retains it.
    pub fn identity(&self) -> Arc<str> {
        self.canonical().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "manual distinct-descriptor reload retention measurement"]
    fn distinct_descriptor_reload_retention() {
        let mut encoded_bytes = 0;
        let mut retained_bytes = 0;
        let mut previous = 0;
        for count in [10, 100, 1_000, 10_000] {
            let start = std::time::Instant::now();
            for revision in previous..count {
                let schema = Schema {
                    name: format!("reload-probe/{revision}"),
                    revision: 1,
                    structure: "record{value:i64}".into(),
                    codec: "i64-le-v1".into(),
                };
                let identity = schema.identity();
                encoded_bytes += identity.len();
                let weak = Arc::downgrade(&identity);
                drop(identity);
                if let Some(identity) = weak.upgrade() {
                    retained_bytes += identity.len();
                }
            }
            println!(
                "distinct={count} encoded_string_bytes={encoded_bytes} batch_elapsed={:?}",
                start.elapsed()
            );
            println!("retained_string_bytes={retained_bytes}");
            assert_eq!(retained_bytes, 0);
            previous = count;
        }
    }

    #[test]
    fn identities_are_shared_by_clones_and_reclaimed_after_repeated_unloads() {
        for revision in 0..10_000 {
            let schema = Schema {
                name: format!("reload/{}", revision % 2),
                revision,
                structure: "record{value:i64}".into(),
                codec: "i64-le-v1".into(),
            };
            let identity = schema.identity();
            let independent = schema.identity();
            assert_eq!(identity, independent);
            assert!(!Arc::ptr_eq(&identity, &independent));
            let weak = Arc::downgrade(&identity);
            let cloned = identity.clone();
            assert!(Arc::ptr_eq(&identity, &cloned));
            assert_eq!(Arc::strong_count(&identity), 2);
            drop(schema);
            drop(identity);
            assert!(weak.upgrade().is_some());
            drop(cloned);
            assert!(weak.upgrade().is_none());
            // An equal identity is not a hidden strong owner of this allocation.
            drop(independent);
        }
    }

    #[test]
    fn framing_and_all_contract_components_are_authoritative() {
        let base = Schema {
            name: "x".into(),
            revision: 1,
            structure: "y".into(),
            codec: "z".into(),
        };
        for changed in [
            Schema {
                name: "x:y".into(),
                ..base.clone()
            },
            Schema {
                revision: 2,
                ..base.clone()
            },
            Schema {
                structure: "y:z".into(),
                ..base.clone()
            },
            Schema {
                codec: "other".into(),
                ..base.clone()
            },
        ] {
            assert_ne!(base.identity(), changed.identity());
        }
        assert_eq!(base.identity(), base.clone().identity());
        let required = Schema::record("r", &[("a", false, base.clone())]);
        assert_ne!(required, Schema::record("r", &[("a", true, base.clone())]));
        assert_ne!(required, Schema::record("r", &[("b", false, base)]));
    }
}
